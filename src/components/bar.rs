//! Everything the compositor draws itself, on top of the client windows: the status bar, the tab/stack bar of
//! the monocle/tabbed/list layouts, and window borders. Backends call `overlay()` and render the result.
//! Text goes through `font::Canvas` into an ARGB buffer that is cached per output and only redrawn when its
//! content changes.

use std::hash::{Hash, Hasher};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                Id, Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::SolidColorRenderElement,
            },
            gles::GlesRenderer,
            utils::CommitCounter,
        },
    },
    output::Output,
    utils::{Physical, Point, Rectangle, Transform},
};

use super::{
    stats,
    engine::layout_engine::BarKind,
    barfit,
    font::{self, Canvas},
    theme::{Rgb, to_f32},
    widgets,
};
use crate::{
    Desktop,
    config::BarPos,
};

smithay::backend::renderer::element::render_elements! {
    pub Ov<=GlesRenderer>;
    Mem = MemoryRenderBufferRenderElement<GlesRenderer>,
    Solid = SolidColorRenderElement,
}

/// A picture after an item's text: a history graph (last samples, 0-100) or a fill bar (0-100).
#[derive(Debug, Clone, Hash, PartialEq)]
enum Gfx {
    None,
    Graph(Vec<u8>),
    Fill(u8),
}

#[derive(Debug, Clone, Hash)]
struct Item {
    text: String,
    fg: Rgb,
    bg: Option<Rgb>,
    gfx: Gfx,
}

fn it(text: impl Into<String>, fg: Rgb) -> Item {
    Item { text: text.into(), fg, bg: None, gfx: Gfx::None }
}

fn with_gfx(text: &str, fg: Rgb, gfx: Gfx) -> Item {
    Item { text: text.into(), fg, bg: None, gfx }
}

/// Width of the picture part, in pixels.
fn gfx_w(g: &Gfx, cw: i32, scale: i32) -> i32 {
    match g {
        Gfx::None => 0,
        Gfx::Graph(_) => widgets::HIST as i32 * 2 * scale,
        Gfx::Fill(_) => 8 * cw,
    }
}

/// Text plus gap plus picture.
fn item_w(i: &Item, cw: i32, scale: i32) -> i32 {
    let text = i.text.chars().count() as i32 * cw;
    let g = gfx_w(&i.gfx, cw, scale);
    if g == 0 { text } else { text + if text > 0 { cw / 2 } else { 0 } + g }
}

/// The picture of `i` with its left edge at `x`, spanning bar height `bh`.
fn draw_gfx(cv: &mut Canvas, i: &Item, x: i32, bh: i32, cw: i32, scale: i32, t: &super::theme::Theme) {
    let (gy, gh) = (3 * scale, bh - 6 * scale);
    let gw = gfx_w(&i.gfx, cw, scale);
    cv.rect(x, gy, gw, gh, t.border_normal);
    let hot = |v: u8| stats::level_color(v as u32, t.accent, t);
    match &i.gfx {
        Gfx::None => {}
        Gfx::Graph(h) => {
            let col = 2 * scale;
            let first = widgets::HIST - h.len().min(widgets::HIST);
            for (k, &v) in h.iter().enumerate() {
                let bar = (v as i32 * gh / 100).max(if v > 0 { 1 } else { 0 });
                cv.rect(x + (first + k) as i32 * col, gy + gh - bar, col, bar, hot(v));
            }
        }
        Gfx::Fill(v) => cv.rect(x, gy, (*v as i32).min(100) * gw / 100, gh, hot(*v)),
    }
}

/// What kind of thing a bar item is; neighbours of different kinds get a separator by themselves.
fn kind(name: &str) -> &'static str {
    match name {
        "tags" | "layout" => "workspaces",
        "spk" | "vol" | "mic" => "audio",
        "bat" => "power",
        "date" | "time" | "clock" => "clock",
        "notify" => "notify",
        "cpu" | "ram" | "swap" | "gpu" | "vram" | "disk" | "disks" | "disk-bar" | "stats" => "stats",
        n if n.ends_with("-graph") => "stats",
        _ => "custom",
    }
}

