//! `zwp_linux_dmabuf_v1`: GPU clients (Vulkan, EGL) hand over their buffers without copying. Without this global a
//! proprietary NVIDIA Vulkan driver cannot present to the desktop at all (and wgpu/vkcube find no adapter).
//! The import test runs on the DRM renderer; the global itself is created next to it (`compositor_drm`).
//! Implicit sync: a commit that attaches a dmabuf waits until the dmabuf is readable (`blocker`).

use crate::Desktop;
use smithay::{
    backend::allocator::dmabuf::Dmabuf,
    reexports::{calloop::Interest, wayland_server::{protocol::wl_surface::WlSurface, Resource}},
    wayland::{
        compositor::{add_blocker, add_pre_commit_hook, with_states, BufferAssignment, SurfaceAttributes},
        dmabuf::{get_dmabuf, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
    },
};

impl DmabufHandler for Desktop {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(&mut self, _: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        if self.drm.is_none() {
            self.dmabuf_pending.push((dmabuf, notifier));
            return;
        }
        let ok = self.drm.as_mut().is_some_and(|d| d.import_dmabuf(&dmabuf));
        if ok {
            let _ = notifier.successful::<Desktop>();
        } else {
            tracing::warn!("dmabuf import refused");
            notifier.failed();
        }
    }
}

/// Called for every new surface: hold a commit back until its dmabuf is ready to be read.
pub fn hook_surface(surface: &WlSurface) {
    add_pre_commit_hook::<Desktop, _>(surface, |state, _dh, surface| {
        let dmabuf = with_states(surface, |states| {
            let mut guard = states.cached_state.get::<SurfaceAttributes>();
            match guard.pending().buffer.as_ref() {
                Some(BufferAssignment::NewBuffer(buf)) => get_dmabuf(buf).ok().cloned(),
                _ => None,
            }
        });
        let Some(dmabuf) = dmabuf else { return };
        let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ) else { return };
        let Some(client) = surface.client() else { return };
        let added = state.loop_handle.insert_source(source, move |_, _, d| {
            let dh = d.display_handle.clone();
            use smithay::wayland::compositor::CompositorHandler;
            d.client_compositor_state(&client).blocker_cleared(d, &dh);
            Ok(())
        });
        if added.is_ok() {
            add_blocker(surface, blocker);
        }
    });
}
