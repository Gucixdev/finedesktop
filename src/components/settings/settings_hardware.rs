//! The hardware panels of Ustawienia: screen brightness, wallpaper, devices (speakers, microphones, cameras) and
//! drivers. Everything is read from sysfs, `wpctl` and `lspci` when they exist and degrades to a one-line note when
//! they do not, so a panel is never empty or broken. The parsers are plain functions with tests.

use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

use crate::{
    components::{
        menu::{e, head, Act, Entry},
        panels::{choice, mark},
    },
    Desktop,
};

fn out(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new("timeout").arg("2").arg(cmd).args(args).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn has(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

// ---- brightness -------------------------------------------------------------------------------------------------

/// The first backlight with a readable maximum: its directory and the maximum.
fn backlight() -> Option<(PathBuf, u32)> {
    let mut dirs: Vec<PathBuf> = fs::read_dir("/sys/class/backlight").ok()?.flatten().map(|d| d.path()).collect();
    dirs.sort();
    dirs.into_iter().find_map(|d| {
        let max = fs::read_to_string(d.join("max_brightness")).ok()?.trim().parse::<u32>().ok().filter(|m| *m > 0)?;
        Some((d, max))
    })
}

pub fn percent_of(cur: u32, max: u32) -> u32 {
    ((cur as u64 * 100 + max as u64 / 2) / max.max(1) as u64).min(100) as u32
}

pub fn raw_of(percent: u32, max: u32) -> u32 {
    // never fully dark: 0 % would leave a laptop screen unreadable with no way to see the slider
    ((percent.clamp(1, 100) as u64 * max as u64 + 50) / 100).max(1) as u32
}

pub fn brightness() -> Option<u32> {
    let (dir, max) = backlight()?;
    Some(percent_of(fs::read_to_string(dir.join("brightness")).ok()?.trim().parse().ok()?, max))
}

/// Write the level; without the right to write sysfs (no udev rule) `brightnessctl` is tried.
pub fn set_brightness(percent: u32) -> Result<(), String> {
    let (dir, max) = backlight().ok_or("no backlight (a desktop monitor has none: use its own buttons or ddcutil)")?;
    if fs::write(dir.join("brightness"), raw_of(percent, max).to_string()).is_ok() {
        return Ok(());
    }
    let st = Command::new("brightnessctl").args(["-q", "set", &format!("{}%", percent.clamp(1, 100))]).status();
    match st {
        Ok(s) if s.success() => Ok(()),
        _ => Err("cannot write the backlight: install brightnessctl or add a udev rule for the video group".into()),
    }
}

// ---- wallpaper --------------------------------------------------------------------------------------------------

/// Pictures a wallpaper can be: the usual folders, one level deep, png / jpg / webp.
pub fn wallpapers() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = [format!("{home}/Pictures/Wallpapers"), format!("{home}/Pictures"), format!("{home}/.local/share/backgrounds"), "/usr/share/backgrounds".into()];
    let mut v: Vec<PathBuf> = Vec::new();
    for d in dirs {
        for f in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = f.path();
            let ext = p.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
            if p.is_file() && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") && !v.contains(&p) {
                v.push(p);
            }
        }
    }
    v.sort();
    v.truncate(60);
    v
}

// ---- audio and video devices -------------------------------------------------------------------------------------

/// `wpctl status`: the lines of one section (`Sinks`, `Sources`) as (id, name, is the default).
pub fn parse_wpctl(text: &str, section: &str) -> Vec<(u32, String, bool)> {
    let mut v = Vec::new();
    let mut on = false;
    for line in text.lines() {
        let t = line.trim_start_matches(|c: char| c == ' ' || c == '│' || c == '├' || c == '└' || c == '─');
        if t.starts_with(&format!("{section}:")) {
            on = true;
            continue;
        }
        if on && (t.ends_with(':') || t.is_empty()) {
            if t.ends_with(':') {
                on = false;
            }
            continue;
        }
        if !on {
            continue;
        }
        let def = t.starts_with('*');
        let t = t.trim_start_matches(['*', ' ']);
        if let Some((id, rest)) = t.split_once('.') {
            if let Ok(id) = id.trim().parse::<u32>() {
                let name = rest.split(" [vol:").next().unwrap_or(rest).trim().to_string();
                v.push((id, name, def));
            }
        }
    }
    v
}

