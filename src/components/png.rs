//! A small PNG decoder for icons: no crate, about 250 lines. Reads 8-bit (and 16-bit, reduced to 8) grayscale, grayscale+alpha,
//! RGB, RGBA and palette images, not interlaced (icons never are); anything else is an error, never a panic.
//! `decode` gives straight RGBA8; `Image::scaled` makes a nearest/box-filtered copy for the size a row needs.

pub struct Image {
    pub w: usize,
    pub h: usize,
    /// R, G, B, A per pixel, row by row
    pub rgba: Vec<u8>,
}

impl Image {
    /// The image at `size` x `size` (icons are square), box-averaged when it shrinks, nearest when it grows.
    pub fn scaled(&self, size: usize) -> Image {
        if size == 0 || self.w == 0 || self.h == 0 {
            return Image { w: 0, h: 0, rgba: Vec::new() };
        }
        let mut out = vec![0u8; size * size * 4];
        for y in 0..size {
            let (y0, y1) = (y * self.h / size, ((y + 1) * self.h).div_ceil(size).max(y * self.h / size + 1).min(self.h));
            for x in 0..size {
                let (x0, x1) = (x * self.w / size, ((x + 1) * self.w).div_ceil(size).max(x * self.w / size + 1).min(self.w));
                // premultiplied average, so a transparent neighbour does not darken the edge
                let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        let p = &self.rgba[(yy * self.w + xx) * 4..][..4];
                        let al = p[3] as u32;
                        r += p[0] as u32 * al;
                        g += p[1] as u32 * al;
                        b += p[2] as u32 * al;
                        a += al;
                        n += 1;
                    }
                }
                let o = &mut out[(y * size + x) * 4..][..4];
                if a > 0 {
                    o.copy_from_slice(&[(r / a) as u8, (g / a) as u8, (b / a) as u8, (a / n) as u8]);
                }
            }
        }
        Image { w: size, h: size, rgba: out }
    }
}

pub fn decode(data: &[u8]) -> Result<Image, String> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    if data.len() < 8 || data[..8] != SIG {
        return Err("not a PNG".into());
    }
    let (mut w, mut h, mut depth, mut color, mut interlace) = (0usize, 0usize, 0u8, 0u8, 0u8);
    let (mut palette, mut trns, mut idat): (Vec<u8>, Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new(), Vec::new());
    let mut pos = 8;
    while pos + 8 <= data.len() {
        let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = &data[pos + 4..pos + 8];
        let body = data.get(pos + 8..pos + 8 + len).ok_or("truncated chunk")?;
        match kind {
            b"IHDR" if len >= 13 => {
                w = u32::from_be_bytes(body[0..4].try_into().unwrap()) as usize;
                h = u32::from_be_bytes(body[4..8].try_into().unwrap()) as usize;
                (depth, color, interlace) = (body[8], body[9], body[12]);
            }
            b"PLTE" => palette = body.to_vec(),
            b"tRNS" => trns = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err("bad size".into());
    }
    if interlace != 0 {
        return Err("interlaced PNG".into());
    }
    let channels = match color {
        0 | 3 => 1,
        4 => 2,
        2 => 3,
        6 => 4,
        _ => return Err("bad color type".into()),
    };
    if !matches!((color, depth), (0, 1 | 2 | 4 | 8 | 16) | (3, 1 | 2 | 4 | 8) | (2 | 4 | 6, 8 | 16)) {
        return Err("bad bit depth".into());
    }
    let bits = channels * depth as usize;
    let stride = (w * bits).div_ceil(8);
    let bpp = bits.div_ceil(8);
    // zlib: two header bytes, deflate stream, adler (ignored)
    if idat.len() < 6 {
        return Err("no image data".into());
    }
    let raw = inflate(&idat[2..], (stride + 1) * h)?;
    if raw.len() < (stride + 1) * h {
        return Err("short image data".into());
    }
    let mut img = vec![0u8; stride * h];
    for y in 0..h {
        let f = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..][..stride];
        for x in 0..stride {
            let a = if x >= bpp { img[y * stride + x - bpp] as i32 } else { 0 };
            let b = if y > 0 { img[(y - 1) * stride + x] as i32 } else { 0 };
            let c = if y > 0 && x >= bpp { img[(y - 1) * stride + x - bpp] as i32 } else { 0 };
            let add = match f {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => {
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                }
                _ => return Err("bad filter".into()),
            };
            img[y * stride + x] = line[x].wrapping_add(add as u8);
        }
    }
    // sample n of a row, scaled to 0..=255 (palette indices stay as they are)
    let sample = |row: &[u8], n: usize| -> u8 {
        match depth {
            8 => row[n],
            16 => row[n * 2],
            d => {
                let per = 8 / d as usize;
                let v = (row[n / per] >> (8 - d as usize * (n % per + 1))) & ((1 << d) - 1);
                if color == 3 { v } else { (v as u32 * 255 / ((1 << d) - 1)) as u8 }
            }
        }
    };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = &img[y * stride..][..stride];
        for x in 0..w {
            let px = match color {
                0 => {
                    let g = sample(row, x);
                    // tRNS for gray: one 16-bit value, compared on the 8-bit sample (good enough for icons)
                    let a = if trns.len() >= 2 && depth == 8 && trns[1] == g { 0 } else { 255 };
                    [g, g, g, a]
                }
                2 => [sample(row, x * 3), sample(row, x * 3 + 1), sample(row, x * 3 + 2), 255],
                3 => {
                    let i = sample(row, x) as usize;
                    let p = palette.get(i * 3..i * 3 + 3).ok_or("palette index out of range")?;
                    [p[0], p[1], p[2], trns.get(i).copied().unwrap_or(255)]
                }
                4 => {
                    let g = sample(row, x * 2);
                    [g, g, g, sample(row, x * 2 + 1)]
                }
                _ => [sample(row, x * 4), sample(row, x * 4 + 1), sample(row, x * 4 + 2), sample(row, x * 4 + 3)],
            };
            rgba.extend_from_slice(&px);
        }
    }
    Ok(Image { w, h, rgba })
}

