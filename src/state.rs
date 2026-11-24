use std::{ffi::OsString, sync::Arc};

use crate::components::engine::layout_engine::{Bar, Layout, Params, Rect};
use crate::config::{Action, Config, Dir, LayoutSel, WsSel};
use smithay::output::Output;

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
        selection::{
            data_device::DataDeviceState, ext_data_control, primary_selection::PrimarySelectionState,
            wlr_data_control,
        },
        shell::xdg::XdgShellState,
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

pub struct Workspace {
    pub clients: Vec<Window>,
    pub focus: usize,
    pub layout: Layout,
    pub params: Params,
}

pub struct Mon {
    pub output: Output,
    pub ws: Vec<Workspace>,
    pub cur: usize,
    /// Tab / stack bar of the last plan for the current workspace.
    pub bar: Option<Bar>,
}

pub struct Desktop {
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,
    pub loop_handle: smithay::reexports::calloop::LoopHandle<'static, Desktop>,
    pub drm: Option<crate::components::compositor_drm::Drm>,
    /// libinput devices that are plugged in (to apply `pointer_speed`, `natural_scroll`, `tap_to_click` when the config changes)
    /// the theme being made in the theme editor (components/panels.rs)
    pub theme_draft: Option<crate::components::panels::Draft>,
    pub input_devices: Vec<smithay::reexports::input::Device>,
    pub dmabuf_state: smithay::wayland::dmabuf::DmabufState,
    /// imports waiting for a renderer that is not in the state (the nested winit backend answers them on its next frame)
    pub dmabuf_pending: Vec<(smithay::backend::allocator::dmabuf::Dmabuf, smithay::wayland::dmabuf::ImportNotifier)>,
    pub dmabuf_global: Option<smithay::wayland::dmabuf::DmabufGlobal>,

    // Smithay State
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: smithay::wayland::shell::xdg::decoration::XdgDecorationState,
    pub layer_shell_state: smithay::wayland::shell::wlr_layer::WlrLayerShellState,
    /// layer surface (launcher) that holds the keyboard until it goes away
    pub layer_focus: Option<WlSurface>,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Desktop>,
    pub data_device_state: DataDeviceState,
    /// middle-click paste of whatever is selected
    pub primary_selection_state: PrimarySelectionState,
    /// wl-copy / wl-paste / cliphist talk to the clipboard through these
    pub wlr_data_control_state: wlr_data_control::DataControlState,
    pub ext_data_control_state: ext_data_control::DataControlState,
    pub popups: PopupManager,

    pub seat: Seat<Self>,

    pub config: Config,
    pub mons: Vec<Mon>,
    pub focus_mon: usize,
    pub pointer_loc: Point<f64, Logical>,
    /// set while the pointer itself moves focus (focus_follows_mouse), so `mouse_follows_focus` does not fight it
    pub hovering: bool,

    /// `finedesktop ctl subscribe` clients: every event is written to them as one line
    pub bus: crate::flows::bus::Bus,
    pub life: crate::flows::lifecycle::Lifecycle,
    pub tags: crate::flows::tags::Tags,
    /// active mode and the config it replaced (restored on `mode off`)
    pub mode: Option<(String, Box<Config>, String)>,
    pub widgets: crate::components::widgets::Widgets,
    pub engine: crate::engine_host::EngineHost,
    pub menu: Option<crate::components::menu::Menu>,
    /// the navigation key held in the menu (repeated by `menu_hold_step`)
    pub menu_hold: Option<crate::components::menu::Hold>,
    /// something the bar shows changed; redraw on the next frame
    pub bar_dirty: bool,
    /// `ctl set KEY VALUE` without `-s`: values in force that the config files do not hold (gone at the next reload)
    pub runtime_set: Vec<(String, String)>,
    /// windows already announced: id -> (app_id, title)
    pub known: std::collections::HashMap<u32, (String, String)>,
    pub last_focus: Option<u32>,
    /// the window focused now and the one before it (`focus last`)
    pub last_win: Option<Window>,
    pub prev_win: Option<Window>,
    pub welcome: crate::components::welcome::Welcome,
    pub capture: crate::components::capture::Capture,
    pub notes: crate::components::notification::notts::Notes,
    /// the wallpaper `swaybg` was last started for (None: not yet applied)
    pub wallpaper_shown: Option<String>,
    pub fx: crate::components::fx::Fx,
    /// windows the rules have been applied to
    pub ruled: std::collections::HashSet<u32>,
    /// places saved for apps that were started again after a restart (components/restore.rs)
    pub pending: Vec<crate::components::restore::Row>,
    pub low_warned: bool,
    /// custom tab labels by window id (`ctl name`)
    pub names: std::collections::HashMap<u32, String>,
    /// None: the panels show on empty workspaces; Some: forced on or off with `welcome`
    pub panel_show: Option<bool>,
    pub bar_cache: std::collections::HashMap<String, (u64, smithay::backend::renderer::element::memory::MemoryRenderBuffer)>,
    pub solid_ids: std::collections::HashMap<(String, usize), smithay::backend::renderer::element::Id>,
}

