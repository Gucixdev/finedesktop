//! Panels: text widgets drawn on the desktop. One built-in panel lists the key bindings straight from the live
//! config (so it can never go stale); every other panel is a markdown file: `panels/*.md` in the config dir and any
//! `panel = PATH.md` line in the config. Markdown is rendered as plain terminal text (headings, lists, code, tables,
//! quotes; links and emphasis marks are stripped). Rows flow into columns, left to right.

use std::hash::Hash;

use crate::{
    components::{font::Canvas, theme::Theme},
    config::{Action, Config, WsSel},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    H1,
    H2,
    Text,
    Bullet,
    Code,
    Key,
    Blank,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PLine {
    pub kind: Kind,
    pub text: String,
    /// `Key` rows: the action, `text` is the key label
    pub extra: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Panel {
    pub title: String,
    pub lines: Vec<PLine>,
}

fn line(kind: Kind, text: impl Into<String>) -> PLine {
    PLine { kind, text: text.into(), extra: String::new() }
}

/// Drop emphasis marks, inline code ticks and turn `[text](url)` into `text`.
fn strip_inline(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        let (head, tail) = rest.split_at(i);
        out.push_str(head);
        if let Some(j) = tail.find("](") {
            if let Some(k) = tail[j..].find(')') {
                out.push_str(&tail[1..j]);
                rest = &tail[j + k + 1..];
                continue;
            }
        }
        out.push('[');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out.replace("**", "").replace("__", "").replace('`', "")
}

pub fn parse_md(src: &str) -> Vec<PLine> {
    let mut out = Vec::new();
    let mut in_code = false;
    for raw in src.lines() {
        let l = raw.trim_end();
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push(line(Kind::Code, l));
            continue;
        }
        let t = l.trim_start();
        let indent = l.len() - t.len();
        if t.is_empty() {
            out.push(line(Kind::Blank, ""));
        } else if let Some(h) = t.strip_prefix("## ").or_else(|| t.strip_prefix("### ")) {
            out.push(line(Kind::H2, strip_inline(h)));
        } else if let Some(h) = t.strip_prefix("# ") {
            out.push(line(Kind::H1, strip_inline(h)));
        } else if let Some(b) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
            out.push(line(Kind::Bullet, format!("{}{}", " ".repeat(indent.min(8)), strip_inline(b))));
        } else if let Some(q) = t.strip_prefix("> ") {
            out.push(line(Kind::Text, format!("| {}", strip_inline(q))));
        } else if t.chars().all(|c| matches!(c, '-' | '=' | '*' | '_' | ' ' | '|' | ':')) && t.len() >= 3 {
            // horizontal rule or table separator row: nothing to show
        } else {
            out.push(line(Kind::Text, strip_inline(t)));
        }
    }
    out
}

fn ws_is_number(a: &Action) -> bool {
    matches!(a, Action::View(WsSel::N(_)) | Action::Tag(WsSel::N(_)))
}

/// The key binding panel, generated from the live config.
pub fn keys_panel(cfg: &Config) -> Panel {
    let mut lines = Vec::new();
    for b in &cfg.binds {
        if ws_is_number(&b.action) {
            continue;
        }
        lines.push(PLine { kind: Kind::Key, text: b.label.clone(), extra: b.act.clone() });
    }
    let m = cfg.modkey_label();
    lines.push(PLine { kind: Kind::Key, text: format!("{m}+1..9"), extra: "view workspace".into() });
    lines.push(PLine { kind: Kind::Key, text: format!("{m}+Shift+1..9"), extra: "move window to workspace".into() });
    Panel { title: "Keys".into(), lines }
}

pub fn expand(path: &str) -> std::path::PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(h)) => std::path::PathBuf::from(h).join(rest),
        _ => path.into(),
    }
}

fn file_panel(path: &std::path::Path) -> Option<Panel> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = parse_md(&text);
    // the file's own first "# title" becomes the panel title, otherwise the file name
    let title = match lines.first() {
        Some(l) if l.kind == Kind::H1 => lines.remove(0).text,
        _ => path.file_stem()?.to_string_lossy().into_owned(),
    };
    Some(Panel { title, lines })
}

/// Every panel the config asks for, in order: keys, `panels/*.md`, `panel = FILE` lines.
pub fn load(cfg: &Config) -> Vec<Panel> {
    let mut out = Vec::new();
    if cfg.panel_keys {
        out.push(keys_panel(cfg));
    }
    if let Some(dir) = crate::config::config_dir().map(|d| d.join("panels")) {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")).collect())
            .unwrap_or_default();
        files.sort();
        out.extend(files.iter().filter_map(|p| file_panel(p)));
    }
    out.extend(cfg.panel_files.iter().filter_map(|f| file_panel(&expand(f))));
    out
}

struct Row {
    kind: Kind,
    text: String,
    extra: String,
}

