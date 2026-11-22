//! The one menu panel (Super+W): Aplikacje (native / Flatpak), Gry, Schowek, Ustawienia. Drawn by the compositor like
//! the bar, driven by keys or by `ctl menu ...`, so a script or an agent uses exactly what the user uses.
//! A level is a title plus entries; an entry opens another level or does one thing.

use super::apps;
use crate::config::{Action, LayoutSel};
use crate::Desktop;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Id {
    Root,
    Apps,
    Native,
    Flatpak,
    AllApps,
    Games,
    Clip,
    Calc,
    Settings,
    Prefer,
    Themes,
    Modes,
    Layouts,
}

impl Id {
    pub fn parse(s: &str) -> Option<Id> {
        Some(match s {
            "" | "root" | "main" => Id::Root,
            "apps" => Id::Apps,
            "native" => Id::Native,
            "flatpak" => Id::Flatpak,
            "all" => Id::AllApps,
            "games" => Id::Games,
            "clip" | "clipboard" => Id::Clip,
            "calc" => Id::Calc,
            "settings" => Id::Settings,
            "prefer" => Id::Prefer,
            "themes" | "theme" => Id::Themes,
            "modes" | "mode" => Id::Modes,
            "layouts" | "layout" => Id::Layouts,
            _ => return None,
        })
    }

    fn title(self) -> &'static str {
        match self {
            Id::Root => "Menu",
            Id::Apps => "Aplikacje",
            Id::Native => "Natywne",
            Id::Flatpak => "Flatpak",
            Id::AllApps => "Wszystkie",
            Id::Games => "Gry",
            Id::Clip => "Schowek",
            Id::Calc => "Kalkulator",
            Id::Settings => "Ustawienia",
            Id::Prefer => "Preferowane aplikacje",
            Id::Themes => "Motyw",
            Id::Modes => "Tryb",
            Id::Layouts => "Uklad",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Act {
    Open(Id),
    Run { cmd: String, terminal: bool },
    Theme(String),
    Mode(String),
    Layout(String),
    Clip(String),
    Copy(String),
    /// pick an application for this role
    PickPrefer(String),
    /// make this command the preferred one for the role
    SetPrefer { role: String, cmd: String },
    Reload,
    ReloadEngine,
    EditConfig,
    Nop,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub label: String,
    pub act: Act,
}

#[derive(Debug, Clone)]
pub struct Level {
    pub id: Id,
    pub items: Vec<Entry>,
}

#[derive(Debug, Default)]
pub struct Menu {
    pub stack: Vec<Level>,
    pub query: String,
    pub sel: usize,
}

fn e(label: &str, act: Act) -> Entry {
    Entry { label: label.into(), act }
}

fn app_entries(f: impl Fn(&apps::App) -> bool) -> Vec<Entry> {
    apps::scan()
        .into_iter()
        .filter(|a| f(a))
        .map(|a| Entry { label: a.name, act: Act::Run { cmd: a.exec, terminal: a.terminal } })
        .collect()
}

fn clip_entries() -> Vec<Entry> {
    match std::process::Command::new("cliphist").arg("list").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .take(300)
            .filter_map(|l| l.split_once('\t'))
            .map(|(id, text)| Entry { label: text.trim().chars().take(200).collect(), act: Act::Clip(id.into()) })
            .collect(),
        _ => vec![e("cliphist is not installed or empty", Act::Nop)],
    }
}

impl Desktop {
    pub fn menu_level(&self, id: Id) -> Level {
        let items = match id {
            Id::Root => vec![
                e("Aplikacje", Act::Open(Id::Apps)),
                e("Gry", Act::Open(Id::Games)),
                e("Schowek", Act::Open(Id::Clip)),
                e("Kalkulator", Act::Open(Id::Calc)),
                e("Ustawienia", Act::Open(Id::Settings)),
            ],
            Id::Apps => vec![e("Natywne", Act::Open(Id::Native)), e("Flatpak", Act::Open(Id::Flatpak)), e("Wszystkie", Act::Open(Id::AllApps))],
            Id::Native => app_entries(|a| !a.game && !a.flatpak),
            Id::Flatpak => app_entries(|a| !a.game && a.flatpak),
            Id::AllApps => app_entries(|a| !a.game),
            Id::Games => app_entries(|a| a.game),
            Id::Clip => clip_entries(),
            Id::Calc => vec![e("wpisz wyrazenie, Enter kopiuje wynik", Act::Nop)],
            Id::Settings => vec![
                e("Motyw", Act::Open(Id::Themes)),
                e("Tryb", Act::Open(Id::Modes)),
                e("Uklad okien", Act::Open(Id::Layouts)),
                e("Preferowane aplikacje", Act::Open(Id::Prefer)),
                e("Przeladuj config", Act::Reload),
                e("Przeladuj silniki", Act::ReloadEngine),
                e("Edytuj config", Act::EditConfig),
            ],
            Id::Prefer => crate::components::settings::settings_preferaps::ROLES
                .iter()
                .map(|r| Entry { label: format!("{r}: {}", self.config.prefer_get(r).unwrap_or_default()), act: Act::PickPrefer(r.to_string()) })
                .collect(),
            Id::Themes => crate::components::theme::Theme::list().into_iter().map(|n| Entry { label: n.clone(), act: Act::Theme(n) }).collect(),
            Id::Modes => std::iter::once("off".to_string())
                .chain(Desktop::mode_names())
                .map(|n| Entry { label: n.clone(), act: Act::Mode(n) })
                .collect(),
            Id::Layouts => crate::components::engine::layout_engine::Layout::ALL
                .iter()
                .map(|l| Entry { label: l.name().into(), act: Act::Layout(l.name().into()) })
                .collect(),
        };
        Level { id, items }
    }

