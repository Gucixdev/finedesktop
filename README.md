# finedesktop-rs

Minimal, keyboard-first Wayland desktop in Rust on [smithay](https://github.com/Smithay/smithay).
Smithay is vendored in `vendor/smithay` on purpose: the whole stack stays readable and patchable.

## Run (nested in a window, for development)

    cargo run -- -c foot          # FD_MOD=alt cargo run  when the host compositor eats Super

## Layouts

Pure geometry in `src/components/engine/layout_engine.rs` (unit-tested, no smithay types):
`tile` `float` `monocle` `deck` `grid` `stack` `tab` `spiral` (Fibonacci; `mfact 0.62` is the golden one). New layout = one enum variant + one `match` arm.
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
| Mod+T F M D G S Y R | tile float monocle deck grid stack tab spiral |
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
Built-in bar items: `cpu gpu ram vram` (numbers), `cpu-graph gpu-graph ram-graph vram-graph` (last 30 samples as a mini graph, red above 90%), `disk` (free space) and `disk-bar` (used, as a bar) of the filesystem named by `disk = PATH`.
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
`ctl menu launch` (Mod+W) is the launcher: besides apps it has `power: lock|logout|suspend|hibernate|reboot|poweroff` (`ctl session`) and `backup: NAME` for every executable in `<config dir>/backup/` (one script per backup job). `ctl log [N]` prints the last N log lines (also kept in
`~/.local/state/finedesktop/log`); every action, from a key, `ctl` or a hook, is logged as `action: ...`.

## Tab and stack labels

In `tab`, `stack`, `deck` and `monocle` each window is a label in the bar. `ctl name TEXT` renames the focused one,
`ctl name ID TEXT` a given window (ids are in `ctl state` and in hooks as `FD_WINDOW`), `ctl name -` gives the title
back. A hook can name windows by app: `on-window-open` running `ctl name "$FD_WINDOW" "$FD_APPID"`.
The bar is one text row plus 2 px; `tab_height = N` in the config sets it.

## Own commands, not only `ctl`

Every action is also its own command: `./links` makes `fd-trash`, `fd-notify`, `fd-session`, `fd-screenshot`,
`fd-screenrecord`, `fd-menu`, `fd-name`, `fd-log`, `fd-layout`, ... next to `finedesktop` (symlinks, the binary reads
its own name). `fd-notify hi` is `finedesktop ctl notify hi`. Trash needs no compositor at all.

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
