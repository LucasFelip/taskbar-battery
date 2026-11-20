#![windows_subsystem = "windows"]
//! taskbar-battery: um ícone de bandeja por dispositivo, com consumo mínimo.
//! O processo dorme em GetMessageW; só acorda por timer (10 min), mudança de dispositivos ou retorno do sleep.
mod bt;
mod config;
mod diag;
mod hid;
mod icon;
mod known;
mod last;
mod phone;
mod pin;
mod probe;
mod startup;
mod vendor;

use std::cell::RefCell;
use std::fmt::Write as _;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcess};
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY: u32 = WM_APP + 1;
const WM_PHONE: u32 = WM_APP + 2;
const WM_VENDOR: u32 = WM_APP + 3;
const TIMER_POLL: usize = 1;
const TIMER_DEBOUNCE: usize = 2;
const TIMER_PIN: usize = 3;
const PIN_DELAY_MS: u32 = 1500;
const PIN_MAX_TRIES: u8 = 6;
const POLL_MS: u32 = 10 * 60 * 1000;
const DEBOUNCE_MS: u32 = 1500;
const RESUME_MS: u32 = 4000;
const ID_REFRESH: usize = 1001;
const ID_EXIT: usize = 1002;
const ID_STARTUP: usize = 1005;
const ID_PROBE: usize = 1006;
const ID_STYLE: usize = 1007;
const ID_ALERTS: usize = 1008;
const ID_DIAG: usize = 1009;
const ID_PIN: usize = 1010;
const ID_DEVICE_BASE: usize = 2000;
const ALERT_AT: u8 = 15;
/// depois disso (3 ciclos ≈ 30 min) o valor antigo deixa de ser mostrado
const MAX_STALE_CYCLES: u8 = 3;
const ALERT_REARM: u8 = 20;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Kind {
    Headset,
    Mouse,
    Keyboard,
    Combo,
    Gamepad,
    Phone,
    #[default]
    Other,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Headset => "Headset",
            Kind::Mouse => "Mouse",
            Kind::Keyboard => "Keyboard",
            Kind::Combo => "Mouse+Kbd",
            Kind::Gamepad => "Gamepad",
            Kind::Phone => "Phone",
            Kind::Other => "Device",
        }
    }
}

#[derive(Clone, Default)]
pub struct Device {
    pub id: u32,
    pub name: String,
    pub pct: Option<u8>,
    pub charging: bool,
    pub kind: Kind,
    /// valor antigo reaproveitado porque a leitura atual falhou
    pub stale: bool,
    /// conexões ativas do dispositivo conhecido ("cabo", "2,4 GHz", "Bluetooth"), separadas por vírgula
    pub detail: String,
    /// dispositivo da tabela `known` (um ícone para todas as conexões; guarda o último valor)
    pub known: bool,
    /// quando o valor mostrado foi lido (unix), se for um valor guardado
    pub as_of: Option<u64>,
}

pub fn fnv(b: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &x in b {
        h = (h ^ x as u32).wrapping_mul(0x0100_0193);
    }
    h
}

struct Entry {
    dev: Device,
    icon: HICON,
    spec: icon::Spec,
    /// leituras seguidas em que o valor mostrado é antigo
    stale_cycles: u8,
    /// IsPromoted já gravado para este ícone
    pinned: bool,
}

#[derive(Default)]
struct App {
    hwnd: HWND,
    entries: Vec<Entry>,
    fake: Option<u8>,
    cfg: config::Config,
    /// (id, nome, pct) de tudo que foi visto na última leitura, para o menu
    all: Vec<(u32, String, Option<u8>)>,
    menu_ids: Vec<u32>,
    alerted: Vec<u32>,
    last_summary: String,
    pin_tries: u8,
}

