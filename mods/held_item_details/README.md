# Held Item Details

A few short lines, centred above the hotbar, about the item in your selected hotbar slot:

- **Enchantments:** up to six, named in your language with their levels as the game writes them (`Sharpness V,
  Unbreaking III, Mending`), an enchanted book's stored ones too. They are listed in the order of the game's
  enchantment registry (not always the item tooltip's order); more than six end in `, ...`, and a line wider than the
  screen is cut with `...`.
- **Count:** `x37` for a stack, and `x37 (214 in inventory)` when your inventory holds more of the same item (same item
  and same components) than the stack in your hand. Only the 36 main inventory slots (the hotbar included) are
  counted, not the offhand or the armour slots.
- **Durability:** `Durability 123 / 250`, what is left out of the most, for items that wear out. It turns yellow under a
  quarter and red under a tenth. Unbreakable items show none.

The lines sit above the place where the game shows the item's name and the action bar message, so they never cover
either. Nothing shows for an empty hand or in spectator mode, and nothing while the HUD is hidden (F1). The item is
read once a client tick (20 times a second). It is a client-side display of what your own client already knows (your
own held stack and inventory, nothing about other players or the world), so it gives no edge and works on any server.

## Install

Once it is listed in the Horizon mod registry: in the Horizon launcher, **Mods**, then **Horizon**, find Held Item
Info, **Install**. It is a WebAssembly mod, so nothing is built on your computer.

## Settings

**Options > Mods > Held Item Details > Config:** Shown (on), Durability (on), Count (on), Enchantments (on).

The mod has no keys and no commands of its own.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                       # the lines, their colours, roman numerals, the shortening and the count
cargo horizon-mod build --wasm   # target/horizon-mod/held_item_details.hmod
cargo horizon-mod check target/horizon-mod/held_item_details.hmod
```

In this repository GitHub Actions builds, checks and tests it.

## Credit

A new WebAssembly rewrite of Held Item Info by AltMugen
([AltMugen/horizon-held-item-info](https://github.com/AltMugen/horizon-held-item-info), MIT) for Horizon's current
mod SDK; no code was copied. The original was made for Horizon's earlier mod system and is archived in the registry's
`archive/native-v1/mods/held_item_info.toml`.

## Licence

MIT, see `LICENSE`.
