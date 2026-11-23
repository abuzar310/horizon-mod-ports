# Block Outline

Outlines the block you are looking at in a colour, opacity and thickness you pick, instead of the game's thin black
line. Cyan at 80% and 2 pixels wide until you change it.

The outline shows exactly when the game's own would: not with the HUD hidden (F1), not while you look at an entity,
and not in a game mode that can't build there. It is a client-side look only, so it works in single player and on any
server, and nothing is sent.

## What it outlines

- The block's own outline shape, the same boxes the game outlines (a slab is half a block, a fence post a thin post
  one block tall). Each box gets its 12 edges, pushed out by 0.002 of a block so they don't flicker against the
  block's faces, and hidden behind anything nearer, as the game's are.
- A block made of several boxes (stairs, a hopper, a cauldron) shows the edges of each box, so you may see a line
  where two of its boxes meet.
- Blocks you can walk through (torches, flowers, grass, buttons, rails, signs) keep the game's black outline. Many of
  them sit at a random spot inside their block, which the outline shape the mod reads doesn't include, so the game's
  own outline is the right one there. So does a block whose solid part sits off its outline's centre, and a shape of
  more than 32 boxes. Bamboo and pointed dripstone, the solid blocks the game also moves at random inside their
  block, are of that kind: their solid part moves with them, so they keep the game's outline too.
- The outline follows the block's state (a door open or shut, stairs turned), not the world around it: a scaffolding
  block while you hold scaffolding, or a shulker box open upwards or downwards, is outlined as its plain shape.
- The mod's lines are drawn after water and other see-through blocks, so a glass or ice block's outline isn't washed
  out by the block itself. The other side of that: a block you look at through the surface of water (down into a
  pond or the sea) can get no outline at all, since the water hides the mod's lines and the game's are turned off.
- Horizon's own **PvP & HUD** page has a block outline option, and Accessibility has a high-contrast outline. Neither
  applies to a block this mod outlines; turn the mod off to use them.

## Settings

**Options > Mods > Block Outline:** the mod's on/off switch (off gives back the game's outline), and

| Setting | Default | |
|---|---|---|
| **Colour** | cyan (`#00FFFF`) | the outline's colour |
| **Opacity** | 0.8 | from 0.1 to 1, in steps of 0.05 |
| **Thickness** | 2 | from 1 to 4 pixels on the screen, in steps of 0.5 |

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                       # a box's 12 edges, growing a box, the colour, which blocks it takes over
cargo horizon-mod build --wasm   # target/horizon-mod/block_outline.hmod
cargo horizon-mod check target/horizon-mod/block_outline.hmod
```

## Credit

An original mod, written for Horizon's current mod SDK; it is not a port of another mod.

## Licence

MIT, see `LICENSE`.
