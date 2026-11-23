# Speedrun Clock

A real-time (RTA) speedrun timer in a corner of the screen, with live milliseconds: `12:34.567`, and `01:23:45.678`
once an hour has passed.

- **Starts** on your first move after a world loads: a step or a jump away from where you appeared. Falling onto the
  ground at spawn, or the game putting you at the spawn point, does not start it. Until then the timer waits in grey
  at `00:00.000`.
- **Keeps counting** through deaths, respawns and changes of dimension, and while the game is paused: it is real time.
- **Stops** when the end credits open, as you leave the End. The credits from Options > Credits & Attribution don't
  stop it.
- **Resets** when you leave the world, ready for the next run.
- **Keeps** every finished run, and your personal best. The best shows in grey under the timer, and a finished run that
  beats it turns green (your first finished run sets the first best). A tie is not a new best.

The game shows the credits only the first time a player leaves the End in a world, so a world you have already beaten
never stops the timer on its own: use `/speedrun stop`, or start a new world.

It is a client-side display of your own run, so it works in single player and on any server.

## Install

Once it is listed in the Horizon mod registry: in the Horizon launcher, **Mods**, then **Horizon**, find Speedrun
Timer, **Install**. It is a WebAssembly mod, so nothing is built on your computer.

## Settings

**Options > Mods > Speedrun Clock > Config:** Shown (on), Corner (top right), Show personal best (on). The timer keeps
counting while hidden, hides with the HUD (F1), and gives way to the debug screen (F3) in the corners it takes.

## Commands

| Command | What it does |
|---|---|
| `/speedrun` | The time, whether it is running, and your best, in chat. |
| `/speedrun start` | Starts a run from zero now, without waiting for a move. |
| `/speedrun stop` | Stops the timer. A run stopped by hand is not kept and never becomes your best. |
| `/speedrun reset` | Back to `00:00.000`: the timer starts again on your next move. |
| `/speedrun best` | Your personal best and how many finished runs are kept. |

## Files

In the game folder, `horizon-mod-data/speedrun_clock/`:

- `best.txt`: your personal best, one line: the milliseconds, then the time as the timer shows it
  (`754321 12:34.321`). Delete it to start over with no best.
- `runs.txt`: every finished run, one line each in the same form, oldest first.

Lines that don't start with a number are skipped, so the files can be edited by hand.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                       # the time format, the run's states, the best and the files' format
cargo horizon-mod build --wasm   # target/horizon-mod/speedrun_clock.hmod
cargo horizon-mod check target/horizon-mod/speedrun_clock.hmod
```

## Credit

A new WebAssembly rewrite of Speedrun Timer by Demerobit
([Demerobit/Speedrun-Timer-Horizon-Rust-Minecraft](https://github.com/Demerobit/Speedrun-Timer-Horizon-Rust-Minecraft),
MIT) for Horizon's current mod SDK; no code was copied. The original was made for Horizon's earlier mod system and is
archived in the registry's `archive/native-v1/mods/speedrun_timer.toml`.

## Licence

MIT, see `LICENSE`.
