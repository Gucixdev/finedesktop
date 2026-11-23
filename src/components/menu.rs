//! The menu panel (Super+Shift+W; Super+W is the launcher, same root): Aplikacje (ONE list, sections Flatpak / Natywne / Gry), Schowek, Kalkulator, Zasilanie, Ustawienia (a panel with checkboxes and sliders). Drawn by the compositor like
//! the bar, driven by keys or by `ctl menu ...`, so a script or an agent uses exactly what the user uses.
//! A level is a title plus entries; an entry opens another level or does one thing.

use super::apps;
use crate::config::{Action, LayoutSel};
use crate::Desktop;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Id {
    Root,
    Apps,
    AllApps,
    Run,
    Games,
    Scripts,
    Power,
    General,
    Clip,
    Calc,
    Settings,
    Prefer,
    Notes,
    Wallpaper,
    Devices,
    Drivers,
    Update,
    Cleaner,
    Trash,
    System,
    Themes,
    Modes,
    Layouts,
    Fonts,
    Bar,
    Input,
    Screens,
    Audio,
    Keys,
    Autostart,
    Bluetooth,
    ThemeNew,
    ColorPick,
    ThemeImport,
    /// the settings groups (categories with subcategories)
    SetLook,
    SetInput,
    SetHardware,
    SetSystem,
    /// search across every settings panel
    Find,
    /// the options of a dropdown
    Pick,
}

impl Id {
    pub fn parse(s: &str) -> Option<Id> {
        Some(match s {
            "" | "root" | "main" => Id::Root,
            "apps" => Id::Apps,
            "native" | "flatpak" => Id::Apps,
            "all" => Id::AllApps,
            "run" | "launcher" => Id::Run,
            "games" => Id::Games,
            "scripts" | "script" => Id::Scripts,
            "clip" | "clipboard" => Id::Clip,
            "calc" => Id::Calc,
            "settings" => Id::Settings,
            "session" | "power" => Id::Power,
            "general" | "ogolne" => Id::General,
            "notes" | "notifications" => Id::Notes,
            "wallpaper" | "tapeta" => Id::Wallpaper,
            "devices" | "urzadzenia" => Id::Devices,
            "drivers" | "sterowniki" => Id::Drivers,
            "update" | "updates" | "aktualizacje" => Id::Update,
            "cleaner" | "clean" | "czyszczenie" => Id::Cleaner,
            "trash" => Id::Trash,
            "system" => Id::System,
            "prefer" => Id::Prefer,
            "themes" | "theme" => Id::Themes,
            "modes" | "mode" => Id::Modes,
            "layouts" | "layout" => Id::Layouts,
            "fonts" | "font" | "czcionki" => Id::Fonts,
            "bar" | "pasek" => Id::Bar,
            "audio" | "sound" | "dzwiek" => Id::Audio,
            "keys" | "shortcuts" | "skroty" => Id::Keys,
            "autostart" => Id::Autostart,
            "bluetooth" => Id::Bluetooth,
            "input" | "keyboard" | "mouse" | "klawiatura" => Id::Input,
            "screens" | "screen" | "display" | "ekrany" => Id::Screens,
            "create_theme" | "theme-new" | "newtheme" => Id::ThemeNew,
            "color" | "colour" => Id::ColorPick,
            "theme-import" | "import_theme" => Id::ThemeImport,
            "look" | "wyglad" => Id::SetLook,
            "inputs" | "wejscie" => Id::SetInput,
            "hardware" | "sprzet" => Id::SetHardware,
            "sys" | "komputer" => Id::SetSystem,
            "find" | "search" | "szukaj" => Id::Find,
            _ => return None,
        })
    }

