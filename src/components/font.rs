//! Bitmap font (Terminus 8x16, PSF2 with unicode table, Latin-2 so Polish letters work) and a tiny software canvas.
//! No font library: the bar and the tab bars are drawn here into an ARGB8888 buffer.

use std::{collections::HashMap, sync::OnceLock};

use super::theme::Rgb;

const PSF: &[u8] = include_bytes!("../.finedesktop/fonts/terminus16.psf");

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
        s.chars().count() as i32 * Font::builtin().w * scale
    }

    /// Draw `s` with its top-left at (x, y); returns the x after the last glyph. Clipped at `max_x`.
    pub fn text(&mut self, x: i32, y: i32, s: &str, fg: Rgb, scale: i32, max_x: i32) -> i32 {
        let f = Font::builtin();
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
    fn text_draws_pixels_and_clips() {
        let mut c = Canvas::new(40, 16, [0, 0, 0]);
        let end = c.text(0, 0, "Hello world", [255, 255, 255], 1, 40);
        assert_eq!(end, 40);
        assert!(c.px.chunks(4).any(|p| p[0] == 255));
        assert_eq!(c.px.len(), 40 * 16 * 4);
    }
}
