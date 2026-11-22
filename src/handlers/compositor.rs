use crate::{Desktop, grabs::resize_grab, state::ClientState};
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    reexports::wayland_server::{
        Client,
        protocol::{wl_buffer, wl_surface::WlSurface},
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent, is_sync_subsurface,
        },
        shm::{ShmHandler, ShmState},
    },
};

use super::xdg_shell;

impl CompositorHandler for Desktop {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.find_window(&root).and_then(|l| self.window_at(l)) {
                window.on_commit();
            }
        };

        let window = self.find_window(surface).and_then(|l| self.window_at(l).cloned());
        xdg_shell::handle_commit(&mut self.popups, window, surface);
        resize_grab::handle_commit(&mut self.space, surface);
    }
}

impl BufferHandler for Desktop {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Desktop {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}