    /// Where "back" leads: the menu is a hierarchy, and opening any level opens the path to it.
    pub fn parent(self) -> Option<Id> {
        match self {
            Id::Root | Id::Run => None,
            Id::Apps | Id::Games | Id::Scripts | Id::Power | Id::Clip | Id::Calc | Id::Settings => Some(Id::Root),
            Id::SetLook | Id::SetInput | Id::SetHardware | Id::SetSystem | Id::Find => Some(Id::Settings),
            Id::Themes | Id::Modes | Id::Layouts | Id::Fonts | Id::Bar | Id::Wallpaper => Some(Id::SetLook),
            Id::Input | Id::Keys => Some(Id::SetInput),
            Id::Screens | Id::Audio | Id::Bluetooth | Id::Devices | Id::Drivers => Some(Id::SetHardware),
            Id::General | Id::Prefer | Id::Notes | Id::Trash | Id::System | Id::Autostart | Id::Update | Id::Cleaner => Some(Id::SetSystem),
            Id::Pick => None,
            Id::ThemeNew | Id::ThemeImport => Some(Id::Themes),
            Id::ColorPick => Some(Id::ThemeNew),
            Id::AllApps => Some(Id::Prefer),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Id::Root => "Menu",
            Id::Apps => "Aplikacje",
            Id::AllApps => "Wszystkie",
            Id::Run => "Uruchom",
            Id::Games => "Gry",
            Id::Scripts => "Skrypty",
            Id::Clip => "Schowek",
            Id::Calc => "Kalkulator",
            Id::Settings => "Ustawienia",
            Id::Power => "Zasilanie",
            Id::General => "Ogolne",
            Id::Notes => "Powiadomienia",
            Id::Trash => "Kosz",
            Id::System => "Config i silniki",
            Id::Prefer => "Preferowane aplikacje",
            Id::Themes => "Motyw",
            Id::Modes => "Tryb",
            Id::Layouts => "Uklad",
            Id::Fonts => "Czcionki",
            Id::Bar => "Pasek",
            Id::Audio => "Dzwiek",
            Id::Keys => "Skroty",
            Id::Autostart => "Autostart",
            Id::Bluetooth => "Bluetooth",
            Id::Input => "Klawiatura i mysz",
            Id::Screens => "Ekrany",
            Id::Wallpaper => "Tapeta",
            Id::Devices => "Urzadzenia",
            Id::Drivers => "Sterowniki",
            Id::Update => "Aktualizacje",
            Id::Cleaner => "Czyszczenie",
            Id::ThemeNew => "Utworz motyw",
            Id::ColorPick => "Kolor",
            Id::ThemeImport => "Importuj motyw",
            Id::SetLook => "Wyglad",
            Id::SetInput => "Wejscie",
            Id::SetHardware => "Sprzet",
            Id::SetSystem => "System",
            Id::Find => "Szukaj",
            Id::Pick => "Wybierz",
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
    /// the global font: a `.bdf`/`.psf` path, empty for the built-in one
    Font(String),
    /// a choice: this config key gets this value
    Set { key: &'static str, val: String },
    /// a colour of the theme being made: Enter opens the picker
    Color { key: &'static str, rgb: [u8; 3] },
    /// the colour in the picker, shown as a swatch
    Swatch([u8; 3]),
    /// a typed #rrggbb to take
    SetColor([u8; 3]),
    /// hue (ch 0) / saturation (1) / value (2) slider of the picker
    Hsv { ch: u8, val: f32, max: f32, step: f32 },
    /// an autostart file: on while executable
    AutoToggle { name: String, on: bool },
    /// add this `MODS, key, action` shortcut
    AddBind(String),
    /// remove the shortcut shown with this label
    DelBind(String),
    /// write the theme being made under this name
    SaveTheme(String),
    /// copy this theme file into the config and use it
    ImportTheme(String),
    /// a screen setting line: `NAME mode=WxH@HZ`, `NAME pos=X,Y`, `NAME off`, `NAME on`
    Out(String),
    Clip(String),
    Copy(String),
    /// pick an application for this role
    PickPrefer(String),
    /// make this command the preferred one for the role
    SetPrefer { role: String, cmd: String },
    /// a checkbox: flips the config key `key` (written to the user config)
    Toggle { key: &'static str, on: bool },
    /// a dropdown: the current value is shown on the row, Enter opens the options (`sel` is the current one)
    Choice { cur: String, sel: usize, options: Vec<Entry> },
    /// a slider: moves the config key `key` by `step` inside min..=max
    Slide { key: &'static str, min: f32, max: f32, step: f32, val: f32 },
    Reload,
    ReloadEngine,
    EditConfig,
    /// any ctl line, so a tab entry is exactly what a script would type
    Ctl(String),
    /// a ctl line that leaves the panel open (it is drawn again): the buttons of a settings panel
    CtlStay(String),
    Nop,
    /// a section title in a list: drawn, never selected
    Head,
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// icon name (an application's `Icon=`), drawn before the label when a PNG is found
    pub icon: Option<String>,
    pub label: String,
    pub act: Act,
}

#[derive(Debug, Clone)]
pub struct Level {
    pub id: Id,
    pub items: Vec<Entry>,
}

/// The left column of the settings panel.
#[derive(Debug, Clone)]
pub struct Tabs {
    pub labels: Vec<String>,
    pub active: usize,
    pub focused: bool,
    /// the level the active tab opens
    pub id: Option<Id>,
}

/// One drawn line of the list.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Head(String),
    Item(String),
    /// label and the icon name drawn before it
    Icon(String, String),
    Check(String, bool),
    /// label, 0..=1 position, the value as text
    Slider(String, f32, String),
    /// label and a colour swatch
    Color(String, [u8; 3]),
    /// label and the current value of a dropdown
    Choice(String, String),
}

#[derive(Debug, Default)]
pub struct Menu {
    pub stack: Vec<Level>,
    pub query: String,
    pub sel: usize,
}

impl Entry {
    /// How the entry is drawn: plain text, a checkbox or a slider.
    pub fn row(&self) -> Row {
        match &self.act {
            Act::Toggle { on, .. } | Act::AutoToggle { on, .. } => Row::Check(self.label.clone(), *on),
            Act::Slide { min, max, step, val, .. } => {
                let text = if *step < 1.0 { format!("{val:.2}") } else { format!("{val:.0}") };
                Row::Slider(self.label.clone(), ((val - min) / (max - min).max(f32::EPSILON)).clamp(0.0, 1.0), text)
            }
            Act::Hsv { val, max, step, .. } => {
                let text = if *step < 1.0 { format!("{val:.2}") } else { format!("{val:.0}") };
                Row::Slider(self.label.clone(), (val / max.max(f32::EPSILON)).clamp(0.0, 1.0), text)
            }
            Act::Color { rgb, .. } | Act::Swatch(rgb) | Act::SetColor(rgb) => Row::Color(self.label.clone(), *rgb),
            Act::Choice { cur, .. } => Row::Choice(self.label.clone(), cur.clone()),
            _ => match &self.icon {
                Some(i) => Row::Icon(self.label.clone(), i.clone()),
                None => Row::Item(self.label.clone()),
            },
        }
    }

    /// Left / right change the value of this entry.
    pub fn is_slider(&self) -> bool {
        matches!(self.act, Act::Slide { .. } | Act::Hsv { .. })
    }
}

pub(super) fn e(label: &str, act: Act) -> Entry {
    Entry { icon: None, label: label.into(), act }
}

pub(super) fn head(label: &str) -> Entry {
    Entry { icon: None, label: label.into(), act: Act::Head }
}

/// The entries of a settings group: one Open per subcategory.
fn group(items: &[(&str, Id)]) -> Vec<Entry> {
    items.iter().map(|(l, id)| e(l, Act::Open(*id))).collect()
}

/// The leaf panels that the settings search reads, in the order of the groups. Not in it: the shortcut editor (it is
/// its own search), the trash and Bluetooth (they list things, not settings).
const FIND_PANELS: [Id; 17] = [
    Id::Themes, Id::Modes, Id::Layouts, Id::Fonts, Id::Bar, Id::Wallpaper, Id::Input, Id::Screens, Id::Audio, Id::Devices, Id::Drivers, Id::General, Id::Autostart, Id::Prefer, Id::Notes, Id::Update, Id::Cleaner,
];

/// Every wanted app in ONE list, under a title per kind that has any entries.
fn grouped(kinds: &[(&str, fn(&apps::App) -> bool)]) -> Vec<Entry> {
    let all = apps::scan();
    let mut out = Vec::new();
    for (title, f) in kinds {
        let mut part: Vec<Entry> = all.iter().filter(|a| f(a)).map(|a| Entry { icon: Some(a.icon.clone()).filter(|i| !i.is_empty()), label: a.name.clone(), act: Act::Run { cmd: a.exec.clone(), terminal: a.terminal } }).collect();
        if !part.is_empty() {
            out.push(head(title));
            out.append(&mut part);
        }
    }
    out
}

fn is_native(a: &apps::App) -> bool {
    !a.game && !a.flatpak && !a.terminal
}
fn is_terminal(a: &apps::App) -> bool {
    !a.game && !a.flatpak && a.terminal
}
fn is_flatpak(a: &apps::App) -> bool {
    !a.game && a.flatpak
}
fn is_game(a: &apps::App) -> bool {
    a.game
}
fn is_game_native(a: &apps::App) -> bool {
    a.game && !a.flatpak
}
fn is_game_flatpak(a: &apps::App) -> bool {
    a.game && a.flatpak
}
fn any_app(_: &apps::App) -> bool {
    true
}

/// Executables in `<config dir>/NAME/`, one entry each, labelled `PREFIX + file name`; empty when there are none.
fn dir_entries(name: &str, prefix: &str) -> Vec<Entry> {
    exec_entries(crate::config::config_dir().map(|d| d.join(name)).as_deref(), prefix)
}

fn exec_entries(dir: Option<&std::path::Path>, prefix: &str) -> Vec<Entry> {
    use std::os::unix::fs::PermissionsExt;
    let mut v: Vec<Entry> = dir
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|f| std::fs::metadata(f.path()).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
        .map(|f| Entry { icon: None, label: format!("{prefix}{}", f.file_name().to_string_lossy()), act: Act::Run { cmd: f.path().to_string_lossy().into_owned(), terminal: false } })
        .collect();
    v.sort_by(|a, b| a.label.cmp(&b.label));
    v
}

/// `*.AppImage` files (executable) in the usual places, two folders deep: one entry each, by file name.
fn appimage_entries() -> Vec<Entry> {
    let Ok(home) = std::env::var("HOME") else { return Vec::new() };
    let dirs: Vec<std::path::PathBuf> =
        ["Applications", "AppImages", ".local/share/AppImage", ".local/share/AppImages", ".local/bin", "Downloads"].iter().map(|d| std::path::Path::new(&home).join(d)).collect();
    appimages_in(&dirs)
}

fn appimages_in(dirs: &[std::path::PathBuf]) -> Vec<Entry> {
    use std::os::unix::fs::PermissionsExt;
    let mut v: Vec<(String, String)> = Vec::new();
    for d in dirs {
        let mut todo = vec![(d.clone(), 0)];
        while let Some((dir, depth)) = todo.pop() {
            for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = f.path();
                let Ok(md) = std::fs::metadata(&p) else { continue };
                if md.is_dir() && depth < 2 {
                    todo.push((p, depth + 1));
                } else if md.is_file() && md.permissions().mode() & 0o111 != 0 && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("appimage")) {
                    let name = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let path = p.to_string_lossy().into_owned();
                    if !v.iter().any(|(_, q)| *q == path) {
                        v.push((name, path));
                    }
                }
            }
        }
    }
    v.sort_by_key(|(n, _)| n.to_lowercase());
    v.into_iter().map(|(n, p)| Entry { icon: None, label: n, act: Act::Run { cmd: format!("'{}'", p.replace('\'', "'\\''")), terminal: false } }).collect()
}

/// Executables in `<config dir>/scripts/`: drop a file there and it is in the menu.
fn script_entries() -> Vec<Entry> {
    let mut v = dir_entries("scripts", "");
    if v.is_empty() {
        v.push(e("no scripts: put executables in <config dir>/scripts/", Act::Nop));
    }
    v
}

/// The launcher's power section: the `ctl session` verbs.
const POWER: [&str; 6] = ["lock", "logout", "suspend", "hibernate", "reboot", "poweroff"];

/// The launcher's backup section: executables in `<config dir>/backup/` (a script per backup job).
fn backup_entries() -> Vec<Entry> {
    let mut v = dir_entries("backup", "backup: ");
    if v.is_empty() {
        v.push(e("backup: put executables in <config dir>/backup/", Act::Nop));
    }
    v
}

fn clip_entries() -> Vec<Entry> {
    match std::process::Command::new("cliphist").arg("list").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .take(300)
            .filter_map(|l| l.split_once('\t'))
            .map(|(id, text)| Entry { icon: None, label: text.trim().chars().take(200).collect(), act: Act::Clip(id.into()) })
            .collect(),
        _ => vec![e("cliphist is not installed or empty", Act::Nop)],
    }
}

impl Desktop {
    pub fn menu_level(&self, id: Id) -> Level {
        let items = match id {
            Id::Root => {
                // the two sound sliders sit on top of the launcher
                let mut v = self.audio_entries(false);
                v.extend([
                    e("Aplikacje", Act::Open(Id::Apps)),
                    e("Schowek", Act::Open(Id::Clip)),
                    e("Kalkulator", Act::Open(Id::Calc)),
                    e("Zasilanie", Act::Open(Id::Power)),
                    e("Ustawienia", Act::Open(Id::Settings)),
                ]);
                v
            }
            Id::Apps | Id::AllApps => {
                let mut v = grouped(&[("Flatpak", is_flatpak), ("Natywne", is_native), ("Terminal", is_terminal), ("Gry", is_game)]);
                for (title, mut part) in [("Skrypty", dir_entries("scripts", "")), ("AppImage", appimage_entries())] {
                    if !part.is_empty() {
                        v.push(head(title));
                        v.append(&mut part);
                    }
                }
                v
            }
            Id::Games => grouped(&[("Natywne", is_game_native), ("Flatpak", is_game_flatpak)]),
            Id::Run => {
                let mut v = grouped(&[("Aplikacje", any_app)]);
                v.push(head("Zasilanie"));
                v.extend(POWER.iter().map(|s| e(&format!("power: {s}"), Act::Ctl(format!("session {s}")))));
                v.push(head("Backup"));
                v.extend(backup_entries());
                // the launcher opens at once: panels that run lspci / wpctl to be built stay out, their `otworz:` row is enough
                v.push(head("Ustawienia"));
                let quick: Vec<Id> = FIND_PANELS.iter().copied().filter(|p| !matches!(p, Id::Devices | Id::Drivers | Id::Audio)).collect();
                v.extend(self.find_in(&quick));
                for id in [Id::Audio, Id::Devices, Id::Drivers] {
                    v.push(e(&format!("otworz: {}", id.title()), Act::Open(id)));
                }
                v.push(head("Polecenia"));
                v.extend(super::launcher::commands().into_iter().map(|c| Entry { icon: None, label: c.clone(), act: Act::Run { cmd: c, terminal: false } }));
                v
            }
            Id::Scripts => script_entries(),
            Id::Clip => clip_entries(),
            Id::Calc => vec![e("wpisz wyrazenie, Enter kopiuje wynik", Act::Nop)],
            // the tabs of the settings panel, drawn down the left side
            Id::Settings => vec![
                e("Wyglad", Act::Open(Id::SetLook)),
                e("Wejscie", Act::Open(Id::SetInput)),
                e("Sprzet", Act::Open(Id::SetHardware)),
                e("System", Act::Open(Id::SetSystem)),
            ],
            Id::SetLook => group(&[("Motyw", Id::Themes), ("Tryb", Id::Modes), ("Uklad okien", Id::Layouts), ("Czcionki", Id::Fonts), ("Pasek", Id::Bar), ("Tapeta", Id::Wallpaper)]),
            Id::SetInput => group(&[("Klawiatura i mysz", Id::Input), ("Skroty", Id::Keys)]),
            Id::SetHardware => group(&[("Ekrany", Id::Screens), ("Dzwiek", Id::Audio), ("Bluetooth", Id::Bluetooth), ("Urzadzenia: glosniki, mikrofony, kamery, jasnosc", Id::Devices), ("Sterowniki", Id::Drivers)]),
            Id::SetSystem => group(&[("Ogolne", Id::General), ("Autostart", Id::Autostart), ("Aplikacje", Id::Prefer), ("Powiadomienia", Id::Notes), ("Kosz", Id::Trash), ("Aktualizacje", Id::Update), ("Czyszczenie dysku", Id::Cleaner), ("Config i silniki", Id::System)]),
            Id::Find => self.find_entries(),
            Id::Pick => self.menu.as_ref().and_then(|m| m.stack.iter().rev().find(|l| l.id == Id::Pick).map(|l| l.items.clone())).unwrap_or_default(),
            Id::General => self.general_entries(),
            Id::Fonts => self.font_entries(),
            Id::Bar => self.bar_entries(),
            Id::Audio => self.audio_entries(true),
            Id::Keys => self.shortcuts_entries(&self.menu.as_ref().map(|m| m.query.clone()).unwrap_or_default()),
            Id::Autostart => self.autostart_entries(),
            Id::Bluetooth => bluetooth_entries(),
            Id::Input => self.input_entries(),
            Id::Screens => self.screens_entries(),
            Id::Wallpaper => self.wallpaper_entries(),
            Id::Devices => self.devices_entries(),
            Id::Drivers => self.drivers_entries(),
            Id::Update => update_entries(),
            Id::Cleaner => cleaner_entries(),
            Id::ThemeNew => self.theme_new_entries(),
            Id::ColorPick => self.color_pick_entries(),
            Id::ThemeImport => self.theme_import_entries(),
            Id::Power => POWER.iter().map(|s| e(s, Act::Ctl(format!("session {s}")))).collect(),
            Id::Notes => self.notts_entries(),
            Id::Trash => {
                let v: Vec<Entry> = super::trash::list().into_iter().map(|(n, p)| e(&format!("{n}  <-  {p}"), Act::Ctl(format!("trash restore {n}")))).collect();
                if v.is_empty() { vec![e("trash is empty (ctl trash PATH)", Act::Nop)] } else { v }
            }
            Id::System => vec![
                e("Przeladuj config", Act::Reload),
                e("Przeladuj silniki", Act::ReloadEngine),
                e("Edytuj config", Act::EditConfig),
            ],
            Id::Prefer => {
                use crate::components::settings::settings_preferaps::{formats, PROGRAMS, ROLES};
                let cur = |r: &str| self.config.prefer_get(r).filter(|c| !c.is_empty()).unwrap_or_else(|| "(domyslna systemu)".into());
                let mut v: Vec<Entry> = ROLES[..PROGRAMS].iter().map(|r| Entry { icon: None, label: format!("{r}: {}", cur(r)), act: Act::PickPrefer(r.to_string()) }).collect();
                v.push(head("co otwiera dany format plikow"));
                v.extend(ROLES[PROGRAMS..].iter().map(|r| Entry { icon: None, label: format!("{r} ({}): {}", formats(r), cur(r)), act: Act::PickPrefer(r.to_string()) }));
                v
            }
            Id::Themes => {
                let mut v: Vec<Entry> = crate::components::theme::Theme::list().into_iter().map(|n| Entry { icon: None, label: n.clone(), act: Act::Theme(n) }).collect();
                v.push(head("jasny / ciemny"));
                v.push(e("Przelacz jasny / ciemny", Act::Ctl("theme toggle".into())));
                v.push(head("wlasne"));
                v.push(e("Utworz motyw (kolory z paletą)", Act::Open(Id::ThemeNew)));
                v.push(e("Importuj motyw z pliku", Act::Open(Id::ThemeImport)));
                v
            }
            Id::Modes => std::iter::once("off".to_string())
                .chain(Desktop::mode_names())
                .map(|n| Entry { icon: None, label: n.clone(), act: Act::Mode(n) })
                .collect(),
            Id::Layouts => crate::components::engine::layout_engine::Layout::ALL
                .iter()
                .map(|l| Entry { icon: None, label: l.name().into(), act: Act::Layout(l.name().into()) })
                .collect(),
        };
        Level { id, items }
    }

    /// The checkboxes and sliders of the settings panel, with the values the config has now.
    fn general_entries(&self) -> Vec<Entry> {
        let c = &self.config;
        let t = |label: &str, key: &'static str, on: bool| e(label, Act::Toggle { key, on });
        let s = |label: &str, key: &'static str, min: f32, max: f32, step: f32, val: f32| e(label, Act::Slide { key, min, max, step, val });
        vec![
            head("spacja przelacza, strzalki lewo/prawo ruszaja suwak"),
            t("Fokus za myszka", "focus_follows_mouse", c.focus_follows_mouse),
            t("Mysz za fokusem", "mouse_follows_focus", c.mouse_follows_focus),
            t("Przywracaj aplikacje po restarcie", "restore", c.restore),
            t("Widgety paska", "widgets", c.widgets),
            t("Panel powitalny", "welcome_auto", c.panels),
            head("animacje"),
            t("Animacje", "animations", c.theme.animations),
            s("Szybkosc animacji (1 = domyslna)", "anim_speed", 0.25, 3.0, 0.25, c.anim_speed),
            s("Sprezystosc (0 = bez przeskoku)", "anim_bounce", 0.0, 1.0, 0.1, c.anim_bounce),
            head("okna"),
            s("Odstep miedzy oknami", "gap", 0.0, 40.0, 1.0, c.gap as f32),
            s("Ramka", "border", 0.0, 8.0, 1.0, c.theme.border as f32),
            s("Szerokosc okna master", "mfact", 0.1, 0.9, 0.05, c.mfact),
            s("Okna master", "nmaster", 0.0, 5.0, 1.0, c.nmaster as f32),
            head("system"),
            t("Bez czekania na vblank (tearing, tylko DRM)", "tearing", c.tearing),
            s("Pulpity (wiecej od razu, mniej po restarcie)", "workspaces", 1.0, 16.0, 1.0, c.workspaces as f32),
            s("Ostrzezenie o baterii (%)", "low_battery", 0.0, 50.0, 5.0, c.low_battery as f32),
            head("wyglad"),
            s("Skala czcionki", "font_scale", 1.0, 4.0, 1.0, c.font_scale as f32),
            s("Odswiezanie paska (s)", "bar_interval", 1.0, 10.0, 1.0, c.bar_interval as f32),
        ]
    }

    /// The global font: the built-in one, every font file found, and the scale.
    fn font_entries(&self) -> Vec<Entry> {
        let cur = self.config.font.trim().to_string();
        let mark = |on: bool, name: &str| if on { format!("* {name}") } else { format!("  {name}") };
        let mut v = vec![head("czcionka globalna: pasek, menu, tytuly i terminal (gdy fontconfig ja zna)")];
        v.push(e(&mark(cur.is_empty(), "wbudowana (Terminus 8x16)"), Act::Font(String::new())));
        for (label, path) in super::fonts::available() {
            let p = path.to_string_lossy().into_owned();
            v.push(e(&mark(cur == p, &label), Act::Font(p)));
        }
        v.push(head("rozmiar"));
        v.push(e("Skala czcionki", Act::Slide { key: "font_scale", min: 1.0, max: 4.0, step: 1.0, val: self.config.font_scale as f32 }));
        v
    }

    /// Choose the global font: saved in the config, loaded, and the terminal follows when fontconfig knows the file.
    fn set_font(&mut self, path: &str) -> String {
        let r = self.set_setting("font", path);
        if r != "ok" || path.is_empty() {
            return r;
        }
        match super::fonts::sync_foot(std::path::Path::new(path)) {
            Ok(true) => "ok (terminal too: new terminals use it)".into(),
            Ok(false) => "ok (terminal unchanged: fontconfig does not know this file)".into(),
            Err(e) => format!("ok (terminal not written: {e})"),
        }
    }

    /// Change one setting of the settings panel: written to the user config, then the config is loaded again.
    fn set_setting(&mut self, key: &str, val: &str) -> String {
        if let Some(r) = self.audio_setting(key, val) {
            return r;
        }
        if key == "notts_dnd" {
            self.notify_dnd(Some(val == "on"));
            return "ok".into();
        }
        if key == "brightness" {
            return self.brightness_set(val);
        }
        if key == "bar_stats" {
            return self.bar_stats_set(val == "on");
        }
        if let Err(e) = crate::components::settings::settings_preferaps::persist(key, val) {
            return format!("error: the config was not written: {e}");
        }
        self.load_config();
        "ok".into()
    }

    /// The entry under the cursor, if there is one.
    fn menu_selected(&self) -> Option<Entry> {
        let menu = self.menu.as_ref()?;
        let vis = menu.visible();
        let i = *vis.get(menu.sel.min(vis.len().saturating_sub(1)))?;
        menu.level().items.get(i).cloned()
    }

    /// Redraw the open level from the current config (a checkbox or slider just changed), cursor where it was.
    /// The settings search: the entries of every panel in FIND_PANELS under a title per panel, the panel itself first so
    /// that its name finds it. Each entry acts exactly as in its own panel.
    pub(crate) fn find_entries(&self) -> Vec<Entry> {
        self.find_in(&FIND_PANELS)
    }

    fn find_in(&self, panels: &[Id]) -> Vec<Entry> {
        let mut v = Vec::new();
        for &id in panels {
            let items = self.menu_level(id).items;
            v.push(head(id.title()));
            v.push(e(&format!("otworz: {}", id.title()), Act::Open(id)));
            v.extend(items.into_iter().filter(|i| !matches!(i.act, Act::Head | Act::Nop)));
        }
        v
    }

    fn menu_refresh(&mut self) {
        let Some(id) = self.menu.as_ref().map(|m| m.level().id) else { return };
        if id == Id::Pick {
            return; // the options were made when the dropdown opened
        }
        let level = self.menu_level(id);
        if let Some(l) = self.menu.as_mut().and_then(|m| m.stack.last_mut()) {
            *l = level;
        }
        self.bar_dirty = true;
    }

    /// Flip the checkbox (`dir` 0) or move the slider (`dir` -1 / +1) under the cursor; false when it is neither.
    fn menu_control(&mut self, dir: i32) -> bool {
        let Some(entry) = self.menu_selected() else { return false };
        match entry.act {
            Act::Toggle { key, on } if dir == 0 => {
                self.set_setting(key, if on { "off" } else { "on" });
            }
            Act::Slide { key, min, max, step, val } if dir != 0 => {
                let v = (val + dir as f32 * step).clamp(min, max);
                if (v - val).abs() > f32::EPSILON {
                    self.set_setting(key, &if step < 1.0 { format!("{v:.2}") } else { format!("{v:.0}") });
                }
            }
            Act::Hsv { ch, max, step, val } if dir != 0 => {
                self.picker_slide(ch, (val + dir as f32 * step).clamp(0.0, max));
            }
            Act::AutoToggle { name, on } if dir == 0 => {
                self.autostart_toggle(&name, !on);
            }
            Act::Toggle { .. } | Act::Slide { .. } | Act::Hsv { .. } | Act::AutoToggle { .. } => {}
            _ => return false,
        }
        self.menu_refresh();
        true
    }

    /// The launcher (Super+W): the same root list as the menu, a toggle; typing there searches every app and command.
    pub fn menu_launch(&mut self) {
        self.menu = Some(Menu { stack: vec![self.menu_level(Id::Root)], ..Menu::default() });
        self.root_sel_first_entry();
        self.panel_fx();
        self.bar_dirty = true;
    }

    /// The cursor starts on the first real entry (Aplikacje), not on the sound sliders above it.
    fn root_sel_first_entry(&mut self) {
        if let Some(m) = self.menu.as_mut() {
            let vis = m.visible();
            let items = &m.level().items;
            m.sel = vis.iter().position(|&i| matches!(items[i].act, Act::Open(_))).unwrap_or(0);
        }
    }

    /// Start the slide-in of the panel with the user's animation settings.
    fn panel_fx(&mut self) {
        let c = &self.config;
        self.fx.panel_open(c.theme.animations, c.anim_speed, c.anim_bounce);
    }

    pub fn menu_open(&mut self, id: Id) {
        if matches!(id, Id::ThemeNew | Id::ColorPick) {
            self.ensure_draft();
        }
        // opening a sub-menu directly still has its parents under it (hfsm path), so back goes up
        let stack = crate::flows::hfsm::path_of(id, Id::parent).into_iter().map(|i| self.menu_level(i)).collect();
        if id == Id::Notes {
            self.notes.mark_seen();
        }
        self.menu = Some(Menu { stack, ..Menu::default() });
        if id == Id::Root {
            self.root_sel_first_entry();
        }
        self.panel_fx();
        self.bar_dirty = true;
    }
}

impl Menu {
    pub fn level(&self) -> &Level {
        self.stack.last().expect("a menu always has a level")
    }

    /// Indices of the selectable entries matching every word of the query (case-insensitive); inside a section the
    /// prefix matches come first, sections keep their order.
    pub fn visible(&self) -> Vec<usize> {
        let items = &self.level().items;
        if matches!(self.level().id, Id::Calc | Id::ThemeNew | Id::ColorPick | Id::ThemeImport) {
            return (0..items.len()).filter(|&i| !matches!(items[i].act, Act::Head)).collect(); // the query is an expression, a name, a colour or a path here, not a filter
        }
        let q = self.query.to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let mut group = 0usize;
        let mut head = String::new();
        let find = self.level().id == Id::Find;
        let mut v: Vec<(usize, bool, usize)> = Vec::new();
        for (i, it) in items.iter().enumerate() {
            if matches!(it.act, Act::Head) {
                group += 1;
                head = it.label.to_lowercase();
                continue;
            }
            // in the settings search the name of the panel counts too: "tapeta" finds everything under Tapeta
            let l = if find { format!("{head} {}", it.label.to_lowercase()) } else { it.label.to_lowercase() };
            if words.iter().all(|w| l.contains(w)) {
                v.push((group, !words.first().is_some_and(|f| l.starts_with(f)), i));
            }
        }
        v.sort_by_key(|&(g, p, _)| (g, p));
        v.into_iter().map(|x| x.2).collect()
    }

    /// What to draw: section titles between the visible entries, and which row is selected.
    pub fn rows(&self) -> (Vec<Row>, usize) {
        let items = &self.level().items;
        let mut head_of: Vec<Option<&str>> = Vec::with_capacity(items.len());
        let mut cur = None;
        for it in items {
            if matches!(it.act, Act::Head) {
                cur = Some(it.label.as_str());
            }
            head_of.push(cur);
        }
        let vis = self.visible();
        let sel = self.sel.min(vis.len().saturating_sub(1));
        let (mut rows, mut last, mut sel_row) = (Vec::new(), None, 0);
        for (n, &i) in vis.iter().enumerate() {
            if head_of[i] != last {
                last = head_of[i];
                if let Some(h) = last {
                    rows.push(Row::Head(h.to_string()));
                }
            }
            if n == sel {
                sel_row = rows.len();
            }
            rows.push(items[i].row());
        }
        (rows, sel_row)
    }

    pub fn selectable(&self) -> usize {
        self.level().items.iter().filter(|i| !matches!(i.act, Act::Head)).count()
    }

    /// The settings panel is tabbed with the tabs on the left: while the stack is inside `Id::Settings` the tabs are the
    /// entries of that level. `focused` = the cursor is on the tabs (the right side then previews the tab under it).
    pub fn tabs(&self) -> Option<Tabs> {
        let si = self.stack.iter().position(|l| l.id == Id::Settings)?;
        let items = &self.stack[si].items;
        let labels = items.iter().map(|i| i.label.clone()).collect();
        let (active, focused) = if si + 1 == self.stack.len() {
            (self.sel.min(items.len().saturating_sub(1)), true)
        } else {
            let open = self.stack[si + 1].id;
            (items.iter().position(|i| matches!(i.act, Act::Open(id) if id == open)).unwrap_or(0), false)
        };
        let id = match items.get(active).map(|i| &i.act) {
            Some(Act::Open(id)) => Some(*id),
            _ => None,
        };
        Some(Tabs { labels, active, focused, id })
    }

    /// "Menu > Aplikacje > Flatpak"
    pub fn crumbs(&self) -> String {
        self.stack.iter().map(|l| l.id.title()).collect::<Vec<_>>().join(" > ")
    }
}

/// A held navigation key in the open menu. The menu takes every key, so no client repeats it: the compositor does.
#[derive(Debug, Clone)]
pub struct Hold {
    pub cmd: String,
    /// seconds held
    pub t: f32,
    /// repeats owed, fractional
    pub acc: f32,
}

/// Seconds before a held key starts repeating.
const HOLD_DELAY: f32 = 0.25;

/// Repeats per second after `held` seconds of holding: it starts at a steady scroll and speeds up the longer the key is held.
pub fn hold_rate(held: f32) -> f32 {
    match held {
        h if h < 0.4 => 20.0,
        h if h < 1.0 => 45.0,
        _ => 120.0,
    }
}

impl Desktop {
    /// A key went down in the open menu (`cmd` is the `ctl menu` line it ran): arrows and pages start repeating.
    pub fn menu_hold_press(&mut self, cmd: &str) {
        let rest = cmd.strip_prefix("key ").unwrap_or("");
        self.menu_hold = matches!(rest, "up" | "down" | "pageup" | "pagedown" | "left" | "right").then(|| Hold { cmd: cmd.into(), t: 0.0, acc: 0.0 });
    }

    /// Every frame: repeat the held key. Left / right only repeat on a slider (elsewhere left is "back").
    pub fn menu_hold_step(&mut self, dt: f32) {
        let Some(h) = self.menu_hold.as_mut() else { return };
        if self.menu.is_none() {
            self.menu_hold = None;
            return;
        }
        h.t += dt;
        if h.t < HOLD_DELAY {
            return;
        }
        let page = h.cmd.ends_with("pageup") || h.cmd.ends_with("pagedown");
        let side = h.cmd.ends_with("left") || h.cmd.ends_with("right");
        let rate = if side { 10.0 } else if page { hold_rate(h.t - HOLD_DELAY) / 6.0 } else { hold_rate(h.t - HOLD_DELAY) };
        h.acc += dt * rate;
        let n = h.acc.floor();
        h.acc -= n;
        let cmd = h.cmd.clone();
        if side && !self.menu_selected().is_some_and(|e| e.is_slider()) {
            self.menu_hold = None;
            return;
        }
        for _ in 0..(n as usize).min(30) {
            self.menu_cmd(&cmd);
        }
    }

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
                    None => "error: menu open root|apps|games|scripts|clip|calc|settings|themes|modes|layouts, or menu launch".into(),
                };
            }
            "close" => {
                self.menu = None;
                return "ok".into();
            }
            "launch" | "run" => {
                // a toggle, so the same key that opened the launcher closes it
                if self.menu.is_some() {
                    self.menu = None;
                } else {
                    self.menu_launch();
                }
                return "ok".into();
            }
            _ => {}
        }
        if self.menu.is_none() {
            return "error: menu is not open".into();
        }
        // typing on the root list searches everything, like dmenu_run: the search level opens under it
        if verb == "type" && self.menu.as_ref().is_some_and(|m| m.level().id == Id::Root) {
            let level = self.menu_level(Id::Run);
            if let Some(m) = self.menu.as_mut() {
                m.stack.push(level);
            }
        }
        // typing in the settings tabs or in a group searches every panel: the search level opens above it
        if verb == "type" && self.menu.as_ref().is_some_and(|m| matches!(m.level().id, Id::Settings | Id::SetLook | Id::SetInput | Id::SetHardware | Id::SetSystem)) {
            let level = self.menu_level(Id::Find);
            if let Some(m) = self.menu.as_mut() {
                m.stack.push(level);
            }
        }
        // space flips a checkbox when the cursor is on one; left / right move a slider
        if (verb == "key" && rest == "space") || (verb == "space") {
            if self.menu_control(0) {
                return "ok".into();
            }
        }
        if matches!((verb, rest), ("key", "left" | "right") | ("left" | "right", "")) {
            let dir = if rest == "left" || verb == "left" { -1 } else { 1 };
            if self.menu_selected().is_some_and(|e| e.is_slider()) {
                self.menu_control(dir);
                return "ok".into();
            }
        }
        let Some(menu) = self.menu.as_mut() else { return "error: menu is not open".into() };
        let vis = menu.visible();
        match verb {
            "list" => {
                let (rows, sel) = menu.rows();
                return std::iter::once(menu.crumbs())
                    .chain(rows.iter().enumerate().map(|(n, r)| match r {
                        Row::Head(h) => format!("-- {h}"),
                        Row::Item(l) | Row::Icon(l, _) => format!("{}{l}", if n == sel { "> " } else { "  " }),
                        Row::Check(l, on) => format!("{}[{}] {l}", if n == sel { "> " } else { "  " }, if *on { "x" } else { " " }),
                        Row::Slider(l, _, v) => format!("{}{l}: {v}", if n == sel { "> " } else { "  " }),
                        Row::Color(l, c) => format!("{}{l} {}", if n == sel { "> " } else { "  " }, crate::components::theme::hex(*c)),
                        Row::Choice(l, v) => format!("{}{l}: [{v} v]", if n == sel { "> " } else { "  " }),
                    }))
                    .collect::<Vec<_>>()
                    .join("\n");
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
                // the list is a ring: Up on the first row lands on the last, Down on the last on the first
                "up" => menu.sel = if menu.sel == 0 { vis.len().saturating_sub(1) } else { menu.sel.min(vis.len()) - 1 },
                "down" | "tab" => menu.sel = if menu.sel + 1 >= vis.len() { 0 } else { menu.sel + 1 },
                "pageup" => menu.sel = menu.sel.saturating_sub(10),
                "pagedown" => menu.sel = (menu.sel + 10).min(vis.len().saturating_sub(1)),
                "esc" => self.menu = None,
                // Left: one step back (the query first, then the level); Right: go in
                "back" | "left" => {
                    if !menu.query.is_empty() {
                        menu.query.clear();
                        menu.sel = 0;
                        self.menu_recalc();
                    } else if menu.stack.len() > 1 {
                        let gone = menu.stack.pop().map(|l| l.id);
                        // back on the tabs of the settings panel: the cursor stays on the tab we came from
                        menu.sel = match (menu.level().id, gone) {
                            (Id::Settings | Id::SetLook | Id::SetInput | Id::SetHardware | Id::SetSystem, Some(g)) => menu.level().items.iter().position(|i| matches!(i.act, Act::Open(id) if id == g)).unwrap_or(0),
                            _ => 0,
                        };
                    }
                }
                "right" => return self.menu_enter(),
                "backspace" => {
                    if menu.query.pop().is_some() {
                        menu.sel = 0;
                        // the search opened from the root: erasing it all goes back to the root list
                        if menu.query.is_empty() && matches!(menu.level().id, Id::Run | Id::Find) && menu.stack.len() > 1 {
                            menu.stack.pop();
                        }
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
            "up" | "down" | "left" | "right" | "enter" | "back" | "esc" | "backspace" | "pageup" | "pagedown" | "space" => return self.menu_cmd(&format!("key {verb}")),
            _ => return format!("error: unknown menu command {verb:?} (open, launch, close, toggle, list, type TEXT, key up|down|enter|back|esc|space)"),
        }
        "ok".into()
    }

    /// In the calculator the single entry is the value of what has been typed.
    fn menu_recalc(&mut self) {
        // a name, a colour or a path is being typed: the level shows what the text means
        if self.menu.as_ref().is_some_and(|m| matches!(m.level().id, Id::ThemeNew | Id::ColorPick | Id::ThemeImport | Id::Keys)) {
            self.menu_refresh();
            return;
        }
        let Some(menu) = self.menu.as_mut() else { return };
        let level = menu.stack.last_mut().expect("a menu always has a level");
        if level.id == Id::Run {
            // the launcher searches files too: the "Pliki" section at the end follows what is typed (plocate, instant)
            let hits = file_hits(menu.query.trim());
            if let Some(at) = level.items.iter().position(|it| matches!(it.act, Act::Head) && it.label == "Pliki") {
                level.items.truncate(at);
            }
            if !hits.is_empty() {
                level.items.push(head("Pliki"));
                level.items.extend(hits);
            }
            return;
        }
        if level.id != Id::Calc {
            return;
        }
        level.items = vec![match crate::components::calc::eval(&menu.query) {
            Some(v) => {
                let r = crate::components::calc::show(v);
                Entry { icon: None, label: format!("= {r}"), act: Act::Copy(r) }
            }
            None if menu.query.trim().is_empty() => e("wpisz wyrazenie, Enter kopiuje wynik", Act::Nop),
            None => e("blad skladni", Act::Nop),
        }];
        menu.sel = 0;
    }

    /// Enter on the selected row. A choice made in a dropdown closes the dropdown and shows the panel under it again.
    fn menu_enter(&mut self) -> String {
        let in_pick = self.menu.as_ref().is_some_and(|m| m.level().id == Id::Pick);
        let r = self.menu_enter_row();
        if in_pick {
            if let Some(m) = self.menu.as_mut() {
                if m.level().id == Id::Pick && m.stack.len() > 1 && !r.starts_with("error") {
                    m.stack.pop();
                    m.sel = 0;
                }
            }
            self.menu_refresh();
        }
        r
    }

    fn menu_enter_row(&mut self) -> String {
        let Some(menu) = self.menu.as_ref() else { return "error: menu is not open".into() };
        let vis = menu.visible();
        let Some(&i) = vis.get(menu.sel.min(vis.len().saturating_sub(1))) else {
            // dmenu_run: what was typed is the command when nothing matches
            if menu.level().id == Id::Run && !menu.query.trim().is_empty() {
                let cmd = menu.query.trim().to_string();
                self.menu = None;
                crate::keys::spawn(&cmd);
                return "ok".into();
            }
            return "error: nothing to select".into();
        };
        let entry = menu.level().items[i].clone();
        if let Act::Choice { sel, options, .. } = &entry.act {
            let menu = self.menu.as_mut().unwrap();
            menu.stack.push(Level { id: Id::Pick, items: options.clone() });
            menu.query.clear();
            menu.sel = *sel;
            return "ok".into();
        }
        if let Act::Open(id) = entry.act {
            if id == Id::Notes {
                self.notes.mark_seen();
            }
            if id == Id::ThemeNew {
                self.theme_draft = None;
                self.ensure_draft();
            }
            let level = self.menu_level(id);
            let menu = self.menu.as_mut().unwrap();
            menu.stack.push(level);
            menu.query.clear();
            menu.sel = 0;
            return "ok".into();
        }
        if let Act::Slide { key, .. } = &entry.act {
            if let Some((mic, false)) = crate::components::audio::parse_key(key) {
                let r = self.audio_mute(mic, None);
                self.menu_refresh();
                return r;
            }
        }
        if matches!(entry.act, Act::Toggle { .. } | Act::AutoToggle { .. }) {
            self.menu_control(0);
            return "ok".into();
        }
        if matches!(entry.act, Act::Slide { .. }) {
            return "ok".into();
        }
        match entry.act.clone() {
            Act::Set { key, val } => {
                let r = self.set_setting(key, &val);
                self.menu_refresh();
                return r;
            }
            Act::Color { key, .. } => {
                self.picker_open(key);
                let level = self.menu_level(Id::ColorPick);
                let menu = self.menu.as_mut().unwrap();
                menu.stack.push(level);
                menu.query.clear();
                menu.sel = 1;
                return "ok".into();
            }
            Act::SetColor(c) => {
                self.picker_set(c);
                if let Some(m) = self.menu.as_mut() {
                    m.query.clear();
                }
                self.menu_refresh();
                return "ok".into();
            }
            Act::Out(line) => {
                let r = self.output_change(&line);
                self.menu_refresh();
                return r;
            }
            Act::AddBind(text) => {
                let r = self.bind_add(&text);
                if r == "ok" {
                    if let Some(m) = self.menu.as_mut() {
                        m.query.clear();
                    }
                }
                self.menu_refresh();
                return r;
            }
            Act::DelBind(label) => {
                let r = self.bind_del(&label);
                self.menu_refresh();
                return r;
            }
            Act::SaveTheme(name) => {
                let r = self.theme_save(&name);
                if r.starts_with("ok") {
                    self.menu = None;
                }
                return r;
            }
            Act::ImportTheme(path) => {
                let r = self.theme_import(&path);
                if r.starts_with("ok") {
                    self.menu = None;
                }
                return r;
            }
            Act::Swatch(_) | Act::Hsv { .. } => return "ok".into(),
            _ => {}
        }
        if let Act::Font(path) = entry.act {
            let r = self.set_font(&path);
            self.menu_refresh();
            return r;
        }
        if let Act::PickPrefer(role) = entry.act {
            let term = self.config.terminal.clone();
            if role == "shell" {
                // shells are not desktop applications: the installed ones from /etc/shells
                let text = std::fs::read_to_string("/etc/shells").unwrap_or_default();
                let mut items: Vec<Entry> = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| l.starts_with('/') && !l.contains("nologin") && !l.ends_with("/false"))
                    .map(|p| Entry { icon: None, label: p.rsplit('/').next().unwrap_or(p).to_string(), act: Act::SetPrefer { role: role.clone(), cmd: p.to_string() } })
                    .collect();
                items.dedup_by(|a, b| a.label == b.label);
                if items.is_empty() {
                    items.push(e("bash", Act::SetPrefer { role: role.clone(), cmd: "bash".into() }));
                }
                let menu = self.menu.as_mut().unwrap();
                menu.stack.push(Level { id: Id::AllApps, items });
                menu.query.clear();
                menu.sel = 0;
                return "ok".into();
            }
            let items = apps::scan()
                .into_iter()
                .map(|a| Entry { icon: None, label: a.name.clone(), act: Act::SetPrefer { role: role.clone(), cmd: crate::components::settings::settings_preferaps::command_for(&a, &term) } })
                .collect();
            let menu = self.menu.as_mut().unwrap();
            menu.stack.push(Level { id: Id::AllApps, items });
            menu.query.clear();
            menu.sel = 0;
            return "ok".into();
        }
        if let Act::CtlStay(line) = &entry.act {
            let r = match crate::config::parse_action(line) {
                Ok(a) => self.run_action(a),
                Err(e) => format!("error: {e}"),
            };
            self.menu_refresh();
            return r;
        }
        if matches!(entry.act, Act::Nop | Act::Head) {
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
            Act::Ctl(line) => match crate::config::parse_action(&line) {
                Ok(a) => self.run_action(a),
                Err(e) => format!("error: {e}"),
            },
            Act::Reload => self.run_action(Action::Reload),
            Act::ReloadEngine => self.run_action(Action::ReloadEngine),
            Act::EditConfig => {
                let path = crate::config::user_path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                let term = self.config.terminal.clone();
                crate::keys::spawn(&format!("{term} -e sh -c '${{EDITOR:-frost}} \"{path}\"'"));
                "ok".into()
            }
            Act::AutoToggle { .. } | Act::AddBind(_) | Act::DelBind(_) | Act::Set { .. } | Act::Color { .. } | Act::Swatch(_) | Act::SetColor(_) | Act::Hsv { .. } | Act::SaveTheme(_) | Act::ImportTheme(_) | Act::Out(_) | Act::Font(_) | Act::CtlStay(_) | Act::Open(_) | Act::PickPrefer(_) | Act::Toggle { .. } | Act::Slide { .. } | Act::Choice { .. } | Act::Nop | Act::Head => "ok".into(),
        }
    }
}

