//! "Fixar ícones na barra": o Windows 11 guarda a escolha de mostrar um ícone de bandeja direto na
//! barra (fora do overflow) em HKCU\Control Panel\NotifyIconSettings\<id>\IsPromoted (1 = na barra).
//! Cada ícone do app tem uma entrada própria (ExecutablePath + UID = o uID do Shell_NotifyIcon).
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::*;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn read_sz(k: HKEY, name: &str) -> Option<String> {
    let mut buf = [0u16; 520];
    let mut n = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(k, std::ptr::null(), w(name).as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr() as *mut _, &mut n) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn read_dword(k: HKEY, name: &str) -> Option<u32> {
    let mut v = 0u32;
    let mut n = 4u32;
    let r = unsafe { RegGetValueW(k, std::ptr::null(), w(name).as_ptr(), RRF_RT_REG_DWORD, null_mut(), &mut v as *mut u32 as *mut _, &mut n) };
    (r == ERROR_SUCCESS).then_some(v)
}

/// Define IsPromoted = `value` nas entradas deste executável cujo UID está em `uids`.
/// Retorna os UIDs que encontrou (o Windows cria a entrada um instante depois do NIM_ADD).
pub fn set_for(uids: &[u32], value: u32) -> Vec<u32> {
    let mut found = Vec::new();
    let Ok(exe) = std::env::current_exe() else { return found };
    let exe = exe.to_string_lossy().to_ascii_lowercase();
    unsafe {
        let mut root: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, w("Control Panel\\NotifyIconSettings").as_ptr(), 0, KEY_READ, &mut root) != ERROR_SUCCESS {
            return found;
        }
        let mut i = 0u32;
        loop {
            let mut name = [0u16; 260];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(root, i, name.as_mut_ptr(), &mut len, null_mut(), null_mut(), null_mut(), null_mut()) != ERROR_SUCCESS {
                break;
            }
            i += 1;
            let mut sub: HKEY = null_mut();
            if RegOpenKeyExW(root, name.as_ptr(), 0, KEY_QUERY_VALUE | KEY_SET_VALUE, &mut sub) != ERROR_SUCCESS {
                continue;
            }
            let path = read_sz(sub, "ExecutablePath").unwrap_or_default().to_ascii_lowercase();
            if path == exe
                && let Some(uid) = read_dword(sub, "UID")
                && uids.contains(&uid)
            {
                let r = RegSetValueExW(sub, w("IsPromoted").as_ptr(), 0, REG_DWORD, &value as *const u32 as *const u8, 4);
                if r == ERROR_SUCCESS {
                    found.push(uid);
                }
            }
            RegCloseKey(sub);
        }
        RegCloseKey(root);
    }
    found
}