/// Deflate (RFC 1951): stored, fixed and dynamic Huffman blocks. `want` is only a capacity hint and a bomb limit.
fn inflate(data: &[u8], want: usize) -> Result<Vec<u8>, String> {
    struct Bits<'a> {
        d: &'a [u8],
        pos: usize,
        bit: u32,
        n: u32,
    }
    impl Bits<'_> {
        fn get(&mut self, k: u32) -> Result<u32, String> {
            while self.n < k {
                let b = *self.d.get(self.pos).ok_or("unexpected end of data")?;
                self.pos += 1;
                self.bit |= (b as u32) << self.n;
                self.n += 8;
            }
            let v = self.bit & ((1u64 << k) - 1) as u32;
            self.bit >>= k;
            self.n -= k;
            Ok(v)
        }
    }
    // canonical Huffman table: counts per length and symbols in code order
    struct Huff {
        count: [u16; 16],
        sym: Vec<u16>,
    }
    fn build(lens: &[u8]) -> Huff {
        let mut count = [0u16; 16];
        for &l in lens {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + count[i - 1];
        }
        let mut sym = vec![0u16; lens.len()];
        for (s, &l) in lens.iter().enumerate() {
            if l != 0 {
                sym[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Huff { count, sym }
    }
    fn decode(b: &mut Bits, h: &Huff) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.get(1)? as i32;
            let c = h.count[len] as i32;
            if code - c < first {
                return Ok(h.sym[(index + (code - first)) as usize]);
            }
            index += c;
            first = (first + c) << 1;
            code <<= 1;
        }
        Err("bad Huffman code".into())
    }
    const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
    const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
    const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

    let mut b = Bits { d: data, pos: 0, bit: 0, n: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(want);
    let limit = want + want / 4 + 1024;
    loop {
        let last = b.get(1)?;
        match b.get(2)? {
            0 => {
                b.bit = 0;
                b.n = 0;
                let h = b.d.get(b.pos..b.pos + 4).ok_or("truncated stored block")?;
                let len = u16::from_le_bytes([h[0], h[1]]) as usize;
                b.pos += 4;
                out.extend_from_slice(b.d.get(b.pos..b.pos + len).ok_or("truncated stored block")?);
                b.pos += len;
            }
            t @ (1 | 2) => {
                let (lit, dist) = if t == 1 {
                    let mut l = [8u8; 288];
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    (build(&l), build(&[5u8; 30]))
                } else {
                    let (nlen, ndist, ncode) = (b.get(5)? as usize + 257, b.get(5)? as usize + 1, b.get(4)? as usize + 4);
                    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut cl = [0u8; 19];
                    for &o in ORDER.iter().take(ncode) {
                        cl[o] = b.get(3)? as u8;
                    }
                    let ch = build(&cl);
                    let mut lens = vec![0u8; nlen + ndist];
                    let mut i = 0;
                    while i < nlen + ndist {
                        let s = decode(&mut b, &ch)?;
                        let (val, rep) = match s {
                            0..=15 => (s as u8, 1),
                            16 => (*lens.get(i.wrapping_sub(1)).ok_or("repeat with no previous length")?, 3 + b.get(2)? as usize),
                            17 => (0, 3 + b.get(3)? as usize),
                            _ => (0, 11 + b.get(7)? as usize),
                        };
                        if i + rep > lens.len() {
                            return Err("too many code lengths".into());
                        }
                        lens[i..i + rep].fill(val);
                        i += rep;
                    }
                    (build(&lens[..nlen]), build(&lens[nlen..]))
                };
                loop {
                    let s = decode(&mut b, &lit)? as usize;
                    match s {
                        0..=255 => out.push(s as u8),
                        256 => break,
                        257..=285 => {
                            let k = s - 257;
                            let len = LBASE[k] as usize + b.get(LEXT[k] as u32)? as usize;
                            let d = decode(&mut b, &dist)? as usize;
                            if d >= 30 {
                                return Err("bad distance code".into());
                            }
                            let dist = DBASE[d] as usize + b.get(DEXT[d] as u32)? as usize;
                            if dist > out.len() {
                                return Err("distance too far back".into());
                            }
                            for _ in 0..len {
                                out.push(out[out.len() - dist]);
                            }
                        }
                        _ => return Err("bad length code".into()),
                    }
                    if out.len() > limit {
                        return Err("image data larger than the header says".into());
                    }
                }
            }
            _ => return Err("bad block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
        if out.len() > limit {
            return Err("image data larger than the header says".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x2 RGBA made with PIL: red, green / blue, half-transparent white.
    const TINY: &[u8] = include_bytes!("png_tiny.png");

    #[test]
    fn decodes_a_small_rgba_image() {
        let i = decode(TINY).unwrap();
        assert_eq!((i.w, i.h), (2, 2));
        assert_eq!(i.rgba, [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128]);
    }

    #[test]
    fn garbage_and_truncation_are_errors_not_panics() {
        assert!(decode(b"nope").is_err());
        for n in [8, 20, 33, TINY.len() - 5] {
            let _ = decode(&TINY[..n.min(TINY.len())]);
        }
        let mut bad = TINY.to_vec();
        for i in 40..bad.len().min(60) {
            bad[i] ^= 0xff;
        }
        let _ = decode(&bad);
    }

    #[test]
    fn scaling_down_averages_and_keeps_alpha() {
        let i = Image { w: 2, h: 2, rgba: vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 0] };
        let s = i.scaled(1);
        assert_eq!(s.rgba, vec![255, 0, 0, 191]);
        assert_eq!(i.scaled(4).w, 4);
    }

    /// Dev check against a reference decoder: `PNG_LIST=files.txt cargo test png_dump -- --ignored --nocapture`
    /// prints `path fnv1a64(rgba)` per file (or `path ERR`); PIL gives the same numbers.
    #[test]
    #[ignore]
    fn png_dump() {
        let list = std::fs::read_to_string(std::env::var("PNG_LIST").unwrap()).unwrap();
        for p in list.lines() {
            match decode(&std::fs::read(p).unwrap()) {
                Ok(i) => {
                    let h = i.rgba.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3));
                    println!("{p} {}x{} {h:016x}", i.w, i.h);
                }
                Err(e) => println!("{p} ERR {e}"),
            }
        }
    }
}
