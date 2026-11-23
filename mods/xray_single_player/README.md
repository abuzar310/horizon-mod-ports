# X-Ray (single player)

A new WebAssembly rewrite of X To Xray by RealCat1st ([github.com/RealCat1st/XToXrayRust](https://github.com/RealCat1st/XToXrayRust),
Apache-2.0) for Horizon's current mod SDK; no code was copied. The original was made for Horizon's earlier mod system and
is archived in the registry as `archive/native-v1/mods/xtoxray.toml`. This rewrite is MIT licensed.

Press **X** to see ores through the ground, **in single player only**. Press X again to see the world as usual.

While X-ray is on, the common blocks the ground is made of draw nothing, so what is left among them stands out:
ores, chests, spawners, water and lava, and the caves, dungeons and buildings around them.

## Single player only

X-ray works only in your own single-player world, while you are alone in it. It checks this when you press X, on every
game tick while X-ray is on, and when a world or dimension loads. When the check fails, X-ray turns off and says once in
chat: *X-Ray works only in single player*.

- **Servers:** X-ray never turns on. Pressing X only shows that line.
- **A world opened to LAN:** while you are alone in it, the SDK can't tell it apart from a closed single-player world,
  so X-ray keeps working. As soon as anyone else joins, X-ray turns off; it turns on again only when you press X after
  you are alone again.
- **Splitscreen guests** don't own the world, so X-ray doesn't turn on for them.

How the check works: the mod asks the game whether this window runs the world's own (integrated) server from a save
folder, and whether the Tab list holds only you. Both must be true. A Tab list that holds nobody yet (the first moments
after you open the world) counts as you alone, since no one can join a world before its owner.

## What is hidden

Only filler blocks are hidden. The mod attaches a block part to them that answers the game's `block_render_visibility`
hook, so the game draws them as air. Light, collision and the server are untouched: you still walk on the ground you
can't see.

| Hidden | How the mod names them |
| --- | --- |
| Stone, granite, diorite, andesite, tuff, deepslate | the game's tag `minecraft:base_stone_overworld` |
| Netherrack, basalt, blackstone | `minecraft:base_stone_nether` |
| Dirt, coarse dirt, rooted dirt | `minecraft:dirt` |
| Grass block, podzol, mycelium | `minecraft:grass_blocks` |
| Mud, muddy mangrove roots | `minecraft:mud` |
| Moss block, pale moss block | `minecraft:moss_blocks` |
| Crimson and warped nylium | `minecraft:nylium` |
| Terracotta of every colour | `minecraft:terracotta` |
| Calcite, dripstone block, smooth basalt, gravel, sand, red sand, sandstone, red sandstone, clay, end stone, soul sand, soul soil | by block id |

The tags are read from the game's own data when a world opens, so the lists above are what Minecraft 26.3 puts in them;
a data pack that changes those tags changes what is hidden.

Never hidden: ores (including ancient debris), chests of every kind, spawners and trial spawners, vaults, suspicious
sand and gravel, water and lava. The `minecraft:sand` tag is not used, because it holds suspicious sand.

There is **no full brightness**: the SDK gives a WebAssembly mod no call for gamma or night vision, so caves stay as dark
as they are. A torch, night vision or the game's own brightness slider still help.

Turning X-ray on or off makes the game rebuild the loaded terrain once, which takes the game a few seconds.

## Key and settings

| Key (Options > Controls, under X-Ray (single player)) | Default | What it does |
| --- | --- | --- |
| Toggle X-ray | `X` | Turns X-ray on or off. Holding the key flips it once. |

In creative, `X` is also the game's **Load Hotbar Activator**; rebind either key if you use that.

| Setting (Options > Mods > X-Ray (single player) > Config) | Default | What it does |
| --- | --- | --- |
| Remember state | off | Off: X-ray starts off in every world you open. On: if X-ray was on, it turns back on in the next single-player world you open, until you quit the game. It never turns on on a server or in a LAN world someone else is in. |

Switching the mod off in **Options > Mods** turns X-ray off at once.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                        # the single-player lock, the toggle, the key, the hidden list, the settings
cargo horizon-mod build --wasm    # target/horizon-mod/xray_single_player.hmod
cargo horizon-mod check target/horizon-mod/xray_single_player.hmod
```

In this repository GitHub Actions builds, checks and tests it.

## Licence

MIT, see `LICENSE`. The original X To Xray by RealCat1st is Apache-2.0; see `NOTICE`. No code from it was copied.
