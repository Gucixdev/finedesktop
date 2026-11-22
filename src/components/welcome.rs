//! The welcome widget: a small browser for markdown files, drawn on the desktop (shown on an empty workspace).
//! Pages are the built-in key list (generated from the live config, so it never goes stale), `welcome/*.md` in the
//! config dir and every `welcome = PATH.md` line. `[text](other.md)` links open that page inside the widget, `http(s)`
//! links open the preferred browser; back and forward keep a history. Rows flow into several columns.
//! Everything is reachable as `ctl welcome ...`, with the keys or with the mouse.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::{
    Desktop,
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

/// A piece of a line; `link` is an index into the page's links.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub text: String,
    pub link: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Line {
    pub kind: Kind,
    pub spans: Vec<Span>,
    /// `Key` rows: the key label (the spans hold the action)
    pub key: String,
    pub indent: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Page(String),
    Url(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Doc {
    /// "home", "keys", or the canonical path of the file
    pub id: String,
    pub title: String,
    pub stem: String,
    pub dir: Option<PathBuf>,
    pub lines: Vec<Line>,
    /// link targets as written in the file
    pub raw_links: Vec<String>,
    /// the same, resolved; None = not a page we have, drawn as plain text
    pub links: Vec<Option<Target>>,
}

impl Doc {
    /// Indices of the links that lead somewhere, in page order.
    pub fn valid_links(&self) -> Vec<usize> {
        (0..self.links.len()).filter(|&i| self.links[i].is_some()).collect()
    }
}

fn line(kind: Kind, spans: Vec<Span>) -> Line {
    Line { kind, spans, key: String::new(), indent: 0 }
}

fn plain(s: &str) -> Vec<Span> {
    vec![Span { text: s.to_string(), link: None }]
}

fn clean(s: &str) -> String {
    s.replace("**", "").replace("__", "").replace('`', "")
}

/// Text with `[label](target)` pieces cut out as link spans; emphasis marks and code ticks are dropped.
fn inline(s: &str, links: &mut Vec<String>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut rest = s;
    let mut text = String::new();
    while let Some(i) = rest.find('[') {
        text.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(j) = tail.find("](") {
            if let Some(k) = tail[j..].find(')') {
                if !text.is_empty() {
                    out.push(Span { text: clean(&std::mem::take(&mut text)), link: None });
                }
                links.push(tail[j + 2..j + k].trim().to_string());
                out.push(Span { text: clean(&tail[1..j]), link: Some(links.len() - 1) });
                rest = &tail[j + k + 1..];
                continue;
            }
        }
        text.push('[');
        rest = &tail[1..];
    }
    text.push_str(rest);
    if !text.is_empty() {
        out.push(Span { text: clean(&text), link: None });
    }
    out
}

pub fn parse_md(src: &str, links: &mut Vec<String>) -> Vec<Line> {
    let mut out = Vec::new();
    let mut in_code = false;
    for raw in src.lines() {
        let l = raw.trim_end();
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push(line(Kind::Code, plain(l)));
            continue;
        }
        let t = l.trim_start();
        let indent = (l.len() - t.len()).min(8);
        if t.is_empty() {
            out.push(line(Kind::Blank, Vec::new()));
        } else if let Some(h) = t.strip_prefix("## ").or_else(|| t.strip_prefix("### ")) {
            out.push(line(Kind::H2, inline(h, links)));
        } else if let Some(h) = t.strip_prefix("# ") {
            out.push(line(Kind::H1, inline(h, links)));
        } else if let Some(b) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
            out.push(Line { indent, ..line(Kind::Bullet, inline(b, links)) });
        } else if let Some(q) = t.strip_prefix("> ") {
            let mut spans = plain("| ");
            spans.extend(inline(q, links));
            out.push(line(Kind::Text, spans));
        } else if t.len() >= 3 && t.chars().all(|c| matches!(c, '-' | '=' | '*' | '_' | ' ' | '|' | ':')) {
            // horizontal rule or table separator row: nothing to show
        } else {
            out.push(line(Kind::Text, inline(t.trim_matches('|').trim(), links)));
        }
    }
    out
}

