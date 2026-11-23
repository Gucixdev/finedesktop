//! Themes: a theme is a plain `key = value` file. Lookup order: `$XDG_CONFIG_HOME/finedesktop/themes/NAME`,
//! then the built-in ones from `src/.finedesktop/themes/`. A theme only sets looks; it never touches modes,
//! hooks or key bindings, and modes never touch the theme.

pub type Rgb = [u8; 3];

const BUILTIN: [(&str, &str); 8] = [
    ("dwl", include_str!("../.finedesktop/themes/dwl")),
    ("zen", include_str!("../.finedesktop/themes/zen")),
    ("matrix", include_str!("../.finedesktop/themes/matrix")),
    ("catpuccino", include_str!("../.finedesktop/themes/catpuccino")),
    ("dracula", include_str!("../.finedesktop/themes/dracula")),
    ("steam", include_str!("../.finedesktop/themes/steam")),
    ("official-dark", include_str!("../.finedesktop/themes/official-dark")),
    ("official-light", include_str!("../.finedesktop/themes/official-light")),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub bg: Rgb,
    pub fg: Rgb,
    pub dim: Rgb,
    pub accent: Rgb,
    /// red: errors, 90 % and more of a resource (a theme file may still say `urgent`, the old name)
    pub err: Rgb,
    /// yellow: warnings, 60 % and more of a resource
    pub warn: Rgb,
    pub bar_bg: Rgb,
    pub bar_fg: Rgb,
    pub sel_bg: Rgb,
    pub sel_fg: Rgb,
    pub border_normal: Rgb,
    pub border_focus: Rgb,
    pub border: i32,
    pub gap: i32,
    pub animations: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::parse("dwl", BUILTIN[0].1).unwrap_or_else(|_| Theme::fallback())
    }
}

pub fn parse_rgb(s: &str) -> Result<Rgb, String> {
    let h = s.trim().strip_prefix('#').ok_or_else(|| format!("colour must look like #rrggbb: {s:?}"))?;
    if h.len() != 6 {
        return Err(format!("colour must look like #rrggbb: {s:?}"));
    }
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("bad colour {s:?}"))?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn to_f32(c: Rgb) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
}

fn truthy(v: &str) -> Result<bool, String> {
    match v {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => Err(format!("expected on|off, got {v:?}")),
    }
}

/// The colours a theme editor can change: (key in a theme file, label).
pub const COLOR_KEYS: [(&str, &str); 12] = [
    ("bg", "Tlo"),
    ("fg", "Tekst"),
    ("dim", "Tekst przygaszony"),
    ("accent", "Akcent"),
    ("err", "Blad (czerwony, od 90%)"),
    ("warn", "Ostrzezenie (zolty, od 60%)"),
    ("bar_bg", "Pasek: tlo"),
    ("bar_fg", "Pasek: tekst"),
    ("sel_bg", "Zaznaczenie: tlo"),
    ("sel_fg", "Zaznaczenie: tekst"),
    ("border_normal", "Ramka"),
    ("border_focus", "Ramka aktywnego okna"),
];

