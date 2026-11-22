//! Control socket: one line in (the same action syntax as the config's `bind =` lines), one reply out.
//! `finedesktop ctl layout grid`, `finedesktop ctl state`. This is also what agents and tests drive.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction, generic::Generic};

use crate::state::Desktop;

pub fn socket_path(wayland_name: &str) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(dir).join(format!("finedesktop-{wayland_name}.sock"))
}

pub fn init(handle: &LoopHandle<'static, Desktop>, wayland_name: &str) -> std::io::Result<PathBuf> {
    let path = socket_path(wayland_name);
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    handle
        .insert_source(Generic::new(listener, Interest::READ, Mode::Level), |_, listener, state| {
            // Safety: the listener is not dropped while the source is alive.
            let listener = unsafe { listener.get_mut() };
            while let Ok((stream, _)) = listener.accept() {
                serve(stream, state);
            }
            Ok(PostAction::Continue)
        })
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(path)
}

fn serve(stream: UnixStream, state: &mut Desktop) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    if line.trim() == "subscribe" {
        // keep the connection: every later event is written to it as one line
        let _ = stream.set_read_timeout(None);
        let _ = stream.set_nonblocking(true);
        let _ = (&stream).write_all(b"ok\n");
        state.subs.push(stream);
        return;
    }
    let reply = match crate::config::parse_action(&line) {
        Ok(action) => state.run_action(action),
        Err(e) => format!("error: {e}"),
    };
    let _ = (&stream).write_all(reply.as_bytes());
    let _ = (&stream).write_all(b"\n");
}

/// `finedesktop ctl ...` client side. Returns the process exit code.
pub fn ctl(args: &[String]) -> i32 {
    let path = std::env::var_os("FD_SOCK").map(PathBuf::from).or_else(newest_socket);
    let Some(path) = path else {
        eprintln!("finedesktop ctl: no running finedesktop (set FD_SOCK)");
        return 2;
    };
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("finedesktop ctl: {}: {e}", path.display());
            return 2;
        }
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if writeln!(stream, "{}", args.join(" ")).is_err() {
        return 2;
    }
    let mut out = String::new();
    let _ = std::io::Read::read_to_string(&mut stream, &mut out);
    print!("{out}");
    if out.starts_with("error:") { 1 } else { 0 }
}

fn newest_socket() -> Option<PathBuf> {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("finedesktop-") && e.file_name().to_string_lossy().ends_with(".sock"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}