pub fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(h)) => PathBuf::from(h).join(rest),
        _ => path.into(),
    }
}

fn doc_from_file(path: &Path) -> Option<Doc> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut raw_links = Vec::new();
    let mut lines = parse_md(&text, &mut raw_links);
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    // the file's own first "# title" becomes the page title, otherwise the file name
    let title = match lines.first() {
        Some(l) if l.kind == Kind::H1 => lines.remove(0).spans.iter().map(|s| s.text.as_str()).collect(),
        _ => stem.clone(),
    };
    let id = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned();
    Some(Doc { id, title, stem, dir: path.parent().map(Path::to_path_buf), lines, links: Vec::new(), raw_links })
}

fn ws_is_number(a: &Action) -> bool {
    matches!(a, Action::View(WsSel::N(_)) | Action::Tag(WsSel::N(_)))
}

/// The key binding page, generated from the live config.
pub fn keys_doc(cfg: &Config) -> Doc {
    let mut lines = Vec::new();
    let mut key = |label: String, act: String| lines.push(Line { key: label, ..line(Kind::Key, plain(&act)) });
    for b in &cfg.binds {
        if !ws_is_number(&b.action) {
            key(b.label.clone(), b.act.clone());
        }
    }
    let m = cfg.modkey_label();
    key(format!("{m}+1..9"), "view workspace".into());
    key(format!("{m}+Shift+1..9"), "move window to workspace".into());
    Doc { id: "keys".into(), title: "Keys".into(), stem: "keys".into(), dir: None, lines, raw_links: Vec::new(), links: Vec::new() }
}

fn home_doc(docs: &[Doc]) -> Doc {
    let mut lines = vec![line(Kind::Text, plain("Click a page, or Tab and Enter. Left / Right go back and forward, Home comes back here.")), line(Kind::Blank, Vec::new())];
    let mut links = Vec::new();
    for d in docs {
        links.push(Some(Target::Page(d.id.clone())));
        lines.push(Line { indent: 0, ..line(Kind::Bullet, vec![Span { text: d.title.clone(), link: Some(links.len() - 1) }]) });
    }
    if docs.is_empty() {
        lines.push(line(Kind::Text, plain("No pages yet: put .md files in the welcome folder of the config dir.")));
    }
    Doc { id: "home".into(), title: "Welcome".into(), stem: "home".into(), dir: None, lines, raw_links: Vec::new(), links }
}

/// Where a link written in `from` leads, if anywhere.
fn resolve(raw: &str, from: &Doc, docs: &[Doc]) -> Option<Target> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return (!raw.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '\\')).then(|| Target::Url(raw.into()));
    }
    let t = raw.split('#').next().unwrap_or("").trim();
    if t.is_empty() {
        return None;
    }
    if t == "home" || t == "keys" {
        return Some(Target::Page(t.into()));
    }
    let mut candidates = vec![expand(t)];
    if let Some(dir) = &from.dir {
        candidates.insert(0, dir.join(expand(t)));
    }
    for p in candidates {
        if let Ok(c) = std::fs::canonicalize(&p) {
            let id = c.to_string_lossy();
            if let Some(d) = docs.iter().find(|d| d.id == id) {
                return Some(Target::Page(d.id.clone()));
            }
        }
    }
    let stem = t.trim_end_matches(".md");
    docs.iter().find(|d| d.stem == stem).map(|d| Target::Page(d.id.clone()))
}

