//! DRM/KMS + libinput backend: runs on a bare tty (or a QEMU virtio-gpu window) without a host compositor.
//! One GPU, every connected connector becomes an output laid out left to right, GLES rendering,
//! software cursor. Sessions come from libseat (seatd/logind), so VT switching works.

use std::{collections::HashMap, time::Duration};

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode,
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
        },
        egl::{EGLContext, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::WaylandSurfaceRenderElement,
                Kind,
            },
            gles::GlesRenderer,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, primary_gpu},
    },
    desktop::space::{SpaceRenderElements, space_render_elements},
    output::{Mode, Output, PhysicalProperties},
    reexports::{
        calloop::{EventLoop, RegistrationToken, timer::{TimeoutAction, Timer}},
        drm::control::{Device as ControlDevice, ModeTypeFlags, connector, crtc},
        input::Libinput,
        rustix::fs::OFlags,
    },
    utils::{DeviceFd, Transform},
};

use crate::Desktop;

type Compositor = DrmCompositor<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    (),
    DrmDeviceFd,
>;

smithay::backend::renderer::element::render_elements! {
    pub Elem<=GlesRenderer>;
    Cursor = MemoryRenderBufferRenderElement<GlesRenderer>,
    Overlay = super::bar::Ov,
    Space = SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
}

struct Head {
    output: Output,
    compositor: Compositor,
}

/// Everything the DRM backend owns; lives in `Desktop::drm`.
pub struct Drm {
    pub session: LibSeatSession,
    libinput: Libinput,
    renderer: GlesRenderer,
    drm: DrmDevice,
    heads: HashMap<crtc::Handle, Head>,
    cursor: MemoryRenderBuffer,
    active: bool,
    _drm_token: RegistrationToken,
}

/// 12x19 arrow, '#' black outline, 'o' white fill.
const ARROW: [&str; 19] = [
    "#...........", "##..........", "#o#.........", "#oo#........", "#ooo#.......", "#oooo#......",
    "#ooooo#.....", "#oooooo#....", "#ooooooo#...", "#oooooooo#..", "#ooooo#####.", "#oo#oo#.....",
    "#o#.#oo#....", "##..#oo#....", "#....#oo#...", ".....#oo#...", "......#oo#..", "......#oo#..",
    ".......##...",
];

fn cursor_buffer() -> MemoryRenderBuffer {
    let mut px = Vec::with_capacity(12 * 19 * 4);
    for row in ARROW {
        for c in row.chars() {
            // Argb8888 little endian: B G R A
            px.extend_from_slice(match c {
                '#' => &[0, 0, 0, 255],
                'o' => &[255, 255, 255, 255],
                _ => &[0, 0, 0, 0],
            });
        }
    }
    MemoryRenderBuffer::from_slice(&px, Fourcc::Argb8888, (12, 19), 1, Transform::Normal, None)
}

