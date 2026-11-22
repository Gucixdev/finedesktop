use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement, gles::GlesRenderer,
        },
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Transform},
};

use crate::Desktop;

pub fn init_winit(
    event_loop: &mut EventLoop<Desktop>,
    state: &mut Desktop,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut backend, winit) = winit::init::<smithay::backend::renderer::gles::GlesRenderer>()?;

    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };

    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Smithay".into(),
            model: "Winit".into(),
            serial_number: "Unknown".into(),
        },
    );
    let _global = output.create_global::<Desktop>(&state.display_handle);
    output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some((0, 0).into()));
    output.set_preferred(mode);

    state.add_output(output.clone());

    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    // zwp_linux_dmabuf_v1 for the nested session too (no drm node: the host's render node is not known here)
    if let Some(dev) = host_render_dev() {
        let formats = backend.renderer().egl_context().dmabuf_render_formats().clone();
        if let Ok(fb) = smithay::wayland::dmabuf::DmabufFeedbackBuilder::new(dev, formats.iter().copied()).build() {
            state.dmabuf_global = Some(state.dmabuf_state.create_global_with_default_feedback::<Desktop>(&state.display_handle, &fb));
            tracing::info!(dev, "linux-dmabuf offered (nested)");
        }
    }

    event_loop.handle().insert_source(winit, move |event, _, state| {
        match event {
            WinitEvent::Resized { size, .. } => {
                output.change_current_state(
                    Some(Mode {
                        size,
                        refresh: 60_000,
                    }),
                    None,
                    None,
                    None,
                );
                state.retile();
            }
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => {
                let size = backend.window_size();
                let damage = Rectangle::from_size(size);

                for (d, n) in std::mem::take(&mut state.dmabuf_pending) {
                    use smithay::backend::renderer::ImportDma;
                    if backend.renderer().import_dmabuf(&d, None).is_ok() {
                        let _ = n.successful::<Desktop>();
                    } else {
                        n.failed();
                    }
                }

                {
                    let (renderer, mut framebuffer) = backend.bind().unwrap();
                    // same bind as the frame: a second backend.bind() would lose the window surface
                    super::capture::service(state, renderer, &output);
                    let ov = super::bar::overlay(state, renderer, &output);
                    smithay::desktop::space::render_output::<
                        _,
                        super::bar::Ov,
                        _,
                        _,
                    >(
                        &output,
                        renderer,
                        &mut framebuffer,
                        1.0,
                        0,
                        [&state.space],
                        &ov,
                        &mut damage_tracker,
                        state.config.background,
                    )
                    .unwrap();
                }
                backend.submit(Some(&[damage])).unwrap();

                state.space.elements().for_each(|window| {
                    window.send_frame(
                        &output,
                        state.start_time.elapsed(),
                        Some(Duration::ZERO),
                        |_, _| Some(output.clone()),
                    )
                });

                state.layer_frames(&output, state.start_time.elapsed());
                state.space.refresh();
                state.popups.cleanup();
                let _ = state.display_handle.flush_clients();

                // Ask for redraw to schedule new frame.
                backend.window().request_redraw();
            }
            WinitEvent::CloseRequested => {
                state.loop_signal.stop();
            }
            _ => (),
        };
    })?;

    Ok(())
}

/// The first render node (`/dev/dri/renderD*`): nested sessions run on the host's gpu.
fn host_render_dev() -> Option<libc::dev_t> {
    use std::os::unix::fs::MetadataExt;
    let mut nodes: Vec<_> = std::fs::read_dir("/dev/dri").ok()?.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("renderD")).collect();
    nodes.sort_by_key(|e| e.file_name());
    nodes.first().and_then(|e| e.metadata().ok()).map(|m| m.rdev())
}
