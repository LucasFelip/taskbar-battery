//! Fase 0: sondagem. Lista o que cada dispositivo expÃµe sobre bateria (HID e WPD).
//! Ferramenta de desenvolvimento; nÃ£o faz parte do binÃ¡rio final.
use std::mem::size_of;
use windows::{
    core::*,
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, HumanInterfaceDevice::*, PortableDevices::*},
        Foundation::*,
        Storage::FileSystem::*,
        System::Com::*,
            },
};

fn wstr(p: &[u16]) -> String {
    let n = p.iter().position(|&c| c == 0).unwrap_or(p.len());
    String::from_utf16_lossy(&p[..n])
}

unsafe fn hid_scan() {
    println!("=== HID ===");
    let guid = unsafe { HidD_GetHidGuid() };
    let set = unsafe {
        SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE)
    }
    .expect("SetupDiGetClassDevsW");
    let mut i = 0u32;
    loop {
        let mut ifd = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInterfaces(set, None, &guid, i, &mut ifd) }.is_err() {
            break;
        }
        i += 1;
        let mut need = 0u32;
        let _ = unsafe { SetupDiGetDeviceInterfaceDetailW(set, &ifd, None, 0, Some(&mut need), None) };
        let mut buf = vec![0u8; need as usize];
        let det = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        unsafe { (*det).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 };
        if unsafe { SetupDiGetDeviceInterfaceDetailW(set, &ifd, Some(det), need, None, None) }.is_err() {
            continue;
        }
        let path = PCWSTR(unsafe { (*det).DevicePath.as_ptr() });
        let pstr = unsafe { path.to_string().unwrap_or_default() };
        let lp = pstr.to_ascii_lowercase();
        if !(lp.contains("vid_") || lp.contains("vid&")) {
            continue; // ignora HID de sistema (teclado/mouse embutidos virtuais)
        }
        let h = match unsafe {
            CreateFileW(
                path,
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        } {
            Ok(h) => h,
            Err(e) => {
                println!("{pstr}\n   (nao abriu: {e})");
                continue;
            }
        };
        let mut attr = HIDD_ATTRIBUTES { Size: size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
        let _ = unsafe { HidD_GetAttributes(h, &mut attr) };
        let mut name = [0u16; 128];
        let _ = unsafe { HidD_GetProductString(h, name.as_mut_ptr() as *mut _, (name.len() * 2) as u32) };
        let mut pp = PHIDP_PREPARSED_DATA::default();
        let mut line = format!(
            "VID {:04X} PID {:04X} '{}'\n   {}",
            attr.VendorID,
            attr.ProductID,
            wstr(&name),
            pstr
        );
        if unsafe { HidD_GetPreparsedData(h, &mut pp) } {
            let mut caps = HIDP_CAPS::default();
            if unsafe { HidP_GetCaps(pp, &mut caps) } == HIDP_STATUS_SUCCESS {
                line += &format!(
                    "\n   page {:#06X} usage {:#04X} | in {}B out {}B feat {}B",
                    caps.UsagePage, caps.Usage, caps.InputReportByteLength, caps.OutputReportByteLength,
                    caps.FeatureReportByteLength
                );
                for (kind, label) in [(HidP_Input, "IN"), (HidP_Feature, "FEAT")] {
                    let mut n = caps.NumberInputValueCaps.max(caps.NumberFeatureValueCaps).max(1);
                    let mut vc = vec![HIDP_VALUE_CAPS::default(); n as usize];
                    if unsafe { HidP_GetValueCaps(kind, vc.as_mut_ptr(), &mut n, pp) } == HIDP_STATUS_SUCCESS {
                        for v in &vc[..n as usize] {
                            let u = unsafe { v.Anonymous.NotRange.Usage };
                            let bat = (v.UsagePage == 0x06 && u == 0x20) || v.UsagePage == 0x85;
                            if bat || v.UsagePage >= 0xFF00 {
                                line += &format!(
                                    "\n   {label} value page {:#06X} usage {:#04X} rid {}{}",
                                    v.UsagePage,
                                    u,
                                    v.ReportID,
                                    if bat { "  <== BATERIA" } else { "" }
                                );
                            }
                        }
                    }
                }
            }
            let _ = unsafe { HidD_FreePreparsedData(pp) };
        }
        println!("{line}");
        let _ = unsafe { CloseHandle(h) };
    }
    let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
}

