//! The panels of Ustawienia that are more than a list of checkboxes: the theme editor with a colour picker (and import of a
//! theme file), bar, keyboard and mouse choices, and the screens. All of it is `Entry` lists for `menu`, so a script drives
//! them with `ctl menu ...` exactly as the user does.

use super::menu::{e, head, Act, Entry, Id};
use super::outputs::{self, OutputCfg};
use super::theme::{self, Rgb, Theme};
use crate::Desktop;

/// The theme being made in Ustawienia > Motyw > Utworz motyw. The panel and the bar show it live; nothing is written until
/// "Zapisz", and closing the menu throws it away.
#[derive(Debug, Clone)]
pub struct Draft {
    pub theme: Theme,
    /// the colour the picker edits
    pub key: &'static str,
    /// (hue 0..360, saturation 0..1, value 0..1) of that colour, kept apart from the rgb so a grey does not forget its hue
    pub hsv: (f32, f32, f32),
    /// bumps on every change: the bar's cache is keyed by the theme name
    pub n: u32,
}

impl Draft {
    pub fn new(base: &Theme) -> Draft {
        let mut d = Draft { theme: base.clone(), key: theme::COLOR_KEYS[0].0, hsv: (0.0, 0.0, 0.0), n: 0 };
        d.pick(d.key);
        d
    }

    pub fn pick(&mut self, key: &'static str) {
        self.key = key;
        self.hsv = theme::rgb_to_hsv(self.theme.color(key).unwrap_or(self.theme.fg));
    }

    pub fn color(&self) -> Rgb {
        self.theme.color(self.key).unwrap_or(self.theme.fg)
    }

    fn touch(&mut self) {
        self.n += 1;
        self.theme.name = format!("~draft{}", self.n);
    }

    /// Change one of hue / saturation / value (`ch` 0 / 1 / 2).
    pub fn set_hsv(&mut self, ch: u8, v: f32) {
        match ch {
            0 => self.hsv.0 = v.clamp(0.0, 360.0),
            1 => self.hsv.1 = v.clamp(0.0, 1.0),
            _ => self.hsv.2 = v.clamp(0.0, 1.0),
        }
        let c = theme::hsv_to_rgb(self.hsv.0, self.hsv.1, self.hsv.2);
        self.theme.set_color(self.key, c);
        self.touch();
    }

    pub fn set_rgb(&mut self, c: Rgb) {
        self.theme.set_color(self.key, c);
        self.hsv = theme::rgb_to_hsv(c);
        self.touch();
    }
}

/// `#rrggbb` or `rrggbb`, as typed.
pub fn typed_color(q: &str) -> Option<Rgb> {
    let q = q.trim();
    theme::parse_rgb(&if q.starts_with('#') { q.to_string() } else { format!("#{q}") }).ok()
}

pub(crate) fn mark(on: bool, name: &str) -> String {
    if on { format!("* {name}") } else { format!("  {name}") }
}

/// A choice: one value of one config key, starred when it is the current one.
pub(crate) fn choice(label: &str, key: &'static str, val: &str, cur: &str) -> Entry {
    e(&mark(cur == val, label), Act::Set { key, val: val.into() })
}

impl Desktop {
    fn menu_query(&self) -> String {
        self.menu.as_ref().map(|m| m.query.clone()).unwrap_or_default()
    }

    /// The draft exists while the theme editor is on the menu's stack.
    pub fn ensure_draft(&mut self) {
        if self.theme_draft.is_none() {
            self.theme_draft = Some(Draft::new(&self.config.theme));
        }
    }

    /// Show the draft as the real theme (not saved).
    fn draft_apply(&mut self) {
        if let Some(d) = &self.theme_draft {
            self.config.background = theme::to_f32(d.theme.bg);
            self.config.theme = d.theme.clone();
            self.bar_dirty = true;
        }
    }

    /// Every frame: a draft nobody is editing any more (the menu closed or went back) is dropped and the saved theme returns.
    pub fn draft_gc(&mut self) {
        if self.theme_draft.is_none() {
            return;
        }
        let editing = self.menu.as_ref().is_some_and(|m| m.stack.iter().any(|l| matches!(l.id, Id::ThemeNew | Id::ColorPick)));
        if !editing {
            self.theme_draft = None;
            self.load_config();
        }
    }

