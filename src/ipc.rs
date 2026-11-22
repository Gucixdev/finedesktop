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
    crate::components::security::lock_socket(&path);
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
    if line.trim() == "subscribe" || line.trim().starts_with("subscribe ") {
        let topics: Vec<String> = line.split_whitespace().skip(1).map(String::from).collect();
        // keep the connection: every later event is written to it as one line
        let _ = stream.set_read_timeout(None);
        let _ = stream.set_nonblocking(true);
        let _ = (&stream).write_all(b"ok\n");
        state.bus.subscribe(topics, crate::flows::bus::Sink::Stream(stream));
        return;
    }
    if crate::components::security::denied(&state.config.ctl_deny, &line) {
        let _ = (&stream).write_all(b"error: denied by ctl_deny\n");
        return;
    }
    let reply = match crate::config::parse_action(&line) {
        Ok(action) => {
            let shot = matches!(action, crate::config::Action::Screenshot(_));
            let r = state.run_action(action);
            if shot && !r.starts_with("error") {
                // answered by capture::service once the file is written
                let _ = stream.set_read_timeout(None);
                if let (Some(s), Ok(c)) = (state.capture.shots.last_mut(), stream.try_clone()) {
                    s.reply = Some(c);
                    return;
                }
            }
            r
        }
        Err(e) => format!("error: {e}"),
    };
    let _ = (&stream).write_all(reply.as_bytes());
    let _ = (&stream).write_all(b"\n");
}

/// `finedesktop ctl ...` client side. Returns the process exit code.
pub fn ctl(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("trash") {
        return trash_local(&args[1..]);
    }
    ctl_one(args)
}

/// Files only, so no socket: paths are taken relative to the shell that typed them, one trashed per argument.
fn trash_local(args: &[String]) -> i32 {
    use crate::components::trash::run;
    if args.is_empty() || args[0] == "list" || args[0] == "restore" {
        let out = run(&args.join(" "));
        println!("{out}");
        return i32::from(out.starts_with("error:"));
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut code = 0;
    for a in args {
        let out = run(&cwd.join(a).to_string_lossy());
        println!("{out}");
        code |= i32::from(out.starts_with("error:"));
    }
    code
}

fn ctl_one(args: &[String]) -> i32 {
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
    if writeln!(stream, "{}", args.join(" ")).is_err() {
        return 2;
    }
    if args.first().map(String::as_str) == Some("subscribe") {
        // stream: print every event line as it arrives until the compositor goes away
        let mut out = std::io::stdout();
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if line == "ok" {
                continue;
            }
            if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
        return 0;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
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
