//! Sound from the menu: two sliders (speakers and microphone) with a mute each, at the top of the launcher and in
//! Ustawienia > Dzwiek. The values come from the same poll the bar uses (`widgets::Shared`), changes go through `wpctl`
//! and are written back into that state at once, so the slider moves without waiting for the next poll.

use std::process::{Command, Stdio};

use super::menu::{e, head, Act, Entry};
use crate::Desktop;

fn target(mic: bool) -> &'static str {
    if mic { "@DEFAULT_AUDIO_SOURCE@" } else { "@DEFAULT_AUDIO_SINK@" }
}

fn wpctl(args: &[&str]) -> bool {
    Command::new("timeout").arg("1").arg("wpctl").args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// `audio_spk`, `audio_mic`, `audio_spk_mute`, `audio_mic_mute`: (is the microphone, is it the mute switch).
pub fn parse_key(key: &str) -> Option<(bool, bool)> {
    match key {
        "audio_spk" => Some((false, false)),
        "audio_mic" => Some((true, false)),
        "audio_spk_mute" => Some((false, true)),
        "audio_mic_mute" => Some((true, true)),
        _ => None,
    }
}

impl Desktop {
    fn audio_state(&self, mic: bool) -> Option<(u32, bool)> {
        let sh = self.widgets.shared.lock().unwrap();
        if mic { sh.mic } else { sh.vol }
    }

    /// The two sliders. `with_mute` adds the mute checkboxes (the panel); in the launcher Enter on a slider mutes.
    pub fn audio_entries(&self, with_mute: bool) -> Vec<Entry> {
        let mut v = vec![head("dzwiek: lewo/prawo glosnosc, Enter wycisza")];
        for (mic, label, key, mkey) in [(false, "Glosnik", "audio_spk", "audio_spk_mute"), (true, "Mikrofon", "audio_mic", "audio_mic_mute")] {
            match self.audio_state(mic) {
                Some((val, muted)) => {
                    let label = if muted { format!("{label} (wyciszony)") } else { label.to_string() };
                    v.push(e(&label, Act::Slide { key, min: 0.0, max: 100.0, step: 5.0, val: val as f32 }));
                    if with_mute {
                        v.push(e(&format!("Wycisz {}", label.split(' ').next().unwrap_or("").to_lowercase()), Act::Toggle { key: mkey, on: muted }));
                    }
                }
                None => v.push(e(&format!("{label}: brak urzadzenia"), Act::Nop)),
            }
        }
        v
    }

    /// Volume of the speakers or the microphone, in percent, capped at 100.
    pub fn audio_set(&mut self, mic: bool, percent: u32) -> String {
        let p = percent.min(100);
        if !wpctl(&["set-volume", target(mic), &format!("{p}%")]) {
            return "error: wpctl could not set the volume".into();
        }
        let mut sh = self.widgets.shared.lock().unwrap();
        let slot = if mic { &mut sh.mic } else { &mut sh.vol };
        if let Some(s) = slot.as_mut() {
            s.0 = p;
        }
        "ok".into()
    }

    /// Mute (`Some(true)`), unmute, or flip (`None`).
    pub fn audio_mute(&mut self, mic: bool, on: Option<bool>) -> String {
        let want = on.map_or("toggle", |m| if m { "1" } else { "0" });
        if !wpctl(&["set-mute", target(mic), want]) {
            return "error: wpctl could not change the mute".into();
        }
        let mut sh = self.widgets.shared.lock().unwrap();
        let slot = if mic { &mut sh.mic } else { &mut sh.vol };
        if let Some(s) = slot.as_mut() {
            s.1 = on.unwrap_or(!s.1);
        }
        "ok".into()
    }

    /// A settings key from the menu; `None` when the key is not an audio one.
    pub fn audio_setting(&mut self, key: &str, val: &str) -> Option<String> {
        let (mic, mute) = parse_key(key)?;
        Some(if mute { self.audio_mute(mic, Some(val == "on")) } else { self.audio_set(mic, val.parse::<f32>().map_or(0, |f| f.max(0.0) as u32)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_keys_tell_device_and_kind() {
        assert_eq!(parse_key("audio_spk"), Some((false, false)));
        assert_eq!(parse_key("audio_mic_mute"), Some((true, true)));
        assert_eq!(parse_key("gap"), None);
    }
}
