//! Screen capture for agents and humans, through the same `ctl` surface as everything else:
//!   ctl screenshot [PATH]          one PNG of the focused monitor; the reply comes after the file is written
//!   ctl screenrecord start [PATH]  frames piped to ffmpeg (PATH ends in .mp4/.webm/.gif; FD_REC_FPS, default 15)
//!   ctl screenrecord stop          finish the file
//!   ctl screenrecord               status
//! The frame is rendered again into an offscreen texture (bar, windows, panels; no cursor), so it works the same on
//! the DRM and the nested backend. PNG is written here by hand (stored deflate): no image crate.

use std::{
    io::Write,
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{Sender, channel},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, Offscreen,
            damage::OutputDamageTracker,
            gles::{GlesRenderer, GlesTexture},
        },
    },
    output::Output,
    utils::{Buffer as BufferCoord, Rectangle, Size, Transform},
};

use crate::Desktop;

pub struct Shot {
    pub path: PathBuf,
    pub reply: Option<UnixStream>,
}

pub struct Rec {
    path: PathBuf,
    tx: Sender<Vec<u8>>,
    writer: JoinHandle<()>,
    child: Child,
    size: (i32, i32),
    every: Duration,
    next: Instant,
    frames: u32,
}

#[derive(Default)]
pub struct Capture {
    pub shots: Vec<Shot>,
    pub rec: Option<Rec>,
}

fn stamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn default_path(ext: &str) -> PathBuf {
    let dir = std::env::var_os("FD_CAPTURE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_PICTURES_DIR").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Pictures")))
        .unwrap_or_else(|| "/tmp".into());
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("finedesktop-{}.{ext}", stamp()))
}

impl Desktop {
    /// `ctl screenshot [PATH]`: queue a shot for the next frame. serve() attaches the client's stream afterwards.
    pub fn screenshot_cmd(&mut self, path: &str) -> String {
        let path = if path.is_empty() { default_path("png") } else { PathBuf::from(path) };
        let shown = path.display().to_string();
        self.capture.shots.push(Shot { path, reply: None });
        format!("queued {shown}")
    }

