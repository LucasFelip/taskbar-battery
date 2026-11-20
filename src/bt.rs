//! Fonte Bluetooth: dispositivos pareados e CONECTADOS (nós BTHENUM\DEV_* e BTHLE\DEV_*).
//! A bateria vem da propriedade PnP DEVPKEY_Bluetooth_Battery; se ausente, o dispositivo
//! aparece com `pct = None` (ícone "?").
use crate::{Device, Kind};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Foundation::DEVPROPKEY;

const GUID_BT: windows_sys::core::GUID =
    windows_sys::core::GUID { data1: 0x104EA319, data2: 0x6EE2, data3: 0x4701, data4: [0xBD, 0x47, 0x8D, 0xDB, 0xF4, 0x25, 0xBB, 0xE5] };
const KEY_BATTERY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID_BT, pid: 2 };
const KEY_FRIENDLY: DEVPROPKEY = DEVPROPKEY {
    fmtid: windows_sys::core::GUID { data1: 0xA45C254E, data2: 0xDF1C, data3: 0x4EFD, data4: [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0] },
    pid: 14,
};
const KEY_COD: DEVPROPKEY = DEVPROPKEY {
    fmtid: windows_sys::core::GUID { data1: 0x2BD67D8B, data2: 0x8BEB, data3: 0x48D5, data4: [0x87, 0xE0, 0x6C, 0xDA, 0x34, 0x28, 0x04, 0x0A] },
    pid: 10,
};

/// Tipo a partir da Class of Device Bluetooth (ou, na falta, do nome).
fn kind_from(cod: Option<u32>, name: &str) -> Kind {
    if let Some(c) = cod {
        match (c >> 8) & 0x1F {
            4 => return Kind::Headset,
            2 => return Kind::Phone,
            5 => {
                if (c >> 2) & 0xF == 1 || (c >> 2) & 0xF == 2 {
                    return Kind::Gamepad;
                }
                if c & 0x80 != 0 {
                    return Kind::Mouse;
                }
                if c & 0x40 != 0 {
                    return Kind::Keyboard;
                }
            }
            _ => {}
        }
    }
    let n = name.to_ascii_lowercase();
    let has = |ks: &[&str]| ks.iter().any(|k| n.contains(k));
    if has(&["buds", "airpods", "headset", "headphone", "fone"]) {
        Kind::Headset
    } else if has(&["keyboard", "teclado"]) {
        Kind::Keyboard
    } else if has(&["mouse"]) {
        Kind::Mouse
    } else if has(&["controller", "gamepad", "joy-con"]) {
        Kind::Gamepad
    } else {
        Kind::Other
    }
}
/// hora (FILETIME) em que o Windows leu a bateria
const KEY_STAMP: DEVPROPKEY = DEVPROPKEY { fmtid: GUID_BT, pid: 7 };
const KEY_CONNECTED: DEVPROPKEY = DEVPROPKEY {
    fmtid: windows_sys::core::GUID { data1: 0x83DA6326, data2: 0x97A6, data3: 0x4088, data4: [0x94, 0x53, 0xA1, 0x92, 0x3F, 0x57, 0x3B, 0x29] },
    pid: 15,
};

/// VID/PID USB do dispositivo Bluetooth, lido dos nós filhos (ex.: BTHLEDEVICE\...\_DEV_VID&0205AC_PID&024F_...).
fn usb_ids(dn: u32) -> Option<(u16, u16)> {
    let mut c = 0u32;
    if unsafe { CM_Get_Child(&mut c, dn, 0) } != CR_SUCCESS {
        return None;
    }
    loop {
        let mut buf = [0u16; 260];
        if unsafe { CM_Get_Device_IDW(c, buf.as_mut_ptr(), 260, 0) } == CR_SUCCESS {
            let n = buf.iter().position(|&x| x == 0).unwrap_or(0);
            let s = String::from_utf16_lossy(&buf[..n]).to_ascii_uppercase();
            if let (Some(i), Some(j)) = (s.find("VID&02"), s.find("PID&"))
                && let (Some(v), Some(p)) = (s.get(i + 6..i + 10), s.get(j + 4..j + 8))
                && let (Ok(v), Ok(p)) = (u16::from_str_radix(v, 16), u16::from_str_radix(p, 16))
            {
                return Some((v, p));
            }
        }
        let mut sib = 0u32;
        if unsafe { CM_Get_Sibling(&mut sib, c, 0) } != CR_SUCCESS {
            return None;
        }
        c = sib;
    }
}

unsafe fn prop(dev: u32, key: &DEVPROPKEY, buf: &mut [u8]) -> Option<usize> {
    let mut ty: u32 = 0;
    let mut sz = buf.len() as u32;
    let r = unsafe { CM_Get_DevNode_PropertyW(dev, key, &mut ty, buf.as_mut_ptr(), &mut sz, 0) };
    if r == CR_SUCCESS { Some(sz as usize) } else { None }
}