pub fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// RGB to (hue 0..360, saturation 0..1, value 0..1).
pub fn rgb_to_hsv(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if mx == 0.0 { 0.0 } else { d / mx }, mx)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgb {
    let (h, s, v) = (h.rem_euclid(360.0), s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [((r + m) * 255.0).round() as u8, ((g + m) * 255.0).round() as u8, ((b + m) * 255.0).round() as u8]
}

impl Theme {
    pub fn color(&self, key: &str) -> Option<Rgb> {
        Some(match key {
            "bg" => self.bg,
            "fg" => self.fg,
            "dim" => self.dim,
            "accent" => self.accent,
            "err" => self.err,
            "warn" => self.warn,
            "bar_bg" => self.bar_bg,
            "bar_fg" => self.bar_fg,
            "sel_bg" => self.sel_bg,
            "sel_fg" => self.sel_fg,
            "border_normal" => self.border_normal,
            "border_focus" => self.border_focus,
            _ => return None,
        })
    }

    pub fn set_color(&mut self, key: &str, c: Rgb) -> bool {
        let slot = match key {
            "bg" => &mut self.bg,
            "fg" => &mut self.fg,
            "dim" => &mut self.dim,
            "accent" => &mut self.accent,
            "err" => &mut self.err,
            "warn" => &mut self.warn,
            "bar_bg" => &mut self.bar_bg,
            "bar_fg" => &mut self.bar_fg,
            "sel_bg" => &mut self.sel_bg,
            "sel_fg" => &mut self.sel_fg,
            "border_normal" => &mut self.border_normal,
            "border_focus" => &mut self.border_focus,
            _ => return false,
        };
        *slot = c;
        true
    }

    /// The theme as a theme file: every colour, so `Theme::parse(name, &t.to_text())` gives the same theme back.
    pub fn to_text(&self) -> String {
        let mut s = String::from("# made in Ustawienia > Motyw > Utworz motyw\n");
        for (k, _) in COLOR_KEYS {
            s += &format!("{k} = {}\n", hex(self.color(k).unwrap_or(self.fg)));
        }
        s += &format!("border = {}\ngap = {}\nanimations = {}\n", self.border, self.gap, if self.animations { "on" } else { "off" });
        s
    }
}

/// Theme names are file names: letters, digits, `-`, `_`, at most 32.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 32 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Theme {
    fn fallback() -> Theme {
        Theme {
            name: "fallback".into(),
            bg: [0x22, 0x22, 0x22],
            fg: [0xeb, 0xdb, 0xb2],
            dim: [0x92, 0x83, 0x74],
            accent: [0xfa, 0xbd, 0x2f],
            err: [0xff, 0, 0],
            warn: [0xf5, 0xc2, 0x1b],
            bar_bg: [0x22, 0x22, 0x22],
            bar_fg: [0xeb, 0xdb, 0xb2],
            sel_bg: [0x45, 0x85, 0x88],
            sel_fg: [0xeb, 0xdb, 0xb2],
            border_normal: [0x44, 0x44, 0x44],
            border_focus: [0, 0x55, 0x77],
            border: 1,
            gap: 0,
            animations: false,
        }
    }

    /// Parse a theme file. Colours not given are derived (bar from bg/fg, selection from the accent).
    pub fn parse(name: &str, text: &str) -> Result<Theme, String> {
        let mut t = Theme::fallback();
        t.name = name.into();
        let (mut bar_bg, mut bar_fg, mut sel_bg, mut sel_fg, mut bn, mut bf) = (None, None, None, None, None, None);
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split(" # ").next().unwrap_or(raw).trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let err = |e: String| format!("{name} line {}: {e}", i + 1);
            let (k, v) = line.split_once('=').ok_or_else(|| err("expected `key = value`".into()))?;
            let (k, v) = (k.trim(), v.trim());
            let int = |v: &str| v.parse::<i32>().map(|n| n.max(0)).map_err(|_| err(format!("not an integer: {v:?}")));
            match k {
                "bg" => t.bg = parse_rgb(v).map_err(err)?,
                "fg" => t.fg = parse_rgb(v).map_err(err)?,
                "dim" => t.dim = parse_rgb(v).map_err(err)?,
                "accent" => t.accent = parse_rgb(v).map_err(err)?,
                "err" | "urgent" => t.err = parse_rgb(v).map_err(err)?,
                "warn" => t.warn = parse_rgb(v).map_err(err)?,
                "bar_bg" => bar_bg = Some(parse_rgb(v).map_err(err)?),
                "bar_fg" => bar_fg = Some(parse_rgb(v).map_err(err)?),
                "sel_bg" => sel_bg = Some(parse_rgb(v).map_err(err)?),
                "sel_fg" => sel_fg = Some(parse_rgb(v).map_err(err)?),
                "border_normal" => bn = Some(parse_rgb(v).map_err(err)?),
                "border_focus" => bf = Some(parse_rgb(v).map_err(err)?),
                "border" => t.border = int(v)?,
                "gap" => t.gap = int(v)?,
                "animations" => t.animations = truthy(v).map_err(err)?,
                other => return Err(err(format!("unknown key {other:?}"))),
            }
        }
        t.bar_bg = bar_bg.unwrap_or(t.bg);
        t.bar_fg = bar_fg.unwrap_or(t.fg);
        t.sel_bg = sel_bg.unwrap_or(t.accent);
        t.sel_fg = sel_fg.unwrap_or(t.bg);
        t.border_normal = bn.unwrap_or(t.dim);
        t.border_focus = bf.unwrap_or(t.accent);
        Ok(t)
    }

    /// A theme by name: the user's own file first, then the built-in ones.
    pub fn load(name: &str) -> Result<Theme, String> {
        if name.is_empty() || name.contains('/') || name.starts_with('.') {
            return Err(format!("bad theme name {name:?}"));
        }
        if let Some(dir) = crate::config::config_dir() {
            if let Ok(text) = std::fs::read_to_string(dir.join("themes").join(name)) {
                return Theme::parse(name, &text);
            }
        }
        match BUILTIN.iter().find(|(n, _)| *n == name) {
            Some((_, text)) => Theme::parse(name, text),
            None => Err(format!("unknown theme {name:?} (have: {})", Theme::list().join(" "))),
        }
    }

    /// Names of the built-in themes plus the files in the user's themes dir.
    /// Is the background light? (mid grey is the line)
    pub fn is_light(&self) -> bool {
        let [r, g, b] = self.bg;
        (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000 > 127
    }

    /// The name of the theme `word` asks for: `dark` / `light` (the current one when it already is that), `toggle` (the other kind).
    /// The partner is the same name with dark and light swapped (`official-dark` <-> `official-light`); a theme without a partner
    /// falls back to the official pair.
    pub fn counterpart(current: &Theme, word: &str, list: &[String]) -> Result<String, String> {
        let want_light = match word {
            "light" => true,
            "dark" => false,
            "toggle" => !current.is_light(),
            _ => return Err(format!("{word:?} is not dark, light or toggle")),
        };
        if want_light == current.is_light() {
            return Ok(current.name.clone());
        }
        let swapped = if current.name.contains("dark") { current.name.replace("dark", "light") } else { current.name.replace("light", "dark") };
        let fallback = if want_light { "official-light" } else { "official-dark" };
        [swapped.as_str(), fallback]
            .into_iter()
            .find(|n| *n != current.name && list.iter().any(|l| l == n))
            .map(str::to_string)
            .ok_or_else(|| format!("no {} theme found", if want_light { "light" } else { "dark" }))
    }

    pub fn list() -> Vec<String> {
        let mut v: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
        if let Some(dir) = crate::config::config_dir() {
            if let Ok(rd) = std::fs::read_dir(dir.join("themes")) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if !n.starts_with('.') && !v.contains(&n) {
                        v.push(n);
                    }
                }
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_and_light_pick_the_partner_theme() {
        let list: Vec<String> = ["official-dark", "official-light", "matrix"].iter().map(|s| s.to_string()).collect();
        let theme = |n: &str| Theme::parse(n, BUILTIN.iter().find(|(b, _)| *b == n).map_or("", |(_, t)| t)).unwrap_or_default();
        let (d, l) = (theme("official-dark"), theme("official-light"));
        assert!(!d.is_light() && l.is_light());
        assert_eq!(Theme::counterpart(&d, "toggle", &list).unwrap(), "official-light");
        assert_eq!(Theme::counterpart(&l, "toggle", &list).unwrap(), "official-dark");
        assert_eq!(Theme::counterpart(&d, "dark", &list).unwrap(), "official-dark", "already dark stays");
        let m = Theme { name: "matrix".into(), ..d.clone() };
        assert_eq!(Theme::counterpart(&m, "light", &list).unwrap(), "official-light", "no partner: the official pair");
        assert!(Theme::counterpart(&m, "light", &["matrix".to_string()]).is_err());
        assert!(Theme::counterpart(&d, "sepia", &list).is_err());
    }

    #[test]
    fn a_theme_survives_a_round_trip_through_its_file() {
        for (n, text) in BUILTIN {
            let t = Theme::parse(n, text).unwrap();
            let back = Theme::parse(n, &t.to_text()).unwrap_or_else(|e| panic!("{n}: {e}"));
            for (k, _) in COLOR_KEYS {
                assert_eq!(t.color(k), back.color(k), "{n} {k}");
            }
            assert_eq!((t.border, t.gap, t.animations), (back.border, back.gap, back.animations));
        }
    }

    #[test]
    fn hsv_and_rgb_agree() {
        for c in [[0, 0, 0], [255, 255, 255], [255, 0, 0], [0, 128, 255], [12, 200, 77], [250, 189, 47]] {
            let (h, s, v) = rgb_to_hsv(c);
            let back = hsv_to_rgb(h, s, v);
            for i in 0..3 {
                assert!((c[i] as i32 - back[i] as i32).abs() <= 1, "{c:?} -> {back:?}");
            }
        }
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hex([255, 0, 16]), "#ff0010");
    }

    #[test]
    fn err_is_the_red_and_urgent_still_reads() {
        assert_eq!(Theme::parse("x", "err = #ff0001\n").unwrap().err, [255, 0, 1]);
        assert_eq!(Theme::parse("x", "urgent = #ff0002\n").unwrap().err, [255, 0, 2], "old theme files keep working");
        assert!(COLOR_KEYS.iter().all(|(k, _)| *k != "urgent"));
    }

    #[test]
    fn theme_names_are_file_safe() {
        assert!(valid_name("my-theme_2") && !valid_name("") && !valid_name("../x") && !valid_name("a b") && !valid_name(&"x".repeat(33)));
    }

    #[test]
    fn colours_are_set_by_key() {
        let mut t = Theme::default();
        assert!(t.set_color("warn", [1, 2, 3]));
        assert_eq!(t.color("warn"), Some([1, 2, 3]));
        assert!(!t.set_color("nope", [0; 3]));
    }

    #[test]
    fn every_builtin_theme_parses() {
        for (n, text) in BUILTIN {
            Theme::parse(n, text).unwrap_or_else(|e| panic!("{e}"));
        }
    }

    #[test]
    fn derived_colours() {
        let t = Theme::parse("x", "bg = #101010\nfg = #ffffff\naccent = #ff0000\n").unwrap();
        assert_eq!(t.bar_bg, [0x10; 3]);
        assert_eq!(t.sel_bg, [255, 0, 0]);
        assert_eq!(t.sel_fg, [0x10; 3]);
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(Theme::parse("x", "bg = red").is_err());
        assert!(Theme::parse("x", "nope = 1").is_err());
        assert!(Theme::load("../etc").is_err());
    }
}