impl Desktop {
    pub fn new(event_loop: &mut EventLoop<'static, Self>, display: Display<Self>) -> Self {
        let start_time = std::time::Instant::now();

        let dh = display.handle();

        // Here we initialize implementations of some wayland protocols
        // Some of them require us to implement traits on the Desktop state,
        // you can find those implementations in the `crate::handlers` module

        // Initialize protocols needed for displaying windows
        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = smithay::wayland::shell::xdg::decoration::XdgDecorationState::new::<Self>(&dh);
        let layer_shell_state = smithay::wayland::shell::wlr_layer::WlrLayerShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let popups = PopupManager::default();

        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);

        // Data device is responsible for clipboard and drag-and-drop
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&dh);
        let wlr_data_control_state =
            wlr_data_control::DataControlState::new::<Self, _>(&dh, Some(&primary_selection_state), |_| true);
        let ext_data_control_state =
            ext_data_control::DataControlState::new::<Self, _>(&dh, Some(&primary_selection_state), |_| true);

        // A seat is a group of keyboards, pointer and touch devices.
        // A seat typically has a pointer and maintains a keyboard focus and a pointer focus.
        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "winit");

        // Notify clients that we have a keyboard, for the sake of the example we assume that keyboard is always present.
        // You may want to track keyboard hot-plug in real compositor.
        let config = Config::load();
        seat.add_keyboard(Self::xkb(&config), config.kb_repeat_delay, config.kb_repeat_rate).unwrap();

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
            loop_handle: event_loop.handle(),
            drm: None,
            dmabuf_state: smithay::wayland::dmabuf::DmabufState::new(),
            dmabuf_global: None,
            dmabuf_pending: Vec::new(),
            socket_name,

            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            layer_shell_state,
            layer_focus: None,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            primary_selection_state,
            wlr_data_control_state,
            ext_data_control_state,
            popups,
            seat,
            config,
            mons: Vec::new(),
            focus_mon: 0,
            pointer_loc: (0.0, 0.0).into(),
            hovering: false,
            bus: Default::default(),
            life: Default::default(),
            tags: Default::default(),
            mode: None,
            widgets: crate::components::widgets::Widgets::start(),
            engine: Default::default(),
            menu: None,
            menu_hold: None,
            theme_draft: None,
            input_devices: Vec::new(),
            bar_dirty: true,
            runtime_set: Vec::new(),
            known: Default::default(),
            last_focus: None,
            last_win: None,
            prev_win: None,
            welcome: Default::default(),
            capture: Default::default(),
            notes: crate::components::notification::notts::Notes::load(crate::components::notification::notts::state_dir()),
            wallpaper_shown: None,
            fx: Default::default(),
            ruled: Default::default(),
            pending: Vec::new(),
            low_warned: false,
            names: Default::default(),
            panel_show: None,
            bar_cache: Default::default(),
            solid_ids: Default::default(),
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
        use smithay::wayland::shell::wlr_layer::Layer;
        if let Some(h) = self.layer_surface_under(pos, [Layer::Overlay, Layer::Top]) {
            return Some(h);
        }
        let win = self.space.element_under(pos).and_then(|(window, location)| {
            window
                .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + location).to_f64()))
        });
        win.or_else(|| self.layer_surface_under(pos, [Layer::Bottom, Layer::Background]))
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


/// Where a window lives: (monitor, workspace, index in the workspace's client list).
pub type Loc = (usize, usize, usize);

impl Desktop {
    pub fn new_workspace(&self) -> Workspace {
        let c = &self.config;
        Workspace {
            clients: Vec::new(),
            focus: 0,
            layout: c.layout,
            params: Params { nmaster: c.nmaster, mfact: c.mfact, gap: c.gap, bar_h: c.tab_bar_height() },
        }
    }

    /// Register an output: map it right of the existing ones and give it its workspaces.
    pub fn add_output(&mut self, output: Output) {
        let x: i32 = self.mons.iter().filter_map(|m| self.space.output_geometry(&m.output)).map(|g| g.size.w).sum();
        let at = self.config.outputs.iter().find(|o| o.name == output.name()).and_then(|o| o.pos).unwrap_or((x, 0));
        self.space.map_output(&output, at);
        let ws = (0..self.config.workspaces).map(|_| self.new_workspace()).collect();
        self.mons.push(Mon { output, ws, cur: 0, bar: None });
    }

    pub fn mon_by_output(&self, o: &Output) -> Option<usize> {
        self.mons.iter().position(|m| m.output == *o)
    }

    pub fn find_window(&self, surface: &WlSurface) -> Option<Loc> {
        for (mi, m) in self.mons.iter().enumerate() {
            for (wi, w) in m.ws.iter().enumerate() {
                if let Some(ci) = w.clients.iter().position(|c| c.toplevel().is_some_and(|t| t.wl_surface() == surface)) {
                    return Some((mi, wi, ci));
                }
            }
        }
        None
    }

    pub fn window_at(&self, loc: Loc) -> Option<&Window> {
        self.mons.get(loc.0)?.ws.get(loc.1)?.clients.get(loc.2)
    }

    /// Focused window of the focused monitor's current workspace.
    pub fn focused(&self) -> Option<(Loc, &Window)> {
        let m = self.mons.get(self.focus_mon)?;
        let ws = &m.ws[m.cur];
        let w = ws.clients.get(ws.focus)?;
        Some(((self.focus_mon, m.cur, ws.focus), w))
    }

