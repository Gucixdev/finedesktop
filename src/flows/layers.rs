//! Layers are names for the z-order, bottom to top; panels are the things that live on them and can be shown and hidden:
//!   background  the wallpaper colour      bottom  the welcome panel      tiled / floating  windows (by the layout)
//!   top  the bar                          overlay  the menu and launcher
//!   ctl layers                  what is on each layer, top first
//!   ctl panels [list]           every panel: name, layer, shown or hidden
//!   ctl panels show|hide|toggle NAME
//! A panel is not a new mechanism: this asks the bar, the welcome panel and the menu to do what they already do.

use crate::state::Desktop;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Background,
    Bottom,
    Tiled,
    Floating,
    Top,
    Overlay,
}

impl Layer {
    pub const ALL: [Layer; 6] = [Layer::Background, Layer::Bottom, Layer::Tiled, Layer::Floating, Layer::Top, Layer::Overlay];

    pub fn name(self) -> &'static str {
        match self {
            Layer::Background => "background",
            Layer::Bottom => "bottom",
            Layer::Tiled => "tiled",
            Layer::Floating => "floating",
            Layer::Top => "top",
            Layer::Overlay => "overlay",
        }
    }

    pub fn parse(s: &str) -> Option<Layer> {
        Layer::ALL.into_iter().find(|l| l.name() == s)
    }
}

pub struct Panel {
    pub name: &'static str,
    pub layer: Layer,
    pub shown: bool,
}

impl Desktop {
    pub fn panels(&self) -> Vec<Panel> {
        vec![
            Panel { name: "bar", layer: Layer::Top, shown: self.config.bar != crate::config::BarPos::Off },
            Panel { name: "welcome", layer: Layer::Bottom, shown: self.panel_visible(self.focus_mon.min(self.mons.len().saturating_sub(1))) },
            Panel { name: "menu", layer: Layer::Overlay, shown: self.menu.is_some() },
        ]
    }

    pub fn layers_cmd(&mut self) -> String {
        let (tiled, floating) = self.mons.iter().fold((0, 0), |(t, f), m| {
            let n = m.ws.iter().map(|w| if w.layout.name() == "float" { (0, w.clients.len()) } else { (w.clients.len(), 0) }).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
            (t + n.0, f + n.1)
        });
        let panels = self.panels();
        let mut out = Vec::new();
        for l in Layer::ALL.into_iter().rev() {
            let names: Vec<String> = panels.iter().filter(|p| p.layer == l).map(|p| format!("{}{}", p.name, if p.shown { "" } else { " (hidden)" })).collect();
            let mut items = names;
            match l {
                Layer::Tiled => items.push(format!("{tiled} windows")),
                Layer::Floating => items.push(format!("{floating} windows")),
                Layer::Background => items.push("colour".into()),
                _ => {}
            }
            out.push(format!("{:<10} {}", l.name(), items.join(", ")));
        }
        out.join("\n")
    }

    pub fn panels_cmd(&mut self, arg: &str) -> String {
        let (verb, name) = arg.trim().split_once(' ').map_or((arg.trim(), ""), |(a, b)| (a, b.trim()));
        match verb {
            "" | "list" => self.panels().iter().map(|p| format!("{} {} {}", p.name, p.layer.name(), if p.shown { "shown" } else { "hidden" })).collect::<Vec<_>>().join("\n"),
            "show" | "hide" | "toggle" => {
                let Some(p) = self.panels().into_iter().find(|p| p.name == name) else { return format!("error: no panel {name:?} (ctl panels)") };
                let want = match verb {
                    "show" => true,
                    "hide" => false,
                    _ => !p.shown,
                };
                if want == p.shown {
                    return "ok (no change)".into();
                }
                match p.name {
                    "bar" => {
                        self.run_action(crate::config::Action::Bar(if want { "top".into() } else { "off".into() }));
                    }
                    "welcome" => return self.welcome_cmd(if want { "show" } else { "hide" }),
                    _ => {
                        if want {
                            self.menu_open(crate::components::menu::Id::Root);
                        } else {
                            self.menu = None;
                        }
                        self.bar_dirty = true;
                    }
                }
                "ok".into()
            }
            _ => "error: panels [list] | show|hide|toggle NAME".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_bottom_to_top() {
        assert!(Layer::Background < Layer::Tiled && Layer::Tiled < Layer::Floating && Layer::Floating < Layer::Top && Layer::Top < Layer::Overlay);
        assert_eq!(Layer::parse("overlay"), Some(Layer::Overlay));
        assert_eq!(Layer::parse("nope"), None);
        assert!(Layer::ALL.iter().all(|l| Layer::parse(l.name()) == Some(*l)));
    }
}