/// Every page the config asks for: home, keys, `welcome/*.md`, `welcome = FILE` lines.
pub fn load(cfg: &Config) -> Vec<Doc> {
    let mut docs = Vec::new();
    if cfg.panel_keys {
        docs.push(keys_doc(cfg));
    }
    if let Some(dir) = crate::config::config_dir().map(|d| d.join("welcome")) {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")).collect())
            .unwrap_or_default();
        files.sort();
        docs.extend(files.iter().filter_map(|p| doc_from_file(p)));
    }
    docs.extend(cfg.panel_files.iter().filter_map(|f| doc_from_file(&expand(f))));
    let mut seen = std::collections::HashSet::new();
    docs.retain(|d| seen.insert(d.id.clone()));
    let all = docs.clone();
    for d in &mut docs {
        d.links = d.raw_links.iter().map(|r| resolve(r, d, &all)).collect();
    }
    docs.insert(0, home_doc(&docs));
    docs
}

#[derive(Debug, Default)]
pub struct Welcome {
    pub docs: Vec<Doc>,
    pub hash: u64,
    pub cur: String,
    pub back: Vec<(String, usize)>,
    pub fwd: Vec<(String, usize)>,
    pub scroll: usize,
    /// position in the page's valid links
    pub sel: Option<usize>,
}

impl Welcome {
    pub fn page(&self) -> Option<&Doc> {
        self.docs.iter().find(|d| d.id == self.cur).or_else(|| self.docs.first())
    }

    /// Re-read the pages; true when something changed.
    pub fn reload(&mut self, cfg: &Config) -> bool {
        let docs = load(cfg);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        docs.hash(&mut h);
        let hash = h.finish();
        if hash == self.hash && !self.docs.is_empty() {
            return false;
        }
        self.docs = docs;
        self.hash = hash;
        if !self.docs.iter().any(|d| d.id == self.cur) {
            self.cur = "home".into();
            self.scroll = 0;
        }
        self.back.retain(|(id, _)| self.docs.iter().any(|d| d.id == *id));
        self.fwd.retain(|(id, _)| self.docs.iter().any(|d| d.id == *id));
        self.sel = None;
        true
    }

    pub fn goto(&mut self, id: &str) -> bool {
        if !self.docs.iter().any(|d| d.id == id) {
            return false;
        }
        if id != self.cur {
            self.back.push((std::mem::replace(&mut self.cur, id.to_string()), self.scroll));
            self.fwd.clear();
            self.scroll = 0;
            self.sel = None;
        }
        true
    }

    pub fn go_back(&mut self) {
        if let Some((id, sc)) = self.back.pop() {
            self.fwd.push((std::mem::replace(&mut self.cur, id), self.scroll));
            self.scroll = sc;
            self.sel = None;
        }
    }

    pub fn go_forward(&mut self) {
        if let Some((id, sc)) = self.fwd.pop() {
            self.back.push((std::mem::replace(&mut self.cur, id), self.scroll));
            self.scroll = sc;
            self.sel = None;
        }
    }

    pub fn scroll_by(&mut self, d: i32) {
        let max = self.page().map_or(0, |p| p.lines.len());
        self.scroll = (self.scroll as i64 + d as i64).clamp(0, max as i64) as usize;
    }

    /// Move the selection over the page's links, wrapping around.
    pub fn select(&mut self, d: i32) {
        let n = self.page().map_or(0, |p| p.valid_links().len());
        if n == 0 {
            self.sel = None;
            return;
        }
        self.sel = Some(match self.sel {
            None => if d >= 0 { 0 } else { n - 1 },
            Some(i) => (i as i64 + d as i64).rem_euclid(n as i64) as usize,
        });
    }

    /// The target of the selected link, or of link number `n` (0-based, among the valid ones).
    pub fn target(&self, n: Option<usize>) -> Option<Target> {
        let p = self.page()?;
        let v = p.valid_links();
        p.links[*v.get(n.or(self.sel)?)?].clone()
    }
}

// ---- layout and drawing ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Fg,
    Accent,
    Dim,
    Link,
    Sel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Back,
    Forward,
    Home,
    /// index into the page's links
    Link(usize),
}

#[derive(Clone, Debug)]
pub struct Draw {
    pub x: i32,
    pub y: i32,
    pub text: String,
    pub role: Role,
    pub underline: bool,
    pub hit: Option<Hit>,
}

