//! "Iniciar com o Windows": valor em HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
//!
//! O valor sempre aponta para a cópia **instalada** (`%LOCALAPPDATA%\Programs\taskbar-battery`) quando ela
//! existe. Assim, abrir o app a partir de outro lugar (por exemplo um `.exe` baixado em Downloads, que o
//! Windows marca como "vindo da internet" e pede confirmação a cada login) não muda o que inicia com o sistema.
use std::path::PathBuf;
use std::ptr::null_mut;
use windows_sys::Win32::System::Registry::*;

const SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0";
const NAME: &str = "TaskbarBattery\0";

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn install_path() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    p.push("Programs");
    p.push("taskbar-battery");
    p.push("taskbar-battery.exe");
    Some(p)
}

/// Executável que deve iniciar com o Windows: a cópia instalada, se existir; senão o que está rodando.
fn target(installed: Option<PathBuf>, current: Option<PathBuf>) -> Option<PathBuf> {
    installed.filter(|p| p.is_file()).or(current)
}

fn command_for(exe: &std::path::Path) -> String {
    format!("\"{}\"", exe.display())
}

fn read_value() -> Option<String> {
    let (k, n) = (w(SUBKEY), w(NAME));
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr() as *mut _, &mut size) };
    if r != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buf[..len]))
}

pub fn is_enabled() -> bool {
    read_value().is_some()
}

pub fn set(enable: bool) {
    let (k, n) = (w(SUBKEY), w(NAME));
    unsafe {
        let mut h: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_SET_VALUE, &mut h) != 0 {
            return;
        }
        if enable {
            if let Some(exe) = target(install_path(), std::env::current_exe().ok()) {
                let v: Vec<u16> = command_for(&exe).encode_utf16().chain(Some(0)).collect();
                RegSetValueExW(h, n.as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
            }
        } else {
            RegDeleteValueW(h, n.as_ptr());
        }
        RegCloseKey(h);
    }
}

/// Se "Iniciar com o Windows" está ligado mas aponta para outro lugar que não a cópia instalada
/// (por exemplo Downloads), corrige o valor. Não liga nada que o usuário tenha desligado.
pub fn repair() {
    let Some(installed) = install_path().filter(|p| p.is_file()) else { return };
    let want = command_for(&installed);
    if let Some(cur) = read_value()
        && !cur.eq_ignore_ascii_case(&want)
    {
        set(true);
        crate::diag::log(&format!("iniciar com o Windows corrigido: {cur} -> {want}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn prefers_installed_copy_when_it_exists() {
        let cur = PathBuf::from(r"C:\Users\x\Downloads\taskbar-battery-v0.2.0-windows-x64.exe");
        // a cópia instalada não existe: usa o que está rodando
        let missing = PathBuf::from(r"C:\nao\existe\taskbar-battery.exe");
        assert_eq!(target(Some(missing), Some(cur.clone())), Some(cur.clone()));
        // a cópia instalada existe: usa ela (aqui, o próprio executável de teste)
        let real = std::env::current_exe().unwrap();
        assert_eq!(target(Some(real.clone()), Some(cur)), Some(real));
    }

    #[test]
    fn quotes_the_path() {
        assert_eq!(command_for(Path::new(r"C:\a b\x.exe")), "\"C:\\a b\\x.exe\"");
    }
}
