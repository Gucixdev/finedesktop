//! Bar widgets. Built-in ones (cpu ram vol bat clock) are sampled by one background thread, so the compositor
//! never waits on /proc, wpctl or a slow script. Your own widgets are
//!   - `segment = NAME SECONDS COMMAND` in the config, or
//!   - an executable `widgets/NAME` in the config dir, used when NAME appears in `bar_left` / `bar_right`.
//!     An optional first comment line `# every 30` sets its refresh in seconds (default 5).
//! The first line of the output is the text; a leading `#rrggbb ` colours it.

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::config::Config;

pub const BUILTIN: [&str; 19] = [
    "tags", "layout", "|", "spk", "vol", "mic", "cpu", "gpu", "ram", "vram", "bat", "date", "clock", "disk", "disk-bar", "cpu-graph", "ram-graph", "gpu-graph",
    "vram-graph",
];

/// Samples kept for the mini graphs (one per `bar_interval`, so 30 s by default).
pub const HIST: usize = 30;

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub name: String,
    pub every: u64,
    pub cmd: String,
}

#[derive(Default)]
pub struct Shared {
    pub cpu: u32,
    pub ram: u32,
    pub bat: Option<(u32, bool)>,
    pub vol: Option<(u32, bool)>,
    pub mic: Option<(u32, bool)>,
    /// (gpu busy %, vram used %)
    pub gpu: Option<(u32, u32)>,
    /// (total, free) bytes of the `disk =` path
    pub disk: Option<(u64, u64)>,
    disk_path: String,
    /// last `HIST` samples (0-100) per graph: cpu ram gpu vram
    pub hist: HashMap<&'static str, Vec<u8>>,
    pub text: HashMap<String, String>,
    jobs: Vec<Job>,
    enabled: bool,
    interval: u64,
}

pub struct Widgets {
    pub shared: Arc<Mutex<Shared>>,
}

fn first_line(cmd: &str) -> String {
    let out = Command::new("timeout")
        .args(["2", "sh", "-c", cmd])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().chars().take(100).collect(),
        Err(_) => String::new(),
    }
}

/// Append a sample, keep the last `HIST`.
pub fn push_hist(h: &mut Vec<u8>, v: u32) {
    h.push(v.min(100) as u8);
    if h.len() > HIST {
        h.remove(0);
    }
}

/// (total, free-to-users) bytes of the filesystem holding `path`.
pub fn read_disk(path: &str) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let fr = s.f_frsize as u64;
    let total = s.f_blocks as u64 * fr;
    (total > 0).then(|| (total, s.f_bavail as u64 * fr))
}

/// 455G, 1.2T, 850M (1024-based like `df -h`): short enough for a bar.
pub fn human(b: u64) -> String {
    const U: [(&str, f64); 4] = [("T", 1099511627776.0), ("G", 1073741824.0), ("M", 1048576.0), ("K", 1024.0)];
    for (n, d) in U {
        let v = b as f64 / d;
        if v >= 1.0 {
            return if v < 10.0 { format!("{v:.1}{n}") } else { format!("{:.0}{n}", v) };
        }
    }
    format!("{b}B")
}

fn read_cpu() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    let v: Vec<u64> = s.lines().next()?.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if v.len() < 5 {
        return None;
    }
    let total: u64 = v.iter().take(8).sum();
    Some((total, v[3] + v[4]))
}

fn read_ram() -> Option<u32> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let get = |k: &str| s.lines().find(|l| l.starts_with(k))?.split_whitespace().nth(1)?.parse::<u64>().ok();
    let (total, avail) = (get("MemTotal:")?, get("MemAvailable:")?);
    (total > 0).then(|| ((total - avail.min(total)) * 100 / total) as u32)
}

fn read_bat() -> Option<(u32, bool)> {
    for e in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let p = e.path();
        if !e.file_name().to_string_lossy().starts_with("BAT") {
            continue;
        }
        let cap = std::fs::read_to_string(p.join("capacity")).ok()?.trim().parse().ok()?;
        let charging = std::fs::read_to_string(p.join("status")).map(|s| s.trim() == "Charging").unwrap_or(false);
        return Some((cap, charging));
    }
    None
}

/// "Volume: 0.50" or "Volume: 0.50 [MUTED]"
pub fn parse_vol(s: &str) -> Option<(u32, bool)> {
    let v: f32 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(((v * 100.0).round() as u32, s.contains("MUTED")))
}

