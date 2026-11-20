//! "Iniciar com o Windows": valor em HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
use std::ptr::null_mut;
use windows_sys::Win32::System::Registry::*;

const SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0";
const NAME: &str = "TaskbarBattery\0";

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn is_enabled() -> bool {
    let (k, n) = (w(SUBKEY), w(NAME));
    unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), null_mut()) == 0 }
}

pub fn set(enable: bool) {
    let (k, n) = (w(SUBKEY), w(NAME));
    unsafe {
        let mut h: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_SET_VALUE, &mut h) != 0 {
            return;
        }
        if enable {
            if let Ok(exe) = std::env::current_exe() {
                let v: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
                RegSetValueExW(h, n.as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
            }
        } else {
            RegDeleteValueW(h, n.as_ptr());
        }
        RegCloseKey(h);
    }
}