    /// Output area minus layer-shell exclusive zones (bars), in global coordinates.
    pub fn usable_area(&self, mi: usize) -> Option<Rect> {
        let output = &self.mons.get(mi)?.output;
        let geo = self.space.output_geometry(output)?;
        // the zone is cached from the last arrange(): redo it so a resized output is not tiled at its old size
        let mut map = smithay::desktop::layer_map_for_output(output);
        map.arrange();
        let zone = map.non_exclusive_zone();
        drop(map);
        let mut r = Rect { x: geo.loc.x + zone.loc.x, y: geo.loc.y + zone.loc.y, w: zone.size.w, h: zone.size.h };
        // the status bar is ours, not a layer surface: take its row off the top or bottom
        let bh = self.config.bar_height();
        match self.config.bar {
            crate::config::BarPos::Top => {
                r.y += bh;
                r.h -= bh;
            }
            crate::config::BarPos::Bottom => r.h -= bh,
            crate::config::BarPos::Off => {}
        }
        Some(r)
    }

    /// Tell the widget worker what the config wants (segments, interval, on/off) and redraw the bar.
    pub fn apply_widget_state(&mut self) {
        self.reload_panels();
        self.widgets.configure(&self.config);
        self.bar_dirty = true;
        self.retile();
    }

    /// Drop dead windows, run the layout engine on every monitor and apply the plans.
    pub fn retile(&mut self) {
        use smithay::utils::IsAlive;
        for mi in 0..self.mons.len() {
            for ws in &mut self.mons[mi].ws {
                let focused = ws.clients.get(ws.focus).cloned();
                ws.clients.retain(|w| w.alive());
                ws.focus = focused
                    .and_then(|f| ws.clients.iter().position(|c| *c == f))
                    .unwrap_or(ws.focus.min(ws.clients.len().saturating_sub(1)));
            }
            let Some(area) = self.usable_area(mi) else { continue };
            let cur = self.mons[mi].cur;
            // hide the other workspaces
            let hidden: Vec<Window> = self.mons[mi]
                .ws
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != cur)
                .flat_map(|(_, w)| w.clients.clone())
                .collect();
            for w in hidden {
                self.space.unmap_elem(&w);
            }
            let ws = &self.mons[mi].ws[cur];
            let plan = self.engine.arrange(ws.layout, area, ws.clients.len(), ws.focus, &ws.params);
            let clients = ws.clients.clone();
            let focus = ws.focus;
            self.mons[mi].bar = plan.bar;
            for (ci, (win, slot)) in clients.into_iter().zip(plan.slots).enumerate() {
                let active = mi == self.focus_mon && ci == focus;
                win.set_activated(active);
                let Some(slot) = slot else {
                    // float: client picks its size, stays where it is
                    if let Some(top) = win.toplevel() {
                        top.with_pending_state(|st| {
                            st.size = None;
                            set_tiled(st, false);
                        });
                        top.send_pending_configure();
                    }
                    if self.space.element_location(&win).is_none() {
                        let (ax, ay) = (area.x + 40 + 24 * ci as i32, area.y + 40 + 24 * ci as i32);
                        self.space.map_element(win, (ax, ay), false);
                    }
                    continue;
                };
                if !slot.visible {
                    self.space.unmap_elem(&win);
                    continue;
                }
                if let Some(top) = win.toplevel() {
                    let size = (slot.rect.w, slot.rect.h).into();
                    top.with_pending_state(|st| {
                        st.size = Some(size);
                        set_tiled(st, true);
                    });
                    top.send_pending_configure();
                }
                if self.space.element_location(&win) != Some((slot.rect.x, slot.rect.y).into()) {
                    self.space.map_element(win, (slot.rect.x, slot.rect.y), false);
                }
            }
        }
    }

    /// Emit close / focus / title events by comparing against what we last announced.
    fn diff_events(&mut self) {
        use crate::components::hooks::Event;
        let mut alive = std::collections::HashMap::new();
        for m in &self.mons {
            for w in m.ws.iter().flat_map(|ws| ws.clients.iter()) {
                let (id, app, title) = Desktop::win_info(w);
                alive.insert(id, (app, title));
            }
        }
        let gone: Vec<u32> = self.known.keys().filter(|id| !alive.contains_key(id)).copied().collect();
        for id in gone {
            let (app, title) = self.known.remove(&id).unwrap_or_default();
            self.names.remove(&id);
            self.ruled.remove(&id);
            self.tags.forget(id);
            self.emit(Event::close(id, &app, &title));
        }
        for (id, (app, title)) in alive {
            if self.known.get(&id).is_some_and(|(_, t)| *t != title) {
                self.emit(Event::title(id, &app, &title));
            }
            self.known.insert(id, (app, title));
        }
        self.apply_rules();
        let now = self.focused().map(|(_, w)| Desktop::win_info(w));
        let id = now.as_ref().map(|n| n.0);
        if id != self.last_focus {
            self.last_focus = id;
            self.prev_win = self.last_win.take();
            self.last_win = self.focused().map(|(_, w)| w.clone());
            if let Some((id, app, title)) = now {
                self.emit(Event::focus(id, &app, &title));
            }
        }
    }

    /// Give keyboard focus to the focused window (or nobody) and re-run the layouts.
    pub fn refocus(&mut self) {
        self.retile();
        self.diff_events();
        if self.layer_focus.as_ref().is_some_and(|s| !smithay::reexports::wayland_server::Resource::is_alive(s)) {
            self.layer_focus = None;
        }
        let surf = self.layer_focus.clone().or_else(|| self.focused().and_then(|(_, w)| w.toplevel().map(|t| t.wl_surface().clone())));
        if let Some(w) = self.focused().map(|(_, w)| w.clone()) {
            self.space.raise_element(&w, false);
        }
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, surf, serial);
        }
        if self.config.mouse_follows_focus && !self.hovering && self.layer_focus.is_none() {
            self.warp_to_focused();
        }
    }

    /// Put the pointer in the middle of the focused window, unless it is inside that window already.
    fn warp_to_focused(&mut self) {
        let Some(g) = self.focused().and_then(|(_, w)| self.space.element_geometry(w)) else { return };
        let p = self.pointer_loc;
        if p.x >= g.loc.x as f64 && p.x < (g.loc.x + g.size.w) as f64 && p.y >= g.loc.y as f64 && p.y < (g.loc.y + g.size.h) as f64 {
            return;
        }
        let to = (g.loc.x as f64 + g.size.w as f64 / 2.0, g.loc.y as f64 + g.size.h as f64 / 2.0).into();
        self.pointer_loc = to;
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let Some(pointer) = self.seat.get_pointer() else { return };
        let under = self.surface_under(to);
        let time = smithay::backend::input::InputTime::now();
        pointer.motion(self, under, &smithay::input::pointer::MotionEvent { location: to, serial, time });
        pointer.frame(self);
        self.bar_dirty = true;
    }

    /// Rectangles of the current workspace's windows on monitor `mi`, `None` for the ones not shown.
    fn ws_rects(&self, mi: usize) -> Vec<Option<crate::components::engine::layout_engine::Rect>> {
        use crate::components::engine::layout_engine::Rect;
        let m = &self.mons[mi];
        m.ws[m.cur]
            .clients
            .iter()
            .map(|w| {
                let g = self.space.element_geometry(w)?;
                Some(Rect { x: g.loc.x, y: g.loc.y, w: g.size.w, h: g.size.h })
            })
            .collect()
    }

    fn mon_rects(&self) -> Vec<crate::components::engine::layout_engine::Rect> {
        use crate::components::engine::layout_engine::Rect;
        self.mons
            .iter()
            .map(|m| {
                let g = self.space.output_geometry(&m.output).unwrap_or_default();
                Rect { x: g.loc.x, y: g.loc.y, w: g.size.w, h: g.size.h }
            })
            .collect()
    }

    /// One window at a time (monocle, tabbed, list): left/up is previous, right/down is next.
    fn one_at_a_time(rects: &[Option<crate::components::engine::layout_engine::Rect>]) -> bool {
        rects.len() > 1 && rects.iter().filter(|r| r.is_some()).count() <= 1
    }

    pub fn focus_dir(&mut self, d: crate::components::engine::focus_engine::Cardinal) {
        use crate::components::engine::focus_engine::Cardinal;
        let mi = self.focus_mon.min(self.mons.len().saturating_sub(1));
        if self.mons.is_empty() {
            return;
        }
        let (cur, f) = (self.mons[mi].cur, self.mons[mi].ws[self.mons[mi].cur].focus);
        let rects = self.ws_rects(mi);
        if Self::one_at_a_time(&rects) {
            let n = rects.len();
            let dir = if matches!(d, Cardinal::Left | Cardinal::Up) { Dir::Prev } else { Dir::Next };
            self.mons[mi].ws[cur].focus = Self::step(f, n, dir);
            self.refocus();
            return;
        }
        if let Some(i) = self.engine.neighbor(&rects, f, d) {
            self.focus_loc((mi, cur, i));
            return;
        }
        // nothing that way on this monitor: go to the monitor beyond, onto its focused window
        if let Some(to) = self.engine.monitor_towards(&self.mon_rects(), mi, d) {
            self.focus_mon = to;
            self.refocus();
        }
    }

    /// Load (or reload) the engine library and lay the windows out again with it.
    pub fn reload_engine(&mut self) -> Result<String, String> {
        let path = crate::config::engine_path(&self.config).ok_or("no engine path")?;
        let msg = self.engine.reload(&path)?;
        tracing::info!("{msg}");
        self.retile();
        self.bar_dirty = true;
        Ok(msg)
    }

    pub fn move_dir(&mut self, d: crate::components::engine::focus_engine::Cardinal) {
        use crate::components::engine::focus_engine::Cardinal;
        if self.mons.is_empty() {
            return;
        }
        let mi = self.focus_mon.min(self.mons.len() - 1);
        let cur = self.mons[mi].cur;
        let f = self.mons[mi].ws[cur].focus;
        if f >= self.mons[mi].ws[cur].clients.len() {
            return;
        }
        if self.mons[mi].ws[cur].layout == crate::components::engine::layout_engine::Layout::Float {
            let w = self.mons[mi].ws[cur].clients[f].clone();
            if let Some(loc) = self.space.element_location(&w) {
                let (dx, dy) = match d {
                    Cardinal::Left => (-60, 0),
                    Cardinal::Right => (60, 0),
                    Cardinal::Up => (0, -60),
                    Cardinal::Down => (0, 60),
                };
                self.space.map_element(w, (loc.x + dx, loc.y + dy), true);
            }
            return;
        }
        let rects = self.ws_rects(mi);
        if Self::one_at_a_time(&rects) {
            let n = rects.len();
            let to = Self::step(f, n, if matches!(d, Cardinal::Left | Cardinal::Up) { Dir::Prev } else { Dir::Next });
            self.mons[mi].ws[cur].clients.swap(f, to);
            self.mons[mi].ws[cur].focus = to;
            self.refocus();
            return;
        }
        if let Some(i) = self.engine.neighbor(&rects, f, d) {
            let ws = &mut self.mons[mi].ws[cur];
            ws.clients.swap(f, i);
            ws.focus = i;
            self.refocus();
        } else if let Some(to) = self.engine.monitor_towards(&self.mon_rects(), mi, d) {
            let w = self.mons[mi].ws[cur].clients.remove(f);
            let ws = &mut self.mons[mi].ws[cur];
            ws.focus = f.min(ws.clients.len().saturating_sub(1));
            let tc = self.mons[to].cur;
            self.mons[to].ws[tc].clients.insert(0, w);
            self.mons[to].ws[tc].focus = 0;
            self.focus_mon = to;
            self.refocus();
        }
    }

    /// Back to the window that had focus before this one (Alt-Tab style), across workspaces and monitors.
    pub fn focus_last(&mut self) {
        let Some(prev) = self.prev_win.clone() else { return };
        for (mi, m) in self.mons.iter().enumerate() {
            for (wi, ws) in m.ws.iter().enumerate() {
                if let Some(ci) = ws.clients.iter().position(|c| *c == prev) {
                    self.focus_loc((mi, wi, ci));
                    return;
                }
            }
        }
    }

    /// Focus follows the pointer: the window under it gets keyboard focus when it is not the focused one.
    pub fn hover_focus(&mut self) {
        if !self.config.focus_follows_mouse {
            return;
        }
        self.hovering = true;
        self.hover_focus_inner();
        self.hovering = false;
    }

    fn hover_focus_inner(&mut self) {
        let pos = self.pointer_loc;
        let hit = self
            .space
            .element_under(pos)
            .and_then(|(w, _)| w.toplevel().map(|t| t.wl_surface().clone()))
            .and_then(|s| self.find_window(&s));
        match hit {
            Some(loc) => {
                if self.focused().map(|(l, _)| l) != Some(loc) {
                    self.focus_loc(loc);
                }
            }
            None => {
                // empty desktop under the pointer: at least the monitor under it becomes the focused one
                let under = self.mon_rects().iter().position(|r| {
                    pos.x >= r.x as f64 && pos.x < (r.x + r.w) as f64 && pos.y >= r.y as f64 && pos.y < (r.y + r.h) as f64
                });
                if let Some(mi) = under.filter(|mi| *mi != self.focus_mon) {
                    self.focus_mon = mi;
                    self.refocus();
                }
            }
        }
    }

    pub fn focus_loc(&mut self, loc: Loc) {
        let (mi, wi, ci) = loc;
        if self.window_at(loc).is_none() {
            return;
        }
        self.focus_mon = mi;
        self.mons[mi].cur = wi;
        self.mons[mi].ws[wi].focus = ci;
        self.refocus();
    }

    /// `ctl name [WINDOW] TEXT`: set the label shown in the tab / stack bar. `-` goes back to the window title.
    pub fn name_cmd(&mut self, text: &str) -> String {
        let (first, rest) = text.split_once(' ').map_or((text, ""), |(a, b)| (a, b.trim()));
        let (id, label) = match first.parse::<u32>() {
            Ok(id) if !rest.is_empty() => (id, rest),
            _ => match self.focused() {
                Some((_, w)) => (Desktop::win_info(w).0, text.trim()),
                None => return "error: no focused window".into(),
            },
        };
        if label == "-" {
            self.names.remove(&id);
        } else {
            self.names.insert(id, label.to_string());
        }
        self.bar_dirty = true;
        "ok".into()
    }

    /// (id, app_id, title) of a toplevel, for events and the bar.
    pub fn win_info(w: &Window) -> (u32, String, String) {
        let Some(t) = w.toplevel() else { return (0, String::new(), String::new()) };
        // protocol ids repeat between clients (each has its own numbering): hash the full object id
        let id = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            smithay::reexports::wayland_server::Resource::id(t.wl_surface()).hash(&mut h);
            (h.finish() % 1_000_000) as u32 + 1
        };
        let (app, title) = smithay::wayland::compositor::with_states(t.wl_surface(), |s| {
            s.data_map
                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok().map(|d| (d.app_id.clone().unwrap_or_default(), d.title.clone().unwrap_or_default())))
                .unwrap_or_default()
        });
        (id, app, title)
    }

    pub fn add_window(&mut self, win: Window) {
        let mi = self.focus_mon.min(self.mons.len().saturating_sub(1));
        let Some(m) = self.mons.get_mut(mi) else { return };
        let cur = m.cur;
        let out = m.output.name();
        let (id, app, title) = Desktop::win_info(&win);
        m.ws[cur].clients.insert(0, win);
        m.ws[cur].focus = 0;
        self.known.insert(id, (app.clone(), title.clone()));
        self.emit(crate::components::hooks::Event::open(id, &out, cur + 1, &app, &title));
        self.refocus();
    }

    pub fn remove_dead(&mut self) {
        self.refocus();
    }

    fn step(i: usize, n: usize, d: Dir) -> usize {
        match d {
            Dir::Next => (i + 1) % n,
            Dir::Prev => (i + n - 1) % n,
        }
    }

    fn ws_target(&self, mi: usize, sel: WsSel) -> usize {
        let m = &self.mons[mi];
        match sel {
            WsSel::N(n) => (n - 1).min(m.ws.len() - 1),
            WsSel::Dir(d) => Self::step(m.cur, m.ws.len(), d),
        }
    }

    fn xkb(c: &Config) -> smithay::input::keyboard::XkbConfig<'_> {
        smithay::input::keyboard::XkbConfig {
            layout: &c.kb_layout,
            options: (!c.kb_options.is_empty()).then(|| c.kb_options.clone()),
            ..Default::default()
        }
    }

    /// Put the pointer and touchpad settings of the config on every plugged-in libinput device (what a device lacks is skipped).
    pub fn apply_input(&mut self) {
        let c = &self.config;
        for d in &mut self.input_devices {
            if d.config_accel_is_available() {
                let _ = d.config_accel_set_speed(c.pointer_speed as f64);
            }
            if d.config_scroll_has_natural_scroll() {
                let _ = d.config_scroll_set_natural_scroll_enabled(c.natural_scroll);
            }
            if d.config_tap_finger_count() > 0 {
                let _ = d.config_tap_set_enabled(c.tap_to_click);
            }
            if d.config_accel_is_available() {
                let _ = d.config_accel_set_profile(if c.accel_flat { smithay::reexports::input::AccelProfile::Flat } else { smithay::reexports::input::AccelProfile::Adaptive });
            }
            if d.config_left_handed_is_available() {
                let _ = d.config_left_handed_set(c.left_handed);
            }
            if d.config_middle_emulation_is_available() {
                let _ = d.config_middle_emulation_set_enabled(c.middle_emulation);
            }
            if d.config_dwt_is_available() {
                let _ = d.config_dwt_set_enabled(c.disable_while_typing);
            }
        }
    }

    pub fn load_config(&mut self) {
        self.config = Config::load();
        self.runtime_set.clear();
        self.config_applied();
        self.emit(crate::components::hooks::Event::reload());
    }

    /// Everything that follows a changed `self.config`: keyboard, input devices, wallpaper, widgets, panels, workspaces.
    pub fn config_applied(&mut self) {
        if let Some(kb) = self.seat.get_keyboard() {
            let cfg = self.config.clone();
            if let Err(e) = kb.set_xkb_config(self, Self::xkb(&cfg)) {
                tracing::warn!("xkb config: {e:?}");
            }
            kb.change_repeat_info(cfg.kb_repeat_rate, cfg.kb_repeat_delay);
        }
        self.apply_input();
        self.wallpaper_apply();
        self.mode = None;
        self.widgets.configure(&self.config);
        self.reload_panels();
        self.bar_dirty = true;
        let c = self.config.clone();
        for m in &mut self.mons {
            while m.ws.len() < c.workspaces {
                m.ws.push(Workspace {
                    clients: Vec::new(),
                    focus: 0,
                    layout: c.layout,
                    params: Params { nmaster: c.nmaster, mfact: c.mfact, gap: c.gap, bar_h: c.tab_bar_height() },
                });
            }
            for ws in &mut m.ws {
                ws.params.gap = c.gap;
                ws.params.bar_h = c.tab_bar_height();
            }
        }
        self.refocus();
    }

    pub fn run_action(&mut self, a: Action) -> String {
        if !matches!(a, Action::Log(_) | Action::State(_) | Action::Get(_)) {
            tracing::info!("action: {a:?}");
        }
        let n_mon = self.mons.len();
        if n_mon == 0 {
            return "error: no output".into();
        }
        let mi = self.focus_mon.min(n_mon - 1);
        let cur = self.mons[mi].cur;
        match a {
            Action::Spawn(cmd) => crate::keys::spawn(&cmd),
            Action::Close => {
                if let Some(w) = self.focused().map(|(_, w)| w.clone()) {
                    self.tmux_kill_for(&w);
                    if let Some(t) = w.toplevel() {
                        t.send_close();
                    }
                }
            }
            Action::Quit => self.loop_signal.stop(),
            Action::Vt(n) => {
                if let Some(d) = self.drm.as_mut() {
                    d.change_vt(n as i32);
                }
            }
            Action::Reload => {
                self.load_config();
                // an engine library that is there (and newer, or not loaded yet) is picked up too
                if crate::config::engine_path(&self.config).is_some_and(|p| p.is_file()) {
                    if let Err(e) = self.reload_engine() {
                        tracing::warn!("engine: {e}");
                    }
                }
            }
            Action::Menu(c) => return self.menu_cmd(&c),
            Action::Prefer(a) => return self.prefer(&a),
            Action::Wallpaper(a) => return self.wallpaper_cmd(&a),
            Action::Layouts => return Layout::ALL.iter().map(|l| l.name()).collect::<Vec<_>>().join(" "),
            Action::ReloadEngine => {
                return match self.reload_engine() {
                    Ok(m) => m,
                    Err(e) => format!("error: {e}"),
                };
            }
            Action::State(true) => return self.report_json(),
            Action::State(false) => return self.report(),
            Action::Name(text) => return self.name_cmd(&text),
            Action::Session(what) => return self.session_cmd(&what),
            Action::Autostart(arg) => return self.autostart_cmd(&arg),
            Action::Tags(arg) => return self.tags_cmd(&arg),
            Action::Group(arg) => return self.group_cmd(&arg),
            Action::Panels(arg) => return self.panels_cmd(&arg),
            Action::Layers => return self.layers_cmd(),
            Action::Components => return self.components_cmd(),
            Action::Enable(n) => return self.set_component(&n, true),
            Action::Disable(n) => return self.set_component(&n, false),
            Action::Restart => return self.life_restart(),
            Action::Observe(arg) => return self.observe_cmd(&arg),
            Action::Power(arg) => return self.power_cmd(&arg),
            Action::Crash(arg) => return self.crash_cmd(&arg),
            Action::Trash(arg) => return self.trash_cmd(&arg),
            Action::Notify(arg) => return self.notify_cmd(&arg),
            Action::Log(n) => return crate::components::log::tail(&n),
            Action::Theme(name) => return self.set_theme(&name),
            Action::Set(arg) => return self.set_cmd(&arg),
            Action::Get(key) => return self.get_cmd(&key),
            Action::Mode(name) => return self.set_mode(&name),
            Action::Emit(text) => {
                let (name, rest) = text.split_once(' ').unwrap_or((text.as_str(), ""));
                self.emit(crate::components::hooks::Event::user(name, rest));
            }
            Action::Panel(how) => return self.welcome_cmd(&how),
            Action::Screenshot(path) => return self.screenshot_cmd(&path),
            Action::Screenrecord(how) => return self.screenrecord_cmd(&how),
            Action::Bar(how) => {
                use crate::config::BarPos;
                let c = &mut self.config;
                c.bar = match how.as_str() {
                    "top" => BarPos::Top,
                    "bottom" => BarPos::Bottom,
                    "off" | "hide" => BarPos::Off,
                    "toggle" => if c.bar == BarPos::Off { BarPos::Top } else { BarPos::Off },
                    _ => return "error: bar top|bottom|off|toggle".into(),
                };
                self.bar_dirty = true;
                self.retile();
            }
            Action::Focus(d) => {
                let n = self.mons[mi].ws[cur].clients.len();
                if n > 0 {
                    let f = self.mons[mi].ws[cur].focus;
                    self.mons[mi].ws[cur].focus = Self::step(f, n, d);
                    self.refocus();
                }
            }
            Action::FocusDir(d) => self.focus_dir(d),
            Action::MoveDir(d) => self.move_dir(d),
            Action::FocusLast => self.focus_last(),
            Action::Zoom => {
                let ws = &mut self.mons[mi].ws[cur];
                if ws.focus > 0 && ws.focus < ws.clients.len() {
                    let w = ws.clients.remove(ws.focus);
                    ws.clients.insert(0, w);
                    ws.focus = 0;
                    self.refocus();
                }
            }
            Action::Layout(sel) => {
                let ws = &mut self.mons[mi].ws[cur];
                ws.layout = match sel {
                    LayoutSel::Set(l) => l,
                    LayoutSel::Dir(Dir::Next) => ws.layout.next(),
                    LayoutSel::Dir(Dir::Prev) => ws.layout.prev(),
                };
                let name = ws.layout.name();
                let out = self.mons[mi].output.name();
                self.refocus();
                self.emit(crate::components::hooks::Event::layout(&out, name));
            }
            Action::Mfact(d) => {
                let p = &mut self.mons[mi].ws[cur].params;
                p.mfact = (p.mfact + d).clamp(0.1, 0.9);
                self.retile();
            }
            Action::Nmaster(d) => {
                let p = &mut self.mons[mi].ws[cur].params;
                p.nmaster = (p.nmaster as i32 + d).max(0) as usize;
                self.retile();
            }
            Action::View(sel) => {
                let t = self.ws_target(mi, sel);
                self.mons[mi].cur = t;
                let out = self.mons[mi].output.name();
                self.refocus();
                self.emit(crate::components::hooks::Event::tag(&out, t + 1, 0));
            }
            Action::Tag(sel) => {
                let t = self.ws_target(mi, sel);
                let ws = &mut self.mons[mi].ws;
                if t != cur && ws[cur].focus < ws[cur].clients.len() {
                    let f = ws[cur].focus;
                    let w = ws[cur].clients.remove(f);
                    ws[cur].focus = f.min(ws[cur].clients.len().saturating_sub(1));
                    ws[t].clients.insert(0, w);
                    ws[t].focus = 0;
                    self.refocus();
                }
            }
            Action::FocusMon(d) => {
                self.focus_mon = Self::step(mi, n_mon, d);
                self.refocus();
            }
            Action::SendMon(d) => {
                let to = Self::step(mi, n_mon, d);
                let ws = &mut self.mons[mi].ws[cur];
                if to != mi && ws.focus < ws.clients.len() {
                    let f = ws.focus;
                    let w = ws.clients.remove(f);
                    ws.focus = f.min(ws.clients.len().saturating_sub(1));
                    let tc = self.mons[to].cur;
                    self.mons[to].ws[tc].clients.insert(0, w);
                    self.mons[to].ws[tc].focus = 0;
                    self.focus_mon = to;
                    self.refocus();
                }
            }
        }
        "ok".into()
    }

    /// Human and machine readable snapshot, used by `finedesktop ctl state` and the tests.
    fn title_of(&self, c: &Window) -> String {
        c.toplevel()
            .map(|t| {
                smithay::wayland::compositor::with_states(t.wl_surface(), |s| {
                    s.data_map
                        .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                        .and_then(|d| d.lock().ok().and_then(|d| d.title.clone()))
                        .unwrap_or_default()
                })
            })
            .unwrap_or_default()
    }

    /// `ctl state json`: the same facts as `state`, for programs and agents, plus theme, mode, menu and widget.
    pub fn report_json(&self) -> String {
        let mut mons = Vec::new();
        for (mi, m) in self.mons.iter().enumerate() {
            let g = self.space.output_geometry(&m.output).unwrap_or_default();
            let ws = &m.ws[m.cur];
            let mut wins = Vec::new();
            for (wi, w) in m.ws.iter().enumerate() {
                for (ci, c) in w.clients.iter().enumerate() {
                    let geo = match self.space.element_geometry(c) {
                        Some(g) => format!("[{},{},{},{}]", g.loc.x, g.loc.y, g.size.w, g.size.h),
                        None => "null".into(),
                    };
                    wins.push(format!(
                        "{{\"ws\":{},\"index\":{ci},\"title\":{},\"geometry\":{geo},\"focused\":{}}}",
                        wi + 1,
                        json_str(&self.title_of(c)),
                        wi == m.cur && ci == w.focus && mi == self.focus_mon,
                    ));
                }
            }
            mons.push(format!(
                "{{\"index\":{mi},\"name\":{},\"focused\":{},\"geometry\":[{},{},{},{}],\"workspace\":{},\"workspaces\":{},\"layout\":{},\"mfact\":{:.2},\"nmaster\":{},\"windows\":[{}]}}",
                json_str(&m.output.name()), mi == self.focus_mon, g.loc.x, g.loc.y, g.size.w, g.size.h,
                m.cur + 1, m.ws.len(), json_str(ws.layout.name()), ws.params.mfact, ws.params.nmaster, wins.join(","),
            ));
        }
        let page = self.welcome.page().map(|p| p.title.clone()).unwrap_or_default();
        format!(
            "{{\"theme\":{},\"menu\":{},\"welcome\":{},\"monitors\":[{}]}}",
            json_str(&self.config.theme.name),
            self.menu.is_some(),
            json_str(&page),
            mons.join(","),
        )
    }

    pub fn report(&self) -> String {
        let mut out = String::new();
        for (mi, m) in self.mons.iter().enumerate() {
            let g = self.space.output_geometry(&m.output).unwrap_or_default();
            let ws = &m.ws[m.cur];
            out += &format!(
                "monitor {mi}{} {} {}x{}@{},{} ws {}/{} layout {} mfact {:.2} nmaster {}\n",
                if mi == self.focus_mon { "*" } else { "" },
                m.output.name(), g.size.w, g.size.h, g.loc.x, g.loc.y,
                m.cur + 1, m.ws.len(), ws.layout.name(), ws.params.mfact, ws.params.nmaster,
            );
            for (wi, w) in m.ws.iter().enumerate() {
                for (ci, c) in w.clients.iter().enumerate() {
                    let title = c.toplevel().map(|t| {
                        smithay::wayland::compositor::with_states(t.wl_surface(), |s| {
                            s.data_map
                                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                                .and_then(|d| d.lock().ok().and_then(|d| d.title.clone()))
                                .unwrap_or_default()
                        })
                    });
                    let geo = self.space.element_geometry(c);
                    out += &format!(
                        "  ws{} #{ci}{} {:?} {}\n",
                        wi + 1,
                        if wi == m.cur && ci == w.focus && mi == self.focus_mon { "*" } else { "" },
                        title.unwrap_or_default(),
                        match geo { Some(g) => format!("{}x{}@{},{}", g.size.w, g.size.h, g.loc.x, g.loc.y), None => "hidden".into() },
                    );
                }
            }
        }
        out
    }
}

/// Tiled windows get the Tiled* states so clients stop snapping to their own size increments
/// (foot's cell grid) and drop shadows/rounded corners.
fn set_tiled(st: &mut smithay::wayland::shell::xdg::ToplevelState, on: bool) {
    use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::*;
    for s in [TiledLeft, TiledRight, TiledTop, TiledBottom] {
        if on {
            st.states.set(s);
        } else {
            st.states.unset(s);
        }
    }
}

/// A JSON string literal: quotes, backslashes and control characters escaped.
pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod json_tests {
    use super::json_str;

    #[test]
    fn strings_are_escaped() {
        assert_eq!(json_str("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
        assert_eq!(json_str("\u{1}"), "\"\\u0001\"");
        assert_eq!(json_str("zażółć"), "\"zażółć\"");
    }
}
