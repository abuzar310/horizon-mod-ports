# Light Levels

A new WebAssembly rewrite of Light Overlay by CayMac ([github.com/CayMac97/light-overlay](https://github.com/CayMac97/light-overlay),
MIT) for Horizon's current mod SDK; no code was copied. The original was made for Horizon's earlier mod system and is
archived in the registry as `archive/native-v1/mods/light_overlay.toml`. This rewrite is MIT licensed.

Press **F7** to mark the blocks around you where hostile mobs can spawn. Press F7 again to hide the marks.

- **Red**: the spot is dark by day too (sky light below 8), as in caves and rooms. Mobs can spawn there at any time.
- **Yellow**: the spot is under open sky (sky light 8 or more). Mobs spawn there only at night.

A spot is marked when its block light is at most the **Light limit** (0 by default). The marks sit on the top face of the
floor block and are hidden behind anything nearer, the same as the world itself. Nothing is drawn through walls.

## What counts as a spawn spot

A cell is a spawn spot when the block below it is a floor a monster can stand on, and a mob's feet fit in the cell itself:

| | The mod's test |
| --- | --- |
| **Floor** | The block has collision; its outline covers its whole top face (a full block, a top slab or upside-down stairs, but not a bottom slab, upright stairs or a cauldron's open top); it gives off less than light 14; it is not in the game's `minecraft:leaves`, `minecraft:ice` or `minecraft:impermeable` (glass) tags; and it is not a barrier or a magma block. |
| **Feet** | The block has no collision (air, grass, flowers, one layer of snow; a carpet has collision and blocks spawns); it holds no water or lava; and it is not in the `minecraft:prevent_mob_spawning_inside` (rails), `minecraft:buttons` or `minecraft:pressure_plates` tags. |

The tags are read from the game when you play, so a data pack that changes them changes the marks too. A mark on one
layer of snow (or another cover up to a quarter block high) sits on top of the cover; a taller plant's mark stays on
the floor.

### It is an approximation

The mod uses the general monster rule. These cases differ from the game:

- **Per-mob rules are left out:** mushroom fields, slime chunks, structures and biomes with their own spawns, and mobs
  that spawn on leaves (ocelots, parrots), ice (polar bears) or magma (magma cubes and striders in the Nether).
- **Head room is not checked.** A spot with a block right above it is still marked, though a two-block-tall mob such as
  a zombie can't spawn there. Spiders and other small mobs can.
- **Honey and soul sand:** the floor test reads the block's outline, which is a full cube for both. Soul sand really is a
  spawn floor; honey blocks may be marked where mobs can't spawn.
- **The ice tag** holds every ice block, so packed and blue ice are never marked, though the game may let mobs spawn on
  them.
- **Redstone parts** in a mob's feet (redstone dust, levers, tripwire hooks, redstone torches) stop spawns in the game
  but are not in the tags above, so a dark spot holding one can be marked.
- **Glass:** the `minecraft:impermeable` tag holds glass, stained glass and tinted glass. A block that refuses spawns for
  a reason of its own, is in none of the tags and still passes the floor test is marked.

The light is the stored light the client already holds, the same values F3 shows. It updates as the client relights
blocks: a torch placed or broken shows within a second.

## Key and settings

| Key (Options > Controls, under Light Levels) | Default | What it does |
| --- | --- | --- |
| Show light levels | `F7` | Shows or hides the marks. Holding the key flips them once. |

Horizon leaves F7 unbound, so it clashes with nothing until you bind something else to it.

| Setting (Options > Mods > Light Levels) | Default | What it does |
| --- | --- | --- |
| Mark style | Cross | **Cross**: an X on the block. **Square**: a see-through square. **Number**: the spot's block light, turned to read upright from where you face. |
| Radius | 12 | How far around you blocks are marked, 4 to 24 blocks across the ground. |
| Height | 8 | How far above and below your feet blocks are marked, 2 to 16 blocks. |
| Light limit | 0 | The most block light a marked spot can have. 0 is today's rule in the Overworld and the End; 7 for the Nether or servers older than Minecraft 1.18. |
| Day and night colours | on | On: red and yellow as above. Off: every mark is red. |
| Red colour | `#FF3030` | The colour of day-and-night spots. |
| Yellow colour | `#FFD030` | The colour of night-only spots. |
| Line width | 2 | How thick crosses and numbers are, 1 to 4 pixels. |
| Remember state | off | Off: the marks start hidden in every world you join. On: if you left them shown, they show again in the next world you join, until you quit the game. Changing dimension never hides them. |

Switching the mod off in **Options > Mods** hides the marks at once.

## How it works

While the marks are shown, the mod sweeps the box around you column by column, nearest columns first, reading each
column from the top down so every block is read once. It reads at most about 4,096 blocks and light values a game tick,
so the widest box (radius 24, height 16) takes about 16 ticks. When a sweep finishes its marks replace the last ones,
and the next sweep starts once you have moved a block, or after a second if you stand still. What each kind of block
is (floor, room for feet, or neither) is worked out once per block state and kept until you change world or dimension.

At most 1,024 marks are drawn, the nearest first (256 in the Number style, whose digits are up to 14 lines a mark), and
only as many whole marks as the frame has room for beside other mods' drawings.

It only reads blocks and light the client already holds. Nothing is sent to the server, and nothing is done for you:
it is the same information F3 shows, drawn where the blocks are.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                       # the floor test, the colours, the sweep, the key, the marks' shapes, the settings
cargo horizon-mod build --wasm   # target/horizon-mod/light_levels.hmod
cargo horizon-mod check target/horizon-mod/light_levels.hmod
```

In this repository GitHub Actions builds, checks and tests it.

## Licence

MIT, see `LICENSE`. The original Light Overlay by CayMac is MIT licensed too. No code from it was copied: only its
registry description was read, to know what it did.
