//! The global font: which bitmap fonts the compositor can load (`.bdf` / `.psf` in the config dir's `fonts/` and in
//! `~/.local/share/fonts`) and keeping the terminal (foot) on the same one. foot reads fontconfig, which does not know every
//! bitmap file, so the terminal line is only rewritten when fontconfig really resolves the spec to the chosen file.

use std::path::{Path, PathBuf};

fn is_font(p: &Path) -> bool {
    p.extension().is_some_and(|x| x == "bdf" || x == "psf")
}

/// (label, path) of every loadable font, sorted by label.
pub fn available() -> Vec<(String, PathBuf)> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let mut dirs: Vec<PathBuf> = crate::config::config_dir().map(|d| d.join("fonts")).into_iter().collect();
    if let Some(h) = env("HOME") {
        dirs.push(PathBuf::from(env("XDG_DATA_HOME").unwrap_or(format!("{h}/.local/share"))).join("fonts"));
    }
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for d in dirs {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if is_font(&p) && !out.iter().any(|(_, q)| q == &p) {
                out.push((p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), p));
            }
        }
    }
    out.sort_by_key(|(l, _)| l.to_lowercase());
    out
}

/// `family:style=STYLE:pixelsize=N` from the text of `fc-scan --format '%{family[0]}|%{style[0]}|%{pixelsize}'`.
pub fn spec_from_scan(line: &str) -> Option<String> {
    let mut f = line.trim().split('|');
    let (fam, style, px) = (f.next()?.trim(), f.next()?.trim(), f.next()?.trim());
    if fam.is_empty() || fam.contains([':', '=', '\n']) {
        return None;
    }
    let px: f32 = px.parse().ok().filter(|p: &f32| *p >= 1.0)?;
    let style = if style.is_empty() { String::new() } else { format!(":style={style}") };
    Some(format!("{fam}{style}:pixelsize={}", px.round() as u32))
}

fn run(prog: &str, args: &[&str]) -> Option<String> {
    let o = std::process::Command::new(prog).args(args).stderr(std::process::Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The foot font spec for this file, only when fontconfig resolves it back to the same file.
pub fn foot_spec(path: &Path) -> Option<String> {
    let p = path.to_str()?;
    let spec = spec_from_scan(&run("fc-scan", &["--format", "%{family[0]}|%{style[0]}|%{pixelsize}\n", p])?)?;
    let got = run("fc-match", &["-f", "%{file}", &spec])?;
    (std::fs::canonicalize(got.trim()).ok()? == std::fs::canonicalize(path).ok()?).then_some(spec)
}

/// Replace the `font=` line of the `[main]` section of a foot.ini text; None when there is no such line.
pub fn set_foot_line(ini: &str, spec: &str) -> Option<String> {
    let (mut in_main, mut done) = (false, false);
    let lines: Vec<String> = ini
        .lines()
        .map(|l| {
            let t = l.trim();
            if t.starts_with('[') {
                in_main = t == "[main]";
            }
            if in_main && !done && t.starts_with("font=") {
                done = true;
                return format!("font={spec}");
            }
            l.to_string()
        })
        .collect();
    done.then(|| lines.join("\n") + if ini.ends_with('\n') { "\n" } else { "" })
}

/// Point foot at `path`'s font. Ok(true) written, Ok(false) foot left alone (no foot.ini, no font= line, or fontconfig
/// does not know the file).
pub fn sync_foot(path: &Path) -> std::io::Result<bool> {
    let ini = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config"))).map(|d| d.join("foot/foot.ini"));
    let Some(ini) = ini else { return Ok(false) };
    let (Ok(text), Some(spec)) = (std::fs::read_to_string(&ini), foot_spec(path)) else { return Ok(false) };
    let Some(new) = set_foot_line(&text, &spec) else { return Ok(false) };
    let tmp = ini.with_extension("tmp");
    std::fs::write(&tmp, new)?;
    std::fs::rename(tmp, ini)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_line_becomes_a_foot_spec() {
        assert_eq!(spec_from_scan("Fixed6x13|SemiCondensed|13\n").as_deref(), Some("Fixed6x13:style=SemiCondensed:pixelsize=13"));
        assert_eq!(spec_from_scan("Terminus||16").as_deref(), Some("Terminus:pixelsize=16"));
        assert_eq!(spec_from_scan("A|B|0.8"), None, "a scalable font has no pixel size to pin");
        assert_eq!(spec_from_scan("a:b|x|13"), None);
        assert_eq!(spec_from_scan(""), None);
    }

    #[test]
    fn only_the_font_line_of_main_changes() {
        let ini = "[main]\n# THEME\nfont=Old:size=9\ndpi-aware=yes\n\n[other]\nfont=keep\n";
        assert_eq!(set_foot_line(ini, "New:pixelsize=13").as_deref(), Some("[main]\n# THEME\nfont=New:pixelsize=13\ndpi-aware=yes\n\n[other]\nfont=keep\n"));
        assert_eq!(set_foot_line("[other]\nfont=x\n", "New"), None);
    }
}
