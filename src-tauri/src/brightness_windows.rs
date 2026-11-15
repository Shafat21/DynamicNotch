//! Windows: built-in panels through WMI `root\WMI` (`WmiMonitorBrightness` / `WmiSetBrightness`),
//! external monitors through DDC/CI (dxva2). Ids: `wmi:<InstanceName>`, `ddc:<\\.\DISPLAYn>#<index>`.
//! Runs only on the brightness worker thread, which joins the MTA once in `init_thread`.

use super::{from_percent, to_percent, BrightDisplay};
use windows::core::{w, Result as WinResult, BSTR, PCWSTR};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness,
    PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BSTR, VT_I4, VT_UI1};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
};

const RPC_C_AUTHN_WINNT: u32 = 10; // spares the Win32_System_Rpc feature
const RPC_C_AUTHZ_NONE: u32 = 0;

pub fn init_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub fn displays() -> Vec<BrightDisplay> {
    let mut list = unsafe { internal() }.unwrap_or_default();
    list.extend(external());
    list
}

pub fn set(id: &str, percent: u8) -> Result<(), String> {
    let r = if let Some(instance) = id.strip_prefix("wmi:") {
        unsafe { wmi_set(instance, percent) }
    } else if let Some(key) = id.strip_prefix("ddc:") {
        ddc_set(key, percent)
    } else {
        Err(E_INVALIDARG.into())
    };
    r.map_err(|e| e.to_string())
}

// ---------- WMI (built-in panel) ----------

unsafe fn wmi() -> WinResult<IWbemServices> {
    let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
    let none = BSTR::new();
    let svc = locator.ConnectServer(&BSTR::from("root\\WMI"), &none, &none, &none, 0, &none, None)?;
    let _ = CoSetProxyBlanket(&svc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE);
    Ok(svc)
}

unsafe fn query(svc: &IWbemServices, wql: &str) -> WinResult<Vec<IWbemClassObject>> {
    let rows = svc.ExecQuery(&BSTR::from("WQL"), &BSTR::from(wql), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None)?;
    let mut out = Vec::new();
    loop {
        let mut row = [None];
        let mut n = 0;
        let _ = rows.Next(WBEM_INFINITE, &mut row, &mut n);
        match row[0].take() {
            Some(o) if n == 1 => out.push(o),
            _ => return Ok(out),
        }
    }
}

unsafe fn prop(o: &IWbemClassObject, name: PCWSTR) -> Option<VARIANT> {
    let mut v = VARIANT::default();
    o.Get(name, 0, &mut v, None, None).ok()?;
    Some(v)
}

unsafe fn prop_u8(o: &IWbemClassObject, name: PCWSTR) -> Option<u8> {
    let v = prop(o, name)?; // VT_UI1 owns nothing: no VariantClear needed
    let inner = &v.Anonymous.Anonymous;
    (inner.vt == VT_UI1).then(|| inner.Anonymous.bVal)
}

unsafe fn prop_str(o: &IWbemClassObject, name: PCWSTR) -> Option<String> {
    let mut v = prop(o, name)?;
    let s = (v.Anonymous.Anonymous.vt == VT_BSTR).then(|| v.Anonymous.Anonymous.Anonymous.bstrVal.to_string());
    let _ = VariantClear(&mut v);
    s
}

unsafe fn internal() -> WinResult<Vec<BrightDisplay>> {
    let svc = wmi()?;
    Ok(query(&svc, "SELECT InstanceName, CurrentBrightness FROM WmiMonitorBrightness")?
        .iter()
        .filter_map(|o| {
            Some(BrightDisplay {
                id: format!("wmi:{}", prop_str(o, w!("InstanceName"))?),
                name: "Built-in display".into(),
                percent: prop_u8(o, w!("CurrentBrightness"))?.min(100),
                kind: "internal",
            })
        })
        .collect())
}

