//! Screen settings: `output = NAME [mode=WxH[@HZ]] [pos=X,Y] [off]` lines of the config, one per connector. The DRM backend
//! reads them when it starts, so a change made in Ustawienia > Ekrany is saved at once and shows after `ctl session restart`
//! (no live mode switch: a wrong mode must never leave a black screen in the running session). Scale and rotation are not
//! offered: the renderer draws every output at scale 1 (the bar font has its own `font_scale`).

/// What the user asked for one connector.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputCfg {
    pub name: String,
    /// width, height, refresh in Hz (None = the best refresh of that size)
    pub mode: Option<(i32, i32, Option<i32>)>,
    pub pos: Option<(i32, i32)>,
    pub off: bool,
}

/// A mode a connector offers: width, height, refresh in mHz.
pub type ModeInfo = (i32, i32, i32);

impl OutputCfg {
    /// The value of an `output =` line: `HDMI-A-1 mode=1920x1080@60 pos=1920,0`.
    pub fn parse(val: &str) -> Result<OutputCfg, String> {
        let mut w = val.split_whitespace();
        let name = w.next().ok_or("output = NAME [mode=WxH[@HZ]] [pos=X,Y] [off]")?;
        let mut c = OutputCfg { name: name.into(), ..OutputCfg::default() };
        for a in w {
            match a.split_once('=') {
                Some(("mode", m)) => c.mode = Some(parse_mode(m).ok_or_else(|| format!("mode must look like 1920x1080 or 1920x1080@60, got {m:?}"))?),
                Some(("pos", p)) => {
                    let (x, y) = p.split_once(',').ok_or_else(|| format!("pos must look like 1920,0, got {p:?}"))?;
                    c.pos = Some((x.parse().map_err(|_| format!("bad pos x {x:?}"))?, y.parse().map_err(|_| format!("bad pos y {y:?}"))?));
                }
                None if a == "off" => c.off = true,
                _ => return Err(format!("unknown output setting {a:?}")),
            }
        }
        Ok(c)
    }

    pub fn to_line(&self) -> String {
        let mut s = format!("output = {}", self.name);
        if let Some((w, h, r)) = self.mode {
            s += &match r {
                Some(r) => format!(" mode={w}x{h}@{r}"),
                None => format!(" mode={w}x{h}"),
            };
        }
        if let Some((x, y)) = self.pos {
            s += &format!(" pos={x},{y}");
        }
        if self.off {
            s += " off";
        }
        s
    }
}

pub fn parse_mode(m: &str) -> Option<(i32, i32, Option<i32>)> {
    let (size, hz) = match m.split_once('@') {
        Some((s, h)) => (s, Some(h.trim_end_matches("Hz").parse::<f32>().ok()?.round() as i32)),
        None => (m, None),
    };
    let (w, h) = size.split_once('x')?;
    let (w, h) = (w.parse::<i32>().ok()?, h.parse::<i32>().ok()?);
    (w > 0 && h > 0).then_some((w, h, hz))
}

/// Index into `modes` for the wanted mode: exact size and refresh, else the same size at the highest refresh, else None.
pub fn pick_mode(modes: &[ModeInfo], want: (i32, i32, Option<i32>)) -> Option<usize> {
    let same: Vec<usize> = (0..modes.len()).filter(|&i| (modes[i].0, modes[i].1) == (want.0, want.1)).collect();
    if let Some(hz) = want.2 {
        if let Some(&i) = same.iter().find(|&&i| (modes[i].2 as f32 / 1000.0).round() as i32 == hz) {
            return Some(i);
        }
    }
    same.into_iter().max_by_key(|&i| modes[i].2)
}

/// `1920x1080@60` for a mode.
pub fn mode_label(m: ModeInfo) -> String {
    format!("{}x{}@{}", m.0, m.1, (m.2 as f32 / 1000.0).round() as i32)
}

