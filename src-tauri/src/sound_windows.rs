//! Windows: Core Audio (`IMMDeviceEnumerator`, `IAudioEndpointVolume`) for the default render endpoint;
//! switching the default uses the undocumented `IPolicyConfig` (what the Sound control panel uses).
//! Called only on the sound thread, which joined the MTA in `init_thread`.

use super::{AudioDevice, SoundState};
use std::ffi::c_void;
use windows::core::{Interface, Result as WinResult, GUID, HRESULT, IUnknown, PCWSTR, PWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, ERole, IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

/// PKEY_Device_FriendlyName (inlined: spares the large Devices_FunctionDiscovery feature).
const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };
/// CPolicyConfigClient / IPolicyConfig (Windows 7+), as used by AudioDeviceCmdlets' PolicyConfigClient.cs and SoundSwitch.
const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
/// SetDefaultEndpoint is IPolicyConfig's 11th method, after IUnknown's 3 → vtable slot 13.
const SET_DEFAULT_ENDPOINT_SLOT: usize = 13;
type SetDefaultEndpoint = unsafe extern "system" fn(*mut c_void, PCWSTR, ERole) -> HRESULT;

fn err(e: windows::core::Error) -> String {
    e.to_string()
}

pub fn init_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

unsafe fn enumerator() -> WinResult<IMMDeviceEnumerator> {
    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
}

/// Takes ownership of a COM-allocated string.
unsafe fn take(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

unsafe fn friendly_name(d: &IMMDevice) -> WinResult<String> {
    let mut pv = d.OpenPropertyStore(STGM_READ)?.GetValue(&FRIENDLY_NAME)?;
    let name = PropVariantToStringAlloc(&pv).map(|p| take(p));
    let _ = PropVariantClear(&mut pv);
    name
}

/// Volume controls of the default output's audio sessions that belong to media app `app`.
unsafe fn app_sessions(app: &str) -> WinResult<Vec<ISimpleAudioVolume>> {
    let dev = enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let mgr = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)?;
    let list = mgr.GetSessionEnumerator()?;
    let mut out = Vec::new();
    for i in 0..list.GetCount()? {
        let Ok(ctl) = list.GetSession(i).and_then(|s| s.cast::<IAudioSessionControl2>()) else { continue };
        let exe = ctl.GetProcessId().ok().and_then(crate::fullscreen::exe_of_pid);
        if exe.is_some_and(|e| super::matches_app(app, &e)) {
            out.push(ctl.cast::<ISimpleAudioVolume>()?);
        }
    }
    Ok(out)
}

pub fn app_volume(app: &str) -> Result<Option<f32>, String> {
    unsafe {
        let s = app_sessions(app).map_err(err)?;
        Ok(s.first().and_then(|v| v.GetMasterVolume().ok()))
    }
}

pub fn set_app_volume(app: &str, v: f32) -> Result<bool, String> {
    unsafe {
        let s = app_sessions(app).map_err(err)?;
        for vol in &s {
            vol.SetMasterVolume(v, std::ptr::null()).map_err(err)?;
        }
        Ok(!s.is_empty())
    }
}

unsafe fn endpoint_volume() -> WinResult<(IMMDevice, IAudioEndpointVolume)> {
    let dev = enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let vol = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?;
    Ok((dev, vol))
}

pub fn state() -> Result<SoundState, String> {
    unsafe {
        let (dev, vol) = endpoint_volume().map_err(err)?;
        Ok(SoundState {
            volume: vol.GetMasterVolumeLevelScalar().map_err(err)?,
            muted: vol.GetMute().map_err(err)?.as_bool(),
            device_id: take(dev.GetId().map_err(err)?),
        })
    }
}

pub fn set_volume(v: f32) -> Result<(), String> {
    unsafe { endpoint_volume().and_then(|(_, vol)| vol.SetMasterVolumeLevelScalar(v, std::ptr::null())).map_err(err) }
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    unsafe { endpoint_volume().and_then(|(_, vol)| vol.SetMute(muted, std::ptr::null())).map_err(err) }
}

pub fn devices() -> Result<Vec<AudioDevice>, String> {
    unsafe {
        let e = enumerator().map_err(err)?;
        let default_id = e.GetDefaultAudioEndpoint(eRender, eConsole).and_then(|d| d.GetId()).map(|p| take(p)).unwrap_or_default();
        let all = e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).map_err(err)?;
        let mut list = Vec::new();
        for i in 0..all.GetCount().map_err(err)? {
            let Ok(d) = all.Item(i) else { continue };
            let Ok(id) = d.GetId().map(|p| take(p)) else { continue };
            let name = friendly_name(&d).unwrap_or_else(|_| "Audio device".into());
            list.push(AudioDevice { default: id == default_id, id, name });
        }
        Ok(list)
    }
}

/// Make `id` the default output for all three roles, like the Sound control panel does.
pub fn set_default(id: &str) -> Result<(), String> {
    unsafe {
        let unk: IUnknown = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL).map_err(err)?;
        let mut raw: *mut c_void = std::ptr::null_mut();
        unk.query(&IID_POLICY_CONFIG, &mut raw).ok().map_err(err)?;
        let policy = IUnknown::from_raw(raw); // released on drop
        let vtable = *(policy.as_raw() as *const *const *const c_void);
        let set = std::mem::transmute::<*const c_void, SetDefaultEndpoint>(*vtable.add(SET_DEFAULT_ENDPOINT_SLOT));
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        for role in [eConsole, eMultimedia, eCommunications] {
            set(policy.as_raw(), PCWSTR(wide.as_ptr()), role).ok().map_err(err)?;
        }
        Ok(())
    }
}
