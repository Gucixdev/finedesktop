use std::{ffi::OsString, sync::Arc};

use crate::components::engine::layout_engine::{Bar, Layout, Params, Rect, arrange};
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
        selection::data_device::DataDeviceState,
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

    // Smithay State
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: smithay::wayland::shell::xdg::decoration::XdgDecorationState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Desktop>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,

    pub seat: Seat<Self>,

    pub config: Config,
    pub mons: Vec<Mon>,
    pub focus_mon: usize,
    pub pointer_loc: Point<f64, Logical>,

    /// `finedesktop ctl subscribe` clients: every event is written to them as one line
    pub subs: Vec<std::os::unix::net::UnixStream>,
    /// active mode and the config it replaced (restored on `mode off`)
    pub mode: Option<(String, Box<Config>, String)>,
    pub widgets: crate::components::widgets::Widgets,
    /// something the bar shows changed; redraw on the next frame
    pub bar_dirty: bool,
    /// windows already announced: id -> (app_id, title)
    pub known: std::collections::HashMap<u32, (String, String)>,
    pub last_focus: Option<u32>,
    pub panels: Vec<crate::components::engine::widget_engine::Panel>,
    /// None: the panels show on empty workspaces; Some: forced on or off with `panel`
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
        let config = Config::load();
        seat.add_keyboard(Self::xkb(&config), 200, 25).unwrap();

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
            socket_name,

            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups,
            seat,
            config,
            mons: Vec::new(),
            focus_mon: 0,
            pointer_loc: (0.0, 0.0).into(),
            subs: Vec::new(),
            mode: None,
            widgets: crate::components::widgets::Widgets::start(),
            bar_dirty: true,
            known: Default::default(),
            last_focus: None,
            panels: Vec::new(),
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


/// Where a window lives: (monitor, workspace, index in the workspace's client list).
pub type Loc = (usize, usize, usize);

impl Desktop {
    pub fn new_workspace(&self) -> Workspace {
        let c = &self.config;
        Workspace {
            clients: Vec::new(),
            focus: 0,
            layout: c.layout,
            params: Params { nmaster: c.nmaster, mfact: c.mfact, gap: c.gap, bar_h: c.bar_height() },
        }
    }

    /// Register an output: map it right of the existing ones and give it its workspaces.
    pub fn add_output(&mut self, output: Output) {
        let x: i32 = self.mons.iter().filter_map(|m| self.space.output_geometry(&m.output)).map(|g| g.size.w).sum();
        self.space.map_output(&output, (x, 0));
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

    /// Re-read the panels (key list, markdown files); redraw only when something changed.
    pub fn reload_panels(&mut self) {
        let p = crate::components::engine::widget_engine::load(&self.config);
        if p != self.panels {
            self.panels = p;
            self.bar_dirty = true;
        }
    }

    /// Panels are visible when forced on, or when automatic and monitor `mi` shows an empty workspace.
    pub fn panel_visible(&self, mi: usize) -> bool {
        if !self.config.panels || self.panels.is_empty() {
            return false;
        }
        self.panel_show.unwrap_or_else(|| self.mons.get(mi).is_some_and(|m| m.ws[m.cur].clients.is_empty()))
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
            let plan = arrange(ws.layout, area, ws.clients.len(), ws.focus, &ws.params);
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
            self.emit(Event::close(id, &app, &title));
        }
        for (id, (app, title)) in alive {
            if self.known.get(&id).is_some_and(|(_, t)| *t != title) {
                self.emit(Event::title(id, &app, &title));
            }
            self.known.insert(id, (app, title));
        }
        let now = self.focused().map(|(_, w)| Desktop::win_info(w));
        let id = now.as_ref().map(|n| n.0);
        if id != self.last_focus {
            self.last_focus = id;
            if let Some((id, app, title)) = now {
                self.emit(Event::focus(id, &app, &title));
            }
        }
    }

    /// Give keyboard focus to the focused window (or nobody) and re-run the layouts.
    pub fn refocus(&mut self) {
        self.retile();
        self.diff_events();
        let surf = self.focused().and_then(|(_, w)| w.toplevel().map(|t| t.wl_surface().clone()));
        if let Some(w) = self.focused().map(|(_, w)| w.clone()) {
            self.space.raise_element(&w, false);
        }
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, surf, serial);
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

    /// (id, app_id, title) of a toplevel, for events and the bar.
    pub fn win_info(w: &Window) -> (u32, String, String) {
        let Some(t) = w.toplevel() else { return (0, String::new(), String::new()) };
        let id = smithay::reexports::wayland_server::Resource::id(t.wl_surface()).protocol_id();
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

    pub fn load_config(&mut self) {
        self.config = Config::load();
        if let Some(kb) = self.seat.get_keyboard() {
            let cfg = self.config.clone();
            if let Err(e) = kb.set_xkb_config(self, Self::xkb(&cfg)) {
                tracing::warn!("xkb config: {e:?}");
            }
        }
        self.mode = None;
        self.widgets.configure(&self.config);
        self.reload_panels();
        self.bar_dirty = true;
        self.emit(crate::components::hooks::Event::reload());
        let c = self.config.clone();
        for m in &mut self.mons {
            while m.ws.len() < c.workspaces {
                m.ws.push(Workspace {
                    clients: Vec::new(),
                    focus: 0,
                    layout: c.layout,
                    params: Params { nmaster: c.nmaster, mfact: c.mfact, gap: c.gap, bar_h: c.bar_height() },
                });
            }
            for ws in &mut m.ws {
                ws.params.gap = c.gap;
                ws.params.bar_h = c.bar_height();
            }
        }
        self.refocus();
    }

    pub fn run_action(&mut self, a: Action) -> String {
        let n_mon = self.mons.len();
        if n_mon == 0 {
            return "error: no output".into();
        }
        let mi = self.focus_mon.min(n_mon - 1);
        let cur = self.mons[mi].cur;
        match a {
            Action::Spawn(cmd) => crate::keys::spawn(&cmd),
            Action::Close => {
                if let Some(t) = self.focused().and_then(|(_, w)| w.toplevel()) {
                    t.send_close();
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
            }
            Action::State => return self.report(),
            Action::Theme(name) => return self.set_theme(&name),
            Action::Mode(name) => return self.set_mode(&name),
            Action::Emit(text) => {
                let (name, rest) = text.split_once(' ').unwrap_or((text.as_str(), ""));
                self.emit(crate::components::hooks::Event::user(name, rest));
            }
            Action::Panel(how) => {
                match how.as_str() {
                    "toggle" => self.panel_show = Some(!self.panel_visible(mi)),
                    "show" | "on" => self.panel_show = Some(true),
                    "hide" | "off" => self.panel_show = Some(false),
                    "auto" => self.panel_show = None,
                    "reload" => self.reload_panels(),
                    _ => return "error: panel toggle|show|hide|auto|reload".into(),
                }
                self.bar_dirty = true;
            }
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
