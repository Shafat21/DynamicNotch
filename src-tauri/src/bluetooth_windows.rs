//! Windows: WinRT device enumeration (paired classic + LE devices) and `Windows.Devices.Radios`.
//! Windows has no public API to connect a paired device, so `can_connect()` is false and the UI links to
//! Settings. Called only on the bluetooth worker, which joined the MTA in `init_thread`.

use super::{kind_from_appearance, kind_from_class, merge_le, BtDevice};
use windows::core::HSTRING;
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

const DENIED: &str = "Windows didn't allow this app to switch Bluetooth. Use Windows Settings instead.";

fn err(e: windows::core::Error) -> String {
    e.to_string()
}

pub fn init_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub fn can_connect() -> bool {
    false
}

fn bluetooth_radio() -> Result<Option<Radio>, String> {
    let radios = Radio::GetRadiosAsync().map_err(err)?.get().map_err(err)?;
    Ok(radios.into_iter().find(|r| r.Kind() == Ok(RadioKind::Bluetooth)))
}

pub fn radio() -> Result<Option<bool>, String> {
    Ok(bluetooth_radio()?.map(|r| r.State() == Ok(RadioState::On)))
}

pub fn set_radio(on: bool) -> Result<(), String> {
    let r = bluetooth_radio()?.ok_or("No Bluetooth adapter")?;
    if Radio::RequestAccessAsync().map_err(err)?.get().map_err(err)? != RadioAccessStatus::Allowed {
        return Err(DENIED.into());
    }
    let state = if on { RadioState::On } else { RadioState::Off };
    match r.SetStateAsync(state).map_err(err)?.get().map_err(err)? {
        RadioAccessStatus::Allowed => Ok(()),
        _ => Err(DENIED.into()),
    }
}

/// One unreachable device must not hide the rest: a per-device failure just reports it as not connected.
fn paired(selector: HSTRING, le: bool) -> Vec<BtDevice> {
    let Ok(found) = DeviceInformation::FindAllAsyncAqsFilter(&selector).and_then(|o| o.get()) else { return Vec::new() };
    let mut out = Vec::new();
    for info in found {
        let (Ok(id), Ok(name)) = (info.Id(), info.Name()) else { continue };
        let probe = if le {
            BluetoothLEDevice::FromIdAsync(&id).and_then(|o| o.get()).and_then(|d| {
                Ok((d.ConnectionStatus()? == BluetoothConnectionStatus::Connected, kind_from_appearance(d.Appearance()?.RawValue()?)))
            })
        } else {
            BluetoothDevice::FromIdAsync(&id).and_then(|o| o.get()).and_then(|d| {
                Ok((d.ConnectionStatus()? == BluetoothConnectionStatus::Connected, kind_from_class(d.ClassOfDevice()?.RawValue()?)))
            })
        };
        let (connected, kind) = probe.unwrap_or((false, "other"));
        out.push(BtDevice { id: id.to_string(), name: name.to_string(), kind, connected });
    }
    out
}

pub fn devices() -> Result<Vec<BtDevice>, String> {
    let classic = paired(BluetoothDevice::GetDeviceSelectorFromPairingState(true).map_err(err)?, false);
    let le = paired(BluetoothLEDevice::GetDeviceSelectorFromPairingState(true).map_err(err)?, true);
    let mut list = merge_le(classic, le);
    list.retain(|d| !d.name.is_empty());
    list.sort_by(|a, b| b.connected.cmp(&a.connected).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(list)
}

pub fn connect(_id: &str) -> Result<(), String> {
    Err("Windows can't connect Bluetooth devices from here. Use Windows Settings.".into())
}

pub fn disconnect(id: &str) -> Result<(), String> {
    connect(id)
}