fn wrap(text: &str, width: usize, indent: &str) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        let mut word: Vec<char> = word.chars().collect();
        // a word longer than the column is cut into column-sized pieces
        while word.len() > width {
            if !cur.is_empty() {
                rows.push(std::mem::take(&mut cur));
            }
            rows.push(word.drain(..width).collect());
        }
        let w: String = word.into_iter().collect();
        let need = cur.chars().count() + w.chars().count() + usize::from(!cur.is_empty());
        if need > width && !cur.is_empty() {
            rows.push(std::mem::replace(&mut cur, indent.to_string()));
        } else if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&w);
    }
    if !cur.is_empty() || rows.is_empty() {
        rows.push(cur);
    }
    rows
}

const KEY_COL: usize = 17;

fn rows(panels: &[Panel], colw: usize) -> Vec<Row> {
    let mut out = Vec::new();
    for (pi, p) in panels.iter().enumerate() {
        if pi > 0 {
            out.push(Row { kind: Kind::Blank, text: String::new(), extra: String::new() });
        }
        out.push(Row { kind: Kind::H1, text: p.title.to_uppercase(), extra: String::new() });
        for l in &p.lines {
            match l.kind {
                Kind::Key => {
                    let w = colw.saturating_sub(KEY_COL).max(8);
                    for (i, part) in wrap(&l.extra, w, "").into_iter().enumerate() {
                        out.push(Row { kind: Kind::Key, text: if i == 0 { l.text.clone() } else { String::new() }, extra: part });
                    }
                }
                Kind::Bullet => {
                    let ind = l.text.len() - l.text.trim_start().len();
                    for (i, part) in wrap(l.text.trim_start(), colw.saturating_sub(2 + ind), "").into_iter().enumerate() {
                        let lead = if i == 0 { "* " } else { "  " };
                        out.push(Row { kind: Kind::Bullet, text: format!("{}{lead}{part}", " ".repeat(ind)), extra: String::new() });
                    }
                }
                Kind::Code => out.push(Row { kind: Kind::Code, text: l.text.chars().take(colw).collect(), extra: String::new() }),
                Kind::Blank => out.push(Row { kind: Kind::Blank, text: String::new(), extra: String::new() }),
                k => {
                    for part in wrap(&l.text, colw, "") {
                        out.push(Row { kind: k, text: part, extra: String::new() });
                    }
                }
            }
        }
    }
    out
}

/// Draw the panels into `cv`, flowing rows into columns of `colw` characters.
pub fn render(panels: &[Panel], cv: &mut Canvas, th: &Theme, scale: i32) {
    let (cw, ch) = (8 * scale, 16 * scale);
    let colw = 52usize.min((cv.w / cw).max(10) as usize);
    let per_col = ((cv.h / ch).max(1)) as usize;
    let stride = (colw as i32 + 3) * cw;
    for (i, r) in rows(panels, colw).into_iter().enumerate() {
        let (col, row) = (i / per_col, i % per_col);
        let (x, y) = (col as i32 * stride, row as i32 * ch);
        if x + cw > cv.w {
            break;
        }
        let max = (x + (colw as i32 + 1) * cw).min(cv.w);
        match r.kind {
            Kind::H1 => {
                cv.text(x, y, &r.text, th.accent, scale, max);
                cv.rect(x, y + ch - 1, (r.text.chars().count() as i32 * cw).min(max - x), 1, th.dim);
            }
            Kind::H2 => {
                cv.text(x, y, &r.text, th.accent, scale, max);
            }
            Kind::Key => {
                cv.text(x, y, &r.text, th.accent, scale, max);
                cv.text(x + KEY_COL as i32 * cw, y, &r.extra, th.fg, scale, max);
            }
            Kind::Code => {
                cv.text(x, y, &r.text, th.dim, scale, max);
            }
            Kind::Bullet | Kind::Text => {
                cv.text(x, y, &r.text, th.fg, scale, max);
            }
            Kind::Blank => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_basics() {
        let l = parse_md("# T\n\nsome **bold** and [link](http://x) `code`\n- one\n- two\n```\nraw\n```\n> quote\n---\n");
        assert_eq!(l[0], line(Kind::H1, "T"));
        assert_eq!(l[2].text, "some bold and link code");
        assert_eq!(l[3].kind, Kind::Bullet);
        assert!(l.iter().any(|x| x.kind == Kind::Code && x.text == "raw"));
        assert!(l.iter().any(|x| x.text == "| quote"));
        assert!(!l.iter().any(|x| x.text.starts_with("---")));
    }

    #[test]
    fn wrap_keeps_width() {
        for r in wrap("aaa bbb ccc ddd eee", 8, "") {
            assert!(r.chars().count() <= 8, "{r:?}");
        }
        assert_eq!(wrap("", 8, ""), vec![String::new()]);
    }

    #[test]
    fn keys_panel_lists_live_binds() {
        let c = Config::load();
        let p = keys_panel(&c);
        assert!(p.lines.iter().any(|l| l.text.to_lowercase().contains("return") && l.extra.starts_with("spawn")));
        assert!(!p.lines.iter().any(|l| l.extra.starts_with("view 3")));
    }

    #[test]
    fn render_draws_something() {
        let c = Config::load();
        let mut cv = Canvas::new(800, 400, [0, 0, 0]);
        render(&[keys_panel(&c)], &mut cv, &c.theme, 1);
        assert!(cv.px.chunks(4).any(|p| p[1] > 0));
    }
}