const KEY_COL: usize = 17;
const COLW: usize = 52;

type Run = (String, Option<usize>);

struct Row {
    /// characters of indent before the first run
    indent: usize,
    runs: Vec<Run>,
    base: Role,
    /// `Key` rows: label drawn in the key column
    key: Option<String>,
}

/// Greedy word wrap of spans to `width` characters; every row keeps which characters belong to which link.
fn wrap(spans: &[Span], width: usize) -> Vec<Vec<Run>> {
    let width = width.max(1);
    let chars: Vec<(char, Option<usize>)> = spans.iter().flat_map(|s| s.text.chars().map(move |c| (c, s.link))).collect();
    let mut words: Vec<Vec<(char, Option<usize>)>> = Vec::new();
    for w in chars.split(|(c, _)| *c == ' ') {
        let mut w = w.to_vec();
        while w.len() > width {
            words.push(w.drain(..width).collect());
        }
        words.push(w);
    }
    let mut rows: Vec<Vec<(char, Option<usize>)>> = vec![Vec::new()];
    for w in words {
        let cur = rows.last_mut().expect("rows is never empty");
        if !cur.is_empty() && cur.len() + 1 + w.len() > width {
            rows.push(w);
        } else if cur.is_empty() {
            cur.extend(w);
        } else {
            let l = if cur.last().map(|c| c.1) == w.first().map(|c| c.1) { cur.last().and_then(|c| c.1) } else { None };
            cur.push((' ', l));
            cur.extend(w);
        }
    }
    rows.into_iter()
        .map(|r| {
            let mut runs: Vec<Run> = Vec::new();
            for (c, l) in r {
                match runs.last_mut() {
                    Some((t, rl)) if *rl == l => t.push(c),
                    _ => runs.push((c.to_string(), l)),
                }
            }
            runs
        })
        .collect()
}

fn rows_of(doc: &Doc, colw: usize) -> Vec<Row> {
    let mut out = Vec::new();
    for l in &doc.lines {
        match l.kind {
            Kind::Blank => out.push(Row { indent: 0, runs: Vec::new(), base: Role::Fg, key: None }),
            Kind::Code => out.push(Row { indent: 0, runs: vec![(l.spans.first().map(|s| s.text.chars().take(colw).collect()).unwrap_or_default(), None)], base: Role::Dim, key: None }),
            Kind::Key => {
                for (i, runs) in wrap(&l.spans, colw.saturating_sub(KEY_COL).max(8)).into_iter().enumerate() {
                    out.push(Row { indent: KEY_COL, runs, base: Role::Fg, key: (i == 0).then(|| l.key.clone()) });
                }
            }
            Kind::Bullet => {
                for (i, mut runs) in wrap(&l.spans, colw.saturating_sub(2 + l.indent)).into_iter().enumerate() {
                    runs.insert(0, (if i == 0 { "* " } else { "  " }.to_string(), None));
                    out.push(Row { indent: l.indent, runs, base: Role::Fg, key: None });
                }
            }
            k => {
                let base = if k == Kind::Text { Role::Fg } else { Role::Accent };
                for runs in wrap(&l.spans, colw) {
                    out.push(Row { indent: 0, runs, base, key: None });
                }
            }
        }
    }
    out
}

