//! Resource numbers for the bar: swap and every real disk (cpu, ram, gpu, vram are sampled in `widgets`), and the one
//! rule that colours them all: normal below 60 %, the theme's `warn` (yellow) from 60 %, `err` (red) from 90 %.
//! Pure parsing here; the sampler thread in `widgets` calls it, the bar draws the result.

use super::theme::{Rgb, Theme};

/// From this percentage a value turns yellow.
pub const WARN_AT: u32 = 60;
/// From this percentage a value turns red.
pub const HOT_AT: u32 = 90;

/// `normal` below 60 %, `warn` from 60 %, `err` from 90 %.
pub fn level_color(p: u32, normal: Rgb, t: &Theme) -> Rgb {
    if p >= HOT_AT {
        t.err
    } else if p >= WARN_AT {
        t.warn
    } else {
        normal
    }
}

/// Swap used in percent from the text of /proc/meminfo; None when the machine has no swap.
pub fn parse_swap(meminfo: &str) -> Option<u32> {
    let get = |k: &str| meminfo.lines().find(|l| l.starts_with(k))?.split_whitespace().nth(1)?.parse::<u64>().ok();
    let (total, free) = (get("SwapTotal:")?, get("SwapFree:")?);
    (total > 0).then(|| ((total - free.min(total)) * 100 / total) as u32)
}

pub fn read_swap() -> Option<u32> {
    parse_swap(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    pub mount: String,
    pub total: u64,
    pub free: u64,
}

impl Disk {
    pub fn used_pct(&self) -> u32 {
        (self.total.saturating_sub(self.free.min(self.total)) * 100 / self.total.max(1)) as u32
    }

    /// `/` stays `/`, `/data` becomes `data`, `/run/media/x/USB` becomes `USB`.
    pub fn label(&self) -> String {
        match self.mount.rsplit('/').find(|s| !s.is_empty()) {
            Some(s) => s.to_string(),
            None => "/".into(),
        }
    }
}

/// /proc/mounts escapes space, tab, newline and backslash as octal (`\040`).
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            out.push(((b[i + 1] - b'0') << 6) | ((b[i + 2] - b'0') << 3) | (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Mount points of the real disks in /proc/mounts: block devices only (no loop, no tmpfs, no bind of a pseudo fs), and a
/// device mounted several times (btrfs subvolumes) counts once, under its shortest mount point. Order of first appearance.
pub fn parse_mounts(text: &str) -> Vec<String> {
    let mut seen: Vec<(String, String)> = Vec::new();
    for l in text.lines() {
        let mut f = l.split_whitespace();
        let (Some(dev), Some(mnt)) = (f.next(), f.next()) else { continue };
        if !dev.starts_with("/dev/") || dev.starts_with("/dev/loop") || dev.starts_with("/dev/zram") {
            continue;
        }
        let mnt = unescape(mnt);
        match seen.iter_mut().find(|(d, _)| d == dev) {
            Some((_, m)) => {
                if mnt.len() < m.len() {
                    *m = mnt;
                }
            }
            None => seen.push((dev.to_string(), mnt)),
        }
    }
    seen.into_iter().map(|(_, m)| m).collect()
}

pub fn read_disks() -> Vec<Disk> {
    let text = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    parse_mounts(&text).into_iter().filter_map(|m| super::widgets::read_disk(&m).map(|(total, free)| Disk { mount: m, total, free })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_steps_at_60_and_90() {
        let t = Theme::default();
        let n = [1, 2, 3];
        assert_eq!(level_color(0, n, &t), n);
        assert_eq!(level_color(59, n, &t), n);
        assert_eq!(level_color(60, n, &t), t.warn);
        assert_eq!(level_color(89, n, &t), t.warn);
        assert_eq!(level_color(90, n, &t), t.err);
        assert_eq!(level_color(100, n, &t), t.err);
    }

    #[test]
    fn swap_percent_and_no_swap() {
        assert_eq!(parse_swap("SwapTotal: 1000 kB\nSwapFree: 250 kB\n"), Some(75));
        assert_eq!(parse_swap("SwapTotal: 0 kB\nSwapFree: 0 kB\n"), None);
        assert_eq!(parse_swap("MemTotal: 1 kB\n"), None);
    }

    #[test]
    fn many_disks_one_entry_per_device() {
        let m = "proc /proc proc rw 0 0\n/dev/sda2 / btrfs rw 0 0\n/dev/sda2 /home btrfs rw 0 0\n/dev/sda1 /boot vfat rw 0 0\n/dev/loop3 /snap/x squashfs ro 0 0\ntmpfs /tmp tmpfs rw 0 0\n/dev/sdb1 /data btrfs rw 0 0\n/dev/sda2 /srv btrfs rw 0 0\n/dev/sdc1 /run/media/me/My\\040USB vfat rw 0 0\n";
        assert_eq!(parse_mounts(m), vec!["/", "/boot", "/data", "/run/media/me/My USB"]);
    }

    #[test]
    fn disk_labels_and_percent() {
        let d = |m: &str| Disk { mount: m.into(), total: 200, free: 50 };
        assert_eq!(d("/").label(), "/");
        assert_eq!(d("/data").label(), "data");
        assert_eq!(d("/run/media/me/My USB").label(), "My USB");
        assert_eq!(d("/").used_pct(), 75);
    }
}
