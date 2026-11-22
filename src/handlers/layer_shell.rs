//! wlr-layer-shell: bars, launchers (dmenu), wallpapers and notification daemons.
//! Smithay keeps the layers per output (`layer_map_for_output`) and renders them with the space; here we map
//! new surfaces, send the first configure, give keyboard focus to launchers and put the exclusive zones into the layout.

use smithay::{
    desktop::{LayerSurface, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::{Resource, protocol::{wl_output::WlOutput, wl_surface::WlSurface}},
    utils::{Logical, Point},
    wayland::{
        compositor::with_states,
        shell::wlr_layer::{KeyboardInteractivity, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler, WlrLayerShellState},
    },
};

use crate::Desktop;

impl WlrLayerShellHandler for Desktop {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(&mut self, surface: WlrLayerSurface, output: Option<WlOutput>, _layer: Layer, namespace: String) {
        let out = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.mons.get(self.focus_mon).map(|m| m.output.clone()))
            .or_else(|| self.space.outputs().next().cloned());
        let Some(out) = out else {
            surface.send_close();
            return;
        };
        let layer = LayerSurface::new(surface, namespace);
        if let Err(e) = layer_map_for_output(&out).map_layer(&layer) {
            tracing::warn!("layer surface: {e}");
        }
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let outs: Vec<Output> = self.space.outputs().cloned().collect();
        for o in outs {
            let mut map = layer_map_for_output(&o);
            let found = map.layers().find(|l| l.layer_surface() == &surface).cloned();
            if let Some(l) = found {
                map.unmap_layer(&l);
            }
        }
        if self.layer_focus.as_ref().is_some_and(|s| !s.is_alive() || s == surface.wl_surface()) {
            self.layer_focus = None;
        }
        self.retile();
        self.refocus();
    }
}

impl Desktop {
    /// After a commit of a layer surface: first configure, new arrangement (exclusive zones) and keyboard focus.
    /// Returns true when `surface` belongs to a layer surface.
    pub fn layer_commit(&mut self, surface: &WlSurface) -> bool {
        let outs: Vec<Output> = self.space.outputs().cloned().collect();
        for o in outs {
            let mut map = layer_map_for_output(&o);
            let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).cloned() else { continue };
            map.arrange();
            drop(map);
            let sent = with_states(surface, |s| {
                s.data_map.get::<LayerSurfaceData>().map(|d| d.lock().unwrap().initial_configure_sent).unwrap_or(false)
            });
            if !sent {
                layer.layer_surface().send_configure();
            }
            let exclusive = layer.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive;
            if exclusive && matches!(layer.layer(), Layer::Top | Layer::Overlay) {
                if self.layer_focus.as_ref() != Some(layer.wl_surface()) {
                    self.layer_focus = Some(layer.wl_surface().clone());
                    self.refocus();
                }
            } else if self.layer_focus.as_ref() == Some(layer.wl_surface()) {
                self.layer_focus = None;
                self.refocus();
            }
            self.retile();
            return true;
        }
        false
    }

    /// The layer surface (and its position) under `pos`, restricted to the given layers.
    pub fn layer_surface_under(&self, pos: Point<f64, Logical>, layers: [Layer; 2]) -> Option<(WlSurface, Point<f64, Logical>)> {
        let out = self.space.outputs().find(|o| self.space.output_geometry(o).is_some_and(|g| g.to_f64().contains(pos)))?;
        let og = self.space.output_geometry(out)?;
        let rel = pos - og.loc.to_f64();
        let map = layer_map_for_output(out);
        for l in layers {
            let Some(layer) = map.layer_under(l, rel) else { continue };
            let lg = map.layer_geometry(layer)?;
            if let Some((s, p)) = layer.surface_under(rel - lg.loc.to_f64(), WindowSurfaceType::ALL) {
                return Some((s, (p + lg.loc + og.loc).to_f64()));
            }
        }
        None
    }

    /// Frame callbacks for the layer surfaces of `output`.
    pub fn layer_frames(&self, output: &Output, time: std::time::Duration) {
        let map = layer_map_for_output(output);
        for l in map.layers() {
            l.send_frame(output, time, Some(std::time::Duration::ZERO), |_, _| Some(output.clone()));
        }
    }
}