thread_local! { static APP: RefCell<App> = RefCell::new(App::default()); }

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn pct_text(d: &Device) -> String {
    match (d.pct, d.charging, d.stale) {
        (Some(p), true, _) => format!("{p}% (carregando)"),
        (Some(p), false, true) => match d.as_of {
            Some(t) => format!("{p}% (último valor, {})", last::age_text(last::now().saturating_sub(t))),
            None => format!("{p}% (último valor)"),
        },
        (Some(p), false, false) => format!("{p}%"),
        (None, _, _) => "bateria indisponível".into(),
    }
}

fn tip(d: &Device) -> String {
    if d.detail.is_empty() { format!("{} — {}", d.name, pct_text(d)) } else { format!("{} — {} [{}]", d.name, pct_text(d), d.detail) }
}

/// Junta registros do mesmo dispositivo (mesmo id) vindos de fontes diferentes: fica o valor de bateria
/// que existir e a lista de conexões ativas.
fn merge(v: Vec<Device>) -> Vec<Device> {
    let mut out: Vec<Device> = Vec::new();
    for d in v {
        if let Some(e) = out.iter_mut().find(|e| e.id == d.id) {
            if e.pct.is_none() {
                e.pct = d.pct;
                e.charging = d.charging;
                e.stale = d.stale;
            }
            e.known |= d.known;
            for part in d.detail.split(", ").filter(|p| !p.is_empty()) {
                if !e.detail.split(", ").any(|x| x == part) {
                    if !e.detail.is_empty() {
                        e.detail.push_str(", ");
                    }
                    e.detail.push_str(part);
                }
            }
        } else {
            out.push(d);
        }
    }
    out
}

/// Dispositivos conhecidos: guarda o valor ao vivo e, sem leitura (cabo/2,4 GHz), usa o último guardado.
fn persist(v: &mut [Device]) {
    for d in v.iter_mut().filter(|d| d.known) {
        match d.pct {
            Some(p) if !d.stale => {
                last::save(d.id, p);
                d.as_of = None;
            }
            _ => {
                if let Some((p, t)) = last::load(d.id) {
                    d.pct = Some(p);
                    d.stale = true;
                    d.as_of = Some(t);
                }
            }
        }
    }
}

fn nid(hwnd: HWND, id: u32) -> NOTIFYICONDATAW {
    let mut n: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    n.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    n.hWnd = hwnd;
    n.uID = id;
    n
}

fn copy_w(dst: &mut [u16], s: &str) {
    for (i, c) in s.encode_utf16().take(dst.len() - 1).enumerate() {
        dst[i] = c;
    }
}

fn read_all(fake: Option<u8>) -> Vec<Device> {
    let mut v = Vec::new();
    if let Some(p) = fake {
        let mk = |n: &str, pct: Option<u8>, k: Kind| Device { id: fnv(n.as_bytes()), name: n.into(), pct, kind: k, ..Default::default() };
        v.push(mk("Teste headset", Some(p), Kind::Headset));
        v.push(mk("Teste mouse", Some(80), Kind::Mouse));
        v.push(mk("Teste sem bateria", None, Kind::Combo));
    } else {
        bt::read(&mut v);
        hid::read(&mut v);
        if let Some(p) = phone::cached() {
            v.push(p);
        }
    }
    let mut v = merge(v);
    persist(&mut v);
    v.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    v
}

fn balloon(hwnd: HWND, d: &Device) {
    let mut n = nid(hwnd, d.id);
    n.uFlags = NIF_INFO;
    n.dwInfoFlags = NIIF_WARNING;
    copy_w(&mut n.szInfoTitle, "Bateria baixa");
    copy_w(&mut n.szInfo, &format!("{}: {}%", d.name, d.pct.unwrap_or(0)));
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &n) };
}

