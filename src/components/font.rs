//! Bitmap fonts and a tiny software canvas. Built in: Terminus 8x16 (PSF2 with unicode table, Latin-2 so Polish letters
//! work). `font = PATH` in the config loads a `.bdf` (X11 bitmap font, e.g. Fixed 6x13) or a `.psf` instead; a path that
//! cannot be read keeps the built-in one. No font library: the bar, the menu and the tab bars are drawn here into an
//! ARGB8888 buffer, and every size in them comes from the current font's cell.

use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};

use super::theme::Rgb;

const PSF: &[u8] = include_bytes!("../.finedesktop/fonts/terminus16.psf");

static CURRENT: RwLock<Option<&'static Font>> = RwLock::new(None);

pub struct Font {
    pub w: i32,
    pub h: i32,
    row_bytes: usize,
    glyphs: Vec<u8>,
    charsize: usize,
    map: HashMap<char, usize>,
}

fn u32le(d: &[u8], at: usize) -> usize {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) as usize
}

impl Font {
    pub fn parse(d: &[u8]) -> Option<Font> {
        if d.len() < 32 || d[..4] != [0x72, 0xb5, 0x4a, 0x86] {
            return None;
        }
        let (headersize, flags, length, charsize, h, w) = (u32le(d, 8), u32le(d, 12), u32le(d, 16), u32le(d, 20), u32le(d, 24), u32le(d, 28));
        let glyph_end = headersize + length * charsize;
        if w == 0 || h == 0 || d.len() < glyph_end {
            return None;
        }
        let mut map = HashMap::new();
        if flags & 1 != 0 {
            let (mut idx, mut i) = (0usize, glyph_end);
            let mut skip_seq = false;
            while i < d.len() && idx < length {
                match d[i] {
                    0xff => {
                        idx += 1;
                        skip_seq = false;
                        i += 1;
                    }
                    0xfe => {
                        skip_seq = true; // combining sequence, ignore
                        i += 1;
                    }
                    b => {
                        let n = if b < 0x80 { 1 } else if b >> 5 == 0b110 { 2 } else if b >> 4 == 0b1110 { 3 } else { 4 };
                        if !skip_seq {
                            if let Some(c) = d.get(i..i + n).and_then(|s| std::str::from_utf8(s).ok()).and_then(|s| s.chars().next()) {
                                map.entry(c).or_insert(idx);
                            }
                        }
                        i += n;
                    }
                }
            }
        } else {
            for c in 0..length.min(256) {
                map.insert(char::from_u32(c as u32)?, c);
            }
        }
        Some(Font { w: w as i32, h: h as i32, row_bytes: w.div_ceil(8), glyphs: d[headersize..glyph_end].to_vec(), charsize, map })
    }