pub fn init(event_loop: &mut EventLoop<Desktop>, state: &mut Desktop) -> Result<(), Box<dyn std::error::Error>> {
    let (session, notifier) = LibSeatSession::new().map_err(|e| format!("no session (seatd/logind running?): {e}"))?;
    let seat = session.seat();

    let path = primary_gpu(&seat)?.ok_or("no GPU found for this seat")?;
    tracing::info!(?path, "using gpu");
    let fd = session.clone().open(&path, OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK)?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd)?;

    let egl = unsafe { EGLDisplay::new(gbm.clone())? };
    let context = EGLContext::new(&egl)?;
    let mut renderer = unsafe { GlesRenderer::new(context)? };
    let render_formats = renderer.egl_context().dmabuf_render_formats().clone();
    let node = DrmNode::from_path(&path).ok();

    // outputs: one per connected connector, each on its own free crtc
    let res = drm.resource_handles()?;
    let mut used: Vec<crtc::Handle> = Vec::new();
    let mut heads = HashMap::new();
    let mut x = 0;
    for conn in res.connectors() {
        let Ok(info) = drm.get_connector(*conn, true) else { continue };
        if info.state() != connector::State::Connected || info.modes().is_empty() {
            continue;
        }
        let Some(crtc) = info
            .encoders()
            .iter()
            .filter_map(|e| drm.get_encoder(*e).ok())
            .flat_map(|e| res.filter_crtcs(e.possible_crtcs()))
            .find(|c| !used.contains(c))
        else {
            tracing::warn!("no free crtc for {:?}", info.interface());
            continue;
        };
        let mode = info.modes().iter().find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED)).unwrap_or(&info.modes()[0]);
        let wl_mode = Mode::from(*mode);
        let name = format!("{}-{}", info.interface().as_str(), info.interface_id());
        let (w, h) = info.size().unwrap_or((0, 0));
        let output = Output::new(
            name.clone(),
            PhysicalProperties { size: (w as i32, h as i32).into(), subpixel: info.subpixel().into(), make: "Unknown".into(), model: "Unknown".into(), serial_number: "Unknown".into() },
        );
        let _global = output.create_global::<Desktop>(&state.display_handle);
        output.set_preferred(wl_mode);
        output.change_current_state(Some(wl_mode), None, None, Some((x, 0).into()));
        x += wl_mode.size.w;

        let surface = drm.create_surface(crtc, *mode, &[info.handle()])?;
        let allocator = GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT);
        let exporter = GbmFramebufferExporter::new(gbm.clone(), node.into());
        let compositor = DrmCompositor::new(
            &output,
            surface,
            None,
            allocator,
            exporter,
            [Fourcc::Argb8888, Fourcc::Abgr8888],
            render_formats.iter().copied(),
            drm.cursor_size(),
            Some(gbm.clone()),
        )?;
        tracing::info!("output {name} {}x{}@{}", wl_mode.size.w, wl_mode.size.h, wl_mode.refresh / 1000);
        used.push(crtc);
        state.add_output(output.clone());
        heads.insert(crtc, Head { output, compositor });
    }
    if heads.is_empty() {
        return Err("no connected output".into());
    }

    // input
    let mut libinput = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput.udev_assign_seat(&seat).map_err(|_| "libinput: cannot assign seat")?;
    let mut li = libinput.clone();
    event_loop.handle().insert_source(LibinputInputBackend::new(libinput.clone()), |event, _, state| {
        if !matches!(event, InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. }) {
            state.process_input_event(event);
        }
    })?;

    // vblank: the frame went out, draw the next one
    let token = event_loop.handle().insert_source(drm_notifier, |event, _, state| match event {
        DrmEvent::VBlank(crtc) => {
            if let Some(d) = state.drm.as_mut() {
                if let Some(h) = d.heads.get_mut(&crtc) {
                    let _ = h.compositor.frame_submitted();
                }
            }
            render(state, crtc);
        }
        DrmEvent::Error(e) => tracing::error!("drm: {e}"),
    })?;

    // session pause/resume (VT switch)
    let handle = event_loop.handle();
    event_loop.handle().insert_source(notifier, move |event, _, state| match event {
        SessionEvent::PauseSession => {
            li.suspend();
            if let Some(d) = state.drm.as_mut() {
                d.active = false;
                d.drm.pause();
            }
        }
        SessionEvent::ActivateSession => {
            if li.resume().is_err() {
                tracing::error!("libinput resume failed");
            }
            let crtcs: Vec<_> = state.drm.as_mut().map(|d| {
                d.active = true;
                let _ = d.drm.activate(true);
                for h in d.heads.values_mut() {
                    h.compositor.reset_buffers();
                }
                d.heads.keys().copied().collect()
            }).unwrap_or_default();
            for c in crtcs {
                handle.insert_source(Timer::from_duration(Duration::from_millis(5)), move |_, _, s| {
                    render(s, c);
                    TimeoutAction::Drop
                }).ok();
            }
        }
    })?;

    // keep the udev context alive for libinput's seat events
    let _ = UdevBackend::new(&seat)?;

    let crtcs: Vec<_> = heads.keys().copied().collect();
    state.drm = Some(Drm { session, libinput, renderer, drm, heads, cursor: cursor_buffer(), active: true, _drm_token: token });
    for c in crtcs {
        render(state, c);
    }
    Ok(())
}

/// Draw one output and queue the frame. When nothing changed no vblank will come, so poll again shortly.
fn render(state: &mut Desktop, crtc: crtc::Handle) {
    // the overlay needs `&mut Desktop` and the renderer at once: lift the backend out of the state meanwhile
    let Some(mut d) = state.drm.take() else { return };
    render_with(state, &mut d, crtc);
    state.drm = Some(d);
}

fn render_with(state: &mut Desktop, d: &mut Drm, crtc: crtc::Handle) {
    let pointer = state.pointer_loc;
    if !d.active {
        return;
    }
    let Some(head) = d.heads.get_mut(&crtc) else { return };
    let output = head.output.clone();
    let Some(geo) = state.space.output_geometry(&output) else { return };

    super::capture::service(state, &mut d.renderer, &output);

    let mut elements: Vec<Elem> = Vec::new();
    if geo.to_f64().contains(pointer) {
        let at = (pointer - geo.loc.to_f64()).to_physical(1.0);
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(
            &mut d.renderer, at, &d.cursor, None, None, None, Kind::Cursor,
        ) {
            elements.push(e.into());
        }
    }
    let ov = super::bar::overlay(state, &mut d.renderer, &output);
    elements.extend(ov.into_iter().map(Elem::from));
    match space_render_elements(&mut d.renderer, [&state.space], &output, 1.0) {
        Ok(v) => elements.extend(v.into_iter().map(Elem::from)),
        Err(e) => tracing::warn!("render elements: {e}"),
    }

    let mut queued = false;
    match head.compositor.render_frame(&mut d.renderer, &elements, state.config.background, FrameFlags::DEFAULT) {
        Ok(r) => {
            if !r.is_empty {
                match head.compositor.queue_frame(()) {
                    Ok(()) => queued = true,
                    Err(e) => tracing::warn!("queue_frame: {e}"),
                }
            }
        }
        Err(e) => tracing::warn!("render_frame: {e}"),
    }

    let t = state.start_time.elapsed();
    state.space.elements().for_each(|w| w.send_frame(&output, t, Some(Duration::ZERO), |_, _| Some(output.clone())));
    state.layer_frames(&output, t);
    state.space.refresh();
    state.popups.cleanup();
    let _ = state.display_handle.flush_clients();

    if !queued {
        state.loop_handle.insert_source(Timer::from_duration(Duration::from_millis(16)), move |_, _, s| {
            render(s, crtc);
            TimeoutAction::Drop
        }).ok();
    }
}

impl Drm {
    pub fn change_vt(&mut self, vt: i32) {
        if let Err(e) = self.session.change_vt(vt) {
            tracing::warn!("vt switch to {vt}: {e}");
        }
    }
}