/// `kick`: também dispara a leitura do celular (ADB) em segundo plano.
fn refresh(kick: bool) {
    if kick {
        let (hwnd, fake) = APP.with(|a| {
            let a = a.borrow();
            (a.hwnd, a.fake.is_some())
        });
        if !fake {
            phone::kick(hwnd, WM_PHONE);
        }
    }
    APP.with(|a| {
        let mut app = a.borrow_mut();
        let mut devs = read_all(app.fake);
        let hwnd = app.hwnd;
        // leitura falhou agora, mas havia valor: mantém o último, marcado como antigo
        for d in devs.iter_mut() {
            if d.pct.is_none()
                && let Some(e) = app.entries.iter().find(|e| e.dev.id == d.id)
                && e.dev.pct.is_some()
                && e.stale_cycles < MAX_STALE_CYCLES
            {
                d.pct = e.dev.pct;
                d.stale = true;
            }
        }
        app.all = devs.iter().map(|d| (d.id, d.name.clone(), d.pct)).collect();
        let hidden: Vec<u32> = app.cfg.hidden.iter().map(|h| h.0).collect();
        devs.retain(|d| !hidden.contains(&d.id));

        let mut summary = String::new();
        for d in &devs {
            let _ = write!(summary, "{}={}; ", d.name, pct_text(d));
        }
        if summary != app.last_summary {
            diag::log(&format!("dispositivos: {summary}"));
            app.last_summary = summary;
        }

        let base = if icon::taskbar_is_light() { 0x0000_0000 } else { 0x00FF_FFFF };
        let style = app.cfg.style;
        let alerts = app.cfg.alerts;

        // remove quem sumiu
        app.entries.retain(|e| {
            if devs.iter().any(|d| d.id == e.dev.id) {
                true
            } else {
                unsafe {
                    Shell_NotifyIconW(NIM_DELETE, &nid(hwnd, e.dev.id));
                    DestroyIcon(e.icon);
                }
                false
            }
        });

        for d in devs {
            let spec = icon::Spec {
                style,
                kind: d.kind,
                pct: d.pct,
                charging: d.charging,
                rgb: if matches!(d.pct, Some(p) if p < 20) { icon::RED } else { base },
                dim: d.stale,
            };
            let mut n = nid(hwnd, d.id);
            n.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
            n.uCallbackMessage = WM_TRAY;
            copy_w(&mut n.szTip, &tip(&d));
            let id = d.id;
            let (pct, charging, stale) = (d.pct, d.charging, d.stale);
            if let Some(e) = app.entries.iter_mut().find(|e| e.dev.id == d.id) {
                if e.spec != spec {
                    let old = e.icon;
                    let ni = icon::make_icon(&spec);
                    if !ni.is_null() {
                        e.icon = ni;
                        e.spec = spec;
                        unsafe { DestroyIcon(old) };
                    }
                }
                n.hIcon = e.icon;
                unsafe { Shell_NotifyIconW(NIM_MODIFY, &n) };
                e.stale_cycles = if d.stale { e.stale_cycles + 1 } else { 0 };
                e.dev = d;
            } else {
                let ic = icon::make_icon(&spec);
                if ic.is_null() {
                    continue;
                }
                n.hIcon = ic;
                // só registra se o shell aceitou; senão tenta de novo no próximo ciclo
                if unsafe { Shell_NotifyIconW(NIM_ADD, &n) } != 0 {
                    app.entries.push(Entry { dev: d, icon: ic, spec, stale_cycles: 0, pinned: false });
                } else {
                    unsafe { DestroyIcon(ic) };
                }
            }
            // alerta de bateria baixa (uma vez por descarga; rearma ao carregar/subir)
            if let Some(p) = pct {
                if p > ALERT_REARM || charging {
                    app.alerted.retain(|x| *x != id);
                } else if alerts && !stale && p <= ALERT_AT && !app.alerted.contains(&id) {
                    app.alerted.push(id);
                    if let Some(e) = app.entries.iter().find(|e| e.dev.id == id) {
                        balloon(hwnd, &e.dev);
                    }
                }
            }
        }
        // fixar na barra: o Windows cria a entrada no registro um instante depois do NIM_ADD
        if app.cfg.pin && app.entries.iter().any(|e| !e.pinned) {
            app.pin_tries = 0;
            unsafe { SetTimer(hwnd, TIMER_PIN, PIN_DELAY_MS, None) };
        }
        unsafe { K32EmptyWorkingSet(GetCurrentProcess()) };
    });
}

