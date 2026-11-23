# finedesktop-rs

Minimal, keyboard-first Wayland desktop in Rust on [smithay](https://github.com/Smithay/smithay).
Smithay is vendored in `vendor/smithay` on purpose: the whole stack stays readable and patchable.

## Run (nested in a window, for development)

    cargo run -- -c foot          # FD_MOD=alt cargo run  when the host compositor eats Super

## Layouts

Pure geometry in `src/components/engine/layout_engine.rs` (unit-tested, no smithay types):
`tile` `float` `monocle` `list` `grid` `stack` `tab` `spiral` (Fibonacci; `mfact 0.62` is the golden one). New layout = one enum variant + one `match` arm.
`list` was `deck` (the old name still works in configs and ctl): the master on the left, one window of the rest on the right.

On a Steam Deck (DMI product Jupiter or Galileo, or `FD_PROFILE=steamdeck`) `src/steamdeck.conf` is applied after the defaults: doubled font, tall tab bars, `list` layout, 4 workspaces, phone-like trackpads, brightness keys, battery in the bar. Your own config still wins. Not run on a real Deck yet.
Layouts parked for later live in `src/unused/screen_mode/`.

## Keys (Mod = Super, or Alt with `FD_MOD=alt`)

| key | action |
|---|---|
| Mod+Return / Mod+W | terminal (`$TERMINAL`, default foot) / launcher (sections Aplikacje, Zasilanie, Backup, Polecenia; type to filter, unmatched text runs as typed) |
| Mod+Shift+W | the menu (apps, games, clipboard, calculator, settings) |
| Mod+Q, Mod+Shift+C | close focused |
| Mod+Shift+Q | quit |
| Mod+J / Mod+K | focus next / previous |
| Mod+Shift+Return | move focused to master |
| Mod+Space / Mod+Shift+Space | next / previous layout |
| Mod+T F M D G S Y R | tile float monocle list grid stack tab spiral |
| Mod+H / Mod+L | master width - / + |
| Mod+I / Mod+Shift+I | more / fewer masters |

## State

Works nested (winit backend): tiling verified with two real foot clients.
Not done yet: DRM/libinput backend (real session), drawing the tab/stack bar, tags/workspaces,
layer-shell (bar, menu), per-output layouts, config file, hooks.
Key actions are wired but only the layout math is covered by tests (`cargo test`).

## Using it

```
./target/release/finedesktop          # on a tty: DRM + libinput via seatd; inside another compositor: a window
finedesktop ctl theme list|NAME       # look: dwl zen matrix catpuccino dracula steam official-dark official-light
finedesktop ctl mode list|NAME|off    # behaviour: gamemode ecomode afkmode (nightmode/daymode also change the theme, optional)
finedesktop ctl bar toggle|top|bottom|off
finedesktop ctl emit NAME TEXT        # a user event for hooks and subscribers
finedesktop ctl subscribe             # one line per event: start open close focus title tag layout output theme mode reload user minute
```

Config dir: `$FD_CONFIG_DIR` or `~/.config/finedesktop/`: `config`, `themes/NAME`, `mode/NAME`, `hooks/on-<event>` (executable,
started in the background with `FD_*` variables), `widgets/NAME` (executable, shown in the bar when its name is in
`bar_left` / `bar_right`), `segment = NAME SECONDS COMMAND` in the config. Examples are in `examples/`.
Built-in bar items: `cpu gpu ram vram swap` (numbers), `cpu-graph gpu-graph ram-graph vram-graph swap-graph` (percentage plus the last 30 samples as a mini graph), `disk` (free space) and `disk-bar` (used, as a bar) of the filesystem named by `disk = PATH`, `disks` (every real disk: name, used %, fill bar; a device mounted several times counts once) and `stats` = `cpu-graph gpu-graph ram-graph vram-graph swap-graph disks` in one word (`bar_right = stats clock`). Every percentage and graph column is normal below 60%, the theme's `warn` (yellow, theme key `warn = #rrggbb`) from 60% and `err` (red, theme key `err = #rrggbb`; the old name `urgent` still reads) from 90%. Code: `src/components/stats.rs`.
Menu: Aplikacje lists Flatpak, Natywne, Terminal (apps with `Terminal=true`), Gry, Skrypty (`<config dir>/scripts/`) and AppImage (executable `*.AppImage` in ~/Applications, ~/AppImages, ~/.local/share/AppImage, ~/.local/bin, ~/Downloads, two folders deep). Ustawienia > Czcionki picks the global font (`font =`, a `.bdf`/`.psf` from `<config dir>/fonts/` or ~/.local/share/fonts) and the scale; foot's `font=` follows only when fontconfig resolves the file.

Settings are grouped: Wyglad, Wejscie, Sprzet, System (`ctl menu open look|input|hardware|system` or by panel id; the breadcrumb shows the group). Typing in Ustawienia or in a group searches every panel (title "Szukaj"), so `Przyspieszenie` finds the mouse option from anywhere. Options with a fixed set of values (acceleration profile, bar position, screen mode and placement) are dropdowns: the row reads `Label: [value v]`, Enter opens the list ("Wybierz"), Enter on a value saves it. System also has Aktualizacje (`ctl menu open update`: check and upgrade through xbps/pacman/apt/dnf/zypper/apk, Flatpak, rustup, each in a terminal) and Czyszczenie dysku (`cleaner`: lists what would go, asks once).
Ustawienia (`ctl menu open settings`, ids `general`, `themes`, `fonts`, `bar`, `input`, `screens`, `create_theme`, `theme-import`):
- Motyw > Utworz motyw: 12 colours, Enter on one opens a picker (hue / saturation / value sliders, or type `#rrggbb` and Enter). The change shows live; type a name and Enter on "Zapisz jako" writes `<config dir>/themes/NAME` and uses it; Esc throws the draft away.
- Motyw > Importuj motyw z pliku: type a path (or pick a found `*.theme` in ~, ~/Downloads, the config dir). The file must parse as a theme, it is copied into `themes/` and used.
- Pasek: position, clock and date format, the stats item, tab height, refresh. Klawiatura i mysz: layout, options, key repeat, pointer speed, natural scroll, tap to click, focus follows mouse. Ogolne has the animation speed and bounce sliders.
- Ekrany: per screen the mode list, position next to another screen, off/on. It is saved as `output = NAME mode=WxH@HZ pos=X,Y off` and applied when the compositor starts (`finedesktop ctl session restart`); no scale or rotation yet. Not run on a real DRM session, only the nested window.
- Skroty (`ctl menu open keys`): every bind, filtered by typing. Typing `mod+x, spawn foot` offers "Dodaj skrot" (checked, then written as a `bind =` line); Enter on a bind removes its line and, when the default config has it, writes an `unbind =`. Autostart (`ctl menu open autostart`): the files of the autostart folder, on while executable (Space flips the bit), the `exec =` lines, and "run again". Bluetooth (`ctl menu open bluetooth`) opens Blueman's own windows (manager, adapters, services, assistant), one entry per `blueman-*` found in PATH; they are ordinary windows, not embedded in the menu. Ogolne also has tearing, the number of workspaces (more at once, fewer after a restart) and the low battery threshold.
- Dzwiek (`ctl menu open audio`): speakers and microphone sliders with a mute each, through `wpctl`. The same two sliders sit on top of the launcher (Super+W): Left/Right changes the volume, Enter mutes; the cursor still starts on Aplikacje. The mic key (XF86AudioMicMute) now mutes the microphone, not the speakers.
- Dark / light: `ctl theme toggle` (also `dark`, `light`; Super+Shift+D, and Motyw > Przelacz jasny / ciemny) switches to the partner of the current theme (`official-dark` <-> `official-light`, otherwise the official pair) and remembers it in the config.
- Aplikacje: besides terminal, browser, editor and files, a role per kind of file (office, pdf, image, video, audio, archive, cad, mail) with the formats it covers (docx xlsx pptx, stl obj step ...). Choosing an app writes `prefer_<role>` and sets it as the system default for those MIME types (`xdg-mime default`) when it is a desktop application.
- Lists in the menu are rings: Up on the first row goes to the last, Down on the last goes to the first (PageUp/PageDown stop at the ends).

A theme is a separate setting from a mode: a theme is how it looks, a mode is how the system behaves.

## Look and feel options (config)

- `font = PATH` a `.bdf` or `.psf` bitmap font (default `~/.local/share/fonts/Fixed6x13.bdf`; missing file = built-in
  Terminus 8x16); `font_scale = 2` doubles it. Bar, menu and tab sizes follow the font cell.
- `mouse_follows_focus = on` (default): when a key moves focus, the pointer jumps to the middle of that window, unless it is
  inside it already. `focus_follows_mouse` is the other direction.
- `tearing = on` (default off): DRM page flips do not wait for vblank (lower latency, visible tearing). Falls back to a
  normal flip when the driver refuses; not verified on real hardware yet. Applied by `ctl reload`.

## Menu and logs

`Mod+Shift+W` opens the menu: Aplikacje (ONE list with sections Flatpak / Natywne / Gry), Schowek, Skrypty
(executables in `<config dir>/scripts/`), Kalkulator, Ustawienia. Type filters, Left goes back (clears the query first), Right/Enter goes in.
`ctl menu launch` (Mod+W) is the launcher: the root list (Aplikacje, Schowek, Kalkulator, Zasilanie, Ustawienia; Ustawienia is a panel with checkboxes and sliders), and typing there searches every app and command, which include `power: lock|logout|suspend|hibernate|reboot|poweroff` (`ctl session`) and `backup: NAME` for every executable in `<config dir>/backup/` (one script per backup job). `ctl log [N]` prints the last N log lines (also kept in
`~/.local/state/finedesktop/log`); every action, from a key, `ctl` or a hook, is logged as `action: ...`.

## Tab and stack labels

In `tab`, `stack`, `list` and `monocle` each window is a label in the bar. `ctl name TEXT` renames the focused one,
`ctl name ID TEXT` a given window (ids are in `ctl state` and in hooks as `FD_WINDOW`), `ctl name -` gives the title
back. A hook can name windows by app: `on-window-open` running `ctl name "$FD_WINDOW" "$FD_APPID"`.
The bar is one text row plus 2 px; `tab_height = N` in the config sets it.

## Own commands, not only `ctl`

Every action is also its own command: `./links` makes `fd-trash`, `fd-notify`, `fd-session`, `fd-screenshot`,
`fd-screenrecord`, `fd-menu`, `fd-name`, `fd-log`, `fd-layout`, ... next to `finedesktop` (symlinks, the binary reads
its own name). `fd-notify hi` is `finedesktop ctl notify hi`. Trash needs no compositor at all.

- `restore = on` (default): the desktop keeps `~/.local/state/finedesktop/sessions/last.tsv` up to date (app, command, working folder,
  monitor, workspace, tab name, tags) and at the next start launches the apps that have no window yet and puts each new window back
  where its predecessor sat. Wayland windows cannot survive a compositor restart, so what returns is the app and its place, not its
  contents (a browser or editor restores its own tabs). `ctl session save|restore [NAME]`, `ctl session saved`, and
  `ctl session restart` (save, exit 75; `finedesktop-session` starts the installed binary again, up to 5 times in a row). A Flatpak
  app is started from its .desktop entry. Not tested on a live session.
  **Terminals keep their contents**: `session/fd-shell` (install to `~/.local/bin`, `shell=` in foot.ini, config in
  `~/.config/finedesktop/tmux.conf`) runs each terminal window in its own tmux session on a private socket (`-L finedesktop`).
  The saved row of such a window is `foot -e fd-shell NAME`, so a restart attaches to the same session: scrollback and running programs
  are still there. Closing the window with `ctl close` ends its session; `exit` ends it too. Cost: the wheel scrolls tmux's history
  (q leaves it), selecting text with the mouse needs Shift, and `TERM` inside is `tmux-256color`. Remove the `shell=` line to go back.
  **Browser**: Firefox restores its tabs itself; the profile needs `browser.startup.page = 3` and
  `browser.sessionstore.resume_from_crash = true` (a desktop restart looks like a crash to it).
- **trash**: `fd-trash FILE...` moves into the freedesktop trash (`~/.local/share/Trash`), `fd-trash list`,
  `fd-trash restore NAME`. Never deletes; emptying is yours.
- **notify**: `fd-notify TEXT` shows in the bar for 8 s (bar item `notify`), `fd-notify list|clear`, hook `on-notify`
  (`FD_NOTE`, `FD_TEXT`). A D-Bus front for `notify-send` is not written.
- **session**: `fd-session lock|logout|suspend|hibernate|reboot|poweroff` (`lock = CMD` in the config for the locker).
- **security**: the socket is 0600; `ctl_deny = spawn quit session` refuses those over ctl. A guard rail, not a sandbox.
- **settings panel** (`Mod+Shift+W` > Ustawienia): tabs down the left (Motyw, Tryb, Uklad okien, Aplikacje, Sesja,
  Powiadomienia, Kosz, System); Right/Enter opens a tab, Left returns. The panel slides in on a spring (`anim.rs`, `fx.rs`).

- **autostart**: every executable file in `~/.config/finedesktop/autostart/` is started once at login, in name order
  (after the `exec =` lines). Not executable = ignored. `fd-autostart list|run`.
- **window rules**: `rule = APP ws=N name=TEXT` (app id, case-insensitive, trailing `*` = prefix); applied once when the
  window's app id is known. Example: `rule = firefox ws=2 name=web`.
- **power**: `fd-power` shows battery, charger and profile; `fd-power profile [power-saver|balanced|performance]`
  (needs `powerprofilesctl`). `low_battery = 15` posts one notification and an `on-user power` event (0 = off).
- **crash**: a panic writes `~/.local/state/finedesktop/crash-<time>.txt` (message, place, backtrace, last log lines);
  the next start posts a notification with the path. `fd-crash` lists reports (read ones get `.seen`, nothing is deleted).
- **sound items**: `spk` and `mic` always show value and state: `spk 45% on`, `mic 80% off`, `mic --` when there is no
  device (wpctl has no default source, e.g. no microphone plugged in).

## flows: the primitives (src/flows/)

The desktop is built from a few primitives; features register with them instead of each inventing a mechanism.

- **bus** (pub/sub/observer): every event goes through `emit`. Listeners: `ctl subscribe [TOPIC...]` streams
  (`*` and `PREFIX*` work), in-process `Call` subscribers, and the scripts in `hooks/`. `update`, `fixupdate` and `tick`
  are "hot": only delivered when named. Observer: `ctl observe NAME VALUE` (and `Desktop::observe`) publishes
  `change NAME VALUE` only when the value differs; battery and charging are observed this way.
- **lifecycle**: components get `awake`, `start`, `restart`, `update(dt)`, `fixed_update` (20 ms), `tick` (1 s),
  `enabled`, `disabled`, `destroy`. Built in: `crash`, `autostart`, `power`. `ctl components`, `ctl enable|disable NAME`,
  `ctl restart` (config + engine reload, windows stay).
- **tags and groups**: `ctl tags [add|del TAG [WIN]|of]`, `rule = APP tag=a,b`, and `ctl group TAG list|focus|move N|close|tag NEW`
  (a group is the windows carrying a tag).
- **layers and panels**: `ctl layers` (background < bottom < tiled < floating < top < overlay), `ctl panels [show|hide|toggle bar|welcome|menu]`.
- **fsm / hfsm**: flat and nested state machines (the menu's back path is an hfsm path).

Hook scripts (`~/.config/finedesktop/hooks/`): `on-start on-restart on-destroy on-tick on-enabled on-disabled on-change
on-tagged on-hook on-event` besides the older ones. `on-start`, `on-restart`, `on-destroy` run once for the desktop;
`on-enabled`/`on-disabled` per component (`FD_NAME`); `on-event` gets every ordinary event (`FD_EVENT`); `on-hook` after
another script was started. A script on `on-tick` starts one process a second: you asked for it by creating the file.
There is no `on-update` or `on-fixupdate` script (50 processes a second); use `ctl subscribe update fixupdate`.

## Notification center, devices and the shell role

- `ctl notify ...` and Ustawienia > Powiadomienia: history grouped per app, do not disturb, snooze, mute per app, per-app logs under `$XDG_STATE_HOME/finedesktop/notts/log/`. `session/nottsd` (C, sd-bus, `make -C session`) owns `org.freedesktop.Notifications` so notify-send, Discord, Telegram and Element land in the same groups; `session/test-nottsd` checks it on a throwaway bus.
- Preferred applications has a `shell` role (default `bash`, `$shell` in binds); `fd-shell` only wraps the preferred shell in a tmux session.
- Ustawienia > Tapeta (swaybg), Urzadzenia (default speaker and microphone, cameras, brightness), Sterowniki (PCI devices and their kernel drivers, read only). Keyboard and mouse gained the acceleration profile, left-handed, middle-button emulation and disable-while-typing.
- Layout names: `tiled` is `grid` (equal cells), `fibonacci` is `spiral`, `tile` stays the master and stack column layout.
