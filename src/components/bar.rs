//! Everything the compositor draws itself, on top of the client windows: the status bar, the tab/stack bar of
//! the monocle/tabbed/deck layouts, and window borders. Backends call `overlay()` and render the result.
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
    engine::layout_engine::BarKind,
    font::Canvas,
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

#[derive(Debug, Clone, Hash)]
struct Item {
    text: String,
    fg: Rgb,
    bg: Option<Rgb>,
}

fn it(text: impl Into<String>, fg: Rgb) -> Item {
    Item { text: text.into(), fg, bg: None }
}

fn hash_of<T: Hash>(t: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

impl Desktop {
    /// (left items, title, right items) of the status bar of monitor `mi`.
    fn bar_items(&self, mi: usize) -> (Vec<Item>, String, Vec<Item>) {
        let c = &self.config;
        let t = &c.theme;
        let m = &self.mons[mi];
        let sh = self.widgets.shared.lock().unwrap();
        let mut title = String::new();
        let mut one = |name: &str, out: &mut Vec<Item>, title: &mut String| match name {
            "tags" => {
                for (i, ws) in m.ws.iter().enumerate() {
                    let label = format!(" {} ", i + 1);
                    if i == m.cur {
                        out.push(Item { text: label, fg: t.sel_fg, bg: Some(t.sel_bg) });
                    } else if !ws.clients.is_empty() {
                        out.push(it(label, t.fg));
                    } else {
                        out.push(it(label, t.dim));
                    }
                }
            }
            "layout" => out.push(it(format!("[{}]", m.ws[m.cur].layout.name()), t.accent)),
            "title" | "windows" => {
                if let Some(w) = m.ws[m.cur].clients.get(m.ws[m.cur].focus) {
                    *title = Desktop::win_info(w).2;
                }
            }
            "cpu" => out.push(it(format!("cpu {}%", sh.cpu), t.fg)),
            "ram" => out.push(it(format!("ram {}%", sh.ram), t.fg)),
            "vol" => {
                if let Some((v, muted)) = sh.vol {
                    out.push(it(if muted { "vol mute".to_string() } else { format!("vol {v}%") }, if muted { t.dim } else { t.fg }));
                }
            }
            "bat" => {
                if let Some((p, chg)) = sh.bat {
                    out.push(it(format!("bat {p}%{}", if chg { "+" } else { "" }), if p <= 15 && !chg { t.urgent } else { t.fg }));
                }
            }
            "clock" => out.push(it(widgets::now("%a %d %b %H:%M"), t.accent)),
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
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for n in &c.bar_left {
            one(n, &mut left, &mut title);
        }
        for n in &c.bar_right {
            one(n, &mut right, &mut title);
        }
        (left, title, right)
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
    let ty = (bh - 16 * scale) / 2;

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

    // tab / stack bar of monocle, tabbed, deck ... drawn over the top of the window area
    if let Some(bar) = state.mons[mi].bar {
        let ws = &state.mons[mi].ws[state.mons[mi].cur];
        let titles: Vec<String> = ws.clients.iter().map(|w| Desktop::win_info(w).2).collect();
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
                cv.text(x + pad, y + (h - 16 * scale) / 2, ti, fg, scale, x + w - 1);
            }
        });
        let at: Point<f64, Physical> = ((r.x - geo.loc.x) as f64, (r.y - geo.loc.y) as f64).into();
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, at, &buf, None, None, None, Kind::Unspecified) {
            out.push(e.into());
        }
    }

    // status bar
    if pos != BarPos::Off {
        let (left, title, right) = state.bar_items(mi);
        let w = geo.size.w;
        let key = format!("{name}:bar");
        let hash = hash_of(&(&left, &title, &right, w, bh, t.name.clone(), scale));
        let buf = state.cached(key, w, bh, hash, |cv| {
            cv.rect(0, 0, w, bh, t.bar_bg);
            let cw = 8 * scale;
            let mut x = 0;
            for i in &left {
                let tw = i.text.chars().count() as i32 * cw;
                let slot = tw + if i.bg.is_some() { 0 } else { pad };
                if let Some(bg) = i.bg {
                    cv.rect(x, 0, slot, bh, bg);
                }
                cv.text(x + if i.bg.is_some() { 0 } else { pad / 2 }, ty, &i.text, i.fg, scale, w);
                x += slot;
            }
            let mut rx = w;
            for i in right.iter().rev() {
                let tw = i.text.chars().count() as i32 * cw;
                rx -= tw + pad * 2;
                cv.text(rx + pad, ty, &i.text, i.fg, scale, w);
            }
            if !title.is_empty() {
                cv.text(x + pad, ty, &title, t.fg, scale, rx - pad);
            }
            cv.rect(0, if pos == BarPos::Top { bh - 1 } else { 0 }, w, 1, t.border_normal);
        });
        let y = if pos == BarPos::Top { 0 } else { geo.size.h - bh };
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, (0.0, y as f64), &buf, None, None, None, Kind::Unspecified) {
            out.push(e.into());
        }
    }
    out
}
