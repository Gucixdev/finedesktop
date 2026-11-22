//! The compositor entry point: picks the backend and starts it.
//!   compositor_drm.rs    bare tty / VM: DRM+KMS, libinput, libseat
//!   compositor_winit.rs  nested in another Wayland/X11 session (development)
//! FD_BACKEND=drm|winit forces one; otherwise nested when WAYLAND_DISPLAY or DISPLAY is set.

use smithay::reexports::calloop::EventLoop;

use crate::Desktop;

pub fn init(event_loop: &mut EventLoop<'static, Desktop>, state: &mut Desktop) -> Result<(), Box<dyn std::error::Error>> {
    let nested = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    match std::env::var("FD_BACKEND").as_deref() {
        Ok("drm") => super::compositor_drm::init(event_loop, state),
        Ok("winit") => super::compositor_winit::init_winit(event_loop, state),
        _ if nested => super::compositor_winit::init_winit(event_loop, state),
        _ => super::compositor_drm::init(event_loop, state),
    }
}