pub fn read(out: &mut Vec<Device>) {
    for enumerator in ["BTHENUM\0", "BTHLE\0"] {
        let filter: Vec<u16> = enumerator.encode_utf16().collect();
        let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
        let mut len = 0u32;
        if unsafe { CM_Get_Device_ID_List_SizeW(&mut len, filter.as_ptr(), flags) } != CR_SUCCESS || len < 2 {
            continue;
        }
        let mut ids = vec![0u16; len as usize];
        if unsafe { CM_Get_Device_ID_ListW(filter.as_ptr(), ids.as_mut_ptr(), len, flags) } != CR_SUCCESS {
            continue;
        }
        for id in ids.split(|&c| c == 0).filter(|s| !s.is_empty()) {
            // só o nó do dispositivo (2º componente "DEV_<endereço>"); ignora nós de serviço {uuid}
            let s = String::from_utf16_lossy(id);
            let addr = s.split('\\').nth(1).unwrap_or("");
            if !addr.to_ascii_uppercase().starts_with("DEV_") {
                continue;
            }
            let key = crate::fnv(addr.as_bytes());
            if out.iter().any(|d| d.id == key) {
                continue;
            }
            let mut z = id.to_vec();
            z.push(0);
            let mut dn = 0u32;
            if unsafe { CM_Locate_DevNodeW(&mut dn, z.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) } != CR_SUCCESS {
                continue;
            }
            // dispositivo conhecido: mesmo id e nome em todas as conexões (cabo, 2,4 GHz, Bluetooth)
            let known = usb_ids(dn).and_then(|(v, p)| crate::known::find(v, p));
            // pareado e desligado só aparece se for conhecido (mostra o último valor que o Windows guardou)
            let mut c = [0u8; 4];
            let connected = unsafe { prop(dn, &KEY_CONNECTED, &mut c) }.is_some() && c[0] != 0;
            if !connected && known.is_none() {
                continue;
            }
            let mut b = [0u8; 4];
            let mut pct = match unsafe { prop(dn, &KEY_BATTERY, &mut b) } {
                Some(n) if n >= 1 => Some(b[0].min(100)),
                _ => None,
            };
            if pct.is_none() && connected {
                // fones/áudio: a bateria costuma estar no nó de serviço (ex.: Hands-Free) deste endereço
                pct = child_battery(&ids, &addr.to_ascii_uppercase()[4..]);
            }
            let mut nb = [0u8; 256];
            let name = match unsafe { prop(dn, &KEY_FRIENDLY, &mut nb) } {
                Some(n) if n >= 2 => {
                    let w: Vec<u16> = nb[..n].as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).take_while(|&x| x != 0).collect();
                    String::from_utf16_lossy(&w)
                }
                _ => String::from("Bluetooth"),
            };
            let mut cb = [0u8; 4];
            let cod = match unsafe { prop(dn, &KEY_COD, &mut cb) } {
                Some(4) => Some(u32::from_le_bytes(cb)),
                _ => None,
            };
            let kind = kind_from(cod, &name);
            if !connected && pct.is_none() {
                continue;
            }
            let (id, name, kind) = match known {
                Some(k) => (crate::known::id(k), k.name.to_string(), k.kind),
                None => (key, name, kind),
            };
            if out.iter().any(|d| d.id == id && d.detail.contains("Bluetooth")) {
                continue;
            }
            // desconectado: valor guardado pelo Windows, com a hora em que foi lido
            let as_of = if connected {
                None
            } else {
                let mut t = [0u8; 8];
                match unsafe { prop(dn, &KEY_STAMP, &mut t) } {
                    Some(8) => (u64::from_le_bytes(t) / 10_000_000).checked_sub(11_644_473_600),
                    _ => None,
                }
            };
            out.push(Device {
                id,
                name,
                pct,
                kind,
                known: known.is_some(),
                detail: if known.is_some() && connected { "Bluetooth".into() } else { String::new() },
                stale: !connected,
                as_of,
                ..Default::default()
            });
        }
    }
}

/// Procura a bateria nos nós de serviço `BTHENUM\{uuid}_...\...<endereço>_...` do mesmo aparelho.
/// Se houver vários, usa o de timestamp (pid 7) mais recente.
fn child_battery(ids: &[u16], addr: &str) -> Option<u8> {
    let mut best: Option<(u64, u8)> = None;
    for id in ids.split(|&c| c == 0).filter(|s| !s.is_empty()) {
        let s = String::from_utf16_lossy(id).to_ascii_uppercase();
        if !s.contains(addr) || s.split('\\').nth(1).is_some_and(|c| c.starts_with("DEV_")) {
            continue;
        }
        let mut z = id.to_vec();
        z.push(0);
        let mut dn = 0u32;
        if unsafe { CM_Locate_DevNodeW(&mut dn, z.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) } != CR_SUCCESS {
            continue;
        }
        let mut b = [0u8; 4];
        let Some(n) = (unsafe { prop(dn, &KEY_BATTERY, &mut b) }) else { continue };
        if n < 1 {
            continue;
        }
        let mut t = [0u8; 8];
        let stamp = match unsafe { prop(dn, &KEY_STAMP, &mut t) } {
            Some(8) => u64::from_le_bytes(t),
            _ => 0,
        };
        if best.is_none_or(|(s0, _)| stamp >= s0) {
            best = Some((stamp, b[0].min(100)));
        }
    }
    best.map(|(_, p)| p)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_by_class_of_device() {
        assert_eq!(kind_from(Some(0x240404), "x"), Kind::Headset); // áudio/vídeo
        assert_eq!(kind_from(Some(0x5A020C), "x"), Kind::Phone);
        assert_eq!(kind_from(Some(0x002580), "x"), Kind::Mouse); // periférico, bit apontador
        assert_eq!(kind_from(Some(0x002540), "x"), Kind::Keyboard);
        assert_eq!(kind_from(Some(0x002508), "x"), Kind::Gamepad);
    }

    #[test]
    fn kind_falls_back_to_name() {
        assert_eq!(kind_from(None, "Galaxy Buds"), Kind::Headset);
        assert_eq!(kind_from(None, "Coisa"), Kind::Other);
        assert_eq!(kind_from(None, "BT Keyboard"), Kind::Keyboard);
        assert_eq!(kind_from(None, "MX Mouse"), Kind::Mouse);
    }
}
