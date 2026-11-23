# Sprint & Sneak Toggle

A new WebAssembly rewrite of Toggle Sprint by CayMac97 ([github.com/CayMac97/toggle-sprint](https://github.com/CayMac97/toggle-sprint),
MIT) for Horizon's current mod SDK; no code was copied. The original was made for Horizon's earlier mod system and is
archived in the registry as `archive/native-v1/mods/toggle_sprint.toml`.

Press a key once to keep sprinting, or to keep sneaking, instead of holding the game's sprint or sneak key. Press it
again to stop.

While a toggle is on, the mod holds the game's own **Sprint** or **Sneak** key down for you, every tick. The game then
moves you exactly as it does when you hold the key yourself, with all of its own rules: you sprint only while you
walk forward and are not too hungry, sneaking keeps you from falling off edges, and the server sees an ordinary
client. Nothing is faster than holding the key.

- **Screens pause it.** Opening your inventory, a chest or any other screen lets go of the key, as the game always
  does, so you stand up while you sort items (Shift-click works as usual). Close the screen and the toggle holds the
  key again.
- **Dying or leaving the world turns both toggles off.** After you respawn or join again, press the key to turn one
  back on. Switching the mod off in **Options > Mods** turns both off too.
- **The game's own toggle options.** If you turned on the game's **Sprint: Toggle** or **Sneak: Toggle** in
  **Options > Controls**, the mod still works: it only presses the key when it is up, and turns it off with one press
  when its toggle turns off.

## The status line

A small line in a corner says what you are doing, as the classic ToggleSprint HUD did:

| Line | When |
| --- | --- |
| `[Flying]` | You fly (creative or spectator). |
| `[Gliding]` | You glide on elytra. |
| `[Sneaking (Toggled)]` | Sneak is toggled on (by this mod, or by the game's own toggle option). |
| `[Sneaking (Key Held)]` | You hold the sneak key yourself. |
| `[Sprinting (Toggled)]` | Sprint is toggled on (by this mod, or by the game's own toggle option). |
| `[Sprinting (Key Held)]` | You hold the sprint key yourself. |

Only the first that applies shows, in that order; with nothing on, there is no line. `[Sprinting (Toggled)]` means
the toggle is on, so it shows while you stand still too, as it did in the classic HUD.

The original also had `[Swimming]` and `[Riding]`. A client mod cannot read those states on Horizon's SDK (whether you
swim, sprint, crouch or ride is only readable on the server's side), so this version leaves them out rather than guess.
It also leaves out the original's creative fly speed boost: that changes how fast you move, which a WebAssembly mod
cannot do and which would be an edge over other players on a server.

The line gives way to the debug screen (F3) when F3 takes its corner.

## Keys and settings

| Key (Options > Controls, under Sprint & Sneak Toggle) | Default | What it does |
| --- | --- | --- |
| Toggle sprint | `V` | Turns sprint on or off. |
| Toggle sneak | `C` | Turns sneak on or off. |

Holding the key down flips the toggle once. In creative, `C` is also the game's **Save Hotbar Activator**; rebind
either key if you use that.

| Setting (Options > Mods > Sprint & Sneak Toggle > Config) | Default | What it does |
| --- | --- | --- |
| Status line | on | Shows the status line. |
| Corner | Top left | Where the line shows: any corner of the screen. |

The mod is client-side only and gives no advantage on a server: it presses the same keys you could hold yourself.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                        # the toggles (press, release, screens, death), the status lines, keys, settings
cargo horizon-mod build --wasm    # target/horizon-mod/sprint_sneak_toggle.hmod
cargo horizon-mod check target/horizon-mod/sprint_sneak_toggle.hmod
```

In this repository GitHub Actions builds, checks and tests it.

## Licence

MIT, see `LICENSE`. The original Toggle Sprint by CayMac97 is MIT too.
