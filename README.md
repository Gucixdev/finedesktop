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
| Mod+Return / Mod+W | terminal (`$TERMINAL`, default foot) / menu (`$FD_MENU`) |
| Mod+P | launcher (dmenu-style: apps + PATH, Left/Right = back/in, unmatched text runs as typed) |
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
A theme is a separate setting from a mode: a theme is how it looks, a mode is how the system behaves.

## Menu and logs

`Mod+W` opens one centred list: Aplikacje (Natywne, Flatpak and Gry as sections of the same list), Gry, Schowek,
Kalkulator, Ustawienia. Type filters, Left goes back (clears the query first), Right/Enter goes in.
`ctl menu launch` is the launcher. `ctl log [N]` prints the last N log lines (also kept in
`~/.local/state/finedesktop/log`); every action, from a key, `ctl` or a hook, is logged as `action: ...`.