    pub fn theme_new_entries(&self) -> Vec<Entry> {
        let base = self.theme_draft.as_ref().map(|d| d.theme.clone()).unwrap_or_else(|| self.config.theme.clone());
        let mut v = vec![head("kolory: Enter wybiera kolor (suwaki albo wpisany #rrggbb), zmiana jest widoczna od razu")];
        for (k, label) in theme::COLOR_KEYS {
            let c = base.color(k).unwrap_or(base.fg);
            v.push(e(label, Act::Color { key: k, rgb: c }));
        }
        v.push(head("zapis: wpisz nazwe, Enter zapisuje (Esc wyrzuca zmiany)"));
        let q = self.menu_query();
        let name = if q.trim().is_empty() { "custom".to_string() } else { q.trim().to_string() };
        v.push(if theme::valid_name(&name) {
            e(&format!("Zapisz jako: {name}"), Act::SaveTheme(name))
        } else {
            e("nazwa: litery, cyfry, - i _ (do 32 znakow)", Act::Nop)
        });
        v
    }

    pub fn color_pick_entries(&self) -> Vec<Entry> {
        let Some(d) = &self.theme_draft else { return vec![e("brak motywu w edycji", Act::Nop)] };
        let label = theme::COLOR_KEYS.iter().find(|(k, _)| *k == d.key).map_or(d.key, |(_, l)| l);
        let (h, s, v) = d.hsv;
        let mut out = vec![
            head(label),
            e(&theme::hex(d.color()), Act::Swatch(d.color())),
            e("Odcien", Act::Hsv { ch: 0, val: h, max: 360.0, step: 5.0 }),
            e("Nasycenie", Act::Hsv { ch: 1, val: s, max: 1.0, step: 0.05 }),
            e("Jasnosc", Act::Hsv { ch: 2, val: v, max: 1.0, step: 0.05 }),
            head("albo wpisz kod"),
        ];
        out.push(match typed_color(&self.menu_query()) {
            Some(c) => e(&format!("Ustaw {}", theme::hex(c)), Act::SetColor(c)),
            None => e("wpisz #rrggbb i Enter", Act::Nop),
        });
        out
    }

    pub fn theme_import_entries(&self) -> Vec<Entry> {
        let mut v = Vec::new();
        let q = self.menu_query();
        v.push(head("wpisz sciezke do pliku motywu i Enter, albo wybierz znaleziony"));
        if !q.trim().is_empty() {
            v.push(e(&format!("Importuj: {}", q.trim()), Act::ImportTheme(q.trim().into())));
        }
        let mut found = theme_files();
        found.sort();
        if !found.is_empty() {
            v.push(head("znalezione (*.theme w ~, ~/Downloads, folderze configu)"));
            v.extend(found.into_iter().map(|p| e(&p, Act::ImportTheme(p.clone()))));
        }
        v
    }

    /// Hue / saturation / value slider moved.
    pub fn picker_slide(&mut self, ch: u8, val: f32) {
        if let Some(d) = self.theme_draft.as_mut() {
            d.set_hsv(ch, val);
        }
        self.draft_apply();
    }

    pub fn picker_set(&mut self, c: Rgb) {
        if let Some(d) = self.theme_draft.as_mut() {
            d.set_rgb(c);
        }
        self.draft_apply();
    }

    pub fn picker_open(&mut self, key: &'static str) {
        self.ensure_draft();
        if let Some(d) = self.theme_draft.as_mut() {
            d.pick(key);
        }
    }

    /// Write the draft as `themes/NAME` and make it the theme.
    pub fn theme_save(&mut self, name: &str) -> String {
        let Some(d) = self.theme_draft.take() else { return "error: no theme is being made".into() };
        let r = write_theme(name, &d.theme.to_text()).and_then(|()| super::settings::settings_preferaps::persist("theme", name));
        match r {
            Ok(()) => {
                self.load_config();
                format!("ok (theme {name})")
            }
            Err(err) => {
                self.theme_draft = Some(d);
                format!("error: not saved: {err}")
            }
        }
    }

