//! The wallpaper command line as a typed tree. `ctl wallpaper ...` is parsed into a `Cmd` first, so a typo is an
//! error with the usage in it and nothing has run yet; `api.rs` executes the tree.

/// How a picture is fitted to an output (swaybg's `-m`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Fill, Mode::Fit, Mode::Stretch, Mode::Center, Mode::Tile];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Fill => "fill",
            Mode::Fit => "fit",
            Mode::Stretch => "stretch",
            Mode::Center => "center",
            Mode::Tile => "tile",
        }
    }

    pub fn parse(s: &str) -> Result<Mode, String> {
        Mode::ALL.into_iter().find(|m| m.name() == s).ok_or_else(|| format!("mode is {}, not {s:?}", Mode::ALL.map(Mode::name).join("|")))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cmd {
    /// what is set, per output
    Show,
    /// the pictures found
    List,
    /// the next / previous found picture becomes the default one
    Next,
    Prev,
    Mode(Mode),
    /// `set [-o OUTPUT] PATH`; no output = every output without its own picture
    Set { output: Option<String>, path: String },
    /// `clear [OUTPUT]`; no output = the default picture
    Clear { output: Option<String> },
}

pub const USAGE: &str = "wallpaper [list|next|prev|mode fill|fit|stretch|center|tile|set [-o OUTPUT] PATH|clear [OUTPUT]]";

pub fn parse(line: &str) -> Result<Cmd, String> {
    let line = line.trim();
    let (word, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(a, b)| (a, b.trim()));
    let no_args = |c: Cmd| if rest.is_empty() { Ok(c) } else { Err(format!("{word} takes no argument; usage: {USAGE}")) };
    match word {
        "" | "show" => no_args(Cmd::Show),
        "list" => no_args(Cmd::List),
        "next" => no_args(Cmd::Next),
        "prev" => no_args(Cmd::Prev),
        "mode" => Mode::parse(rest).map(Cmd::Mode),
        "clear" if rest.contains(char::is_whitespace) => Err(format!("clear takes one output; usage: {USAGE}")),
        "clear" => Ok(Cmd::Clear { output: (!rest.is_empty()).then(|| rest.to_string()) }),
        "set" => {
            let (output, path) = match rest.strip_prefix("-o ") {
                Some(r) => {
                    let (o, p) = r.trim_start().split_once(char::is_whitespace).ok_or("set -o OUTPUT PATH: the path is missing")?;
                    (Some(o.to_string()), p.trim())
                }
                None => (None, rest),
            };
            if path.is_empty() {
                return Err(format!("set needs a path; usage: {USAGE}"));
            }
            Ok(Cmd::Set { output, path: path.to_string() })
        }
        _ => Err(format!("unknown wallpaper command {word:?}; usage: {USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_parse() {
        assert_eq!(parse(""), Ok(Cmd::Show));
        assert_eq!(parse("list"), Ok(Cmd::List));
        assert_eq!(parse("next"), Ok(Cmd::Next));
        assert_eq!(parse("mode tile"), Ok(Cmd::Mode(Mode::Tile)));
        assert_eq!(parse("clear"), Ok(Cmd::Clear { output: None }));
        assert_eq!(parse("clear DP-1"), Ok(Cmd::Clear { output: Some("DP-1".into()) }));
    }

    #[test]
    fn set_takes_an_optional_output_and_a_path_with_spaces() {
        assert_eq!(parse("set /a/b c.png"), Ok(Cmd::Set { output: None, path: "/a/b c.png".into() }));
        assert_eq!(parse("set -o HDMI-A-1 /a.png"), Ok(Cmd::Set { output: Some("HDMI-A-1".into()), path: "/a.png".into() }));
    }

    #[test]
    fn mistakes_are_errors_that_show_the_usage() {
        for bad in ["set", "set -o DP-1", "mode zoom", "list now", "clear a b", "frobnicate"] {
            let e = parse(bad).unwrap_err();
            assert!(e.contains("fill") || e.contains("usage") || e.contains("missing"), "{bad}: {e}");
        }
    }
}