/// Where to put a screen of size `me` next to the one at `at` with size `other`.
pub fn place(side: &str, at: (i32, i32), other: (i32, i32), me: (i32, i32)) -> (i32, i32) {
    match side {
        "left" => (at.0 - me.0, at.1),
        "above" => (at.0, at.1 - me.1),
        "below" => (at.0, at.1 + other.1),
        _ => (at.0 + other.0, at.1),
    }
}

/// `text` (the config file) with the `output =` line of `c.name` replaced, or appended when there is none.
pub fn set_output_line(text: &str, c: &OutputCfg) -> String {
    let mine = |l: &str| l.trim().strip_prefix("output").and_then(|r| r.trim_start().strip_prefix('=')).is_some_and(|v| v.split_whitespace().next() == Some(c.name.as_str()));
    let mut done = false;
    let mut lines: Vec<String> = Vec::new();
    for l in text.lines() {
        if mine(l) {
            if !done {
                lines.push(c.to_line());
                done = true;
            }
        } else {
            lines.push(l.to_string());
        }
    }
    if !done {
        lines.push(c.to_line());
    }
    lines.join("\n") + "\n"
}

/// Save one output's line into the user config (temp file + rename).
pub fn persist(c: &OutputCfg) -> std::io::Result<()> {
    let path = crate::config::user_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, set_output_line(&old, c))?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_round_trips() {
        let c = OutputCfg::parse("HDMI-A-1 mode=1920x1080@60 pos=1920,0").unwrap();
        assert_eq!(c, OutputCfg { name: "HDMI-A-1".into(), mode: Some((1920, 1080, Some(60))), pos: Some((1920, 0)), off: false });
        assert_eq!(OutputCfg::parse(&c.to_line()["output = ".len()..]).unwrap(), c);
        assert_eq!(OutputCfg::parse("DP-1 off").unwrap().to_line(), "output = DP-1 off");
        assert!(OutputCfg::parse("DP-1 mode=big").is_err() && OutputCfg::parse("DP-1 pos=1").is_err() && OutputCfg::parse("DP-1 scale=2").is_err() && OutputCfg::parse("").is_err());
        assert_eq!(parse_mode("2560x1440"), Some((2560, 1440, None)));
        assert_eq!(parse_mode("1920x1080@59.94Hz"), Some((1920, 1080, Some(60))));
    }

    #[test]
    fn the_mode_is_matched_by_size_then_refresh() {
        let m = [(1920, 1080, 60000), (1920, 1080, 144000), (1280, 720, 60000)];
        assert_eq!(pick_mode(&m, (1920, 1080, Some(60))), Some(0));
        assert_eq!(pick_mode(&m, (1920, 1080, None)), Some(1), "no refresh asked: the fastest of that size");
        assert_eq!(pick_mode(&m, (1920, 1080, Some(75))), Some(1), "refresh not offered: the fastest of that size");
        assert_eq!(pick_mode(&m, (3840, 2160, None)), None);
        assert_eq!(mode_label(m[1]), "1920x1080@144");
    }

    #[test]
    fn screens_are_placed_beside_each_other() {
        assert_eq!(place("right", (0, 0), (1920, 1080), (1280, 720)), (1920, 0));
        assert_eq!(place("left", (1920, 0), (1280, 720), (1920, 1080)), (0, 0));
        assert_eq!(place("below", (0, 0), (1920, 1080), (1280, 720)), (0, 1080));
        assert_eq!(place("above", (0, 1080), (1920, 1080), (1280, 720)), (0, 360));
    }

    #[test]
    fn only_that_outputs_line_changes() {
        let t = "gap = 4\noutput = DP-1 off\noutput = HDMI-A-1 mode=1280x720\nbar = top\n";
        let c = OutputCfg { name: "HDMI-A-1".into(), mode: Some((1920, 1080, None)), ..OutputCfg::default() };
        assert_eq!(set_output_line(t, &c), "gap = 4\noutput = DP-1 off\noutput = HDMI-A-1 mode=1920x1080\nbar = top\n");
        assert_eq!(set_output_line("gap = 4\n", &c), "gap = 4\noutput = HDMI-A-1 mode=1920x1080\n");
    }
}
