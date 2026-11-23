# Pickup Log

A short list in a corner of the screen of what you just picked up: each line is the item's icon and how many you got,
such as `+12 Cobblestone`.

- **One line per item.** Picking up more of an item adds to its line (`+12` becomes `+20`), makes it the newest line
  again and starts its time again.
- **It fades.** A line stays a few seconds (4 by default), then fades and slides out over half a second.
- **Newest at the edge.** The newest line is the one nearest the edge of the screen; at most 6 lines show by default,
  and a new item past that pushes out the oldest line.
- **The item's own name.** A renamed item shows its custom name, as its tooltip does.

It gives way to the debug screen (F3) in the corner F3 takes, as the game's own corner panels do.

## How a pickup is noticed

When an item on the ground is picked up, the server sends `minecraft:take_item_entity`. Once each client tick (20
times a second) the mod counts your items by kind: the 36 main inventory slots, the off hand and the four armour
slots. A kind that grew within half a second of that packet (or up to two ticks before it, as the two can arrive in
either order) is a pickup and gets a line. Counting by kind means moving an item, swapping hands or putting on armour
changes nothing, and a tool wearing down or being repaired is not a pickup.

Some limits that come from what the game tells mods:

- The packet does not say who picked the item up. A gain right after **someone else** near you picks something up
  (another player, a mob, an arrow or experience being collected) can be listed too, for example items given to you
  by a command at that moment.
- Nothing is listed while a **screen is open** (your inventory, a chest, chat, the pause menu): there your items
  change by hand. A pickup made while one is open is not listed.
- Experience orbs are not listed: they change no item count.

With **Every new item** on, anything new in your inventory is listed, picked up or not (items from `/give` or a
kit), except in the first half second after joining or changing dimension, while the inventory is still arriving.
Still nothing is listed while a screen is open, but a stack you were holding on the cursor when you close a screen
goes back into your inventory after it closes, so it can get a line too.

It reads only your own inventory, which your client already has, and sends nothing: it gives no edge and works on any
server.

## Install

Once it is listed in the Horizon mod registry: in the Horizon launcher, **Mods**, then **Horizon**, find Pickup Log,
**Install**. It is a WebAssembly mod, so nothing is built on your computer.

## Settings

**Options > Mods > Pickup Log > Config:**

| Setting | Default | What it does |
| --- | --- | --- |
| Corner | Bottom right | Where the list shows. |
| Seconds shown | 4 (1 to 10) | How long a line stays before it fades. |
| Most lines | 6 (1 to 10) | How many lines show at once. |
| Item icons | On | The item's icon beside each line. |
| Background | On | A dark box behind each line. |
| Every new item | Off | List every new item, not only ones picked up from the ground. |

The mod's own switch in the mod list turns it off altogether. It has no keys and no commands.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                       # counting, the pickup window, the list, fading and the layout
cargo horizon-mod build --wasm   # target/horizon-mod/pickup_log.hmod
cargo horizon-mod check target/horizon-mod/pickup_log.hmod
```

In this repository GitHub Actions builds, checks and tests it.

## Credit

A new WebAssembly rewrite of "Pick Up Notifier" by DeportedToHell
(<https://github.com/DeportedToHell/MCRE-Mods/tree/main/pickupnotifier>, MPL-2.0), which was made for Horizon's
earlier mod system and is archived in the mod registry (`archive/native-v1/mods/pickupnotifier.toml`), for Horizon's
current mod SDK; no code was copied. That port is itself of Fuzs' original
[Pick Up Notifier](https://github.com/Fuzss/pickupnotifier) (MPL-2.0), the idea this mod follows. No code or assets of
either were used. Not affiliated with Fuzs, DeportedToHell, Mojang or Microsoft.

## Licence

MIT, see `LICENSE`. `NOTICE` repeats the credit above.
