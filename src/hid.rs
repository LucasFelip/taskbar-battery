//! Fonte HID (cabo / receptor USB): um dispositivo por pai USB físico, só mouse/teclado
//! (ou qualquer HID que declare bateria). Bateria só quando o descritor declara
//! Battery Strength (página 0x06, usage 0x20); caso contrário `pct = None` ("?").
use crate::{Device, Kind};
use std::mem::size_of;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;

/// Sobe a árvore PnP até o dispositivo USB físico. None se for virtual ou Bluetooth.
pub(crate) fn usb_parent(mut dn: u32) -> Option<String> {
    for _ in 0..8 {
        let mut buf = [0u16; 260];
        if unsafe { CM_Get_Device_IDW(dn, buf.as_mut_ptr(), 260, 0) } != CR_SUCCESS {
            return None;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        let s = String::from_utf16_lossy(&buf[..n]);
        let up = s.to_ascii_uppercase();
        if up.starts_with("BTH") || up.starts_with("LGHUB") || up.starts_with("ROOT") || up.contains("ROOT_HUB") {
            return None;
        }
        if up.starts_with("USB\\VID_") && !up.contains("&MI_") {
            return Some(s);
        }
        let mut p = 0u32;
        if unsafe { CM_Get_Parent(&mut p, dn, 0) } != CR_SUCCESS {
            return None;
        }
        dn = p;
    }
    None
}

/// Lê Battery Strength se o descritor declarar. 0..100.
unsafe fn battery(h: HANDLE, pp: isize, caps: &HIDP_CAPS) -> Option<u8> {
    for (kind, len) in [(HidP_Feature, caps.FeatureReportByteLength), (HidP_Input, caps.InputReportByteLength)] {
        let mut n = 64u16;
        let mut vc = vec![unsafe { std::mem::zeroed::<HIDP_VALUE_CAPS>() }; n as usize];
        if unsafe { HidP_GetValueCaps(kind, vc.as_mut_ptr(), &mut n, pp) } != HIDP_STATUS_SUCCESS {
            continue;
        }
        for v in &vc[..n as usize] {
            if v.UsagePage != 0x06 || v.IsRange || unsafe { v.Anonymous.NotRange.Usage } != 0x20 {
                continue;
            }
            let mut buf = vec![0u8; len as usize];
            if buf.is_empty() {
                continue;
            }
            buf[0] = v.ReportID;
            let ok = if kind == HidP_Feature {
                unsafe { HidD_GetFeature(h, buf.as_mut_ptr() as *mut _, len as u32) }
            } else {
                unsafe { HidD_GetInputReport(h, buf.as_mut_ptr() as *mut _, len as u32) }
            };
            if !ok {
                continue;
            }
            let mut val = 0u32;
            if unsafe { HidP_GetUsageValue(kind, 0x06, v.LinkCollection, 0x20, &mut val, pp, buf.as_mut_ptr() as *mut _, len as u32) } == HIDP_STATUS_SUCCESS {
                let (lo, hi) = (v.LogicalMin as i64, v.LogicalMax as i64);
                let pct = if hi > lo { ((val as i64 - lo) * 100 / (hi - lo)).clamp(0, 100) } else { (val as i64).clamp(0, 100) };
                return Some(pct as u8);
            }
        }
    }
    None
}

pub fn read(out: &mut Vec<Device>) {
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set as isize == -1 {
            return;
        }
        let mut i = 0u32;
        let mut vendor_key: Option<u32> = None;
        loop {
            let mut ifd: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            ifd.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &guid, i, &mut ifd) == 0 {
                break;
            }
            i += 1;
            let mut need = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &ifd, std::ptr::null_mut(), 0, &mut need, std::ptr::null_mut());
            if need == 0 {
                continue;
            }
            let mut buf = vec![0u8; need as usize + 8];
            let det = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*det).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            let mut di: SP_DEVINFO_DATA = std::mem::zeroed();
            di.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &ifd, det, need, std::ptr::null_mut(), &mut di) == 0 {
                continue;
            }
            let Some(parent) = usb_parent(di.DevInst) else { continue };
            let key = crate::fnv(parent.as_bytes());

            let h = CreateFileW((*det).DevicePath.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if h == INVALID_HANDLE_VALUE {
                continue;
            }
            let mut attr: HIDD_ATTRIBUTES = std::mem::zeroed();
            attr.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
            HidD_GetAttributes(h, &mut attr);
            let is_bkr3x = attr.VendorID == crate::vendor::VID && attr.ProductID == crate::vendor::PID;
            if is_bkr3x {
                vendor_key = Some(key);
            }
            let mut pp: isize = 0;
            if HidD_GetPreparsedData(h, &mut pp) {
                let mut caps: HIDP_CAPS = std::mem::zeroed();
                if HidP_GetCaps(pp, &mut caps) == HIDP_STATUS_SUCCESS {
                    let k = match (caps.UsagePage, caps.Usage) {
                        (0x01, 0x02) => Kind::Mouse,
                        (0x01, 0x06) => Kind::Keyboard,
                        (0x01, 0x04) | (0x01, 0x05) => Kind::Gamepad,
                        _ => Kind::Other,
                    };
                    if is_bkr3x && caps.UsagePage == 0xFF01 {
                        let p = (*det).DevicePath.as_ptr();
                        let mut len = 0;
                        while *p.add(len) != 0 {
                            len += 1;
                        }
                        crate::vendor::ensure_listener(std::slice::from_raw_parts(p, len + 1));
                    }
                    let input_dev = caps.UsagePage == 0x01 && (caps.Usage == 0x02 || caps.Usage == 0x06);
                    let bat = battery(h, pp, &caps);
                    if input_dev || bat.is_some() {
                        let pos = out.iter().position(|d| d.id == key);
                        match pos {
                            Some(p) => {
                                if out[p].pct.is_none() {
                                    out[p].pct = bat;
                                }
                                out[p].kind = match (out[p].kind, k) {
                                    (Kind::Mouse, Kind::Keyboard) | (Kind::Keyboard, Kind::Mouse) => Kind::Combo,
                                    (Kind::Other, x) => x,
                                    (x, _) => x,
                                };
                            }
                            None => {
                                let mut name = [0u16; 128];
                                HidD_GetProductString(h, name.as_mut_ptr() as *mut _, (name.len() * 2) as u32);
                                let n = name.iter().position(|&c| c == 0).unwrap_or(0);
                                let mut nm = String::from_utf16_lossy(&name[..n]);
                                if nm.is_empty() {
                                    nm = "Dispositivo USB".into();
                                }
                                out.push(Device { id: key, name: nm, pct: bat, charging: false, kind: k, stale: false });
                            }
                        }
                    }
                }
                HidD_FreePreparsedData(pp);
            }
            CloseHandle(h);
        }
        SetupDiDestroyDeviceInfoList(set);
        // receptor do mouse BK-R3X: bateria vem do canal proprietário (ver vendor.rs)
        if let Some(k) = vendor_key
            && let Some(d) = out.iter_mut().find(|d| d.id == k)
        {
            d.name = crate::vendor::NAME.into();
            d.kind = Kind::Mouse;
            if let Some((p, stale)) = crate::vendor::current() {
                d.pct = Some(p);
                d.charging = false;
                d.stale = stale;
            }
        }
    }
}
