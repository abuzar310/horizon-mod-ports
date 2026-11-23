# Death Coordinates

An original mod for [Horizon](https://github.com/horizon-rust/horizon), written for Horizon's current mod SDK as a
WebAssembly mod. It is not a port: no earlier mod's code was used.

When you die, it remembers where: the block you died in, the dimension and the world's clock at that moment. A line
goes into chat as the death screen opens:

```
You died at 120, 64, -35 in the Overworld
```

The Overworld, the Nether and the End are named; any other dimension shows its id (`You died at 3, 70, 4 in
mymod:mining`).

`/lastdeath` says it again at any time, with the in-game day (counted as F3's `Day #` counts it) and how long ago it
was on the world's clock:

```
You died at 120, 64, -35 in the Overworld (day 3, 12 min ago)
```

or `No death on record for this world or server yet`.

The last death is kept for each world and each server, so it is still there after you quit the game: a single-player
world by its save folder (renaming the world doesn't lose it), a server by its address (`play.example.com` and
`play.example.com:25565` are the same server). When the game can't say which world or server you are in, one shared
record is used.

"How long ago" counts the time the world has run since the death (its game time), not the clock on your wall: a
single-player world doesn't run while you are away from it, a server keeps running.

## Commands and settings

| | What it does |
| --- | --- |
| `/lastdeath` | Says the last death in this world or on this server, or that there is none. |
| **Announce in chat** (on) | **Options > Mods > Death Coordinates > Config.** The line in chat when you die. Off, the death is still kept for `/lastdeath`. |

The mod is client-side only and gives no advantage on a server: it only tells you where you yourself died.

## Limits

- The death is noticed when the death screen opens. On a server or world with the `immediate_respawn` game rule, no
  death screen opens, so nothing is kept.
- If you leave the game while dead and come back to the death screen, that death keeps its first record (it is not a
  second death). A real second death on the very same block in the first five seconds after joining is taken for the
  same one.
- The lines are in English.
- Switched off in **Options > Mods**, it keeps nothing and leaves `/lastdeath` to the server. Switched on while you
  are in a world, `/lastdeath` works at once, but the chat box suggests it only after you join again.

## Where it keeps its data

`horizon-mod-data/death_coords/deaths/` in the game folder: one small text file per world (`world-<save folder>.txt`)
or server (`server-<address>.txt`), or `global.txt`. Every character but ASCII letters, digits, `_`, `-` and `.` is
written as `%XX` (a space is `%20`), and a name longer than 100 characters keeps its first 80 and ends with a short
hash. Delete a file to forget that world's death.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                       # the chat lines, dimension names, the file format and the world/server keys
cargo horizon-mod build --wasm   # target/horizon-mod/death_coords.hmod
cargo horizon-mod check target/horizon-mod/death_coords.hmod
```

In this repository GitHub Actions builds, checks and tests it.

## Licence

MIT, see `LICENSE`.
