# Hotbar Numbers

An original mod for [Horizon](https://github.com/horizon-rust/horizon), written for Horizon's current mod SDK as a
WebAssembly mod. It is not a port: no earlier mod's code was used.

In each of the nine hotbar slots it draws the key that selects that slot, as you bound it in **Options > Controls**
(`key.hotbar.1` to `key.hotbar.9`). The key shows small, in a corner of the slot, with a shadow, right after the game
draws the hotbar. If you moved slot 1 to `R` or slot 5 to a mouse button, the hotbar shows it.

A key name that is too wide for a slot (18 GUI pixels) is shortened: every word but the last gives its first letter,
then the last word follows, cut from its end until it fits. `Left Shift` becomes `LSh`, `Keypad 1` becomes `K1`,
`Button 4` becomes `B4` and `Backspace` becomes `Bac`. An unbound slot shows nothing.

Nothing is drawn in spectator mode (the hotbar is the teleport menu there and its number keys pick from it), or while
the HUD is hidden (F1). The names are read from the game twice a second, so a key you rebind shows as soon as you are
back in the world.

## Settings

**Options > Mods > Hotbar Numbers > Config:**

| Setting | Default | What it does |
| --- | --- | --- |
| Shown | on | Draws the keys. |
| Only rebound keys | off | When on, a slot shows its key only when it is not the usual `1` to `9`. |
| Corner | Top left | Top left or bottom right of the slot. Bottom right is where the game draws a stack's count, so the key covers the count there. |
| Colour | white (`#FFFFFF`) | The colour of the key names. |

The mod has no keys and no commands of its own. It is client-side only and gives no advantage on a server: it shows
your own key bindings.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), from the SDK commit
`Cargo.toml` pins:

```sh
cargo test                        # shortening, slot geometry, plain key names, settings
cargo horizon-mod build --wasm    # target/horizon-mod/hotbar_numbers.hmod
cargo horizon-mod check target/horizon-mod/hotbar_numbers.hmod
```

In this repository GitHub Actions builds, checks and tests it.

## Licence

MIT, see `LICENSE`.
