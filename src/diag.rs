//! Log de diagnóstico em %LOCALAPPDATA%\taskbar-battery\diagnostics.log (limitado a ~512 KB).
//! Só registra ids/nomes de dispositivo e percentuais; nunca endereços Bluetooth, serial ou caminhos.
use std::io::Write;
use std::path::PathBuf;

const MAX_BYTES: u64 = 512 * 1024;

fn path() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    p.push("taskbar-battery");
    let _ = std::fs::create_dir_all(&p);
    p.push("diagnostics.log");
    Some(p)
}

pub fn log(msg: &str) {
    let Some(p) = path() else { return };
    if std::fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        // mantém a metade final
        if let Ok(s) = std::fs::read_to_string(&p) {
            let mut cut = s.len() / 2;
            while !s.is_char_boundary(cut) {
                cut += 1;
            }
            let start = s[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
            let _ = std::fs::write(&p, &s[start..]);
        }
    }
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "[{t}] {msg}");
    }
}

/// Últimas `n` linhas, com cabeçalho de versão (para copiar e colar num relato).
pub fn tail(n: usize) -> String {
    let body = path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let lines: Vec<&str> = body.lines().collect();
    let from = lines.len().saturating_sub(n);
    format!("taskbar-battery v{}\n{}", env!("CARGO_PKG_VERSION"), lines[from..].join("\n"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn utf8_cut_does_not_panic() {
        let s = "início — indisponível\n".repeat(10);
        for n in 0..s.len() {
            let mut cut = n;
            while !s.is_char_boundary(cut) {
                cut += 1;
            }
            let _ = &s[cut..];
        }
    }
}
