//! Dispositivos conhecidos que aparecem por mais de uma conexão (cabo, receptor 2,4 GHz, Bluetooth)
//! com o mesmo VID/PID. Todos viram **um único ícone**, com o nome da tabela.
use crate::Kind;

pub struct Known {
    pub vid: u16,
    pub pid: u16,
    pub name: &'static str,
    pub kind: Kind,
    /// nome de produto USB do receptor 2,4 GHz (qualquer outro USB com o mesmo VID/PID é o cabo)
    pub dongle_product: &'static str,
}

pub const TABLE: &[Known] = &[Known { vid: 0x05AC, pid: 0x024F, name: "Teclado GMK67", kind: Kind::Keyboard, dongle_product: "USB Dongle" }];

pub fn find(vid: u16, pid: u16) -> Option<&'static Known> {
    TABLE.iter().find(|k| k.vid == vid && k.pid == pid)
}

/// Identificador estável do dispositivo, igual em todas as conexões.
pub fn id(k: &Known) -> u32 {
    crate::fnv(format!("known:{:04X}:{:04X}", k.vid, k.pid).as_bytes())
}

/// Rótulo da conexão USB a partir do nome de produto.
pub fn usb_mode(k: &Known, product: &str) -> &'static str {
    if product.eq_ignore_ascii_case(k.dongle_product) { "2,4 GHz" } else { "cabo" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_labels() {
        let k = find(0x05AC, 0x024F).unwrap();
        assert_eq!(k.name, "Teclado GMK67");
        assert_eq!(usb_mode(k, "USB Dongle"), "2,4 GHz");
        assert_eq!(usb_mode(k, "USB DEVICE"), "cabo");
        assert!(find(0x1234, 0x5678).is_none());
        assert_eq!(id(k), id(find(0x05AC, 0x024F).unwrap()));
    }
}