    /// Copy a theme file into the config's `themes/` (after checking that it is one) and use it.
    pub fn theme_import(&mut self, path: &str) -> String {
        let path = match path.strip_prefix("~/") {
            Some(rest) => format!("{}/{rest}", std::env::var("HOME").unwrap_or_default()),
            None => path.to_string(),
        };
        let p = std::path::Path::new(&path);
        let Some(name) = p.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|n| theme::valid_name(n)) else {
            return "error: the file name must be letters, digits, - and _ (then .theme)".into();
        };
        let text = match std::fs::read_to_string(p) {
            Ok(t) => t,
            Err(err) => return format!("error: {path}: {err}"),
        };
        if let Err(err) = Theme::parse(&name, &text) {
            return format!("error: not a theme: {err}");
        }
        match write_theme(&name, &text).and_then(|()| super::settings::settings_preferaps::persist("theme", &name)) {
            Ok(()) => {
                self.load_config();
                format!("ok (theme {name})")
            }
            Err(err) => format!("error: {err}"),
        }
    }

    /// Pasek: where it sits, the clocks, what is on it.
    pub fn bar_entries(&self) -> Vec<Entry> {
        let c = &self.config;
        let bar = match c.bar {
            crate::config::BarPos::Top => "top",
            crate::config::BarPos::Bottom => "bottom",
            crate::config::BarPos::Off => "off",
        };
        let slide = |label: &str, key: &'static str, min: f32, max: f32, step: f32, val: f32| e(label, Act::Slide { key, min, max, step, val });
        vec![
            head("polozenie"),
            choice("u gory", "bar", "top", bar),
            choice("na dole", "bar", "bottom", bar),
            choice("ukryty", "bar", "off", bar),
            head("zegar"),
            choice("14:05:09", "clock_format", "%H:%M:%S", &c.clock_format),
            choice("14:05", "clock_format", "%H:%M", &c.clock_format),
            choice("2:05 PM", "clock_format", "%I:%M %p", &c.clock_format),
            head("data"),
            choice("2026-11-23", "date_format", "%Y-%m-%d", &c.date_format),
            choice("23.11.2026", "date_format", "%d.%m.%Y", &c.date_format),
            choice("pon 23 lis", "date_format", "%a %d %b", &c.date_format),
            head("elementy"),
            e("Statystyki: cpu gpu ram vram swap dyski (kolor od 60% i 90%)", Act::Toggle { key: "bar_stats", on: c.bar_right.iter().any(|n| n == "stats") }),
            slide("Wysokosc paska kart (0 = auto)", "tab_height", 0.0, 40.0, 2.0, c.tab_height as f32),
            slide("Odswiezanie paska (s)", "bar_interval", 1.0, 10.0, 1.0, c.bar_interval as f32),
        ]
    }

    /// Klawiatura i mysz.
    pub fn input_entries(&self) -> Vec<Entry> {
        let c = &self.config;
        let slide = |label: &str, key: &'static str, min: f32, max: f32, step: f32, val: f32| e(label, Act::Slide { key, min, max, step, val });
        let mut v = vec![head("uklad klawiatury")];
        for (label, val) in [("polski", "pl"), ("angielski (US)", "us"), ("angielski (GB)", "gb"), ("niemiecki", "de"), ("polski + US", "pl,us")] {
            v.push(choice(label, "kb_layout", val, &c.kb_layout));
        }
        v.push(head("przelaczanie ukladow i klawisze"));
        for (label, val) in [("zadne", ""), ("Alt+Shift przelacza uklad", "grp:alt_shift_toggle"), ("Win+Spacja przelacza uklad", "grp:win_space_toggle"), ("Caps Lock przelacza uklad", "grp:caps_toggle"), ("Caps Lock jako Escape", "caps:escape"), ("Caps Lock jako Ctrl", "ctrl:nocaps")] {
            v.push(choice(label, "kb_options", val, &c.kb_options));
        }
        v.push(head("powtarzanie klawiszy"));
        v.push(slide("Szybkosc (znaki na sekunde)", "kb_repeat_rate", 5.0, 60.0, 5.0, c.kb_repeat_rate as f32));
        v.push(slide("Opoznienie (ms)", "kb_repeat_delay", 100.0, 800.0, 50.0, c.kb_repeat_delay as f32));
        v.push(head("mysz i touchpad (nowe ustawienia dzialaja od razu na podlaczonych urzadzeniach)"));
        v.push(slide("Szybkosc wskaznika", "pointer_speed", -1.0, 1.0, 0.1, c.pointer_speed));
        v.push(e("Naturalne przewijanie", Act::Toggle { key: "natural_scroll", on: c.natural_scroll }));
        v.push(e("Dotkniecie klika (touchpad)", Act::Toggle { key: "tap_to_click", on: c.tap_to_click }));
        v.push(e("Fokus za myszka", Act::Toggle { key: "focus_follows_mouse", on: c.focus_follows_mouse }));
        v.push(head("mysz i touchpad: wiecej"));
        v.push(choice("Przyspieszenie: adaptacyjne", "accel_profile", "adaptive", if c.accel_flat { "flat" } else { "adaptive" }));
        v.push(choice("Przyspieszenie: plaskie (bez przyspieszenia, do gier)", "accel_profile", "flat", if c.accel_flat { "flat" } else { "adaptive" }));
        v.push(e("Mysz dla leworecznych (zamienione przyciski)", Act::Toggle { key: "left_handed", on: c.left_handed }));
        v.push(e("Srodkowy przycisk z dwoch naraz", Act::Toggle { key: "middle_emulation", on: c.middle_emulation }));
        v.push(e("Touchpad: wylacz podczas pisania", Act::Toggle { key: "disable_while_typing", on: c.disable_while_typing }));
        v
    }

    /// Ekrany: resolution and refresh, place, off.
    pub fn screens_entries(&self) -> Vec<Entry> {
        let mut v = vec![head("zmiany wchodza po `finedesktop ctl session restart` (zapisane w config)")];
        let geos: Vec<(String, (i32, i32), (i32, i32))> = self
            .mons
            .iter()
            .map(|m| {
                let g = self.space.output_geometry(&m.output);
                (m.output.name(), g.map_or((0, 0), |g| (g.loc.x, g.loc.y)), g.map_or((0, 0), |g| (g.size.w, g.size.h)))
            })
            .collect();
        for (mi, m) in self.mons.iter().enumerate() {
            let (name, at, size) = geos[mi].clone();
            let cur = m.output.current_mode();
            let hz = cur.map_or(0, |c| (c.refresh as f32 / 1000.0).round() as i32);
            v.push(head(&format!("{name}: {}x{}@{hz}  pozycja {},{}", size.0, size.1, at.0, at.1)));
            let mut modes: Vec<outputs::ModeInfo> = m.output.modes().iter().map(|m| (m.size.w, m.size.h, m.refresh)).collect();
            modes.sort_by_key(|m| std::cmp::Reverse((m.0 * m.1, m.2)));
            modes.dedup_by_key(|m| (m.0, m.1, (m.2 as f32 / 1000.0).round() as i32));
            for md in modes.into_iter().take(30) {
                let on = cur.is_some_and(|c| (c.size.w, c.size.h, c.refresh) == md);
                let l = outputs::mode_label(md);
                v.push(e(&mark(on, &l), Act::Out(format!("{name} mode={l}"))));
            }
            for (oi, (oname, oat, osize)) in geos.iter().enumerate() {
                if oi == mi {
                    continue;
                }
                for (side, label) in [("right", "na prawo od"), ("left", "na lewo od"), ("above", "nad"), ("below", "pod")] {
                    let p = outputs::place(side, *oat, *osize, size);
                    v.push(e(&format!("Polozenie: {label} {oname}"), Act::Out(format!("{name} pos={},{}", p.0, p.1))));
                }
            }
            if geos.len() > 1 {
                v.push(e(&format!("Wylacz {name}"), Act::Out(format!("{name} off"))));
            }
        }
        for o in self.config.outputs.iter().filter(|o| o.off && !geos.iter().any(|g| g.0 == o.name)) {
            v.push(e(&format!("Wlacz {}", o.name), Act::Out(format!("{} on", o.name))));
        }
        v.push(self.brightness_entry());
        v
    }

    /// `NAME mode=WxH@HZ`, `NAME pos=X,Y`, `NAME off`, `NAME on`: merged into that output's config line and saved.
    pub fn output_change(&mut self, line: &str) -> String {
        let (name, rest) = line.trim().split_once(char::is_whitespace).map_or((line.trim(), ""), |(a, b)| (a, b.trim()));
        let mut cfg = self.config.outputs.iter().find(|o| o.name == name).cloned().unwrap_or(OutputCfg { name: name.into(), ..OutputCfg::default() });
        if rest == "on" {
            cfg.off = false;
        } else {
            let new = match OutputCfg::parse(line) {
                Ok(c) => c,
                Err(err) => return format!("error: {err}"),
            };
            if new.off {
                let active = self.mons.iter().filter(|m| m.output.name() != name).count();
                if active == 0 {
                    return "error: this is the only screen, it stays on".into();
                }
            }
            cfg.mode = new.mode.or(cfg.mode);
            cfg.pos = new.pos.or(cfg.pos);
            cfg.off |= new.off;
        }
        if let Err(err) = outputs::persist(&cfg) {
            return format!("error: not saved: {err}");
        }
        self.config.outputs.retain(|o| o.name != name);
        self.config.outputs.push(cfg);
        "ok: saved, shows after `finedesktop ctl session restart`".into()
    }

    /// A checkbox that is not one config key: the bar's statistics replace the single cpu / ram items.
    pub fn bar_stats_set(&mut self, on: bool) -> String {
        let cur = &self.config.bar_right;
        let new: Vec<String> = if on {
            std::iter::once("stats".to_string()).chain(cur.iter().filter(|n| !["stats", "cpu", "ram", "gpu", "vram", "disk", "disk-bar"].contains(&n.as_str())).cloned()).collect()
        } else {
            cur.iter().flat_map(|n| if n == "stats" { vec!["cpu".to_string(), "ram".to_string()] } else { vec![n.clone()] }).collect()
        };
        match super::settings::settings_preferaps::persist("bar_right", &new.join(" ")) {
            Ok(()) => {
                self.load_config();
                "ok".into()
            }
            Err(err) => format!("error: the config was not written: {err}"),
        }
    }
}

