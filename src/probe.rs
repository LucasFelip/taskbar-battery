//! Probe de dispositivos USB HID não suportados: por N segundos apenas ESCUTA os relatórios de
//! entrada de cada interface HID USB e grava o resultado em %LOCALAPPDATA%\taskbar-battery\probe.txt.
//! Nunca envia comandos nem escreve nos dispositivos. Não grava serial, caminhos nem nomes de usuário.
//! Aviso: interfaces proprietárias de teclados podem carregar teclas digitadas; não digite durante o probe.
use std::fmt::Write as _;
use std::mem::size_of;
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Threading::ExitProcess;

#[derive(Default)]
struct Cap {
    label: String,
    count: u32,
    first: Vec<Vec<u8>>,
    last: Vec<u8>,
    changed: Vec<bool>,
}

pub fn out_path() -> Option<std::path::PathBuf> {
    let mut p = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    p.push("taskbar-battery");
    let _ = std::fs::create_dir_all(&p);
    p.push("probe.txt");
    Some(p)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

#[allow(clippy::while_immutable_condition)]
pub fn run(secs: u64) {
    let mut report = String::new();
    let _ = writeln!(report, "taskbar-battery probe v{} ({} s)", env!("CARGO_PKG_VERSION"), secs);
    let _ = writeln!(report, "Escuta apenas. Nenhum comando enviado. Sem serial/caminhos.\n");
    let caps: Arc<Mutex<Vec<Arc<Mutex<Cap>>>>> = Arc::new(Mutex::new(Vec::new()));

    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        let mut i = 0u32;
        while set as isize != -1 {
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
            if crate::hid::usb_parent(di.DevInst).is_none() {
                continue;
            }
            let path = (*det).DevicePath.as_ptr();
            // acesso de leitura; teclado/mouse padrão são bloqueados pelo Windows e caem no else
            let h = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            let h0 = if h == INVALID_HANDLE_VALUE {
                CreateFileW(path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut())
            } else {
                h
            };
            if h0 == INVALID_HANDLE_VALUE {
                continue;
            }
            let mut attr: HIDD_ATTRIBUTES = std::mem::zeroed();
            attr.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
            HidD_GetAttributes(h0, &mut attr);
            let mut name = [0u16; 128];
            HidD_GetProductString(h0, name.as_mut_ptr() as *mut _, (name.len() * 2) as u32);
            let n = name.iter().position(|&c| c == 0).unwrap_or(0);
            let mut pp: isize = 0;
            let (mut page, mut usage, mut inl, mut outl, mut featl) = (0u16, 0u16, 0u16, 0u16, 0u16);
            if HidD_GetPreparsedData(h0, &mut pp) {
                let mut c: HIDP_CAPS = std::mem::zeroed();
                if HidP_GetCaps(pp, &mut c) == HIDP_STATUS_SUCCESS {
                    (page, usage, inl, outl, featl) = (c.UsagePage, c.Usage, c.InputReportByteLength, c.OutputReportByteLength, c.FeatureReportByteLength);
                }
                HidD_FreePreparsedData(pp);
            }
            let label = format!(
                "VID {:04X} PID {:04X} '{}' | page {:#06X} usage {:#04X} | in {}B out {}B feat {}B",
                attr.VendorID,
                attr.ProductID,
                String::from_utf16_lossy(&name[..n]),
                page,
                usage,
                inl,
                outl,
                featl
            );
            if h == INVALID_HANDLE_VALUE {
                let _ = writeln!(report, "{label}\n   (leitura bloqueada pelo Windows: teclado/mouse padrão)\n");
                CloseHandle(h0);
                continue;
            }
            if inl == 0 {
                let _ = writeln!(report, "{label}\n   (sem relatório de entrada)\n");
                CloseHandle(h);
                continue;
            }
            let cap = Arc::new(Mutex::new(Cap { label, ..Default::default() }));
            caps.lock().unwrap().push(cap.clone());
            let hh = h as usize;
            std::thread::spawn(move || {
                let mut buf = vec![0u8; inl as usize];
                loop {
                    let mut got = 0u32;
                    let ok = ReadFile(hh as HANDLE, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut got, std::ptr::null_mut());
                    if ok == 0 {
                        break;
                    }
                    let r = &buf[..got as usize];
                    let mut c = cap.lock().unwrap();
                    c.count += 1;
                    if c.first.len() < 8 {
                        c.first.push(r.to_vec());
                    }
                    if c.last.len() == r.len() {
                        let prev = c.last.clone();
                        if c.changed.len() != r.len() {
                            c.changed = vec![false; r.len()];
                        }
                        for (k, (a, b)) in prev.iter().zip(r).enumerate() {
                            if a != b {
                                c.changed[k] = true;
                            }
                        }
                    }
                    c.last = r.to_vec();
                }
            });
        }
        if set as isize != -1 {
            SetupDiDestroyDeviceInfoList(set);
        }
    }

    std::thread::sleep(std::time::Duration::from_secs(secs));
    for c in caps.lock().unwrap().iter() {
        let c = c.lock().unwrap();
        let _ = writeln!(report, "{}\n   relatórios recebidos: {}", c.label, c.count);
        if c.count > 0 {
            let ch: Vec<String> = c.changed.iter().enumerate().filter(|(_, v)| **v).map(|(k, _)| k.to_string()).collect();
            let _ = writeln!(report, "   bytes que mudaram: [{}]", ch.join(","));
            for s in &c.first {
                let _ = writeln!(report, "   amostra: {}", hex(s));
            }
        }
        report.push('\n');
    }
    if let Some(p) = out_path() {
        let _ = std::fs::write(&p, report);
        let _ = std::process::Command::new("notepad.exe").arg(p).spawn();
    }
    unsafe { ExitProcess(0) };
}
