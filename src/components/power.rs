//! Power: `ctl power` prints battery, charger and power profile; `ctl power profile [NAME]` gets or sets the profile
//! (through `powerprofilesctl`). `low_battery = N` in the config (default 15, 0 = off) posts one notification and an
//! `on-user` event named `power` when the battery falls to N% while discharging, again only after it was charged above N+5.

use crate::Desktop;

pub struct Battery {
    pub percent: i32,
    pub charging: bool,
}

fn read(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub fn battery() -> Option<Battery> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        if read(&p.join("type")).as_deref() == Some("Battery") {
            let percent = read(&p.join("capacity"))?.parse().ok()?;
            let status = read(&p.join("status")).unwrap_or_default();
            return Some(Battery { percent, charging: status == "Charging" || status == "Full" });
        }
    }
    None
}

fn ac_online() -> Option<bool> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    dir.flatten().map(|e| e.path()).find(|p| read(&p.join("type")).as_deref() == Some("Mains")).map(|p| read(&p.join("online")).as_deref() == Some("1"))
}

fn profile(set: Option<&str>) -> String {
    let mut c = std::process::Command::new("powerprofilesctl");
    match set {
        Some(n) => c.args(["set", n]),
        None => c.arg("get"),
    };
    match c.output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => format!("error: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(_) => "error: powerprofilesctl is not installed".into(),
    }
}

/// Should a low-battery warning go out now? `warned` is the state between calls.
pub fn low_check(percent: i32, charging: bool, limit: i32, warned: &mut bool) -> bool {
    if limit <= 0 || charging || percent > limit + 5 {
        *warned = false;
        return false;
    }
    if percent <= limit && !*warned {
        *warned = true;
        return true;
    }
    false
}

/// Looks at the battery once a minute.
#[derive(Default)]
pub struct Power {
    seconds: u32,
}

impl crate::flows::lifecycle::Component for Power {
    fn name(&self) -> &'static str {
        "power"
    }
    fn start(&mut self, d: &mut Desktop) {
        d.power_tick();
    }
    fn tick(&mut self, d: &mut Desktop) {
        self.seconds += 1;
        if self.seconds % 60 == 0 {
            d.power_tick();
        }
    }
}

impl Desktop {
    pub fn power_cmd(&mut self, arg: &str) -> String {
        match arg.trim().split_once(' ').map_or((arg.trim(), ""), |(a, b)| (a, b.trim())) {
            ("", _) | ("status", _) => {
                let bat = battery().map_or("no battery".to_string(), |b| format!("battery {}% {}", b.percent, if b.charging { "charging" } else { "discharging" }));
                let ac = ac_online().map_or(String::new(), |a| format!(" ac {}", if a { "on" } else { "off" }));
                format!("{bat}{ac} profile {}", profile(None))
            }
            ("profile", "") => profile(None),
            ("profile", name) => {
                let r = profile(Some(name));
                if r.starts_with("error") { r } else { format!("ok {name}") }
            }
            _ => "error: power [status | profile [power-saver|balanced|performance]]".into(),
        }
    }

    /// Called about once a minute.
    pub fn power_tick(&mut self) {
        let Some(b) = battery() else { return };
        self.observe("battery", &b.percent.to_string());
        self.observe("charging", if b.charging { "yes" } else { "no" });
        let limit = self.config.low_battery;
        if low_check(b.percent, b.charging, limit, &mut self.low_warned) {
            let text = format!("battery low: {}%", b.percent);
            self.notify_cmd(&text);
            self.emit(crate::components::hooks::Event::user("power", &text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_once_and_rearms_after_charging() {
        let mut w = false;
        assert!(!low_check(30, false, 15, &mut w));
        assert!(low_check(15, false, 15, &mut w));
        assert!(!low_check(14, false, 15, &mut w));
        assert!(!low_check(18, false, 15, &mut w)); // inside the hysteresis band: still warned
        assert!(!low_check(50, false, 15, &mut w)); // rearmed here
        assert!(low_check(10, false, 15, &mut w));
        assert!(!low_check(5, true, 15, &mut w));
        assert!(!low_check(5, false, 0, &mut w)); // 0 = off
    }
}
