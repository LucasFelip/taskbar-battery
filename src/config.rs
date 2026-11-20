//! Configuração mínima em %APPDATA%\taskbar-battery\config.ini (chave=valor, uma por linha):
//!   hidden=AABBCCDD:Nome,11223344:Outro   dispositivos ocultos (id hex + nome para o menu)
//!   style=0|1|2                           0 bateria, 1 número, 2 dispositivo + barra
//!   alerts=0|1                            aviso de bateria baixa (padrão desligado)
//!   pin=0|1                               fixar ícones na barra (padrão ligado)
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub hidden: Vec<(u32, String)>,
    pub style: u8,
    pub alerts: bool,
    /// fixa os ícones na barra (IsPromoted); padrão ligado
    pub pin: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { hidden: Vec::new(), style: 0, alerts: false, pin: true }
    }
}

fn path() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var_os("APPDATA")?);
    p.push("taskbar-battery");
    p.push("config.ini");
    Some(p)
}

pub fn parse(s: &str) -> Config {
    let mut c = Config::default();
    for l in s.lines() {
        let Some((k, v)) = l.trim().split_once('=') else { continue };
        match k.trim() {
            "hidden" => {
                c.hidden = v
                    .split(',')
                    .filter_map(|x| {
                        let (id, name) = x.split_once(':').unwrap_or((x, ""));
                        Some((u32::from_str_radix(id.trim(), 16).ok()?, name.trim().to_string()))
                    })
                    .collect()
            }
            "style" => c.style = v.trim().parse::<u8>().unwrap_or(0).min(2),
            "alerts" => c.alerts = v.trim() == "1",
            "pin" => c.pin = v.trim() != "0",
            _ => {}
        }
    }
    c
}

pub fn render(c: &Config) -> String {
    let h: Vec<String> = c.hidden.iter().map(|(i, n)| format!("{i:08X}:{}", n.replace([',', ':', '=', '\n', '\r'], " "))).collect();
    format!("hidden={}\nstyle={}\nalerts={}\npin={}\n", h.join(","), c.style, c.alerts as u8, c.pin as u8)
}

pub fn load() -> Config {
    path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| parse(&s)).unwrap_or_default()
}

pub fn save(c: &Config) {
    let Some(p) = path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, render(c));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let c = Config { hidden: vec![(0xAB, "Mouse X".into()), (1, "A:B".into())], style: 2, alerts: true, pin: false };
        let r = parse(&render(&c));
        assert!(!r.pin);
        assert_eq!(r.style, 2);
        assert!(r.alerts);
        assert_eq!(r.hidden[0], (0xAB, "Mouse X".to_string()));
        assert_eq!(r.hidden[1].0, 1);
    }

    #[test]
    fn defaults_and_garbage() {
        let c = parse("lixo\nstyle=9\nhidden=zz:x,10:ok\n");
        assert_eq!(c.style, 2);
        assert!(!c.alerts);
        assert!(c.pin);
        assert_eq!(c.hidden, vec![(0x10, "ok".to_string())]);
    }
}