unsafe fn wpd_scan() -> Result<()> {
    println!("=== WPD (celular/MTP) ===");
    let mgr: IPortableDeviceManager = unsafe { CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER)? };
    let mut n = 0u32;
    unsafe { mgr.GetDevices(std::ptr::null_mut(), &mut n)? };
    let mut ids = vec![PWSTR::null(); n as usize];
    unsafe { mgr.GetDevices(ids.as_mut_ptr(), &mut n)? };
    for id in ids.iter().take(n as usize) {
        let pid = PCWSTR(id.0);
        println!("device: {}", unsafe { pid.to_string().unwrap_or_default() });
        let dev: IPortableDevice = unsafe { CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)? };
        let client: IPortableDeviceValues = unsafe { CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)? };
        unsafe { client.SetStringValue(&WPD_CLIENT_NAME, w!("battery-probe"))? };
        unsafe { client.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 1)? };
        unsafe { client.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 0)? };
        unsafe { client.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0)? };
        if let Err(e) = unsafe { dev.Open(pid, &client) } {
            println!("   Open falhou: {e}");
            continue;
        }
        let content = unsafe { dev.Content()? };
        let props = unsafe { content.Properties()? };
        let keys = unsafe { props.GetSupportedProperties(w!("DEVICE"))? };
        let mut count = 0u32;
        unsafe { keys.GetCount(&mut count)? };
        let vals = unsafe { props.GetValues(w!("DEVICE"), &keys)? };
        for k in 0..count {
            let mut key = PROPERTYKEY::default();
            if unsafe { keys.GetAt(k, &mut key) }.is_err() {
                continue;
            }
            let mut s = String::new();
            if let Ok(p) = unsafe { vals.GetStringValue(&key) } {
                s = unsafe { p.to_string().unwrap_or_default() };
            } else if let Ok(u) = unsafe { vals.GetUnsignedIntegerValue(&key) } {
                s = u.to_string();
            }
            let tag = if key.fmtid == WPD_DEVICE_POWER_LEVEL.fmtid && key.pid == WPD_DEVICE_POWER_LEVEL.pid {
                "  <== POWER LEVEL"
            } else {
                ""
            };
            println!("   {:?} #{} = {}{}", key.fmtid, key.pid, s, tag);
        }
    }
    Ok(())
}