/// The bar text for a sound device: value and state, always shown. `--` when there is no such device
/// (no microphone: wpctl has no default source). The bool is "dim" (muted or absent).
pub fn audio_label(name: &str, v: Option<(u32, bool)>) -> (String, bool) {
    match v {
        Some((v, false)) => (format!("{name} {v}% on"), false),
        Some((v, true)) => (format!("{name} {v}% off"), true),
        None => (format!("{name} --"), true),
    }
}

fn read_vol(target: &str) -> Option<(u32, bool)> {
    let o = Command::new("timeout").args(["1", "wpctl", "get-volume", target]).stderr(Stdio::null()).output().ok()?;
    parse_vol(&String::from_utf8_lossy(&o.stdout))
}

/// "35, 1204, 4096" -> (35, 29): nvidia-smi `utilization.gpu, memory.used, memory.total`.
pub fn parse_nvidia(s: &str) -> Option<(u32, u32)> {
    let v: Vec<u64> = s.lines().next()?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if v.len() < 3 || v[2] == 0 {
        return None;
    }
    Some((v[0].min(100) as u32, (v[1] * 100 / v[2]).min(100) as u32))
}

/// GPU load and VRAM use: nvidia-smi when there is one, else the amdgpu sysfs files.
fn read_gpu() -> Option<(u32, u32)> {
    if let Ok(o) = Command::new("timeout")
        .args(["2", "nvidia-smi", "--query-gpu=utilization.gpu,memory.used,memory.total", "--format=csv,noheader,nounits"])
        .stderr(Stdio::null())
        .output()
    {
        if let Some(r) = parse_nvidia(&String::from_utf8_lossy(&o.stdout)) {
            return Some(r);
        }
    }
    for e in std::fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let d = e.path().join("device");
        let num = |f: &str| std::fs::read_to_string(d.join(f)).ok()?.trim().parse::<u64>().ok();
        if let (Some(busy), Some(used), Some(total)) = (num("gpu_busy_percent"), num("mem_info_vram_used"), num("mem_info_vram_total")) {
            if total > 0 {
                return Some((busy.min(100) as u32, (used * 100 / total).min(100) as u32));
            }
        }
    }
    None
}

impl Widgets {
    pub fn start() -> Widgets {
        let shared = Arc::new(Mutex::new(Shared { enabled: true, interval: 1, ..Shared::default() }));
        let sh = shared.clone();
        std::thread::spawn(move || {
            let long_ago = Instant::now() - Duration::from_secs(99);
            let (mut prev, mut last_stats, mut last_vol, mut last_gpu, mut last_disk) = (read_cpu(), long_ago, long_ago, long_ago, long_ago);
            let mut last_run: HashMap<String, Instant> = HashMap::new();
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let (interval, enabled, jobs) = {
                    let g = sh.lock().unwrap();
                    (g.interval, g.enabled, g.jobs.clone())
                };
                if last_stats.elapsed() >= Duration::from_secs(interval) {
                    last_stats = Instant::now();
                    let cur = read_cpu();
                    let cpu = match (prev, cur) {
                        (Some((pt, pi)), Some((ct, ci))) if ct > pt => (100 * (ct - pt).saturating_sub(ci - pi) / (ct - pt)) as u32,
                        _ => 0,
                    };
                    prev = cur;
                    let (ram, bat) = (read_ram().unwrap_or(0), read_bat());
                    let mut g = sh.lock().unwrap();
                    g.cpu = cpu;
                    g.ram = ram;
                    g.bat = bat;
                    push_hist(g.hist.entry("cpu").or_default(), cpu);
                    push_hist(g.hist.entry("ram").or_default(), ram);
                }
                if last_vol.elapsed() >= Duration::from_secs(interval) {
                    last_vol = Instant::now();
                    let (v, m) = (read_vol("@DEFAULT_AUDIO_SINK@"), read_vol("@DEFAULT_AUDIO_SOURCE@"));
                    let mut g = sh.lock().unwrap();
                    g.vol = v;
                    g.mic = m;
                }
                if last_gpu.elapsed() >= Duration::from_secs(3.max(interval)) {
                    last_gpu = Instant::now();
                    let g = read_gpu();
                    let mut sg = sh.lock().unwrap();
                    if let Some((gp, vr)) = g {
                        push_hist(sg.hist.entry("gpu").or_default(), gp);
                        push_hist(sg.hist.entry("vram").or_default(), vr);
                    }
                    sg.gpu = g;
                }
                if last_disk.elapsed() >= Duration::from_secs(10) {
                    last_disk = Instant::now();
                    let path = sh.lock().unwrap().disk_path.clone();
                    let d = read_disk(if path.is_empty() { "/" } else { &path });
                    sh.lock().unwrap().disk = d;
                }
                if enabled {
                    for j in jobs {
                        let due = last_run.get(&j.name).is_none_or(|t| t.elapsed() >= Duration::from_secs(j.every));
                        if due {
                            last_run.insert(j.name.clone(), Instant::now());
                            let out = first_line(&j.cmd);
                            sh.lock().unwrap().text.insert(j.name, out);
                        }
                    }
                }
            }
        });
        Widgets { shared }
    }

    /// Re-read what to run from the config (segments, widget files used by the bar, enabled, interval).
    pub fn configure(&self, cfg: &Config) {
        let mut jobs: Vec<Job> = cfg.segments.iter().map(|s| Job { name: s.name.clone(), every: s.every, cmd: s.cmd.clone() }).collect();
        if let Some(dir) = crate::config::config_dir().map(|d| d.join("widgets")) {
            for name in cfg.bar_left.iter().chain(&cfg.bar_right) {
                if BUILTIN.contains(&name.as_str()) || jobs.iter().any(|j| &j.name == name) || name.contains('/') {
                    continue;
                }
                let p = dir.join(name);
                let Ok(md) = std::fs::metadata(&p) else { continue };
                if !md.is_file() || md.permissions().mode() & 0o111 == 0 {
                    continue;
                }
                let every = std::fs::read_to_string(&p).ok().and_then(|t| {
                    t.lines().take(3).find_map(|l| l.trim_start_matches('#').trim().strip_prefix("every ").and_then(|n| n.trim().parse().ok()))
                });
                jobs.push(Job { name: name.clone(), every: every.unwrap_or(5), cmd: p.to_string_lossy().into_owned() });
            }
        }
        let mut g = self.shared.lock().unwrap();
        g.text.retain(|k, _| jobs.iter().any(|j| &j.name == k));
        g.jobs = jobs;
        g.enabled = cfg.widgets;
        g.interval = cfg.bar_interval;
        if g.disk_path != cfg.disk {
            g.disk_path = cfg.disk.clone();
            g.disk = read_disk(&cfg.disk);
        }
    }
}

