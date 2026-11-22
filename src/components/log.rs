//! Logs where agents can reach them: every tracing line goes to stderr, to a ring buffer (`ctl log [N]`) and to
//! `$XDG_STATE_HOME/finedesktop/log` (default ~/.local/state/finedesktop/log), so a DRM session is not mute.

use std::{
    collections::VecDeque,
    fs::File,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

const KEEP: usize = 500;

static RING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();

fn file() -> Option<&'static Mutex<File>> {
    FILE.get_or_init(|| {
        let dir = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?
            .join("finedesktop");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("log");
        // one old generation is enough: a log that grows forever is a bug of its own
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 4 << 20) {
            let _ = std::fs::rename(&path, dir.join("log.old"));
        }
        File::options().create(true).append(true).open(path).ok().map(Mutex::new)
    })
    .as_ref()
}

pub struct Tee;

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(f) = file() {
            if let Ok(mut f) = f.lock() {
                let _ = f.write_all(buf);
            }
        }
        if let Ok(mut r) = RING.lock() {
            for l in String::from_utf8_lossy(buf).lines() {
                if r.len() == KEEP {
                    r.pop_front();
                }
                r.push_back(l.to_string());
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The last `n` lines (all kept ones when `arg` is empty or not a number).
pub fn tail(arg: &str) -> String {
    let n = arg.trim().parse::<usize>().unwrap_or(KEEP);
    let r = RING.lock().map(|r| r.iter().skip(r.len().saturating_sub(n)).cloned().collect::<Vec<_>>()).unwrap_or_default();
    if r.is_empty() { "(no log lines)".into() } else { r.join("\n") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_returns_last_lines() {
        let mut t = Tee;
        t.write_all(b"one\ntwo\nthree\n").unwrap();
        assert!(tail("2").ends_with("two\nthree"));
    }
}