/// Descreve (sem enviar nada) os campos que cada interface HID do VID/PID declara no descritor.
unsafe fn describe(vid: u16, pid: u16) {
    use std::mem::size_of;
    println!("=== descritores de {vid:04X}:{pid:04X} ===");
    let guid = unsafe { HidD_GetHidGuid() };
    let set = unsafe { SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) }.expect("setupdi");
    let mut i = 0u32;
    loop {
        let mut ifd = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
        if unsafe { SetupDiEnumDeviceInterfaces(set, None, &guid, i, &mut ifd) }.is_err() {
            break;
        }
        i += 1;
        let mut need = 0u32;
        let _ = unsafe { SetupDiGetDeviceInterfaceDetailW(set, &ifd, None, 0, Some(&mut need), None) };
        let mut buf = vec![0u8; need as usize];
        let det = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        unsafe { (*det).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 };
        if unsafe { SetupDiGetDeviceInterfaceDetailW(set, &ifd, Some(det), need, None, None) }.is_err() {
            continue;
        }
        let path = PCWSTR(unsafe { (*det).DevicePath.as_ptr() });
        let h = match unsafe { CreateFileW(path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) } {
            Ok(h) => h,
            Err(_) => continue,
        };
        let mut attr = HIDD_ATTRIBUTES { Size: size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
        let _ = unsafe { HidD_GetAttributes(h, &mut attr) };
        if attr.VendorID != vid || attr.ProductID != pid {
            let _ = unsafe { CloseHandle(h) };
            continue;
        }
        let mut pp = PHIDP_PREPARSED_DATA::default();
        if unsafe { HidD_GetPreparsedData(h, &mut pp) } {
            let mut caps = HIDP_CAPS::default();
            if unsafe { HidP_GetCaps(pp, &mut caps) } == HIDP_STATUS_SUCCESS {
                println!(
                    "\ninterface: page {:#06X} usage {:#04X} | in {}B out {}B feat {}B | valueCaps in/out/feat = {}/{}/{} | buttonCaps = {}/{}/{}",
                    caps.UsagePage, caps.Usage, caps.InputReportByteLength, caps.OutputReportByteLength, caps.FeatureReportByteLength,
                    caps.NumberInputValueCaps, caps.NumberOutputValueCaps, caps.NumberFeatureValueCaps,
                    caps.NumberInputButtonCaps, caps.NumberOutputButtonCaps, caps.NumberFeatureButtonCaps
                );
                for (kind, label, nv, nb) in [
                    (HidP_Input, "IN", caps.NumberInputValueCaps, caps.NumberInputButtonCaps),
                    (HidP_Output, "OUT", caps.NumberOutputValueCaps, caps.NumberOutputButtonCaps),
                    (HidP_Feature, "FEAT", caps.NumberFeatureValueCaps, caps.NumberFeatureButtonCaps),
                ] {
                    if nv > 0 {
                        let mut n = nv;
                        let mut vc = vec![HIDP_VALUE_CAPS::default(); n as usize];
                        if unsafe { HidP_GetValueCaps(kind, vc.as_mut_ptr(), &mut n, pp) } == HIDP_STATUS_SUCCESS {
                            for v in &vc[..n as usize] {
                                let (u0, u1) = unsafe { if v.IsRange { (v.Anonymous.Range.UsageMin, v.Anonymous.Range.UsageMax) } else { (v.Anonymous.NotRange.Usage, v.Anonymous.NotRange.Usage) } };
                                println!("   {label} value  rid {:3} page {:#06X} usage {:#04X}-{:#04X} bits {} count {} logical {}..{}", v.ReportID, v.UsagePage, u0, u1, v.BitSize, v.ReportCount, v.LogicalMin, v.LogicalMax);
                            }
                        }
                    }
                    if nb > 0 {
                        let mut n = nb;
                        let mut bc = vec![HIDP_BUTTON_CAPS::default(); n as usize];
                        if unsafe { HidP_GetButtonCaps(kind, bc.as_mut_ptr(), &mut n, pp) } == HIDP_STATUS_SUCCESS {
                            for b in &bc[..n as usize] {
                                let (u0, u1) = unsafe { if b.IsRange { (b.Anonymous.Range.UsageMin, b.Anonymous.Range.UsageMax) } else { (b.Anonymous.NotRange.Usage, b.Anonymous.NotRange.Usage) } };
                                println!("   {label} button rid {:3} page {:#06X} usage {:#04X}-{:#04X}", b.ReportID, b.UsagePage, u0, u1);
                            }
                        }
                    }
                }
            }
            let _ = unsafe { HidD_FreePreparsedData(pp) };
        }
        let _ = unsafe { CloseHandle(h) };
    }
    let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("describe") {
        let vid = u16::from_str_radix(&args[2], 16).unwrap();
        let pid = u16::from_str_radix(&args[3], 16).unwrap();
        unsafe { describe(vid, pid) };
        return;
    }
    unsafe {
        hid_scan();
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Err(e) = wpd_scan() {
            println!("WPD erro: {e}");
        }
    }
}