    /// A BDF font. The cell is FONTBOUNDINGBOX; each glyph is placed in it by its own BBX. Encodings are unicode code points.
    pub fn parse_bdf(text: &str) -> Option<Font> {
        let num = |l: &str, i: usize| l.split_whitespace().nth(i).and_then(|v| v.parse::<i32>().ok());
        let mut cell: Option<(i32, i32, i32, i32)> = None; // w h xoff yoff
        let mut lines = text.lines();
        let mut raw: Vec<(char, (i32, i32, i32, i32), Vec<Vec<u8>>)> = Vec::new();
        while let Some(l) = lines.next() {
            if l.starts_with("FONTBOUNDINGBOX") {
                cell = Some((num(l, 1)?, num(l, 2)?, num(l, 3)?, num(l, 4)?));
            } else if l.starts_with("STARTCHAR") {
                let (mut enc, mut bbx) = (-1, (0, 0, 0, 0));
                let mut rows = Vec::new();
                for l in lines.by_ref() {
                    if l.starts_with("ENCODING") {
                        enc = num(l, 1).unwrap_or(-1);
                    } else if l.starts_with("BBX") {
                        bbx = (num(l, 1)?, num(l, 2)?, num(l, 3)?, num(l, 4)?);
                    } else if l.starts_with("BITMAP") {
                        for r in lines.by_ref() {
                            if r.starts_with("ENDCHAR") {
                                break;
                            }
                            rows.push((0..r.len() / 2).filter_map(|i| u8::from_str_radix(r.get(2 * i..2 * i + 2)?, 16).ok()).collect());
                        }
                        break;
                    }
                }
                if let Some(c) = u32::try_from(enc).ok().and_then(char::from_u32) {
                    raw.push((c, bbx, rows));
                }
            }
        }
        let (fw, fh, fx, fy) = cell?;
        if fw <= 0 || fh <= 0 || raw.is_empty() {
            return None;
        }
        let row_bytes = (fw as usize).div_ceil(8);
        let charsize = row_bytes * fh as usize;
        let (mut glyphs, mut map) = (Vec::with_capacity(raw.len() * charsize), HashMap::new());
        for (idx, (c, (gw, gh, gx, gy), rows)) in raw.into_iter().enumerate() {
            let mut g = vec![0u8; charsize];
            for (ri, row) in rows.iter().enumerate().take(gh.max(0) as usize) {
                let y = fy + fh - (gy + gh) + ri as i32;
                if !(0..fh).contains(&y) {
                    continue;
                }
                for col in 0..gw.max(0) {
                    if row.get(col as usize / 8).is_some_and(|b| b & (0x80 >> (col % 8)) != 0) {
                        let x = gx - fx + col;
                        if (0..fw).contains(&x) {
                            g[y as usize * row_bytes + x as usize / 8] |= 0x80 >> (x % 8);
                        }
                    }
                }
            }
            glyphs.extend(g);
            map.insert(c, idx);
        }
        Some(Font { w: fw, h: fh, row_bytes, glyphs, charsize, map })
    }

    /// Make the font at `path` the current one (BDF or PSF by content); an empty or unreadable path means the built-in.
    pub fn select(path: &str) -> Result<(), String> {
        let font = if path.is_empty() {
            None
        } else {
            let d = std::fs::read(path).map_err(|e| format!("font {path}: {e}"))?;
            let f = if d.starts_with(&[0x72, 0xb5, 0x4a, 0x86]) { Font::parse(&d) } else { Font::parse_bdf(&String::from_utf8_lossy(&d)) };
            // a font is loaded once per path and lives as long as the process: reloading the same config costs nothing
            Some(&*Box::leak(Box::new(f.ok_or_else(|| format!("font {path}: not a BDF or PSF2 font"))?)))
        };
        *CURRENT.write().unwrap() = font;
        Ok(())
    }

    /// The font everything is drawn with: the selected one, else the built-in.
    pub fn current() -> &'static Font {
        (*CURRENT.read().unwrap()).unwrap_or_else(Font::builtin)
    }

    pub fn builtin() -> &'static Font {
        static F: OnceLock<Font> = OnceLock::new();
        F.get_or_init(|| Font::parse(PSF).expect("built-in font"))
    }

    fn glyph(&self, c: char) -> &[u8] {
        let i = self.map.get(&c).or_else(|| self.map.get(&'?')).copied().unwrap_or(0);
        &self.glyphs[i * self.charsize..(i + 1) * self.charsize]
    }
}

