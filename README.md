# Horizon mod ports

Client mods for [Horizon](https://github.com/horizon-rust/horizon) (Minecraft: Java Edition 26.3 in Rust), written as
WebAssembly mods on Horizon's mod SDK and published through the reviewed [Horizon mod registry](https://github.com/horizon-rust/horizon-mods).
Install them in the Horizon launcher: **Mods**, then **Horizon**.

Many are rewrites of mods that existed for Horizon's earlier mod system and were frozen when it ended (see the
registry's `archive/native-v1/`). Each folder credits the mod it follows and keeps that mod's licence.

## Layout

| Path | What |
| --- | --- |
| `mods/<id>/` | One mod: `horizon-mod.toml`, `Cargo.toml` (pinned SDK), `src/lib.rs`, `assets/`, `LICENSE`, `README.md` |
| `ci/keystrokes/` | Horizon's official Keystrokes 3.0.1, unchanged ([source](https://github.com/horizon-rust/horizon-mods/tree/main/native/keystrokes), MIT): it must build, pass `cargo horizon-mod check` and its tests here, which proves this pipeline. Never released from here. |
| `.github/workflows/build.yml` | Builds, checks and tests every mod; a tag `<id>-v<version>` publishes `<id>-<version>.hmod` as a release |

## Build

Every build runs on GitHub Actions with `cargo horizon-mod` from the exact SDK commit each mod pins:

```sh
cargo horizon-mod build --wasm    # target/horizon-mod/<id>.hmod
cargo horizon-mod check target/horizon-mod/<id>.hmod
cargo test
```

## Licence

Each mod folder has its own `LICENSE`. Code written here is MIT unless the folder says otherwise.