/// Files under $HOME whose name contains `q` (3+ characters), found by `fd` (hidden files and .gitignore'd build output
/// skipped, links followed, depth 6, 0.3 s at most): an `xdg-open` entry each, at most 15. No fd: nothing.
fn file_hits(q: &str) -> Vec<Entry> {
    if q.chars().count() < 3 || q.contains(char::is_whitespace) {
        return Vec::new();
    }
    let Some(home) = std::env::var_os("HOME").map(|h| h.to_string_lossy().into_owned()) else { return Vec::new() };
    let Ok(out) = std::process::Command::new("timeout")
        .args(["0.3", "fd", "-L", "-d", "6", "-t", "f", "-a", "--max-results", "60", "-F", "-i", "--", q, &home])
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    file_entries(&String::from_utf8_lossy(&out.stdout), &home)
}

fn file_entries(list: &str, home: &str) -> Vec<Entry> {
    let prefix = format!("{home}/");
    list.lines()
        .filter_map(|p| p.strip_prefix(&prefix).map(|rel| (p, rel)))
        .filter(|(_, rel)| !rel.split('/').any(|c| c.starts_with('.') || matches!(c, "target" | "node_modules" | "graphify-out" | "__pycache__")))
        .take(15)
        .map(|(p, rel)| Entry { icon: None, label: format!("~/{rel}"), act: Act::Run { cmd: format!("xdg-open '{}'", p.replace('\'', "'\\''")), terminal: false } })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_entries_keep_home_files_and_drop_hidden_and_build_output() {
        let list = "/home/u/a/notes.txt\n/home/u/.cache/x/notes.txt\n/home/u/p/target/notes.o\n/etc/notes\n/home/u/it's.txt\n";
        let v = file_entries(list, "/home/u");
        let labels: Vec<&str> = v.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["~/a/notes.txt", "~/it's.txt"]);
        assert!(matches!(&v[1].act, Act::Run { cmd, .. } if cmd == "xdg-open '/home/u/it'\\''s.txt'"));
        assert!(file_hits("ab").is_empty());
    }

    #[test]
    fn held_keys_speed_up() {
        assert!(hold_rate(0.0) < hold_rate(0.6) && hold_rate(0.6) < hold_rate(2.0));
    }

    #[test]
    fn checkboxes_and_sliders_draw_as_such() {
        let c = Entry { icon: None, label: "a".into(), act: Act::Toggle { key: "restore", on: true } };
        assert_eq!(c.row(), Row::Check("a".into(), true));
        let sl = Entry { icon: None, label: "g".into(), act: Act::Slide { key: "gap", min: 0.0, max: 40.0, step: 1.0, val: 10.0 } };
        assert_eq!(sl.row(), Row::Slider("g".into(), 0.25, "10".into()));
        assert_eq!(e("x", Act::Nop).row(), Row::Item("x".into()));
    }

    #[test]
    fn opening_a_deep_level_opens_the_path_to_it() {
        let path = crate::flows::hfsm::path_of(Id::Themes, Id::parent);
        assert_eq!(path, [Id::Root, Id::Settings, Id::SetLook, Id::Themes]);
    }

    #[test]
    fn the_package_manager_is_the_first_one_found_and_none_is_handled() {
        let (name, check, up) = pm_commands(|b| b == "pacman" || b == "apt").unwrap();
        assert_eq!((name, check, up), ("pacman", Some("checkupdates"), "sudo pacman -Syu"));
        assert!(pm_commands(|_| false).is_none());
    }

    #[test]
    fn a_choice_row_shows_the_current_value() {
        let c = Entry { icon: None, label: "Uklad".into(), act: Act::Choice { cur: "grid".into(), sel: 0, options: vec![e("grid", Act::Nop)] } };
        assert_eq!(c.row(), Row::Choice("Uklad".into(), "grid".into()));
    }

    #[test]
    fn every_searched_panel_sits_under_a_settings_group() {
        for id in FIND_PANELS {
            let path = crate::flows::hfsm::path_of(id, Id::parent);
            assert!(path.len() >= 4 && path[1] == Id::Settings, "{id:?} is not under a settings group: {path:?}");
        }
    }

    #[test]
    fn appimages_are_found_two_folders_deep_and_quoted() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("fd-menu-ai-{}", std::process::id()));
        std::fs::create_dir_all(d.join("a/b/c")).unwrap();
        for (n, mode) in [("Tool.AppImage", 0o755), ("a/It's.appimage", 0o755), ("a/b/Deep.AppImage", 0o755), ("a/b/c/TooDeep.AppImage", 0o755), ("Plain.AppImage", 0o644), ("x.txt", 0o755)] {
            std::fs::write(d.join(n), "").unwrap();
            std::fs::set_permissions(d.join(n), std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let v = appimages_in(&[d.clone()]);
        let labels: Vec<&str> = v.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Deep", "It's", "Tool"]);
        let Act::Run { cmd, .. } = &v[1].act else { panic!() };
        assert!(cmd.starts_with('\'') && cmd.contains("It'\\''s.appimage"), "{cmd}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn executables_in_a_folder_become_entries_and_plain_files_do_not() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("fd-menu-exec-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for (n, mode) in [("home", 0o755), ("etc", 0o755), ("notes.txt", 0o644)] {
            std::fs::write(d.join(n), "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(d.join(n), std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let labels: Vec<String> = exec_entries(Some(&d), "backup: ").into_iter().map(|e| e.label).collect();
        assert_eq!(labels, ["backup: etc", "backup: home"]);
        assert!(exec_entries(None, "x").is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// Bluetooth is Blueman's job: its own windows open from here, one entry per panel it has installed.
fn have_cmd(c: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(c).is_file()))
}

/// A command in a terminal that stays open to be read afterwards.
fn in_term(cmd: &str) -> Act {
    Act::Run { cmd: format!("{cmd}; printf '\\n[Enter zamyka]'; read _"), terminal: true }
}

/// (check, upgrade) for the package manager found first on PATH.
fn pm_commands(have: impl Fn(&str) -> bool) -> Option<(&'static str, Option<&'static str>, &'static str)> {
    [
        ("xbps-install", "xbps", Some("xbps-install -Sun"), "sudo xbps-install -Su"),
        ("pacman", "pacman", Some("checkupdates"), "sudo pacman -Syu"),
        ("apt", "apt", Some("apt list --upgradable"), "sudo apt update && sudo apt upgrade"),
        ("dnf", "dnf", Some("dnf check-update"), "sudo dnf upgrade"),
        ("zypper", "zypper", Some("zypper list-updates"), "sudo zypper dup"),
        ("apk", "apk", Some("apk version -l '<'"), "sudo apk upgrade -U"),
    ]
    .into_iter()
    .find(|(bin, ..)| have(bin))
    .map(|(_, name, check, up)| (name, check, up))
}

/// Settings > Aktualizacje: check and upgrade through the system's own package manager, in a terminal so that the
/// password prompt and the output are visible. Nothing runs unless an entry is chosen.
fn update_entries() -> Vec<Entry> {
    let mut v = vec![head("uruchamiane w terminalu; system pyta o haslo sam")];
    match pm_commands(have_cmd) {
        Some((name, check, up)) => {
            if let Some(c) = check {
                v.push(e(&format!("Sprawdz aktualizacje ({name})"), in_term(c)));
            }
            v.push(e(&format!("Zaktualizuj system ({name})"), in_term(up)));
        }
        None => v.push(e("Nie znaleziono menedzera pakietow (xbps, pacman, apt, dnf, zypper, apk)", Act::Nop)),
    }
    if have_cmd("flatpak") {
        v.push(e("Zaktualizuj Flatpaki", in_term("flatpak update")));
    }
    if have_cmd("rustup") {
        v.push(e("Zaktualizuj Rusta (rustup)", in_term("rustup update")));
    }
    v
}

/// Settings > Czyszczenie dysku: the `cleaner` tool, which lists what would go and asks once before deleting anything.
fn cleaner_entries() -> Vec<Entry> {
    if have_cmd("cleaner") {
        vec![
            head("cleaner pokazuje co zniknie i pyta raz; nic nie jest kasowane przed potwierdzeniem"),
            e("Znajdz zbedne pliki i posprzataj (cleaner)", in_term("cleaner")),
            e("Co zajmuje miejsce: ~ (du, 15 najwiekszych)", in_term("du -h -d1 ~ 2>/dev/null | sort -rh | head -15")),
        ]
    } else {
        vec![e("cleaner nie jest zainstalowany (Gucixdev/bash: src/cleaner); dodaj go do PATH", Act::Nop)]
    }
}

fn bluetooth_entries() -> Vec<Entry> {
    const PANELS: [(&str, &str); 4] = [
        ("blueman-manager", "Urzadzenia: parowanie, polaczenia, wysylanie plikow"),
        ("blueman-adapters", "Adapter: widocznosc, nazwa, czas widocznosci"),
        ("blueman-services", "Uslugi: transfer plikow, siec, audio"),
        ("blueman-assistant", "Kreator parowania nowego urzadzenia"),
    ];
    let have = |c: &str| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(c).is_file()));
    let mut v = Vec::new();
    for (cmd, what) in PANELS {
        if have(cmd) {
            v.push(e(what, Act::Run { cmd: cmd.into(), terminal: false }));
        }
    }
    if v.is_empty() {
        v.push(e("Blueman nie jest zainstalowany (blueman-manager nie ma w PATH)", Act::Nop));
    } else {
        v.insert(0, head("panele Blueman otwieraja sie jako okna"));
    }
    v
}