    pub fn screenrecord_cmd(&mut self, how: &str) -> String {
        let (verb, arg) = how.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((how, ""));
        match verb {
            "start" => {
                if self.capture.rec.is_some() {
                    return "error: already recording (ctl screenrecord stop)".into();
                }
                let path = if arg.is_empty() { default_path("mp4") } else { PathBuf::from(arg) };
                let Some((w, h)) = self.mons.get(self.focus_mon).and_then(|m| m.output.current_mode()).map(|m| (m.size.w, m.size.h)) else {
                    return "error: no monitor".into();
                };
                let fps: u32 = std::env::var("FD_REC_FPS").ok().and_then(|v| v.parse().ok()).filter(|f| (1..=60).contains(f)).unwrap_or(15);
                match start_rec(path.clone(), (w, h), fps) {
                    Ok(r) => {
                        self.capture.rec = Some(r);
                        format!("recording {} {w}x{h} @{fps}", path.display())
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            "stop" => match self.capture.rec.take() {
                Some(r) => {
                    let (p, n) = (r.path.clone(), r.frames);
                    finish_rec(r);
                    format!("saved {} ({n} frames)", p.display())
                }
                None => "error: not recording".into(),
            },
            "" | "status" => match &self.capture.rec {
                Some(r) => format!("recording {} ({} frames)", r.path.display(), r.frames),
                None => "idle".into(),
            },
            _ => "error: screenrecord start [PATH] | stop | status".into(),
        }
    }
}

fn start_rec(path: PathBuf, size: (i32, i32), fps: u32) -> Result<Rec, String> {
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", size.0, size.1)])
        .args(["-r", &fps.to_string(), "-i", "-"])
        // yuv420p wants even sides
        .args(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p"])
        .arg(&path)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e} (install it: ./requirements in devframework)"))?;
    let mut stdin = child.stdin.take().ok_or("ffmpeg stdin")?;
    let (tx, rx) = channel::<Vec<u8>>();
    // a thread owns the pipe: a slow encoder must never stall the compositor
    let writer = std::thread::spawn(move || {
        for frame in rx {
            if stdin.write_all(&frame).is_err() {
                break;
            }
        }
    });
    Ok(Rec { path, tx, writer, child, size, every: Duration::from_micros(1_000_000 / fps as u64), next: Instant::now(), frames: 0 })
}

fn finish_rec(r: Rec) {
    let Rec { tx, writer, mut child, .. } = r;
    drop(tx);
    let _ = writer.join();
    let _ = child.wait();
}

/// Called by each backend right before it draws `output`. Does nothing unless a shot or a recording wants this frame.
pub fn service(state: &mut Desktop, renderer: &mut GlesRenderer, output: &Output) {
    let c = &state.capture;
    let want_rec = c.rec.as_ref().is_some_and(|r| Instant::now() >= r.next);
    if (c.shots.is_empty() && !want_rec) || state.mons.get(state.focus_mon).is_none_or(|m| m.output != *output) {
        return;
    }
    let frame = grab(state, renderer, output);
    let shots = std::mem::take(&mut state.capture.shots);
    for mut s in shots {
        let msg = match &frame {
            Ok((w, h, rgba)) => match std::fs::write(&s.path, png(*w as u32, *h as u32, rgba)) {
                Ok(()) => format!("ok {} {w}x{h}", s.path.display()),
                Err(e) => format!("error: {}: {e}", s.path.display()),
            },
            Err(e) => format!("error: {e}"),
        };
        match s.reply.take() {
            Some(mut r) => {
                let _ = writeln!(r, "{msg}");
            }
            None => tracing::info!("screenshot: {msg}"),
        }
    }
    if let (Some(r), Ok((w, h, rgba))) = (state.capture.rec.as_mut(), &frame) {
        if (*w, *h) == r.size {
            // catch up when the compositor was idle, at most a few frames, so the video keeps real time
            let now = Instant::now();
            let mut n = 0;
            while r.next <= now && n < 4 {
                let _ = r.tx.send(rgba.clone());
                r.frames += 1;
                r.next += r.every;
                n += 1;
            }
            if r.next <= now {
                r.next = now + r.every;
            }
        }
    }
}

/// The monitor as RGBA bytes, top row first.
fn grab(state: &mut Desktop, renderer: &mut GlesRenderer, output: &Output) -> Result<(i32, i32, Vec<u8>), String> {
    let mode = output.current_mode().ok_or("output has no mode")?;
    let size: Size<i32, BufferCoord> = (mode.size.w, mode.size.h).into();
    let mut tex: GlesTexture = Offscreen::<GlesTexture>::create_buffer(renderer, Fourcc::Abgr8888, size).map_err(|e| format!("offscreen: {e}"))?;
    let ov = super::bar::overlay(state, renderer, output);
    // a fresh tracker: the shot is a full redraw and must not disturb the real one's damage state
    let mut tracker = OutputDamageTracker::new(mode.size, 1.0, Transform::Normal);
    let mut fb = renderer.bind(&mut tex).map_err(|e| format!("bind: {e}"))?;
    smithay::desktop::space::render_output::<_, super::bar::Ov, _, _>(
        output, renderer, &mut fb, 1.0, 0, [&state.space], &ov, &mut tracker, state.config.background,
    )
    .map_err(|e| format!("render: {e}"))?;
    let map = renderer.copy_framebuffer(&fb, Rectangle::from_size(size), Fourcc::Abgr8888).map_err(|e| format!("copy: {e}"))?;
    // upright as is: the tracker above has its own Normal transform, so the nested backend's Flipped180 never leaks in
    let bytes = renderer.map_texture(&map).map_err(|e| format!("map: {e}"))?.to_vec();
    Ok((size.w, size.h, bytes))
}

// ---- PNG, stored deflate ---------------------------------------------------------------------------------------

fn crc32(chunks: &[&[u8]]) -> u32 {
    let mut t = [0u32; 256];
    for (i, e) in t.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    let mut c = !0u32;
    for ch in chunks {
        for &b in *ch {
            c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
        }
    }
    !c
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    out.extend(crc32(&[kind, data]).to_be_bytes());
}

/// RGBA, 8 bit, no compression (the zlib stream is stored blocks): big but trivially correct and fast.
pub fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for row in rgba.chunks(w as usize * 4) {
        raw.push(0); // filter: none
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        z.extend((c.len() as u16).to_le_bytes());
        z.extend((!(c.len() as u16)).to_le_bytes());
        z.extend(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
    }

    #[test]
    fn png_has_signature_and_ends_with_iend() {
        let p = png(2, 2, &[255; 16]);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[p.len() - 8..p.len() - 4], b"IEND");
    }
}