/// Capture nodes of the cameras: (device path, name).
pub fn cameras() -> Vec<(String, String)> {
    let mut v = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir("/sys/class/video4linux").into_iter().flatten().flatten().map(|d| d.path()).collect();
    dirs.sort();
    for d in dirs {
        // a camera exposes several nodes (video, metadata); index 0 is the picture
        if fs::read_to_string(d.join("index")).map(|i| i.trim() != "0").unwrap_or(false) {
            continue;
        }
        let name = fs::read_to_string(d.join("name")).unwrap_or_default().trim().to_string();
        if let Some(node) = d.file_name().and_then(|n| n.to_str()) {
            v.push((format!("/dev/{node}"), name));
        }
    }
    v
}

// ---- drivers ----------------------------------------------------------------------------------------------------

/// `lspci -k`: (slot, device, kernel driver in use or "-").
pub fn parse_lspci(text: &str) -> Vec<(String, String, String)> {
    let mut v: Vec<(String, String, String)> = Vec::new();
    for line in text.lines() {
        if !line.starts_with([' ', '\t']) && !line.is_empty() {
            if let Some((slot, name)) = line.split_once(' ') {
                v.push((slot.to_string(), name.trim().to_string(), "-".into()));
            }
        } else if let Some(d) = line.trim().strip_prefix("Kernel driver in use:") {
            if let Some(last) = v.last_mut() {
                last.2 = d.trim().to_string();
            }
        }
    }
    v
}

/// The same from sysfs when `lspci` is not installed: slot and driver only.
fn sysfs_pci() -> Vec<(String, String, String)> {
    let mut v = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir("/sys/bus/pci/devices").into_iter().flatten().flatten().map(|d| d.path()).collect();
    dirs.sort();
    for d in dirs {
        let slot = d.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let drv = fs::read_link(d.join("driver")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "-".into());
        let class = fs::read_to_string(d.join("class")).unwrap_or_default().trim().to_string();
        v.push((slot, format!("class {class}"), drv));
    }
    v
}

fn modules_loaded() -> usize {
    fs::read_to_string("/proc/modules").map(|s| s.lines().count()).unwrap_or(0)
}

impl Desktop {
    /// The brightness slider, or one line saying why there is none (shown in Ekrany and Urzadzenia).
    pub fn brightness_entry(&self) -> Entry {
        match brightness() {
            Some(p) => e("Jasnosc ekranu", Act::Slide { key: "brightness", min: 5.0, max: 100.0, step: 5.0, val: p as f32 }),
            None => e("Jasnosc ekranu: brak podswietlenia sterowanego z systemu (monitor: jego przyciski lub ddcutil)", Act::Nop),
        }
    }

    pub fn brightness_set(&mut self, val: &str) -> String {
        match val.parse::<f32>() {
            Ok(p) => match set_brightness(p.round() as u32) {
                Ok(()) => "ok".into(),
                Err(e) => format!("error: {e}"),
            },
            Err(_) => format!("error: brightness is a number from 1 to 100, not {val:?}"),
        }
    }

    /// Tapeta: the pictures found, the current one starred; set through `swaybg`.
    pub fn wallpaper_entries(&self) -> Vec<Entry> {
        let cur = &self.config.wallpaper;
        let mut v = vec![if has("swaybg") { head("tapeta: wybor zapisuje sie w config (wallpaper = SCIEZKA)") } else { head("tapeta wymaga programu swaybg (xbps-install swaybg / pacman -S swaybg)") }];
        v.push(super::super::panels::dropdown("Dopasowanie", "wallpaper_mode", &[("wypelnij (przytnij)", "fill"), ("dopasuj (z ramkami)", "fit"), ("rozciagnij", "stretch"), ("do srodka", "center"), ("kafelki", "tile")], self.config.wallpaper_mode.name()));
        v.push(choice("Brak (kolor tla motywu)", "wallpaper", "", cur));
        let list = wallpapers();
        if list.is_empty() {
            v.push(head("brak obrazow w ~/Pictures, ~/Pictures/Wallpapers, ~/.local/share/backgrounds"));
        }
        for p in list {
            let s = p.display().to_string();
            let label = p.file_name().and_then(|n| n.to_str()).unwrap_or(&s).to_string();
            v.push(choice(&label, "wallpaper", &s, cur));
        }
        v
    }