/// Grava IsPromoted=1 nos ícones ainda não fixados; tenta de novo se o Windows ainda não criou a entrada.
fn pin_pass(hwnd: HWND) {
    APP.with(|a| {
        let mut app = a.borrow_mut();
        let todo: Vec<u32> = app.entries.iter().filter(|e| !e.pinned).map(|e| e.dev.id).collect();
        if todo.is_empty() || !app.cfg.pin {
            unsafe { KillTimer(hwnd, TIMER_PIN) };
            return;
        }
        let done = pin::set_for(&todo, 1);
        for e in app.entries.iter_mut() {
            if done.contains(&e.dev.id) {
                e.pinned = true;
            }
        }
        app.pin_tries += 1;
        if app.entries.iter().all(|e| e.pinned) || app.pin_tries >= PIN_MAX_TRIES {
            unsafe { KillTimer(hwnd, TIMER_PIN) };
        }
    });
}

/// Recria todos os ícones (explorer reiniciou).
fn rebuild() {
    APP.with(|a| {
        let mut app = a.borrow_mut();
        let hwnd = app.hwnd;
        for e in app.entries.drain(..) {
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &nid(hwnd, e.dev.id));
                DestroyIcon(e.icon);
            }
        }
    });
    refresh(true);
}

fn style_label(s: u8) -> &'static str {
    match s {
        1 => "Número",
        2 => "Tipo + barra",
        _ => "Bateria",
    }
}

