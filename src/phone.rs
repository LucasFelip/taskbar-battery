//! Celular Android por cabo. Fonte confiável: `adb shell dumpsys battery` (o WPD/MTP mostrou
//! valor defasado na fase 0). A leitura roda em thread curta para não travar a bandeja.
//! Se o servidor ADB não estava rodando, ele é encerrado ao final (sem daemon residente).
use crate::{Device, Kind};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

pub static CACHE: Mutex<Option<Device>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_MS: AtomicU64 = AtomicU64::new(0);
const MIN_INTERVAL_MS: u64 = 30_000;

/// Libera RUNNING mesmo se a thread entrar em pânico.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ID_PHONE: &[u8] = b"phone-adb";

pub fn cached() -> Option<Device> {
    CACHE.lock().ok().and_then(|c| c.clone())
}

/// Há uma interface ADB USB presente? (barato; evita criar processo sem celular)
fn adb_device_present() -> bool {
    let filter: Vec<u16> = "USB\0".encode_utf16().collect();
    let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
    let mut len = 0u32;
    unsafe {
        if CM_Get_Device_ID_List_SizeW(&mut len, filter.as_ptr(), flags) != CR_SUCCESS || len < 2 {
            return false;
        }
        let mut ids = vec![0u16; len as usize];
        if CM_Get_Device_ID_ListW(filter.as_ptr(), ids.as_mut_ptr(), len, flags) != CR_SUCCESS {
            return false;
        }
        ids.split(|&c| c == 0).any(|id| String::from_utf16_lossy(id).to_ascii_uppercase().contains("&ADB\\"))
    }
}

fn find_adb() -> Option<String> {
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("PATH") {
        cands.extend(std::env::split_paths(&p).map(|d| d.join("adb.exe")));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        cands.push(std::path::PathBuf::from(l).join("Android\\Sdk\\platform-tools\\adb.exe"));
    }
    cands.push("C:\\Program Files (x86)\\Iriun Webcam\\adb.exe".into());
    cands.into_iter().find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
}

fn server_running() -> bool {
    use std::net::{SocketAddr, TcpStream};
    let a: SocketAddr = "127.0.0.1:5037".parse().unwrap();
    TcpStream::connect_timeout(&a, Duration::from_millis(200)).is_ok()
}

fn run(adb: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut c = Command::new(adb).args(args).creation_flags(CREATE_NO_WINDOW).stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null()).spawn().ok()?;
    let t0 = Instant::now();
    loop {
        match c.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t0.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = c.kill();
                let _ = c.wait();
                return None;
            }
        }
    }
    let out = c.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse(s: &str) -> Option<Device> {
    let mut lines = s.lines();
    let model = lines.next().map(|l| l.trim().to_string()).filter(|m| !m.is_empty() && !m.contains(':'));
    let (mut level, mut powered) = (None, false);
    for l in s.lines() {
        let l = l.trim();
        if let Some(v) = l.strip_prefix("level:") {
            level = v.trim().parse::<u8>().ok();
        } else if l.ends_with("powered: true") {
            powered = true;
        }
    }
    let name = model.map(|m| format!("Celular {m}")).unwrap_or_else(|| "Celular Android".into());
    level.map(|p| Device { id: crate::fnv(ID_PHONE), name, pct: Some(p.min(100)), charging: powered, kind: Kind::Phone, ..Default::default() })
}

/// Lê o celular agora (bloqueante, até ~25 s). Assume que há uma interface ADB presente.
fn read_device() -> Device {
    let unknown = |why: &str| Device { id: crate::fnv(ID_PHONE), name: format!("Celular Android ({why})"), pct: None, kind: Kind::Phone, ..Default::default() };
    match find_adb() {
        None => {
            crate::diag::log("phone: adb.exe não encontrado (PATH, Android SDK, Iriun)");
            unknown("ADB não encontrado")
        }
        Some(adb) => {
            let was_running = server_running();
            let out = run(&adb, &["shell", "getprop ro.product.model; dumpsys battery"], Duration::from_secs(20));
            if !was_running {
                let _ = run(&adb, &["kill-server"], Duration::from_secs(5));
            }
            if out.is_none() {
                crate::diag::log("phone: adb sem resposta (depuração USB não autorizada?)");
            }
            out.and_then(|o| parse(&o)).unwrap_or_else(|| unknown("sem acesso ADB"))
        }
    }
}

/// Versão síncrona para `--once`.
pub fn fetch_blocking() -> Option<Device> {
    adb_device_present().then(read_device)
}

/// Dispara a leitura em segundo plano. Ao terminar, grava no cache e posta `msg` à janela.
pub fn kick(hwnd: HWND, msg: u32) {
    if !adb_device_present() {
        if let Ok(mut c) = CACHE.lock() {
            *c = None;
        }
        return;
    }
    // intervalo mínimo entre leituras (WM_SETTINGCHANGE/WM_DEVICECHANGE podem ser frequentes)
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let last = LAST_MS.load(Ordering::Relaxed);
    if last != 0 && now.wrapping_sub(last) < MIN_INTERVAL_MS && cached().is_some() {
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    LAST_MS.store(now, Ordering::Relaxed);
    let h = hwnd as usize;
    std::thread::spawn(move || {
        let _guard = Guard;
        let dev = read_device();
        if let Ok(mut c) = CACHE.lock() {
            *c = Some(dev);
        }
        drop(_guard);
        unsafe { PostMessageW(h as HWND, msg, 0, 0) };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dumpsys() {
        let s = "SM-A315G\nCurrent Battery Service state:\n  AC powered: false\n  USB powered: true\n  status: 5\n  level: 100\n  scale: 100\n";
        let d = parse(s).unwrap();
        assert_eq!(d.pct, Some(100));
        assert!(d.charging);
        assert_eq!(d.name, "Celular SM-A315G");
    }

    #[test]
    fn parse_not_charging_and_garbage() {
        let d = parse("X1\n  AC powered: false\n  USB powered: false\n  level: 42\n").unwrap();
        assert_eq!(d.pct, Some(42));
        assert!(!d.charging);
        assert!(parse("error: device unauthorized").is_none());
    }
}
