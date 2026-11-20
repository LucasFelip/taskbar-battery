//! Desenha o ícone de bandeja em DIB 32 bpp: o texto/glifo é renderizado branco sobre preto e a
//! luminância vira alfa; a cor final vem do tema (ou vermelho em bateria baixa).
//! Estilos: 0 = glifo de bateria (Segoe Fluent Icons), 1 = número, 2 = glifo do tipo + barra de nível.
use crate::Kind;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const RED: u32 = 0x00E8_1123; // 0x00RRGGBB

/// Código do glifo de bateria. Validado visualmente contra a fonte instalada:
/// E850..E859 = 0..90 %, E83F = 100 %, E85A..E862 = carregando 0..80 %, E83E = carregando cheio.
pub fn glyph_for(pct: Option<u8>, charging: bool) -> u16 {
    let Some(p) = pct else { return 0xE850 };
    let lvl = ((p as u16 + 5) / 10).min(10);
    match (charging, lvl) {
        (false, 10) => 0xE83F,
        (false, l) => 0xE850 + l,
        (true, 9..=10) => 0xE83E,
        (true, l) => 0xE85A + l,
    }
}

/// Glifo do tipo de dispositivo (Segoe Fluent Icons, conferido na fonte instalada).
pub fn kind_glyph(k: Kind) -> u16 {
    match k {
        Kind::Headset => 0xE7F6,
        Kind::Mouse => 0xE962,
        Kind::Keyboard => 0xE765,
        Kind::Combo => 0xE961,
        Kind::Gamepad => 0xE7FC,
        Kind::Phone => 0xE8EA,
        Kind::Other => 0xE772,
    }
}

/// true quando a barra de tarefas usa o tema claro (glifo escuro).
pub fn taskbar_is_light() -> bool {
    let mut v: u32 = 0;
    let mut n: u32 = 4;
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0".encode_utf16().collect();
    let val: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, null_mut(), &mut v as *mut u32 as *mut _, &mut n) };
    r == 0 && v != 0
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spec {
    pub style: u8,
    pub kind: Kind,
    pub pct: Option<u8>,
    pub charging: bool,
    pub rgb: u32,
    pub dim: bool,
}

unsafe fn font(hdc: HDC, name: &str, h: i32, weight: u32) -> (HGDIOBJ, HGDIOBJ) {
    let face: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let f = unsafe { CreateFontW(-h, 0, 0, 0, weight as i32, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, ANTIALIASED_QUALITY as u32, 0, face.as_ptr()) };
    (f as HGDIOBJ, unsafe { SelectObject(hdc, f as _) })
}

unsafe fn text(hdc: HDC, s: &[u16], rc: &mut RECT) {
    unsafe { DrawTextW(hdc, s.as_ptr(), s.len() as i32, rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX) };
}

/// Cria um HICON 32 bpp.
pub fn make_icon(sp: &Spec) -> HICON {
    unsafe {
        let s = GetSystemMetrics(SM_CXSMICON).max(16);
        let hdc = CreateCompatibleDC(null_mut());
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = s;
        bi.bmiHeader.biHeight = -s; // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        let mut bits: *mut core::ffi::c_void = null_mut();
        let dib = CreateDIBSection(hdc, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if dib.is_null() || bits.is_null() {
            DeleteDC(hdc);
            return null_mut();
        }
        let old = SelectObject(hdc, dib as _);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, 0x00FF_FFFF);
        let mut full = RECT { left: 0, top: 0, right: s, bottom: s };

        match sp.style {
            1 => {
                // número
                let t: Vec<u16> = match sp.pct {
                    Some(p) => p.to_string().encode_utf16().collect(),
                    None => vec![b'?' as u16],
                };
                let h = if t.len() >= 3 { s * 55 / 100 } else { s * 82 / 100 };
                let (f, o) = font(hdc, "Segoe UI", h, FW_BOLD);
                text(hdc, &t, &mut full);
                SelectObject(hdc, o);
                DeleteObject(f);
            }
            2 => {
                // glifo do tipo na parte de cima + barra de nível embaixo
                let bar_h = (s / 6).max(2);
                let mut top = RECT { left: 0, top: 0, right: s, bottom: s - bar_h - 1 };
                let (f, o) = font(hdc, "Segoe Fluent Icons", s * 78 / 100, FW_NORMAL);
                text(hdc, &[kind_glyph(sp.kind)], &mut top);
                SelectObject(hdc, o);
                DeleteObject(f);
                let track = CreateSolidBrush(0x0055_5555);
                let fill = CreateSolidBrush(0x00FF_FFFF);
                let tr = RECT { left: 1, top: s - bar_h, right: s - 1, bottom: s };
                FillRect(hdc, &tr, track);
                if let Some(p) = sp.pct {
                    let w = (s - 2) * p as i32 / 100;
                    let fr = RECT { left: 1, top: s - bar_h, right: 1 + w.max(1), bottom: s };
                    FillRect(hdc, &fr, fill);
                }
                DeleteObject(track);
                DeleteObject(fill);
                if sp.pct.is_none() {
                    let q = [b'?' as u16];
                    let (f2, o2) = font(hdc, "Segoe UI", s * 45 / 100, FW_BOLD);
                    let mut mid = RECT { left: 0, top: 0, right: s, bottom: s - bar_h - 1 };
                    text(hdc, &q, &mut mid);
                    SelectObject(hdc, o2);
                    DeleteObject(f2);
                }
            }
            _ => {
                let (f, o) = font(hdc, "Segoe Fluent Icons", s, FW_NORMAL);
                text(hdc, &[glyph_for(sp.pct, sp.charging)], &mut full);
                SelectObject(hdc, o);
                DeleteObject(f);
                if sp.pct.is_none() {
                    let q = [b'?' as u16];
                    let (f2, o2) = font(hdc, "Segoe UI", s * 6 / 10, FW_BOLD);
                    text(hdc, &q, &mut full);
                    SelectObject(hdc, o2);
                    DeleteObject(f2);
                }
            }
        }
        GdiFlush();

        let (r, gc, b) = ((sp.rgb >> 16) & 0xFF, (sp.rgb >> 8) & 0xFF, sp.rgb & 0xFF);
        let scale = if sp.dim { 55 } else { 100 };
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (s * s) as usize);
        for p in px.iter_mut() {
            let a0 = (*p & 0xFF).max((*p >> 8) & 0xFF).max((*p >> 16) & 0xFF);
            let a = a0 * scale / 100;
            *p = (a << 24) | ((r * a / 255) << 16) | ((gc * a / 255) << 8) | (b * a / 255); // pré-multiplicado
        }
        let mask = CreateBitmap(s, s, 1, 1, null());
        let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: dib };
        let icon = CreateIconIndirect(&ii);

        SelectObject(hdc, old);
        DeleteObject(mask as _);
        DeleteObject(dib as _);
        DeleteDC(hdc);
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_levels() {
        assert_eq!(glyph_for(None, false), 0xE850);
        assert_eq!(glyph_for(Some(0), false), 0xE850);
        assert_eq!(glyph_for(Some(50), false), 0xE855);
        assert_eq!(glyph_for(Some(100), false), 0xE83F);
        assert_eq!(glyph_for(Some(100), true), 0xE83E);
        assert_eq!(glyph_for(Some(30), true), 0xE85D);
    }
}
