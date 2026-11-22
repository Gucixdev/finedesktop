use std::{ffi::OsString, sync::Arc};

use smithay::{
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{Seat, SeatState},
    reexports::{
        calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::XdgShellState,
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

pub struct Desktop {
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,

    // Smithay State
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Desktop>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,

    pub seat: Seat<Self>,

    /// Tiling order, index 0 is the first master.
    pub clients: Vec<Window>,
    pub layout: crate::components::engine::layout_engine::Layout,
    pub params: crate::components::engine::layout_engine::Params,
    /// Tab / stack title bar of the last plan (drawing it is still TODO).
    pub bar: Option<crate::components::engine::layout_engine::Bar>,
    pub modkey: crate::keys::Mod,
}

impl Desktop {
    pub fn new(event_loop: &mut EventLoop<Self>, display: Display<Self>) -> Self {
        let start_time = std::time::Instant::now();

        let dh = display.handle();

        // Here we initialize implementations of some wayland protocols
        // Some of them require us to implement traits on the Desktop state,
        // you can find those implementations in the `crate::handlers` module

        // Initialize protocols needed for displaying windows
        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let popups = PopupManager::default();

        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);

        // Data device is responsible for clipboard and drag-and-drop
        let data_device_state = DataDeviceState::new::<Self>(&dh);

        // A seat is a group of keyboards, pointer and touch devices.
        // A seat typically has a pointer and maintains a keyboard focus and a pointer focus.
        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "winit");

        // Notify clients that we have a keyboard, for the sake of the example we assume that keyboard is always present.
        // You may want to track keyboard hot-plug in real compositor.
        seat.add_keyboard(Default::default(), 200, 25).unwrap();

        // Notify clients that we have a pointer (mouse)
        // Here we assume that there is always pointer plugged in
        seat.add_pointer();

        // A space represents a two-dimensional plane. Windows and Outputs can be mapped onto it.
        //
        // Windows get a position and stacking order through mapping.
        // Outputs become views of a part of the Space and can be rendered via Space::render_output.
        let space = Space::default();

        // Setup a wayland socket that will be used to accept clients
        let socket_name = Self::init_wayland_listener(display, event_loop);

        // Get the loop signal, used to stop the event loop
        let loop_signal = event_loop.get_signal();

        Self {
            start_time,
            display_handle: dh,

            space,
            loop_signal,
            socket_name,

            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups,
            seat,
            clients: Vec::new(),
            layout: crate::components::engine::layout_engine::Layout::Tile,
            params: Default::default(),
            bar: None,
            modkey: crate::keys::Mod::from_env(),
        }
    }

    fn init_wayland_listener(display: Display<Desktop>, event_loop: &mut EventLoop<Self>) -> OsString {
        // Creates a new listening socket, automatically choosing the next available `wayland` socket name.
        let listening_socket = ListeningSocketSource::new_auto().unwrap();

        // Get the name of the listening socket.
        // Clients will connect to this socket.
        let socket_name = listening_socket.socket_name().to_os_string();

        let loop_handle = event_loop.handle();

        loop_handle
            .insert_source(listening_socket, move |client_stream, _, state| {
                // Inside the callback, you should insert the client into the display.
                //
                // You may also associate some data with the client when inserting the client.
                state
                    .display_handle
                    .insert_client(client_stream, Arc::new(ClientState::default()))
                    .unwrap();
            })
            .expect("Failed to init the wayland event source.");

        // You also need to add the display itself to the event loop, so that client events will be processed by wayland-server.
        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // Safety: we don't drop the display
                    unsafe {
                        display.get_mut().dispatch_clients(state).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .unwrap();

        socket_name
    }

    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space.element_under(pos).and_then(|(window, location)| {
            window
                .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + location).to_f64()))
        })
    }
}

/// Data associated with a wayland client that connects to Desktop.
/// One instance of this type per client.
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

impl Desktop {
    /// Run the layout engine for the first output and apply the plan to the clients.
    pub fn retile(&mut self) {
        use crate::components::engine::layout_engine::{Rect, arrange};
        use smithay::utils::IsAlive;
        self.clients.retain(|w| w.alive());
        let Some(output) = self.space.outputs().next().cloned() else { return };
        let Some(geo) = self.space.output_geometry(&output) else { return };
        let area = Rect { x: geo.loc.x, y: geo.loc.y, w: geo.size.w, h: geo.size.h };
        let focused = self.focused_index().unwrap_or(0);
        let plan = arrange(self.layout, area, self.clients.len(), focused, &self.params);
        self.bar = plan.bar;
        for (win, slot) in self.clients.clone().into_iter().zip(plan.slots) {
            let Some(slot) = slot else {
                // float: let the client pick its size, keep it mapped where it is
                if let Some(top) = win.toplevel() {
                    top.with_pending_state(|st| st.size = None);
                    top.send_pending_configure();
                }
                if self.space.element_location(&win).is_none() {
                    self.space.map_element(win, (40, 40), false);
                }
                continue;
            };
            if !slot.visible {
                self.space.unmap_elem(&win);
                continue;
            }
            if let Some(top) = win.toplevel() {
                let size = (slot.rect.w, slot.rect.h).into();
                top.with_pending_state(|st| st.size = Some(size));
                top.send_pending_configure();
            }
            if self.space.element_location(&win) != Some((slot.rect.x, slot.rect.y).into()) {
                self.space.map_element(win, (slot.rect.x, slot.rect.y), false);
            }
        }
    }

    pub fn set_layout(&mut self, layout: crate::components::engine::layout_engine::Layout) {
        tracing::info!("layout: {}", layout.name());
        self.layout = layout;
        self.retile();
    }

    pub fn focused_index(&self) -> Option<usize> {
        let focus = self.seat.get_keyboard()?.current_focus()?;
        self.clients.iter().position(|w| w.toplevel().is_some_and(|t| *t.wl_surface() == focus))
    }

    pub fn focus_index(&mut self, i: usize) {
        let Some(win) = self.clients.get(i).cloned() else { return };
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let surf = win.toplevel().map(|t| t.wl_surface().clone());
        self.space.raise_element(&win, true);
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, surf, serial);
        }
        // hiding layouts (monocle/deck/tab/stack) depend on who is focused
        self.retile();
    }
}