/// Everything the widget shows in a `width` x `height` pixel area, as positioned text runs.
pub fn layout(w: &Welcome, width: i32, height: i32, scale: i32) -> Vec<Draw> {
    let Some(page) = w.page() else { return Vec::new() };
    let (cw, ch) = (8 * scale, 16 * scale);
    let mut out = Vec::new();
    let mut d = |x: i32, y: i32, text: &str, role: Role, hit: Option<Hit>, underline: bool| {
        out.push(Draw { x, y, text: text.to_string(), role, underline, hit });
    };
    // header: < > home  Title
    let on = |b: bool| if b { Role::Accent } else { Role::Dim };
    d(0, 0, " < ", on(!w.back.is_empty()), Some(Hit::Back), false);
    d(3 * cw, 0, " > ", on(!w.fwd.is_empty()), Some(Hit::Forward), false);
    d(7 * cw, 0, " home ", on(page.id != "home"), Some(Hit::Home), false);
    d(15 * cw, 0, &page.title.to_uppercase(), Role::Accent, None, false);

    let colw = COLW.min(((width / cw).max(10) as usize).saturating_sub(1));
    let per_col = ((height / ch) - 2).max(1) as usize;
    let stride = (colw as i32 + 3) * cw;
    let selected = w.sel.and_then(|i| page.valid_links().get(i).copied());
    let rows = rows_of(page, colw);
    let start = w.scroll.min(rows.len().saturating_sub(1));
    for (i, r) in rows.iter().skip(start).enumerate() {
        let (col, row) = (i / per_col, i % per_col);
        let (x0, y) = (col as i32 * stride, (row as i32 + 2) * ch);
        if x0 + cw > width {
            break;
        }
        if let Some(k) = &r.key {
            d(x0, y, k, Role::Accent, None, false);
        }
        let mut x = x0 + r.indent as i32 * cw;
        for (text, link) in &r.runs {
            let live = link.is_some_and(|l| page.links.get(l).is_some_and(|t| t.is_some()));
            let (role, hit, ul) = match link {
                Some(l) if live => (if selected == Some(*l) { Role::Sel } else { Role::Link }, Some(Hit::Link(*l)), true),
                _ => (r.base, None, false),
            };
            d(x, y, text, role, hit, ul);
            x += text.chars().count() as i32 * cw;
        }
    }
    out
}

pub fn render(w: &Welcome, cv: &mut Canvas, th: &Theme, scale: i32) {
    let (cw, ch) = (8 * scale, 16 * scale);
    for d in layout(w, cv.w, cv.h, scale) {
        let n = d.text.chars().count() as i32;
        let fg = match d.role {
            Role::Fg => th.fg,
            Role::Accent => th.accent,
            Role::Dim => th.dim,
            Role::Link => th.accent,
            Role::Sel => th.sel_fg,
        };
        if d.role == Role::Sel {
            cv.rect(d.x, d.y, n * cw, ch, th.sel_bg);
        }
        cv.text(d.x, d.y, &d.text, fg, scale, cv.w);
        if d.underline {
            cv.rect(d.x, d.y + ch - scale.max(1), n * cw, scale.max(1), fg);
        }
    }
}

/// What is under the pixel (x, y) of a `width` x `height` widget.
pub fn hit_at(w: &Welcome, width: i32, height: i32, scale: i32, x: i32, y: i32) -> Option<Hit> {
    let (cw, ch) = (8 * scale, 16 * scale);
    layout(w, width, height, scale).into_iter().find_map(|d| {
        let n = d.text.chars().count() as i32;
        (d.hit.is_some() && x >= d.x && x < d.x + n * cw && y >= d.y && y < d.y + ch).then_some(d.hit).flatten()
    })
}

/// What a key does in the widget, as a `ctl welcome` command (None: the key is not ours).
pub fn key_cmd(sym: u32, shift: bool) -> Option<&'static str> {
    Some(match sym {
        0xff51 | 0xff08 => "back",
        0xff53 => "forward",
        0xff50 => "home",
        0xff52 => "scroll -1",
        0xff54 => "scroll 1",
        0xff55 => "scroll -10",
        0xff56 => "scroll 10",
        0xff09 if shift => "prev",
        0xfe20 => "prev",
        0xff09 => "next",
        0xff0d | 0xff8d => "follow",
        _ => return None,
    })
}

// ---- glue to the desktop --------------------------------------------------------------------------------------

impl Desktop {
    /// Re-read the pages; redraw only when something changed.
    pub fn reload_panels(&mut self) {
        if self.welcome.reload(&self.config) {
            self.bar_dirty = true;
        }
    }

