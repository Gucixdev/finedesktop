#![allow(irrefutable_let_patterns)]

mod handlers;

mod grabs;
mod input;
mod config;
mod ipc;
mod keys;
mod components;
mod state;

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
pub use state::Desktop;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("ctl") {
        std::process::exit(ipc::ctl(&args[1..]));
    }
    init_logging();

    let mut event_loop: EventLoop<'static, Desktop> = EventLoop::try_new()?;
    let display: Display<Desktop> = Display::new()?;
    let mut state = Desktop::new(&mut event_loop, display);

    components::compositor::init(&mut event_loop, &mut state)?;

    // Children (autostart, spawned apps) must talk to us, not to the host compositor
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &state.socket_name) };
    let sock = ipc::init(&event_loop.handle(), &state.socket_name.to_string_lossy())?;
    unsafe { std::env::set_var("FD_SOCK", &sock) };

    for cmd in state.config.exec.clone() {
        keys::spawn(&cmd);
    }
    if let [flag, command, ..] = args.as_slice() {
        if flag == "-c" || flag == "--command" {
            keys::spawn(command);
        }
    }

    event_loop.run(None, &mut state, move |_| {})?;
    let _ = std::fs::remove_file(sock);
    Ok(())
}

fn init_logging() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }
}
