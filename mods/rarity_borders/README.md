# Rarity Borders

A client-side mod for [Horizon](https://github.com/horizon-rust/horizon), a WebAssembly mod for Horizon's current mod
SDK.

Every slot of your inventory and of the container you have open (chests, furnaces, hoppers, stations and mods'
windows) that holds an **uncommon**, **rare** or **epic** item gets a thin border in the colour the item's name has:

| Rarity | Colour |
| --- | --- |
| Uncommon | yellow (`#FFFF55`) |
| Rare | aqua (`#55FFFF`) |
| Epic | light purple (`#FF55FF`) |
| Common (when turned on) | white (`#FFFFFF`) |

The border is a one-pixel outline on the slot's own frame, in the style of the game's pixel outlines: the top and
sides a little darker, the bottom edge the full, brighter colour. On a container screen it is drawn under the item, so
the item, its count, the hover highlight and the tooltip all stay on top. The same borders go round the nine hotbar
items while you play.

The rarity is the one the game gives the item, so it matches the colour of the item's name: the stack's own
`minecraft:rarity` component if it has one, else the item's default, else common. An enchanted item is raised one step
as in vanilla: a common or uncommon item becomes rare, a rare item becomes epic.

Where nothing is drawn:

- the creative inventory, and the survival inventory while the recipe book covers it (the game gives a mod no slot
  positions there);
- the off-hand slot next to the hotbar (a mod can't read which side it is on);
- the hotbar in spectator mode (it is the teleport menu there) and while the HUD is hidden (F1).

## Settings

**Options > Mods > Rarity Borders > Config:**

| Setting | Default | What it does |
| --- | --- | --- |
| Shown | on | Draws the borders. |
| Common items | off | Also draws a white border round slots holding common items. |
| Hotbar | on | Also draws the borders on the hotbar. |

The mod has no keys and no commands. It only colours your own slots, so it gives no advantage on a server.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), from the SDK commit
`Cargo.toml` pins:

```sh
cargo test                        # rarity names, colours, border and hotbar geometry, settings
cargo horizon-mod build --wasm    # target/horizon-mod/rarity_borders.hmod
cargo horizon-mod check target/horizon-mod/rarity_borders.hmod
```

In this repository GitHub Actions builds, checks and tests it.

## Credit

A new WebAssembly rewrite of Rarity Borders by maliferen (listed author Xariami)
(<https://github.com/maliferen/horizon-rarity-borders>, MIT), which was made for Horizon's earlier mod system and is
archived in the mod registry (`archive/native-v1/mods/rarity_borders.toml`), for Horizon's current mod SDK; no code was
copied.

## Licence

MIT, see `LICENSE`.