    /// The widget is visible when forced on, or when automatic and monitor `mi` shows an empty workspace.
    pub fn panel_visible(&self, mi: usize) -> bool {
        if !self.config.panels || self.welcome.docs.len() < 2 {
            return false;
        }
        self.panel_show.unwrap_or_else(|| self.mons.get(mi).is_some_and(|m| m.ws[m.cur].clients.is_empty()))
    }

    /// (x, y, w, h) of the widget inside output `mi`, in that output's pixels.
    pub fn panel_rect(&self, mi: usize) -> Option<(i32, i32, i32, i32)> {
        let geo = self.space.output_geometry(&self.mons.get(mi)?.output)?;
        let (scale, bh, pos) = (self.config.font_scale, self.config.bar_height(), self.config.bar);
        let top = if pos == crate::config::BarPos::Top { bh } else { 0 };
        let bottom = if pos == crate::config::BarPos::Bottom { bh } else { 0 };
        let m = 16 * scale;
        let (w, h) = (geo.size.w - 2 * m, geo.size.h - top - bottom - 2 * m);
        (w > 80 && h > 32).then_some((m, top + m, w, h))
    }

    /// Keys go to the widget when it is visible on the focused monitor and no window has focus.
    pub fn welcome_active(&self) -> bool {
        self.menu.is_none() && self.panel_visible(self.focus_mon) && self.focused().is_none()
    }

    fn welcome_open(&mut self, t: Target) -> String {
        match t {
            Target::Page(id) => {
                self.welcome.goto(&id);
            }
            Target::Url(u) => {
                let b = self.config.prefer.get("browser").cloned().unwrap_or_else(|| "xdg-open".into());
                crate::keys::spawn(&format!("{b} '{u}'"));
            }
        }
        "ok".into()
    }

    /// A click at the global position `pos`; true if the widget took it.
    pub fn welcome_click(&mut self, pos: smithay::utils::Point<f64, smithay::utils::Logical>) -> bool {
        let Some(mi) = self.mons.iter().position(|m| self.space.output_geometry(&m.output).is_some_and(|g| g.to_f64().contains(pos))) else { return false };
        if !self.panel_visible(mi) {
            return false;
        }
        let (Some((x, y, w, h)), Some(geo)) = (self.panel_rect(mi), self.space.output_geometry(&self.mons[mi].output)) else { return false };
        let (lx, ly) = (pos.x as i32 - geo.loc.x - x, pos.y as i32 - geo.loc.y - y);
        let hit = hit_at(&self.welcome, w, h, self.config.font_scale, lx, ly);
        match hit {
            Some(Hit::Back) => self.welcome.go_back(),
            Some(Hit::Forward) => self.welcome.go_forward(),
            Some(Hit::Home) => {
                self.welcome.goto("home");
            }
            Some(Hit::Link(l)) => {
                let t = self.welcome.page().and_then(|p| p.links.get(l).cloned()).flatten();
                if let Some(t) = t {
                    self.welcome_open(t);
                }
            }
            None => return false,
        }
        self.bar_dirty = true;
        true
    }

