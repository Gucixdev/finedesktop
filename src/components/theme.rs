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
    pub urgent: Rgb,
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

impl Theme {
    fn fallback() -> Theme {
        Theme {
            name: "fallback".into(),
            bg: [0x22, 0x22, 0x22],
            fg: [0xeb, 0xdb, 0xb2],
            dim: [0x92, 0x83, 0x74],
            accent: [0xfa, 0xbd, 0x2f],
            urgent: [0xff, 0, 0],
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
                "urgent" => t.urgent = parse_rgb(v).map_err(err)?,
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
