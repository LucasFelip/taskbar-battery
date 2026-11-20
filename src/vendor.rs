//! Mouse BK-R3X pelo receptor 2.4 GHz (VID 249A, PID 5C2F).
//! O receptor não expõe bateria pelo HID padrão; o mouse envia um relatório proprietário (página 0xFF01,
//! 33 bytes) quando liga/desliga. Formato observado: `00 C0 <flag 0|1> <bateria 0..100> 00 …`.
//! O 4º byte é a porcentagem (confirmado com o usuário: ~87 %). O 3º byte (flag) muda entre 0 e 1 ao
//! ligar/desligar a chave, mas NÃO é "carregando" (o mouse estava sem cabo nem base): significado
//! desconhecido, por isso é ignorado na interface.
//! Só ESCUTA (ReadFile); nunca envia nada ao dispositivo. Como o relatório vem por evento, o último
//! valor é guardado em disco e, depois de 12 h, aparece esmaecido como "último valor".
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const VID: u16 = 0x249A;
pub const PID: u16 = 0x5C2F;
pub const NAME: &str = "Mouse BK-R3X";
const STALE_SECS: u64 = 12 * 3600;

static STATE: Mutex<Option<(u8, bool, u64)>> = Mutex::new(None); // (%, carregando, unix secs)
static LOADED: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static HWND_: AtomicUsize = AtomicUsize::new(0);
static MSG: AtomicU32 = AtomicU32::new(0);

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

pub fn init(hwnd: HWND, msg: u32) {
    HWND_.store(hwnd as usize, Ordering::SeqCst);
    MSG.store(msg, Ordering::SeqCst);
}

/// Interpreta um relatório de 33 bytes: (porcentagem, flag desconhecido). None se não for o de bateria.
pub fn decode(r: &[u8]) -> Option<(u8, bool)> {
    if r.len() < 4 || r[1] != 0xC0 || r[3] > 100 {
        return None;
    }
    Some((r[3], r[2] == 1))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn file() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    p.push("taskbar-battery");
    let _ = std::fs::create_dir_all(&p);
    p.push("last-bkr3x.txt");
    Some(p)
}

fn parse_file(s: &str) -> Option<(u8, bool, u64)> {
    let mut it = s.trim().split(',');
    let p: u8 = it.next()?.parse().ok()?;
    let c: u8 = it.next()?.parse().ok()?;
    let t: u64 = it.next()?.parse().ok()?;
    (p <= 100).then_some((p, c == 1, t))
}

fn save(p: u8, c: bool, t: u64) {
    if let Some(f) = file() {
        let _ = std::fs::write(f, format!("{p},{},{t}\n", c as u8));
    }
}

/// (porcentagem, esmaecido). None se nunca houve relatório.
pub fn current() -> Option<(u8, bool)> {
    let mut s = STATE.lock().ok()?;
    if !LOADED.swap(true, Ordering::SeqCst) && s.is_none() {
        *s = file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| parse_file(&t));
    }
    s.map(|(p, _, t)| (p, now().saturating_sub(t) > STALE_SECS))
}

/// Garante uma thread escutando a interface proprietária (`path` terminado em nulo).
pub fn ensure_listener(path: &[u16]) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let path = path.to_vec();
    std::thread::spawn(move || {
        let _g = Guard;
        unsafe {
            let h = CreateFileW(path.as_ptr(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if h == INVALID_HANDLE_VALUE {
                crate::diag::log("bk-r3x: não abriu a interface proprietária");
                return;
            }
            let mut buf = [0u8; 64];
            loop {
                let mut got = 0u32;
                if ReadFile(h, buf.as_mut_ptr() as *mut _, 33, &mut got, std::ptr::null_mut()) == 0 {
                    break; // dispositivo removido
                }
                if let Some((p, c)) = decode(&buf[..got as usize]) {
                    let t = now();
                    if let Ok(mut s) = STATE.lock() {
                        *s = Some((p, c, t));
                    }
                    save(p, c, t);
                    crate::diag::log(&format!("bk-r3x: {p}% flag={} bytes={:02X?}", c as u8, &buf[..got.min(8) as usize]));
                    let hw = HWND_.load(Ordering::SeqCst);
                    if hw != 0 {
                        PostMessageW(hw as HWND, MSG.load(Ordering::SeqCst), 0, 0);
                    }
                }
            }
            CloseHandle(h);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(b1: u8, b2: u8, b3: u8) -> Vec<u8> {
        let mut v = vec![0u8; 33];
        v[1] = b1;
        v[2] = b2;
        v[3] = b3;
        v
    }

    #[test]
    fn decodes_observed_reports() {
        assert_eq!(decode(&report(0xC0, 0, 0x56)), Some((86, false)));
        assert_eq!(decode(&report(0xC0, 1, 0x57)), Some((87, true)));
    }

    #[test]
    fn rejects_other_reports() {
        assert_eq!(decode(&report(0xC1, 0, 50)), None);
        assert_eq!(decode(&report(0xC0, 0, 101)), None);
        assert_eq!(decode(&[0, 0xC0]), None);
    }

    #[test]
    fn file_roundtrip() {
        assert_eq!(parse_file("87,1,1700000000\n"), Some((87, true, 1_700_000_000)));
        assert_eq!(parse_file("101,0,1"), None);
        assert_eq!(parse_file("lixo"), None);
    }
}