    /// Urzadzenia: speakers, microphones (choose the default), cameras (live preview), brightness.
    pub fn devices_entries(&self) -> Vec<Entry> {
        let mut v = vec![self.brightness_entry()];
        let text = out("wpctl", &["status"]);
        for (section, title) in [("Sinks", "glosniki i sluchawki (Enter ustawia domyslne)"), ("Sources", "mikrofony (Enter ustawia domyslny)")] {
            v.push(head(title));
            let list = text.as_deref().map(|t| parse_wpctl(t, section)).unwrap_or_default();
            if list.is_empty() {
                v.push(e("  brak (PipeWire nie dziala albo nic nie podlaczono)", Act::Nop));
            }
            for (id, name, def) in list {
                v.push(e(&mark(def, &name), Act::Run { cmd: format!("wpctl set-default {id}"), terminal: false }));
            }
        }
        v.push(head("kamery (Enter pokazuje podglad, wymaga mpv)"));
        let cams = cameras();
        if cams.is_empty() {
            v.push(e("  brak kamer", Act::Nop));
        }
        for (dev, name) in cams {
            v.push(e(&format!("  {name} ({dev})"), Act::Run { cmd: format!("mpv --profile=low-latency --untimed av://v4l2:{dev}"), terminal: false }));
        }
        v
    }

    /// Sterowniki: read only, the device and the kernel driver that runs it.
    pub fn drivers_entries(&self) -> Vec<Entry> {
        let list = out("lspci", &["-k"]).map(|t| parse_lspci(&t)).filter(|l| !l.is_empty()).unwrap_or_else(sysfs_pci);
        let mut v = vec![head(&format!("moduly jadra: {}  urzadzen PCI: {} (tylko do odczytu)", modules_loaded(), list.len()))];
        for (slot, name, drv) in list {
            v.push(e(&format!("{slot}  {name}  [{drv}]"), Act::Nop));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_maps_both_ways_and_never_reaches_black() {
        assert_eq!(percent_of(50, 100), 50);
        assert_eq!(percent_of(960, 960), 100);
        assert_eq!(raw_of(50, 960), 480);
        assert!(raw_of(0, 960) >= 1);
        assert_eq!(raw_of(500, 255), 255);
    }

    #[test]
    fn wpctl_sinks_and_the_default() {
        let t = "Audio\n ├─ Devices:\n │      42. GPU [alsa]\n │  \n ├─ Sinks:\n │  *   50. HDMI Stereo [vol: 0.40]\n │      51. Built-in Analog [vol: 1.00]\n │  \n ├─ Sources:\n │      60. Mic [vol: 1.00]\n";
        let s = parse_wpctl(t, "Sinks");
        assert_eq!(s, vec![(50, "HDMI Stereo".into(), true), (51, "Built-in Analog".into(), false)]);
        assert_eq!(parse_wpctl(t, "Sources"), vec![(60, "Mic".into(), false)]);
        assert!(parse_wpctl(t, "Filters").is_empty());
    }

    #[test]
    fn lspci_pairs_a_device_with_its_driver() {
        let t = "00:02.0 VGA compatible controller: Intel\n\tSubsystem: x\n\tKernel driver in use: i915\n\tKernel modules: i915\n01:00.0 Audio device: Nvidia\n\tKernel modules: snd_hda_intel\n";
        assert_eq!(
            parse_lspci(t),
            vec![("00:02.0".into(), "VGA compatible controller: Intel".into(), "i915".into()), ("01:00.0".into(), "Audio device: Nvidia".into(), "-".into())]
        );
    }
}
