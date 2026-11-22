//! When the compositor panics the session dies and the terminal it printed to is gone. So a panic hook writes a report
//! to `$XDG_STATE_HOME/finedesktop/crash-<time>.txt` (message, place, backtrace, the last log lines), and the next start
//! posts a notification pointing at it. `ctl crash` lists the reports. Reports are renamed to `.seen`, never deleted.

use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    let d = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?
        .join("finedesktop");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

pub fn report_text(stamp: &str, message: &str, place: &str, backtrace: &str, log: &str) -> String {
    format!("finedesktop crashed at {stamp}\n\n{message}\n  at {place}\n\nbacktrace:\n{backtrace}\n\nlast log lines:\n{log}\n")
}

/// Write a report on panic, then run the default hook (stderr message) as before.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let text = report_text(&crate::components::widgets::now("%Y-%m-%d %H:%M:%S"), &message, &place, &std::backtrace::Backtrace::force_capture().to_string(), &super::log::tail_try(100));
        if let Some(d) = dir() {
            let _ = std::fs::write(d.join(format!("crash-{}.txt", crate::components::widgets::now("%Y%m%d-%H%M%S"))), text);
        }
        default(info);
    }));
}

pub fn reports() -> Vec<PathBuf> {
    let Some(d) = dir() else { return Vec::new() };
    let mut v: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("crash-") && n.to_string_lossy().ends_with(".txt"))).collect();
    v.sort();
    v
}

/// The newest report nobody has been told about yet; it is marked `.seen`.
pub fn take_unseen() -> Option<PathBuf> {
    let last = reports().pop()?;
    std::fs::rename(&last, last.with_extension("txt.seen")).ok()?;
    Some(last)
}

impl crate::Desktop {
    pub fn crash_cmd(&mut self, _arg: &str) -> String {
        let r = reports();
        let seen: Vec<PathBuf> = dir().map(|d| std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".txt.seen")).collect()).unwrap_or_default();
        if r.is_empty() && seen.is_empty() {
            return "(no crash reports)".into();
        }
        r.iter().map(|p| format!("new  {}", p.display())).chain(seen.iter().map(|p| format!("seen {}", p.display()))).collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn report_has_the_parts() {
        let t = super::report_text("T", "boom", "a.rs:1", "bt", "log");
        assert!(t.contains("boom") && t.contains("a.rs:1") && t.contains("last log lines:\nlog"));
    }
}
