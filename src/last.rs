//! Último valor de bateria conhecido por dispositivo (em %LOCALAPPDATA%\taskbar-battery\last-known.ini).
//! Usado quando o dispositivo está num modo que não informa bateria (por exemplo o teclado por cabo ou
//! 2,4 GHz) mas já informou pelo Bluetooth. Formato: `AABBCCDD=<pct>,<unix>` por linha.
use std::path::PathBuf;

fn path() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    p.push("taskbar-battery");
    let _ = std::fs::create_dir_all(&p);
    p.push("last-known.ini");
    Some(p)
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn parse(s: &str) -> Vec<(u32, u8, u64)> {
    s.lines()
        .filter_map(|l| {
            let (id, rest) = l.trim().split_once('=')?;
            let (p, t) = rest.split_once(',')?;
            let pct: u8 = p.trim().parse().ok()?;
            (pct <= 100).then_some((u32::from_str_radix(id.trim(), 16).ok()?, pct, t.trim().parse().ok()?))
        })
        .collect()
}

fn render(v: &[(u32, u8, u64)]) -> String {
    v.iter().map(|(i, p, t)| format!("{i:08X}={p},{t}\n")).collect()
}

fn load_all() -> Vec<(u32, u8, u64)> {
    path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| parse(&s)).unwrap_or_default()
}

pub fn load(id: u32) -> Option<(u8, u64)> {
    load_all().into_iter().find(|e| e.0 == id).map(|e| (e.1, e.2))
}

/// Grava o valor (só escreve se mudou).
pub fn save(id: u32, pct: u8) {
    let mut all = load_all();
    match all.iter_mut().find(|e| e.0 == id) {
        Some(e) if e.1 == pct => return,
        Some(e) => {
            e.1 = pct;
            e.2 = now();
        }
        None => all.push((id, pct, now())),
    }
    if let Some(p) = path() {
        let _ = std::fs::write(p, render(&all));
    }
}

/// "há 5 min", "há 3 h", "há 2 dias".
pub fn age_text(secs: u64) -> String {
    match secs {
        0..=59 => "agora há pouco".into(),
        60..=3599 => format!("há {} min", secs / 60),
        3600..=172_799 => format!("há {} h", secs / 3600),
        _ => format!("há {} dias", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let v = vec![(0xAB, 34, 1_700_000_000), (1, 100, 5)];
        assert_eq!(parse(&render(&v)), v);
        assert!(parse("lixo\nZZ=1,2\nAB=101,3").is_empty());
    }

    #[test]
    fn ages() {
        assert_eq!(age_text(30), "agora há pouco");
        assert_eq!(age_text(600), "há 10 min");
        assert_eq!(age_text(7200), "há 2 h");
        assert_eq!(age_text(3 * 86_400), "há 3 dias");
    }
}