    /// `ctl welcome ...`: visibility, navigation, and a text view of the page for scripts and agents.
    pub fn welcome_cmd(&mut self, cmd: &str) -> String {
        let (verb, rest) = cmd.split_once(' ').map_or((cmd, ""), |(a, b)| (a, b.trim()));
        let mi = self.focus_mon;
        let r = match verb {
            "toggle" => {
                self.panel_show = Some(!self.panel_visible(mi));
                "ok".into()
            }
            "show" | "on" => {
                self.panel_show = Some(true);
                "ok".into()
            }
            "hide" | "off" => {
                self.panel_show = Some(false);
                "ok".into()
            }
            "auto" => {
                self.panel_show = None;
                "ok".into()
            }
            "reload" => {
                self.reload_panels();
                "ok".into()
            }
            "back" => {
                self.welcome.go_back();
                "ok".into()
            }
            "forward" => {
                self.welcome.go_forward();
                "ok".into()
            }
            "home" => {
                self.welcome.goto("home");
                "ok".into()
            }
            "scroll" => match rest.parse::<i32>() {
                Ok(n) => {
                    self.welcome.scroll_by(n);
                    "ok".into()
                }
                Err(_) => "error: welcome scroll N".into(),
            },
            "next" | "prev" => {
                self.welcome.select(if verb == "next" { 1 } else { -1 });
                "ok".into()
            }
            "follow" => match self.welcome.target(None) {
                Some(t) => self.welcome_open(t),
                None => "error: no link selected (welcome next)".into(),
            },
            "link" => match rest.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|n| self.welcome.target(Some(n))) {
                Some(t) => self.welcome_open(t),
                None => "error: welcome link N (see welcome list)".into(),
            },
            "open" => {
                let id = self.welcome.docs.iter().find(|d| d.id == rest || d.stem == rest.trim_end_matches(".md") || d.title.eq_ignore_ascii_case(rest)).map(|d| d.id.clone());
                match id {
                    Some(id) => {
                        self.welcome.goto(&id);
                        "ok".into()
                    }
                    None => format!("error: no page {rest:?} (welcome list)"),
                }
            }
            "list" => self.welcome_text(),
            _ => "error: welcome toggle|show|hide|auto|reload|back|forward|home|open PAGE|scroll N|next|prev|follow|link N|list".into(),
        };
        self.bar_dirty = true;
        r
    }

    /// The current page as text: title, its lines, and the numbered links.
    fn welcome_text(&self) -> String {
        let Some(p) = self.welcome.page() else { return String::new() };
        let mut out = vec![format!("# {}", p.title)];
        for l in &p.lines {
            let t: String = l.spans.iter().map(|s| s.text.as_str()).collect();
            out.push(if l.kind == Kind::Key { format!("{}  {t}", l.key) } else { t });
        }
        for (n, i) in p.valid_links().into_iter().enumerate() {
            let label: String = p.lines.iter().flat_map(|l| l.spans.iter()).filter(|s| s.link == Some(i)).map(|s| s.text.as_str()).collect::<Vec<_>>().join("");
            let to = match &p.links[i] {
                Some(Target::Page(id)) => self.welcome.docs.iter().find(|d| d.id == *id).map_or(id.clone(), |d| d.title.clone()),
                Some(Target::Url(u)) => u.clone(),
                None => String::new(),
            };
            out.push(format!("[{}] {label} -> {to}", n + 1));
        }
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> Doc {
        let mut raw = Vec::new();
        let lines = parse_md(src, &mut raw);
        Doc { id: "d".into(), title: "d".into(), stem: "d".into(), dir: None, lines, raw_links: raw, links: Vec::new() }
    }

    #[test]
    fn markdown_basics_and_links() {
        let mut links = Vec::new();
        let l = parse_md("# T\n\nsome **bold** and [a link](other.md) `code`\n- one\n- [two](two)\n```\nraw\n```\n> quote\n---\n", &mut links);
        assert_eq!(l[0].kind, Kind::H1);
        let text: String = l[2].spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "some bold and a link code");
        assert_eq!(links, vec!["other.md", "two"]);
        assert_eq!(l[2].spans.iter().find_map(|s| s.link), Some(0));
        assert!(l.iter().any(|x| x.kind == Kind::Code && x.spans[0].text == "raw"));
        assert!(!l.iter().any(|x| x.spans.iter().any(|s| s.text.starts_with("---"))));
    }

    #[test]
    fn wrap_keeps_width_and_link_membership() {
        let spans = vec![Span { text: "aaa ".into(), link: None }, Span { text: "bbb ccc".into(), link: Some(0) }, Span { text: " ddd".into(), link: None }];
        let rows = wrap(&spans, 8);
        for r in &rows {
            assert!(r.iter().map(|(t, _)| t.chars().count()).sum::<usize>() <= 8, "{r:?}");
        }
        let linked: String = rows.iter().flatten().filter(|(_, l)| *l == Some(0)).map(|(t, _)| t.as_str()).collect();
        assert!(linked.contains("bbb") && linked.contains("ccc") && !linked.contains("ddd") && !linked.contains("aaa"));
    }

    fn welcome_of(docs: Vec<Doc>) -> Welcome {
        Welcome { docs, cur: "home".into(), ..Welcome::default() }
    }

    #[test]
    fn history_goes_back_and_forward() {
        let mut w = welcome_of(vec![home_doc(&[]), keys_doc(&Config::default())]);
        assert!(w.goto("keys"));
        assert_eq!((w.cur.as_str(), w.back.len()), ("keys", 1));
        w.go_back();
        assert_eq!((w.cur.as_str(), w.fwd.len()), ("home", 1));
        w.go_forward();
        assert_eq!(w.cur, "keys");
        assert!(!w.goto("nope"));
        w.go_back();
        w.goto("keys");
        assert!(w.fwd.is_empty(), "a new visit drops the forward history");
    }

    #[test]
    fn links_select_and_resolve() {
        let mut d = doc("[a](keys) [b](nowhere) [c](https://example.org)");
        let docs = vec![d.clone(), keys_doc(&Config::default())];
        d.links = d.raw_links.iter().map(|r| resolve(r, &d, &docs)).collect();
        assert_eq!(d.links, vec![Some(Target::Page("keys".into())), None, Some(Target::Url("https://example.org".into()))]);
        assert_eq!(d.valid_links(), vec![0, 2]);
        let mut w = welcome_of(vec![d]);
        w.cur = "d".into();
        w.select(1);
        assert_eq!(w.target(None), Some(Target::Page("keys".into())));
        w.select(1);
        assert_eq!(w.target(None), Some(Target::Url("https://example.org".into())));
        w.select(1);
        assert_eq!(w.sel, Some(0), "selection wraps");
    }

    #[test]
    fn unsafe_urls_are_not_links() {
        let d = doc("");
        assert_eq!(resolve("https://a.b/x'; rm -rf ~", &d, &[]), None);
    }

    #[test]
    fn a_click_lands_on_the_link_that_was_drawn() {
        let mut d = doc("# T\n\n- [first](keys)\n- [second](keys)\n");
        d.lines.remove(0);
        let docs = vec![d.clone(), keys_doc(&Config::default())];
        d.links = d.raw_links.iter().map(|r| resolve(r, &d, &docs)).collect();
        let mut w = welcome_of(vec![d]);
        w.cur = "d".into();
        let draws = layout(&w, 800, 400, 1);
        let second = draws.iter().find(|x| x.hit == Some(Hit::Link(1))).expect("second link drawn");
        assert_eq!(hit_at(&w, 800, 400, 1, second.x + 2, second.y + 2), Some(Hit::Link(1)));
        assert_eq!(hit_at(&w, 800, 400, 1, 790, 390), None);
        assert_eq!(hit_at(&w, 800, 400, 1, 8, 4), Some(Hit::Back), "back is dim when there is no history, but still a button");
    }

    #[test]
    fn keys_map_to_commands() {
        assert_eq!(key_cmd(0xff51, false), Some("back"));
        assert_eq!(key_cmd(0xff09, true), Some("prev"));
        assert_eq!(key_cmd(0xff09, false), Some("next"));
        assert_eq!(key_cmd(0x61, false), None);
    }

    #[test]
    fn render_draws_something() {
        let c = Config::default();
        let mut w = welcome_of(vec![home_doc(&[keys_doc(&c)]), keys_doc(&c)]);
        let mut cv = Canvas::new(800, 400, [0, 0, 0]);
        render(&w, &mut cv, &c.theme, 1);
        assert!(cv.px.chunks(4).any(|p| p[1] > 0));
        w.goto("keys");
        assert!(layout(&w, 800, 400, 1).iter().any(|d| d.text.to_lowercase().contains("return") || d.text.contains("Super")));
    }
}