/// Current local time formatted with strftime.
pub fn now(fmt: &str) -> String {
    use std::ffi::CString;
    let f = CString::new(fmt).unwrap_or_default();
    let mut buf = [0u8; 64];
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let n = libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), f.as_ptr(), &tm);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_line() {
        assert_eq!(parse_vol("Volume: 0.50"), Some((50, false)));
        assert_eq!(parse_vol("Volume: 1.00 [MUTED]"), Some((100, true)));
        assert_eq!(parse_vol("nonsense"), None);
    }

    #[test]
    fn audio_labels() {
        assert_eq!(audio_label("mic", Some((80, false))), ("mic 80% on".into(), false));
        assert_eq!(audio_label("spk", Some((45, true))), ("spk 45% off".into(), true));
        assert_eq!(audio_label("mic", None), ("mic --".into(), true));
    }

    #[test]
    fn nvidia_line() {
        assert_eq!(parse_nvidia("35, 1024, 4096\n"), Some((35, 25)));
        assert_eq!(parse_nvidia("garbage"), None);
        assert_eq!(parse_nvidia("1, 2, 0"), None);
    }

    #[test]
    fn clock_has_colon() {
        assert!(now("%H:%M").contains(':'));
    }

    #[test]
    fn script_output_first_line() {
        assert_eq!(first_line("printf 'a\\nb\\n'"), "a");
    }
}

#[cfg(test)]
mod disk_tests {
    use super::*;

    #[test]
    fn human_sizes_are_short() {
        assert_eq!(human(455 << 30), "455G");
        assert_eq!(human(1_320_000_000_000), "1.2T");
        assert_eq!(human(850 << 20), "850M");
        assert_eq!(human(12), "12B");
    }

    #[test]
    fn history_keeps_the_last_samples() {
        let mut h = vec![];
        for i in 0..(HIST as u32 + 5) {
            push_hist(&mut h, i * 10);
        }
        assert_eq!(h.len(), HIST);
        assert_eq!(*h.last().unwrap(), 100); // clamped
        assert_eq!(h[0], ((5 * 10).min(100)) as u8);
    }

    #[test]
    fn root_has_a_size() {
        let (total, free) = read_disk("/").unwrap();
        assert!(total > 0 && free <= total);
        assert!(read_disk("/no/such/dir/here").is_none());
    }
}