    pub fn menu_open(&mut self, id: Id) {
        // opening a sub-menu directly still has the main menu under it, so backspace goes up
        let mut stack = vec![self.menu_level(Id::Root)];
        if id != Id::Root {
            stack.push(self.menu_level(id));
        }
        self.menu = Some(Menu { stack, ..Menu::default() });
        self.bar_dirty = true;
    }
}

impl Menu {
    pub fn level(&self) -> &Level {
        self.stack.last().expect("a menu always has a level")
    }

    /// Indices of the entries matching every word of the query (case-insensitive); prefix matches first.
    pub fn visible(&self) -> Vec<usize> {
        if self.level().id == Id::Calc {
            return (0..self.level().items.len()).collect(); // the query is an expression here, not a filter
        }
        let q = self.query.to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let items = &self.level().items;
        let mut v: Vec<usize> = (0..items.len()).filter(|&i| words.iter().all(|w| items[i].label.to_lowercase().contains(w))).collect();
        if let Some(first) = words.first() {
            v.sort_by_key(|&i| !items[i].label.to_lowercase().starts_with(first));
        }
        v
    }

    /// "Menu > Aplikacje > Flatpak"
    pub fn crumbs(&self) -> String {
        self.stack.iter().map(|l| l.id.title()).collect::<Vec<_>>().join(" > ")
    }
}

impl Desktop {
    /// `ctl menu ...` and the keys while the menu is open. Returns text for `ctl`.
    pub fn menu_cmd(&mut self, cmd: &str) -> String {
        let (verb, rest) = cmd.split_once(' ').map_or((cmd, ""), |(a, b)| (a, b));
        self.bar_dirty = true;
        match verb {
            "" | "noop" => return "ok".into(),
            "toggle" => {
                if self.menu.is_some() {
                    self.menu = None;
                } else {
                    self.menu_open(Id::Root);
                }
                return "ok".into();
            }
            "open" => {
                return match Id::parse(rest.trim()) {
                    Some(id) => {
                        self.menu_open(id);
                        "ok".into()
                    }
                    None => "error: menu open root|apps|native|flatpak|all|games|clip|calc|settings|themes|modes|layouts".into(),
                };
            }
            "close" => {
                self.menu = None;
                return "ok".into();
            }
            _ => {}
        }
        let Some(menu) = self.menu.as_mut() else { return "error: menu is not open".into() };
        let vis = menu.visible();
        match verb {
            "list" => {
                let items = &menu.level().items;
                return std::iter::once(menu.crumbs()).chain(vis.iter().enumerate().map(|(n, &i)| format!("{}{}", if n == menu.sel { "> " } else { "  " }, items[i].label))).collect::<Vec<_>>().join("\n");
            }
            "type" => {
                menu.query.push_str(rest);
                menu.sel = 0;
                self.menu_recalc();
            }
            "key" => match rest {
                "space" => {
                    menu.query.push(' ');
                    menu.sel = 0;
                    self.menu_recalc();
                }
                "up" => menu.sel = menu.sel.saturating_sub(1),
                "down" | "tab" => menu.sel = (menu.sel + 1).min(vis.len().saturating_sub(1)),
                "pageup" => menu.sel = menu.sel.saturating_sub(10),
                "pagedown" => menu.sel = (menu.sel + 10).min(vis.len().saturating_sub(1)),
                "esc" => self.menu = None,
                "backspace" => {
                    if menu.query.pop().is_some() {
                        menu.sel = 0;
                        self.menu_recalc();
                    } else if menu.stack.len() > 1 {
                        menu.stack.pop();
                    } else {
                        self.menu = None;
                    }
                }
                "enter" => return self.menu_enter(),
                other => return format!("error: unknown menu key {other:?}"),
            },
            "up" | "down" | "enter" | "back" | "esc" | "backspace" => return self.menu_cmd(&format!("key {verb}")),
            _ => return format!("error: unknown menu command {verb:?} (open, close, toggle, list, type TEXT, key up|down|enter|back|esc|space)"),
        }
        "ok".into()
    }