unsafe fn context_menu(hwnd: HWND) {
    unsafe {
        let m = CreatePopupMenu();
        // lista de dispositivos (visíveis marcados; ocultos desmarcados)
        let (items, style, alerts, pin) = APP.with(|a| {
            let mut a = a.borrow_mut();
            let mut items: Vec<(u32, String, bool)> = Vec::new();
            for (id, n, p) in &a.all {
                let hidden = a.cfg.hidden.iter().any(|h| h.0 == *id);
                let label = match p {
                    Some(p) => format!("{n} — {p}%"),
                    None => format!("{n} — ?"),
                };
                items.push((*id, label, !hidden));
            }
            for (id, n) in &a.cfg.hidden {
                if !items.iter().any(|i| i.0 == *id) {
                    items.push((*id, if n.is_empty() { "(dispositivo oculto)".into() } else { n.clone() }, false));
                }
            }
            a.menu_ids = items.iter().map(|i| i.0).collect();
            (items, a.cfg.style, a.cfg.alerts, a.cfg.pin)
        });
        for (i, (_, label, shown)) in items.iter().enumerate() {
            let chk = if *shown { MF_CHECKED } else { MF_UNCHECKED };
            AppendMenuW(m, MF_STRING | chk, ID_DEVICE_BASE + i, wide(&label.replace('&', "&&")).as_ptr());
        }
        if !items.is_empty() {
            AppendMenuW(m, MF_SEPARATOR, 0, null());
        }
        AppendMenuW(m, MF_STRING, ID_REFRESH, wide("Atualizar agora").as_ptr());
        AppendMenuW(m, MF_STRING, ID_STYLE, wide(&format!("Estilo do ícone: {} (alternar)", style_label(style))).as_ptr());
        let ca = if alerts { MF_CHECKED } else { MF_UNCHECKED };
        AppendMenuW(m, MF_STRING | ca, ID_ALERTS, wide("Alerta de bateria baixa (15%)").as_ptr());
        let cp = if pin { MF_CHECKED } else { MF_UNCHECKED };
        AppendMenuW(m, MF_STRING | cp, ID_PIN, wide("Fixar ícones na barra").as_ptr());
        let cs = if startup::is_enabled() { MF_CHECKED } else { MF_UNCHECKED };
        AppendMenuW(m, MF_STRING | cs, ID_STARTUP, wide("Iniciar com o Windows").as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        AppendMenuW(m, MF_STRING, ID_PROBE, wide("Sondar dispositivos USB (60 s)").as_ptr());
        AppendMenuW(m, MF_STRING, ID_DIAG, wide("Copiar diagnóstico").as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        AppendMenuW(m, MF_STRING, ID_EXIT, wide("Sair").as_ptr());
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        TrackPopupMenu(m, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(m);
    }
}

unsafe fn copy_to_clipboard(hwnd: HWND, s: &str) {
    unsafe {
        let w = wide(&s.replace('\n', "\r\n"));
        let bytes = w.len() * 2;
        if OpenClipboard(hwnd) == 0 {
            return;
        }
        EmptyClipboard();
        let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if !hmem.is_null() {
            let p = GlobalLock(hmem) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(hmem);
                if SetClipboardData(13, hmem as HANDLE).is_null() {
                    GlobalFree(hmem); // CF_UNICODETEXT recusado: o bloco continua nosso
                }
            } else {
                GlobalFree(hmem);
            }
        }
        CloseClipboard();
    }
}

fn toggle_hidden(index: usize) {
    APP.with(|a| {
        let mut a = a.borrow_mut();
        let Some(&id) = a.menu_ids.get(index) else { return };
        if let Some(p) = a.cfg.hidden.iter().position(|h| h.0 == id) {
            a.cfg.hidden.remove(p);
        } else {
            let name = a.all.iter().find(|x| x.0 == id).map(|x| x.1.clone()).unwrap_or_default();
            a.cfg.hidden.push((id, name));
        }
        config::save(&a.cfg);
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        static mut TASKBAR_CREATED: u32 = 0;
        match msg {
            WM_CREATE => {
                TASKBAR_CREATED = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
                0
            }
            WM_TIMER => {
                if wp == TIMER_PIN {
                    pin_pass(hwnd);
                    return 0;
                }
                if wp == TIMER_DEBOUNCE {
                    KillTimer(hwnd, TIMER_DEBOUNCE);
                }
                refresh(true);
                0
            }
            WM_VENDOR => {
                refresh(false);
                0
            }
            WM_PHONE => {
                refresh(false);
                0
            }
            WM_POWERBROADCAST => {
                // PBT_APMRESUMESUSPEND = 0x7, PBT_APMRESUMEAUTOMATIC = 0x12: dispositivos voltam alguns segundos depois
                if wp == 0x7 || wp == 0x12 {
                    SetTimer(hwnd, TIMER_DEBOUNCE, RESUME_MS, None);
                }
                1
            }
            WM_DEVICECHANGE => {
                // DBT_DEVNODES_CHANGED = 0x0007: junta rajadas de eventos
                if wp == 0x0007 {
                    SetTimer(hwnd, TIMER_DEBOUNCE, DEBOUNCE_MS, None);
                }
                1
            }
            WM_SETTINGCHANGE => {
                // tema claro/escuro: só reagenda; refresh() recalcula a cor sem recriar ícones
                SetTimer(hwnd, TIMER_DEBOUNCE, DEBOUNCE_MS, None);
                0
            }
            WM_TRAY => {
                if (lp as u32) == WM_RBUTTONUP {
                    context_menu(hwnd);
                }
                0
            }
            WM_COMMAND => {
                let id = wp & 0xFFFF;
                match id {
                    ID_REFRESH => refresh(true),
                    ID_STYLE => {
                        APP.with(|a| {
                            let mut a = a.borrow_mut();
                            a.cfg.style = (a.cfg.style + 1) % 3;
                            config::save(&a.cfg);
                        });
                        refresh(false);
                    }
                    ID_ALERTS => {
                        APP.with(|a| {
                            let mut a = a.borrow_mut();
                            a.cfg.alerts = !a.cfg.alerts;
                            a.alerted.clear();
                            config::save(&a.cfg);
                        });
                        refresh(false);
                    }
                    ID_PIN => {
                        let on = APP.with(|a| {
                            let mut a = a.borrow_mut();
                            a.cfg.pin = !a.cfg.pin;
                            config::save(&a.cfg);
                            a.cfg.pin
                        });
                        if on {
                            APP.with(|a| a.borrow_mut().entries.iter_mut().for_each(|e| e.pinned = false));
                            refresh(false);
                        } else {
                            let ids: Vec<u32> = APP.with(|a| a.borrow().entries.iter().map(|e| e.dev.id).collect());
                            pin::set_for(&ids, 0);
                            APP.with(|a| a.borrow_mut().entries.iter_mut().for_each(|e| e.pinned = false));
                        }
                    }
                    ID_STARTUP => startup::set(!startup::is_enabled()),
                    ID_PROBE => {
                        if let Ok(exe) = std::env::current_exe() {
                            let _ = std::process::Command::new(exe).args(["--probe", "60"]).spawn();
                        }
                    }
                    ID_DIAG => copy_to_clipboard(hwnd, &diag::tail(200)),
                    ID_EXIT => {
                        DestroyWindow(hwnd);
                    }
                    i if i >= ID_DEVICE_BASE => {
                        toggle_hidden(i - ID_DEVICE_BASE);
                        refresh(false);
                    }
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                APP.with(|a| {
                    for e in a.borrow_mut().entries.drain(..) {
                        Shell_NotifyIconW(NIM_DELETE, &nid(hwnd, e.dev.id));
                        DestroyIcon(e.icon);
                    }
                });
                PostQuitMessage(0);
                0
            }
            m if m == TASKBAR_CREATED && m != 0 => {
                rebuild();
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Saída para console mesmo sendo app de janela: stdout se herdado, senão o console do processo pai.
fn print_out(s: &str) {
    use std::io::Write;
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        if !h.is_null() && h != INVALID_HANDLE_VALUE {
            let _ = std::io::stdout().write_all(s.as_bytes());
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0
            && let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("CONOUT$")
        {
            let _ = f.write_all(s.as_bytes());
        }
    }
}

/// `--once`: imprime os dispositivos detectados e sai (útil para depurar sem a bandeja).
fn once(fake: Option<u8>) {
    let mut v = read_all(fake);
    if fake.is_none() {
        if let Some(p) = phone::fetch_blocking() {
            v.push(p);
        }
        v.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    }
    let mut s = String::new();
    for d in &v {
        let _ = writeln!(
            s,
            "{:<10} {:<38} {}{}",
            d.kind.label(),
            d.name,
            pct_text(d),
            if d.detail.is_empty() { String::new() } else { format!(" [{}]", d.detail) }
        );
    }
    if v.is_empty() {
        s.push_str("nenhum dispositivo detectado\n");
    }
    print_out(&s);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut fake = None;
    if let Some(i) = args.iter().position(|a| a == "--fake") {
        fake = Some(args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(73));
    }
    if args.iter().any(|a| a == "--version") {
        print_out(&format!("taskbar-battery {}\n", env!("CARGO_PKG_VERSION")));
        return;
    }
    if args.iter().any(|a| a == "--once") {
        once(fake);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--probe") {
        let secs = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(60);
        probe::run(secs);
        return;
    }
    unsafe {
        // instância única
        let mx = CreateMutexW(null(), 0, wide("Local\\TaskbarBatteryMutex").as_ptr());
        if mx.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        // ícones nítidos em telas com escala > 100 %
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hinst = GetModuleHandleW(null());
        let class = wide("TaskbarBatteryWnd");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        // janela de topo oculta (message-only não recebe TaskbarCreated nem WM_DEVICECHANGE)
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW, class.as_ptr(), class.as_ptr(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), hinst, null());
        APP.with(|a| {
            let mut app = a.borrow_mut();
            app.hwnd = hwnd;
            app.fake = fake;
            app.cfg = config::load();
        });
        vendor::init(hwnd, WM_VENDOR);
        diag::log("início");
        SetTimer(hwnd, TIMER_POLL, POLL_MS, None);
        refresh(true);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
