#![allow(irrefutable_let_patterns)]

mod engine_host;
mod handlers;

mod grabs;
mod input;
mod config;
mod ipc;
mod keys;
mod components;
mod flows;
mod state;

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
pub use state::Desktop;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("ctl") {
        std::process::exit(ipc::ctl(&args[1..]));
    }
    // busybox style: `fd-notify hi` is `finedesktop ctl notify hi` (see the `links` script), so each thing is its own command
    let exe = std::env::args().next().map(|a| std::path::Path::new(&a).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).unwrap_or_default();
    if let Some(cmd) = exe.strip_prefix("fd-") {
        let mut a = vec![cmd.to_string()];
        a.extend(args.iter().cloned());
        std::process::exit(ipc::ctl(&a));
    }
    init_logging();
    components::crash::install();

    let mut event_loop: EventLoop<'static, Desktop> = EventLoop::try_new()?;
    let display: Display<Desktop> = Display::new()?;
    let mut state = Desktop::new(&mut event_loop, display);

    components::compositor::init(&mut event_loop, &mut state)?;

    // Children (autostart, spawned apps) must talk to us, not to the host compositor
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &state.socket_name) };
    let sock = ipc::init(&event_loop.handle(), &state.socket_name.to_string_lossy())?;
    unsafe { std::env::set_var("FD_SOCK", &sock) };

    // once a second: redraw the bar when its interval is due, fire the `minute` event on the minute change
    {
        use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
        let (mut last_min, mut tick) = (String::new(), 0u64);
        event_loop.handle().insert_source(Timer::from_duration(std::time::Duration::from_secs(1)), move |_, _, st| {
            tick += 1;
            if tick % 3 == 0 {
                st.reload_panels();
            }
            if tick % st.config.bar_interval.max(1) == 0 {
                st.bar_dirty = true;
            }
            let hm = components::widgets::now("%H:%M");
            if hm != last_min {
                if !last_min.is_empty() {
                    st.emit(components::hooks::Event::minute(&hm));
                }
                last_min = hm;
            }
            TimeoutAction::ToDuration(std::time::Duration::from_secs(1))
        })?;
    }
    state.widgets.configure(&state.config);
    if config::engine_path(&state.config).is_some_and(|p| p.is_file()) {
        if let Err(e) = state.reload_engine() {
            tracing::warn!("engine: {e}");
        }
    }
    state.add_component(Box::new(components::crash::Crash));
    state.add_component(Box::new(components::autostart::Autostart));
    state.add_component(Box::new(components::restore::Restore::default()));
    state.add_component(Box::new(components::power::Power::default()));
    {
        // frame clock for update / fixed update / tick (flows/lifecycle.rs)
        use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
        let step = std::time::Duration::from_millis(16);
        let mut last = std::time::Instant::now();
        event_loop.handle().insert_source(Timer::from_duration(step), move |_, _, st| {
            let now = std::time::Instant::now();
            st.life_step((now - last).as_secs_f32());
            last = now;
            TimeoutAction::ToDuration(step)
        })?;
    }
    state.life_start();
    if let [flag, command, ..] = args.as_slice() {
        if flag == "-c" || flag == "--command" {
            keys::spawn(command);
        }
    }

    event_loop.run(None, &mut state, move |_| {})?;
    state.life_destroy();
    let _ = std::fs::remove_file(sock);
    if components::restore::RESTART.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(components::restore::EXIT_RESTART);
    }
    Ok(())
}

fn init_logging() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).with_writer(|| components::log::Tee).with_ansi(false).init();
    } else {
        tracing_subscriber::fmt().with_writer(|| components::log::Tee).with_ansi(false).init();
    }
}