/// ARGB8888 little endian (B, G, R, A), fully opaque.
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: i32, h: i32, bg: Rgb) -> Canvas {
        let (w, h) = (w.max(1), h.max(1));
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            px.extend_from_slice(&[bg[2], bg[1], bg[0], 255]);
        }
        Canvas { w, h, px }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                let i = ((yy * self.w + xx) * 4) as usize;
                self.px[i..i + 4].copy_from_slice(&[c[2], c[1], c[0], 255]);
            }
        }
    }

    /// Width of `s` in pixels at integer `scale`.
    pub fn text_width(s: &str, scale: i32) -> i32 {
        s.chars().count() as i32 * Font::current().w * scale
    }

    /// Draw `s` with its top-left at (x, y); returns the x after the last glyph. Clipped at `max_x`.
    pub fn text(&mut self, x: i32, y: i32, s: &str, fg: Rgb, scale: i32, max_x: i32) -> i32 {
        let f = Font::current();
        let mut cx = x;
        for ch in s.chars() {
            if cx + f.w * scale > max_x.min(self.w) {
                break;
            }
            let g = f.glyph(ch);
            for row in 0..f.h {
                for col in 0..f.w {
                    let byte = g[row as usize * f.row_bytes + (col / 8) as usize];
                    if byte & (0x80 >> (col % 8)) != 0 {
                        self.rect(cx + col * scale, y + row * scale, scale, scale, fg);
                    }
                }
            }
            cx += f.w * scale;
        }
        cx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_loads_with_polish_letters() {
        let f = Font::builtin();
        assert_eq!((f.w, f.h), (8, 16));
        for c in "aAzZ09ąęłńóśźżĄĘŁŃÓŚŹŻ".chars() {
            assert!(f.map.contains_key(&c), "missing {c}");
        }
    }

    #[test]
    fn bdf_glyphs_land_in_the_cell_by_their_bbx() {
        // 4x5 cell, baseline 1 above the bottom (yoff -1); "A" is a 3x3 box at x=1, bottom on the baseline
        let bdf = "FONTBOUNDINGBOX 4 5 0 -1\nSTARTCHAR A\nENCODING 65\nBBX 3 3 1 0\nBITMAP\nE0\n40\nA0\nENDCHAR\nSTARTCHAR ae\nENCODING 261\nBBX 4 5 0 -1\nBITMAP\n00\n00\n00\n00\nF0\nENDCHAR\nSTARTCHAR skip\nENCODING -1\nBBX 1 1 0 0\nBITMAP\n80\nENDCHAR\n";
        let f = Font::parse_bdf(bdf).unwrap();
        assert_eq!((f.w, f.h), (4, 5));
        assert!(f.map.contains_key(&'A') && f.map.contains_key(&'ą') && f.map.len() == 2);
        // rows top to bottom: cell top is 4 above the baseline, the glyph top is 3 above -> glyph starts on row 1
        let g = f.glyph('A');
        assert_eq!(g, [0x00, 0x70, 0x20, 0x50, 0x00]);
        assert_eq!(f.glyph('ą'), [0x00, 0x00, 0x00, 0x00, 0xF0]);
        assert!(Font::parse_bdf("nothing").is_none());
    }

    #[test]
    fn the_users_fixed_6x13_loads_when_it_is_installed() {
        let Some(path) = std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/share/fonts/Fixed6x13.bdf")).filter(|p| p.is_file()) else { return };
        let f = Font::parse_bdf(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!((f.w, f.h), (6, 13));
        for c in "aAzZ09ąęłńóśźżĄĘŁŃÓŚŹŻ".chars() {
            assert!(f.map.contains_key(&c), "missing {c}");
        }
    }

    #[test]
    fn the_users_templeos_bdf_loads_when_it_is_installed() {
        let Some(path) = std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/share/fonts/TempleOS-Bitmap.bdf")).filter(|p| p.is_file()) else { return };
        let f = Font::parse_bdf(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!((f.w, f.h), (8, 8));
        assert!("aAzZ09".chars().all(|c| f.map.contains_key(&c)));
    }

    #[test]
    fn text_draws_pixels_and_clips() {
        let mut c = Canvas::new(40, 16, [0, 0, 0]);
        let end = c.text(0, 0, "Hello world", [255, 255, 255], 1, 40);
        assert_eq!(end, 40);
        assert!(c.px.chunks(4).any(|p| p[0] == 255));
        assert_eq!(c.px.len(), 40 * 16 * 4);
    }
}