/// Every statistic once: `cpu-graph` already shows the number, so a plain `cpu` next to it goes; `disks` replaces `disk-bar`;
/// a name twice (in any zone) stays where it came first. `|` is kept.
fn tidy(zones: [Vec<String>; 3]) -> [Vec<String>; 3] {
    let all: Vec<&String> = zones.iter().flatten().collect();
    let has = |n: &str| all.iter().any(|x| x.as_str() == n);
    let mut seen: Vec<String> = Vec::new();
    let mut out: [Vec<String>; 3] = Default::default();
    for (z, names) in zones.iter().enumerate() {
        for n in names {
            let covered = ["cpu", "ram", "swap", "gpu", "vram"].contains(&n.as_str()) && has(&format!("{n}-graph")) || n == "disk-bar" && has("disks");
            if n != "|" && (covered || seen.contains(n)) {
                continue;
            }
            seen.push(n.clone());
            out[z].push(n.clone());
        }
    }
    out
}

/// Cut `text` into at most `max` lines of at most `cols` characters; words are not split unless one is longer than a line.
fn wrap(text: &str, cols: usize, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > cols {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            let head: String = word.chars().take(cols).collect();
            word = word.chars().skip(cols).collect();
            lines.push(head);
        }
        let need = if cur.is_empty() { 0 } else { cur.chars().count() + 1 } + word.chars().count();
        if need > cols {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() > max {
        lines.truncate(max);
        if let Some(l) = lines.last_mut() {
            while l.chars().count() + 3 > cols {
                l.pop();
            }
            l.push_str("...");
        }
    }
    lines
}

/// BGRA pixels of the black wedge that rounds one screen corner: `r` by `r`, opaque outside the quarter circle, clear
/// inside, one anti-aliased pixel between. Premultiplied, so black is just the alpha byte.
fn corner_px(r: i32, right: bool, bottom: bool) -> Vec<u8> {
    let mut px = Vec::with_capacity((r * r * 4) as usize);
    for y in 0..r {
        for x in 0..r {
            let (fx, fy) = (if right { r - 1 - x } else { x }, if bottom { r - 1 - y } else { y });
            let d = ((fx as f32 + 0.5 - r as f32).powi(2) + (fy as f32 + 0.5 - r as f32).powi(2)).sqrt();
            let a = ((d - r as f32 + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
            px.extend_from_slice(&[0, 0, 0, a]);
        }
    }
    px
}

fn hash_of<T: Hash>(t: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

impl Desktop {
    /// (left, centre, right) items of the status bar of monitor `mi`.
    fn bar_items(&self, mi: usize) -> (Vec<Item>, Vec<Item>, Vec<Item>) {
        let c = &self.config;
        let t = &c.theme;
        let m = &self.mons[mi];
        let sh = self.widgets.shared.lock().unwrap();
        let one = |name: &str, out: &mut Vec<Item>| match name {
            "tags" => {
                for (i, ws) in m.ws.iter().enumerate() {
                    // the open workspace is shown as >n<, the others as " n "
                    if i == m.cur {
                        out.push(Item { text: format!(">{}<", i + 1), fg: t.sel_fg, bg: Some(t.sel_bg), gfx: Gfx::None });
                    } else if !ws.clients.is_empty() {
                        out.push(it(format!(" {} ", i + 1), t.fg));
                    } else {
                        out.push(it(format!(" {} ", i + 1), t.dim));
                    }
                }
            }
            "layout" => out.push(it(format!("[{}]", m.ws[m.cur].layout.name()), t.accent)),
            "cpu" => out.push(it(format!("cpu {}%", sh.cpu), stats::level_color(sh.cpu, t.fg, t))),
            "ram" => out.push(it(format!("ram {}%", sh.ram), stats::level_color(sh.ram, t.fg, t))),
            "swap" => {
                if let Some(v) = sh.swap {
                    out.push(it(format!("swap {v}%"), stats::level_color(v, t.fg, t)));
                }
            }
            // history graph + the current percentage; both turn yellow from 60 % and red from 90 %
            "cpu-graph" | "ram-graph" | "gpu-graph" | "vram-graph" | "swap-graph" => {
                let key = name.trim_end_matches("-graph");
                if let Some(h) = sh.hist.get(key).filter(|h| !h.is_empty()) {
                    let v = *h.last().unwrap_or(&0) as u32;
                    out.push(with_gfx(&format!("{key} {v}%"), stats::level_color(v, t.fg, t), Gfx::Graph(h.clone())));
                }
            }
            // every real disk: name, used %, a fill bar
            "disks" => {
                for d in &sh.disks {
                    let v = d.used_pct();
                    out.push(with_gfx(&format!("{} {v}%", d.label()), stats::level_color(v, t.fg, t), Gfx::Fill(v as u8)));
                }
            }
            "disk" => {
                if let Some((_, free)) = sh.disk {
                    out.push(it(format!("disk {} free", widgets::human(free)), t.fg));
                }
            }
            "disk-bar" => {
                if let Some((total, free)) = sh.disk {
                    let used = (total - free.min(total)) * 100 / total;
                    out.push(with_gfx(&format!("disk {used}%"), stats::level_color(used as u32, t.fg, t), Gfx::Fill(used as u8)));
                }
            }
            "|" => out.push(it("|", t.dim)),
            "spk" | "vol" | "mic" => {
                let (name, v) = if name == "mic" { ("mic", sh.mic) } else { ("spk", sh.vol) };
                let (text, dim) = super::widgets::audio_label(name, v);
                out.push(it(text, if dim { t.dim } else { t.fg }));
            }
            "gpu" => {
                if let Some((g, _)) = sh.gpu {
                    out.push(it(format!("gpu {g}%"), stats::level_color(g, t.fg, t)));
                }
            }
            "vram" => {
                if let Some((_, v)) = sh.gpu {
                    out.push(it(format!("vram {v}%"), stats::level_color(v, t.fg, t)));
                }
            }
            "bat" => {
                if let Some((p, chg)) = sh.bat {
                    out.push(it(format!("bat {p}%{}", if chg { "+" } else { "" }), if p <= 15 && !chg { t.err } else { t.fg }));
                }
            }
            "notify" => {
                if let Some(n) = self.notes.current() {
                    let from = if n.app == "ctl" { String::new() } else { format!("{}: ", n.app) };
                    out.push(it(format!("! {from}{}", n.text.chars().take(60).collect::<String>()), t.err));
                } else if self.notes.dnd || self.notes.snoozed() {
                    let missed = self.notes.missed();
                    out.push(it(if missed > 0 { format!("dnd ({missed})") } else { "dnd".into() }, t.dim));
                }
            }
            "date" => out.push(it(widgets::now(&c.date_format), t.fg)),
            "clock" | "time" => out.push(it(widgets::now(&c.clock_format), t.accent)),
            other => {
                if let Some(txt) = sh.text.get(other).filter(|s| !s.is_empty()) {
                    let (fg, txt) = match txt.split_once(' ') {
                        Some((c, rest)) if c.starts_with('#') => (super::theme::parse_rgb(c).unwrap_or(t.fg), rest.to_string()),
                        _ => (t.fg, txt.clone()),
                    };
                    out.push(it(txt, fg));
                }
            }
        };
        // `stats` is a shorthand for the whole set
        const STATS: [&str; 6] = ["cpu-graph", "gpu-graph", "ram-graph", "vram-graph", "swap-graph", "disks"];
        let expand = |names: &Vec<String>| -> Vec<String> { names.iter().flat_map(|n| if n == "stats" { STATS.iter().map(|s| s.to_string()).collect() } else { vec![n.clone()] }).collect() };
        let zones = tidy([expand(&c.bar_left), expand(&c.bar_center), expand(&c.bar_right)]);
        let sep = it("|", t.dim);
        let is_sep = |i: &Item| i.text == "|" && i.gfx == Gfx::None;
        let fill = |names: &Vec<String>| -> Vec<Item> {
            let mut out: Vec<Item> = Vec::new();
            let mut last: Option<&str> = None;
            for n in names {
                if n == "|" {
                    if out.last().is_some_and(|i| !is_sep(i)) {
                        out.push(sep.clone());
                    }
                    last = None;
                    continue;
                }
                let before = out.len();
                one(n, &mut out);
                if out.len() == before {
                    continue;
                }
                let k = kind(n);
                // neighbours of different kinds are told apart by a separator the bar draws itself
                if last.is_some_and(|l| l != k) && before > 0 && !is_sep(&out[before - 1]) {
                    out.insert(before, sep.clone());
                }
                last = Some(k);
            }
            while out.last().is_some_and(&is_sep) {
                out.pop();
            }
            out
        };
        let (left, center, right) = (fill(&zones[0]), fill(&zones[1]), fill(&zones[2]));
        (left, center, right)
    }

    fn cached(&mut self, key: String, w: i32, h: i32, hash: u64, draw: impl FnOnce(&mut Canvas)) -> MemoryRenderBuffer {
        if let Some((hh, buf)) = self.bar_cache.get(&key) {
            if *hh == hash {
                return buf.clone();
            }
        }
        let mut cv = Canvas::new(w, h, [0, 0, 0]);
        draw(&mut cv);
        let buf = MemoryRenderBuffer::from_slice(&cv.px, Fourcc::Argb8888, (cv.w, cv.h), 1, Transform::Normal, None);
        self.bar_cache.insert(key, (hash, buf.clone()));
        buf
    }

    fn solid_id(&mut self, key: (String, usize)) -> Id {
        self.solid_ids.entry(key).or_insert_with(Id::new).clone()
    }
}

/// Elements to draw above the windows of `output`, topmost first.
pub fn overlay(state: &mut Desktop, renderer: &mut GlesRenderer, output: &Output) -> Vec<Ov> {
    let mut out: Vec<Ov> = Vec::new();
    let Some(mi) = state.mons.iter().position(|m| m.output == *output) else { return out };
    let Some(geo) = state.space.output_geometry(output) else { return out };
    let (t, scale, bh, pos) = (state.config.theme.clone(), state.config.font_scale, state.config.bar_height(), state.config.bar);
    let name = output.name();
    let pad = 6;
    let (fcw, fch) = { let f = font::Font::current(); (f.w, f.h) };
    let ty = (bh - fch * scale) / 2;

    // window borders (inside the window rectangle), focused one in the focus colour
    if t.border > 0 {
        let focused = state.focused().map(|(_, w)| w.clone());
        let wins: Vec<_> = state.mons[mi].ws[state.mons[mi].cur].clients.clone();
        let mut n = 0;
        for w in wins {
            let Some(g) = state.space.element_geometry(&w) else { continue };
            let g: Rectangle<i32, Physical> = Rectangle::new((g.loc.x - geo.loc.x, g.loc.y - geo.loc.y).into(), (g.size.w, g.size.h).into());
            let col = if Some(&w) == focused.as_ref() && mi == state.focus_mon { t.border_focus } else { t.border_normal };
            let b = t.border;
            let edges = [
                ((g.loc.x, g.loc.y), (g.size.w, b)),
                ((g.loc.x, g.loc.y + g.size.h - b), (g.size.w, b)),
                ((g.loc.x, g.loc.y), (b, g.size.h)),
                ((g.loc.x + g.size.w - b, g.loc.y), (b, g.size.h)),
            ];
            for (p, s) in edges {
                let id = state.solid_id((name.clone(), n));
                n += 1;
                let rect = Rectangle::new(p.into(), s.into());
                let commit = CommitCounter::from(hash_of(&(p, s, col)) as usize);
                out.push(SolidColorRenderElement::new(id, rect, commit, to_f32(col), Kind::Unspecified).into());
            }
        }
    }

    // tab / stack bar of monocle, tabbed, list ... drawn over the top of the window area
    if let Some(bar) = state.mons[mi].bar {
        let ws = &state.mons[mi].ws[state.mons[mi].cur];
        let titles: Vec<String> = ws
            .clients
            .iter()
            .map(|w| {
                let (id, _, title) = Desktop::win_info(w);
                state.names.get(&id).cloned().unwrap_or(title)
            })
            .collect();
        let focus = ws.focus;
        let (r, kind) = (bar.rect, bar.kind);
        let key = format!("{name}:tabs");
        let hash = hash_of(&(&titles, focus, r.w, r.h, t.name.clone(), scale));
        let buf = state.cached(key, r.w, r.h, hash, |cv| {
            cv.rect(0, 0, r.w, r.h, t.bar_bg);
            let n = titles.len().max(1) as i32;
            for (i, ti) in titles.iter().enumerate() {
                let (x, y, w, h) = match kind {
                    BarKind::Tabs => (i as i32 * r.w / n, 0, r.w / n, r.h),
                    BarKind::Rows => (0, i as i32 * (r.h / n), r.w, r.h / n),
                };
                let (bg, fg) = if i == focus { (t.sel_bg, t.sel_fg) } else { (t.bar_bg, t.dim) };
                cv.rect(x, y, w - 1, h, bg);
                cv.text(x + pad, y + (h - fch * scale) / 2, ti, fg, scale, x + w - 1);
            }
        });
        let at: Point<f64, Physical> = ((r.x - geo.loc.x) as f64, (r.y - geo.loc.y) as f64).into();
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, at, &buf, None, None, None, Kind::Unspecified) {
            out.push(e.into());
        }
    }

    // status bar
    if pos != BarPos::Off {
        let (left, center, right) = state.bar_items(mi);
        let w = geo.size.w;
        let key = format!("{name}:bar");
        let hash = hash_of(&(&left, &center, &right, w, bh, t.name.clone(), scale));
        let buf = state.cached(key, w, bh, hash, |cv| {
            cv.rect(0, 0, w, bh, t.bar_bg);
            let cw = fcw * scale;
            // the left zone is packed tighter than the others (half padding)
            let slot = |i: &Item, p: i32| item_w(i, cw, scale) + if i.bg.is_some() { 0 } else { p };
            let widths = |v: &[Item], p: i32| -> Vec<i32> { v.iter().map(|i| slot(i, p)).collect() };
            let fit = barfit::fit(w, pad * 2, &widths(&left, pad), &widths(&center, pad * 2), &widths(&right, pad * 2));
            // one item in a slot starting at x; text and picture are clipped at `clip` so a zone never paints into its neighbour
            let mut put = |i: &Item, x: i32, p: i32, clip: i32| {
                let sw = slot(i, p);
                if let Some(bg) = i.bg {
                    cv.rect(x, 0, sw.min(clip - x).max(0), bh, bg);
                }
                let tx = x + if i.bg.is_some() { 0 } else { p / 2 };
                cv.text(tx, ty, &i.text, if i.gfx == Gfx::None { i.fg } else { t.dim }, scale, clip);
                if i.gfx != Gfx::None {
                    let gx = tx + i.text.chars().count() as i32 * cw + if i.text.is_empty() { 0 } else { cw / 2 };
                    if gx + gfx_w(&i.gfx, cw, scale) <= clip {
                        draw_gfx(cv, i, gx, bh, cw, scale, &t);
                    }
                }
                sw
            };
            let mut x = 0;
            for i in &left[..fit.left] {
                x += put(i, x, pad, fit.center_x);
            }
            let mut x = fit.center_x;
            let cend = x + widths(&center, pad * 2).iter().sum::<i32>();
            for i in &center {
                x += put(i, x, pad * 2, cend.min(w));
            }
            let mut rx = w;
            for i in right[fit.right_skip..].iter().rev() {
                rx -= slot(i, pad * 2);
                put(i, rx, pad * 2, w);
            }
            cv.rect(0, if pos == BarPos::Top { bh - 1 } else { 0 }, w, 1, t.border_normal);
        });
        let y = if pos == BarPos::Top { 0 } else { geo.size.h - bh };
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, (0.0, y as f64), &buf, None, None, None, Kind::Unspecified) {
            out.push(e.into());
        }
    }
    // panels: key list and markdown widgets, on empty workspaces (or when forced with `welcome show`)
    if state.panel_visible(mi) {
        if let Some((x, y, w, h)) = state.panel_rect(mi) {
            let key = format!("{name}:panels");
            let wl = &state.welcome;
            let hash = hash_of(&(&wl.docs, &wl.cur, wl.scroll, wl.sel, wl.back.len(), wl.fwd.len(), w, h, t.name.clone(), scale));
            let th = t.clone();
            let wl = std::mem::take(&mut state.welcome);
            let buf = state.cached(key, w, h, hash, |cv| {
                cv.rect(0, 0, w, h, th.bg);
                crate::components::welcome::render(&wl, cv, &th, scale);
            });
            state.welcome = wl;
            if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, (x as f64, y as f64), &buf, None, None, None, Kind::Unspecified) {
                out.push(e.into());
            }
        }
    }
    // notifications: cards in the top right corner (under the bar), newest on top; they go after a few seconds or `ctl notify dismiss`
    if mi == state.focus_mon {
        let cards: Vec<(u32, String, String, bool)> = state
            .notes
            .toasts(4)
            .iter()
            .map(|n| (n.id, format!("{}  {}", if n.app == "ctl" { "" } else { n.app.as_str() }, n.stamp).trim().to_string(), n.text.clone(), n.urgency == super::notification::notts::Urgency::Critical))
            .collect();
        let cols = (geo.size.w / (fcw * scale) / 3).clamp(20, 44) as usize;
        let cw = fcw * scale;
        let card_w = (cols as i32 * cw + pad * 2).min(geo.size.w - 16);
        let mut y = if pos == BarPos::Top { bh + 8 } else { 8 };
        let mut stack: Vec<Ov> = Vec::new();
        for (n, (id, head, text, crit)) in cards.iter().enumerate() {
            let lines = wrap(text, cols, 4);
            let h = (1 + lines.len() as i32) * fch * scale + pad * 3;
            let edge = if *crit { t.err } else { t.border_focus };
            let hash = hash_of(&(id, head, text, crit, card_w, scale, t.name.clone()));
            let th = t.clone();
            let buf = state.cached(format!("{name}:toast{n}"), card_w, h, hash, |cv| {
                cv.rect(0, 0, card_w, h, edge);
                cv.rect(2, 2, card_w - 4, h - 4, th.bg);
                cv.text(pad, pad, head, th.dim, scale, card_w - pad);
                for (k, l) in lines.iter().enumerate() {
                    cv.text(pad, pad * 2 + (k as i32 + 1) * fch * scale, l, th.fg, scale, card_w - pad);
                }
            });
            if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, ((geo.size.w - card_w - 10) as f64, y as f64), &buf, None, None, None, Kind::Unspecified) {
                stack.push(e.into());
            }
            y += h + 6;
        }
        // topmost first: the newest card is the first one
        out.splice(0..0, stack);
    }
    // the menu: one big centred list panel above everything, on the monitor that has focus. The settings panel has
    // its tabs down the left; the panel slides in on a spring.
    if mi == state.focus_mon {
        let moving = state.fx.step(std::time::Instant::now());
        if moving {
            state.bar_dirty = true;
        }
        let off = state.fx.panel_offset();
        if let Some(menu) = state.menu.as_ref() {
            // 2x text on a big screen; the panel takes most of it, like the old dmenu setup
            let ms = if geo.size.h >= 900 { scale * 2 } else { scale };
            let lh = fch * ms;
            let tabs = menu.tabs();
            let tabw = tabs.as_ref().map_or(0, |t| (t.labels.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i32 + 3) * fcw * ms);
            let wide = if tabs.is_some() { 4 } else { 3 };
            let w = (geo.size.w * wide / 5).clamp(320.min(geo.size.w), 110 * fcw * ms + tabw).min(geo.size.w - 16);
            let max_h = (geo.size.h - bh) * 4 / 5;
            let cap = ((max_h - 2 * pad) / lh - 2).max(1) as usize;
            // the right side: the open level, or (cursor on the tabs) a preview of the tab under the cursor
            let (all, sel_row) = match &tabs {
                Some(t) if t.focused => {
                    let rows = t.id.map(|id| state.menu_level(id).items.into_iter().filter(|e| !matches!(e.act, super::menu::Act::Head)).map(|e| e.row()).collect()).unwrap_or_default();
                    (rows, usize::MAX)
                }
                _ => menu.rows(),
            };
            let nrows = all.len().max(tabs.as_ref().map_or(0, |t| t.labels.len())).min(cap).max(1);
            let first = if sel_row == usize::MAX { 0 } else { (sel_row + 1).saturating_sub(nrows) };
            let shown: Vec<super::menu::Row> = all.iter().skip(first).take(nrows).cloned().collect();
            let sel = sel_row.wrapping_sub(first);
            let head = format!("{} > {}_", menu.crumbs(), menu.query);
            let count = format!("{}/{}", menu.visible().len(), menu.selectable());
            let h = (nrows as i32 + 2) * lh + 2 * pad;
            if w > 80 && h > 32 {
                let key = format!("{name}:menu");
                let hash = hash_of(&(format!("{shown:?}{tabs:?}"), sel, &head, &count, w, h, t.name.clone(), ms));
                let th = t.clone();
                let buf = state.cached(key, w, h, hash, |cv| {
                    cv.rect(0, 0, w, h, th.border_focus);
                    cv.rect(2, 2, w - 4, h - 4, th.bg);
                    cv.text(pad, pad, &head, th.accent, ms, w - pad);
                    let cw = Canvas::text_width(&count, ms);
                    cv.text(w - cw - pad, pad, &count, th.dim, ms, w);
                    if let Some(tb) = &tabs {
                        cv.rect(tabw, 2 * lh + pad, 1, h - 2 * lh - pad - 2, th.border_normal);
                        for (n, l) in tb.labels.iter().enumerate().take(nrows) {
                            let y = pad + (n as i32 + 2) * lh;
                            let on = n == tb.active;
                            if on {
                                cv.rect(2, y, tabw - 2, lh, if tb.focused { th.sel_bg } else { th.bar_bg });
                            }
                            let fg = match (on, tb.focused) {
                                (true, true) => th.sel_fg,
                                (true, false) => th.accent,
                                _ => th.dim,
                            };
                            cv.text(pad, y, l, fg, ms, tabw - 2);
                        }
                    }
                    let x0 = if tabs.is_some() { tabw + pad } else { pad };
                    for (n, r) in shown.iter().enumerate() {
                        let y = pad + (n as i32 + 2) * lh;
                        match r {
                            super::menu::Row::Head(t) => {
                                cv.text(x0, y, &format!("-- {t} --"), th.dim, ms, w - pad);
                            }
                            super::menu::Row::Check(label, on) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel { th.sel_fg } else { th.fg };
                                let bx = x0 + if tabs.is_some() { 0 } else { pad };
                                let sz = lh * 2 / 3;
                                let by = y + (lh - sz) / 2;
                                cv.rect(bx, by, sz, sz, fg);
                                cv.rect(bx + 2, by + 2, sz - 4, sz - 4, if n == sel { th.sel_bg } else { th.bg });
                                if *on {
                                    cv.rect(bx + 4, by + 4, sz - 8, sz - 8, if n == sel { th.sel_fg } else { th.accent });
                                }
                                cv.text(bx + sz + pad, y, label, fg, ms, w - pad);
                            }
                            super::menu::Row::Slider(label, frac, text) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel { th.sel_fg } else { th.fg };
                                let bx = x0 + if tabs.is_some() { 0 } else { pad };
                                let span = w - bx - pad;
                                let (track_x, track_w) = (bx + span * 11 / 20, span * 3 / 10);
                                let th_h = (lh / 4).max(2);
                                let ty = y + (lh - th_h) / 2;
                                cv.text(bx, y, label, fg, ms, track_x - bx - pad);
                                cv.rect(track_x, ty, track_w, th_h, th.border_normal);
                                let fill = (track_w as f32 * frac) as i32;
                                cv.rect(track_x, ty, fill, th_h, if n == sel { th.sel_fg } else { th.accent });
                                let knob = (lh / 2).max(4);
                                cv.rect(track_x + fill - knob / 2, y + (lh - knob) / 2, knob, knob, fg);
                                cv.text(track_x + track_w + pad * 2, y, text, fg, ms, w - pad);
                            }
                            super::menu::Row::Color(label, c) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel { th.sel_fg } else { th.fg };
                                let bx = x0 + if tabs.is_some() { 0 } else { pad };
                                let (sw, sh) = (lh * 2, lh * 2 / 3);
                                cv.text(bx, y, label, fg, ms, w - pad - sw - pad * 8 * ms);
                                let sx = w - pad - sw - Canvas::text_width("#rrggbb", ms) - pad;
                                cv.rect(sx - 1, y + (lh - sh) / 2 - 1, sw + 2, sh + 2, fg);
                                cv.rect(sx, y + (lh - sh) / 2, sw, sh, *c);
                                cv.text(sx + sw + pad, y, &super::theme::hex(*c), fg, ms, w);
                            }
                            super::menu::Row::Choice(label, value) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel { th.sel_fg } else { th.fg };
                                let bx = x0 + if tabs.is_some() { 0 } else { pad };
                                // the closed dropdown: a box with the current value and an arrow, on the right half
                                let bw = (w - bx - pad) * 9 / 20;
                                let box_x = w - pad - bw;
                                cv.text(bx, y, label, fg, ms, box_x - bx - pad);
                                cv.rect(box_x, y + 1, bw, lh - 2, fg);
                                cv.rect(box_x + 1, y + 2, bw - 2, lh - 4, if n == sel { th.sel_bg } else { th.bg });
                                cv.text(box_x + pad, y, value, fg, ms, box_x + bw - lh);
                                cv.text(box_x + bw - lh + pad, y, "v", if n == sel { th.sel_fg } else { th.accent }, ms, box_x + bw);
                            }
                            super::menu::Row::Item(s) | super::menu::Row::Icon(s, _) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel {
                                    th.sel_fg
                                } else if tabs.as_ref().is_some_and(|t| t.focused) {
                                    th.dim
                                } else {
                                    th.fg
                                };
                                let mut tx = x0 + if tabs.is_some() { 0 } else { pad };
                                if let super::menu::Row::Icon(_, name) = r {
                                    // the picture sits in the row's height; an icon without a PNG leaves its space empty so labels line up
                                    let side = (lh - 2).max(8) as usize;
                                    if let Some(img) = super::icons::load(name, side) {
                                        cv.blit(tx, y + 1, &img);
                                    }
                                    tx += side as i32 + pad;
                                }
                                cv.text(tx, y, s, fg, ms, w - pad);
                            }
                        }
                    }
                });
                // slides down from above and fades up as the spring settles
                let slide = (off * 40.0 * ms as f32) as i32;
                let alpha = (1.0 - off).clamp(0.0, 1.0);
                let (x, y) = ((geo.size.w - w) / 2, bh + (geo.size.h - bh - h) / 2 - slide);
                if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, (x as f64, y as f64), &buf, Some(alpha), None, None, Kind::Unspecified) {
                    out.insert(0, e.into());
                }
            }
        }
    }
    // rounded screen corners: black wedges above everything
    let r = state.config.corner_radius;
    if state.config.corners && r > 1 {
        let (w, h) = (geo.size.w, geo.size.h);
        for (k, (x, y, right, bottom)) in [(0, 0, false, false), (w - r, 0, true, false), (0, h - r, false, true), (w - r, h - r, true, true)].into_iter().enumerate() {
            let buf = state.cached(format!("{name}:corner{k}"), r, r, hash_of(&(r, k)), |cv| cv.px = corner_px(r, right, bottom));
            if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, (x as f64, y as f64), &buf, None, None, None, Kind::Unspecified) {
                out.insert(0, e.into());
            }
        }
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn a_statistic_and_its_graph_are_one_item() {
        let z = tidy([v("tags"), v(""), v("cpu ram spk | cpu-graph ram-graph disk-bar disks")]);
        assert_eq!(z[2], v("spk | cpu-graph ram-graph disks"));
    }

    #[test]
    fn the_same_name_is_not_drawn_twice_in_any_zone() {
        let z = tidy([v("tags time"), v(""), v("date time")]);
        assert_eq!((z[0].clone(), z[2].clone()), (v("tags time"), v("date")));
    }

    #[test]
    fn kinds_decide_where_separators_go() {
        assert_eq!(kind("spk"), kind("mic"));
        assert_ne!(kind("mic"), kind("cpu-graph"));
        assert_eq!(kind("date"), kind("time"));
        assert_eq!(kind("cpu-graph"), kind("disks"));
    }

    #[test]
    fn wrap_keeps_words_and_caps_the_lines() {
        assert_eq!(wrap("aa bb cc", 5, 3), vec!["aa bb".to_string(), "cc".to_string()]);
        assert_eq!(wrap("abcdefghij", 4, 5), v("abcd efgh ij"));
        let l = wrap("one two three four five six seven", 7, 2);
        assert_eq!(l.len(), 2);
        assert!(l[1].ends_with("..."), "{l:?}");
        assert!(l.iter().all(|x| x.chars().count() <= 7));
    }

    #[test]
    fn a_corner_is_opaque_outside_and_clear_inside() {
        let r = 16;
        let px = corner_px(r, false, false);
        let a = |x: i32, y: i32| px[((y * r + x) * 4 + 3) as usize];
        assert_eq!(a(0, 0), 255);
        assert_eq!(a(r - 1, r - 1), 0);
        // the other three are the same wedge mirrored
        let br = corner_px(r, true, true);
        assert_eq!(br[((r * r - 1) * 4 + 3) as usize], 255);
        assert_eq!(corner_px(r, true, false)[(r - 1) as usize * 4 + 3], 255);
    }
}