    /// In the calculator the single entry is the value of what has been typed.
    fn menu_recalc(&mut self) {
        let Some(menu) = self.menu.as_mut() else { return };
        let level = menu.stack.last_mut().expect("a menu always has a level");
        if level.id != Id::Calc {
            return;
        }
        level.items = vec![match crate::components::calc::eval(&menu.query) {
            Some(v) => {
                let r = crate::components::calc::show(v);
                Entry { label: format!("= {r}"), act: Act::Copy(r) }
            }
            None if menu.query.trim().is_empty() => e("wpisz wyrazenie, Enter kopiuje wynik", Act::Nop),
            None => e("blad skladni", Act::Nop),
        }];
        menu.sel = 0;
    }

    fn menu_enter(&mut self) -> String {
        let Some(menu) = self.menu.as_ref() else { return "error: menu is not open".into() };
        let vis = menu.visible();
        let Some(&i) = vis.get(menu.sel.min(vis.len().saturating_sub(1))) else { return "error: nothing to select".into() };
        let entry = menu.level().items[i].clone();
        if let Act::Open(id) = entry.act {
            let level = self.menu_level(id);
            let menu = self.menu.as_mut().unwrap();
            menu.stack.push(level);
            menu.query.clear();
            menu.sel = 0;
            return "ok".into();
        }
        if let Act::PickPrefer(role) = entry.act {
            let term = self.config.terminal.clone();
            let items = apps::scan()
                .into_iter()
                .map(|a| Entry { label: a.name.clone(), act: Act::SetPrefer { role: role.clone(), cmd: crate::components::settings::settings_preferaps::command_for(&a, &term) } })
                .collect();
            let menu = self.menu.as_mut().unwrap();
            menu.stack.push(Level { id: Id::AllApps, items });
            menu.query.clear();
            menu.sel = 0;
            return "ok".into();
        }
        if matches!(entry.act, Act::Nop) {
            return "ok".into();
        }
        self.menu = None;
        match entry.act {
            Act::Run { cmd, terminal } => {
                if terminal {
                    let term = self.config.terminal.clone();
                    let mut parts = term.split_whitespace();
                    if let Some(bin) = parts.next() {
                        let r = std::process::Command::new(bin).args(parts).args(["-e", "sh", "-c", &cmd]).spawn();
                        if let Err(e) = r {
                            tracing::warn!("menu: {e}");
                        }
                    }
                } else {
                    crate::keys::spawn(&cmd);
                }
                "ok".into()
            }
            Act::Theme(n) => self.set_theme(&n),
            Act::Mode(n) => self.set_mode(&n),
            Act::Layout(n) => match crate::components::engine::layout_engine::Layout::from_name(&n) {
                Some(l) => self.run_action(Action::Layout(LayoutSel::Set(l))),
                None => format!("error: unknown layout {n:?}"),
            },
            Act::Clip(id) => {
                crate::keys::spawn(&format!("printf '%s\\t' '{id}' | cliphist decode | wl-copy"));
                "ok".into()
            }
            Act::SetPrefer { role, cmd } => self.prefer(&format!("{role} {cmd}")),
            Act::Copy(text) => {
                crate::keys::spawn(&format!("printf '%s' '{text}' | wl-copy"));
                "ok".into()
            }
            Act::Reload => self.run_action(Action::Reload),
            Act::ReloadEngine => self.run_action(Action::ReloadEngine),
            Act::EditConfig => {
                let path = crate::config::user_path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                let term = self.config.terminal.clone();
                crate::keys::spawn(&format!("{term} -e sh -c '${{EDITOR:-frost}} \"{path}\"'"));
                "ok".into()
            }
            Act::Open(_) | Act::PickPrefer(_) | Act::Nop => "ok".into(),
        }
    }
}