unsafe fn wmi_set(instance: &str, percent: u8) -> WinResult<()> {
    let svc = wmi()?;
    let methods = query(&svc, "SELECT * FROM WmiMonitorBrightnessMethods")?;
    let target = methods.iter().find(|o| prop_str(o, w!("InstanceName")).as_deref() == Some(instance)).ok_or(windows::core::Error::from(E_INVALIDARG))?;
    let path = prop_str(target, w!("__PATH")).ok_or(windows::core::Error::from(E_FAIL))?;
    let mut class = None;
    svc.GetObject(&BSTR::from("WmiMonitorBrightnessMethods"), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None)?;
    let mut in_sig = None;
    class.ok_or(windows::core::Error::from(E_FAIL))?.GetMethod(w!("WmiSetBrightness"), 0, &mut in_sig, std::ptr::null_mut())?;
    let args = in_sig.ok_or(windows::core::Error::from(E_FAIL))?.SpawnInstance(0)?;
    // uint32 travels as VT_I4, uint8 as VT_UI1 (CIM type 0 = the declared one)
    let mut timeout = VARIANT::default();
    (*timeout.Anonymous.Anonymous).vt = VT_I4;
    let mut level = VARIANT::default();
    (*level.Anonymous.Anonymous).vt = VT_UI1;
    (*level.Anonymous.Anonymous).Anonymous.bVal = percent.min(100);
    args.Put(w!("Timeout"), 0, &timeout, 0)?;
    args.Put(w!("Brightness"), 0, &level, 0)?;
    svc.ExecMethod(&BSTR::from(path), &BSTR::from("WmiSetBrightness"), WBEM_GENERIC_FLAG_TYPE(0), None, &args, None, None)
}

// ---------- DDC/CI (external monitors) ----------

unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
    (*(data.0 as *mut Vec<HMONITOR>)).push(h);
    true.into()
}

/// Calls `f(id, description, handle)` for every physical monitor, then frees the handles.
fn each_physical(mut f: impl FnMut(&str, &str, windows::Win32::Foundation::HANDLE)) {
    let mut monitors: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut monitors as *mut _ as isize));
        for h in monitors {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if !GetMonitorInfoW(h, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
                continue;
            }
            let device = String::from_utf16_lossy(&info.szDevice).trim_end_matches('\0').to_string();
            let mut n = 0;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(h, &mut n).is_err() || n == 0 {
                continue;
            }
            let mut phys = vec![PHYSICAL_MONITOR::default(); n as usize];
            if GetPhysicalMonitorsFromHMONITOR(h, &mut phys).is_err() {
                continue;
            }
            for (i, p) in phys.iter().enumerate() {
                let (text, handle) = (p.szPhysicalMonitorDescription, p.hPhysicalMonitor); // packed struct: copy out
                let desc = String::from_utf16_lossy(&text).trim_end_matches('\0').to_string();
                f(&format!("{device}#{i}"), &desc, handle);
            }
            let _ = DestroyPhysicalMonitors(&phys);
        }
    }
}

/// (min, current, max), or None when the monitor doesn't answer DDC/CI (built-in panels, TVs, some docks).
fn ddc_get(h: windows::Win32::Foundation::HANDLE) -> Option<(u32, u32, u32)> {
    let (mut min, mut cur, mut max) = (0, 0, 0);
    (unsafe { GetMonitorBrightness(h, &mut min, &mut cur, &mut max) } != 0).then_some((min, cur, max))
}

fn external() -> Vec<BrightDisplay> {
    let mut list = Vec::new();
    each_physical(|key, desc, h| {
        if let Some((min, cur, max)) = ddc_get(h) {
            // shortcut: dxva2 only knows "Generic PnP Monitor" for most screens; the EDID name needs DisplayConfig, add it if users ask
            let name = if desc.is_empty() || desc.starts_with("Generic") { "External display" } else { desc };
            list.push(BrightDisplay { id: format!("ddc:{key}"), name: name.into(), percent: to_percent(min, cur, max), kind: "external" });
        }
    });
    list
}

fn ddc_set(key: &str, percent: u8) -> WinResult<()> {
    let mut r: WinResult<()> = Err(E_INVALIDARG.into());
    each_physical(|k, _, h| {
        if k == key {
            r = match ddc_get(h) {
                Some((min, _, max)) if unsafe { SetMonitorBrightness(h, from_percent(min, max, percent)) } != 0 => Ok(()),
                // dxva2 doesn't reliably set the last error here: a fixed message instead of "completed successfully"
                _ => Err(windows::core::Error::new(E_FAIL, "The monitor did not accept the brightness change (DDC/CI)")),
            };
        }
    });
    r
}