/// `*.theme` files in the home folder, Downloads and the config folder that really parse as themes.
fn theme_files() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs = vec![std::path::PathBuf::from(&home), std::path::Path::new(&home).join("Downloads")];
    dirs.extend(crate::config::config_dir());
    let mut out = Vec::new();
    for d in dirs {
        for f in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = f.path();
            if p.extension().is_some_and(|x| x == "theme") && p.is_file() {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if theme::valid_name(&name) && std::fs::read_to_string(&p).is_ok_and(|t| Theme::parse(&name, &t).is_ok()) {
                    out.push(p.to_string_lossy().into_owned());
                }
            }
        }
    }
    out
}

fn write_theme(name: &str, text: &str) -> std::io::Result<()> {
    let dir = crate::config::config_dir().ok_or_else(|| std::io::Error::other("no config directory"))?.join("themes");
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_colours_with_or_without_the_hash() {
        assert_eq!(typed_color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(typed_color(" 00ff00 "), Some([0, 255, 0]));
        assert_eq!(typed_color("ff80"), None);
        assert_eq!(typed_color("nothex"), None);
        assert_eq!(typed_color(""), None);
    }

    #[test]
    fn the_picker_edits_the_chosen_colour_and_renames_the_theme_each_time() {
        let mut d = Draft::new(&Theme::default());
        d.pick("accent");
        let n0 = d.theme.name.clone();
        d.set_hsv(0, 120.0);
        d.set_hsv(1, 1.0);
        d.set_hsv(2, 1.0);
        assert_eq!(d.theme.accent, [0, 255, 0]);
        assert_ne!(d.theme.name, n0, "the bar caches by theme name");
        d.set_rgb([10, 20, 30]);
        assert_eq!(d.color(), [10, 20, 30]);
        assert_eq!(d.theme.bg, Theme::default().bg, "other colours stay");
    }

    #[test]
    fn a_grey_keeps_its_hue_while_saturation_moves() {
        let mut d = Draft::new(&Theme::default());
        d.pick("fg");
        d.set_hsv(0, 200.0);
        d.set_hsv(1, 0.0);
        d.set_hsv(1, 0.5);
        assert!((d.hsv.0 - 200.0).abs() < 0.01);
    }

    #[test]
    fn a_choice_is_starred_when_current() {
        assert_eq!(choice("x", "bar", "top", "top").label, "* x");
        assert_eq!(choice("x", "bar", "top", "off").label, "  x");
    }
}
