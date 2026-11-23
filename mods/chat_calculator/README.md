# Chat Calculator

A calculator in the chat box. Start a chat message with `=` and it is worked out on your computer and never sent:
only you see the answer, as a line in your own chat.

```text
=2*64+5     ->  2*64+5 = 133 (2 stacks + 5)
=3st+12     ->  3st+12 = 204 (3 stacks + 12)
=1.5sb      ->  1.5sb = 2592 (40 stacks + 32)
=27*64      ->  27*64 = 1728 (27 stacks)
=1/3        ->  1/3 = 0.333333
=2**        ->  Chat Calculator: can't read '2**'
=1/0        ->  Chat Calculator: can't divide by zero in '1/0'
```

It is a client-side convenience, so it works in single player and on any server, and the server never sees the sum.

## What it reads

| | |
|---|---|
| Numbers | `12`, `1.5`, `.5` |
| `+ -` | add, subtract |
| `* / %` | multiply, divide, remainder (`7.5%2` is `1.5`, `-7%3` is `-1`) |
| `^` | power, right to left: `2^3^2` is `2^9` = 512 |
| `-x` | minus: `-2^2` is -4, `2^-3` is 0.125, `2*-3` is -6 |
| `( )` | brackets |
| `st` | a stack: 64 items (`3st` is 192) |
| `sb` | a shulker box full of stacks: 1728 items |
| `dc` | a double chest full of stacks: 3456 items |

`*`, `/` and `%` go before `+` and `-`, and `^` before them all. A unit word goes right after a number or a bracket,
with or without a space, in any case: `3st`, `3 ST`, `(1+1)dc`. Spaces anywhere else are fine too.

## The answer

Up to 6 decimals, without trailing zeros: `0.333333`, `2.5`, `133`. A whole answer of 64 or more also shows its stacks:
`1 stack`, `27 stacks`, `2 stacks + 5`. From 10^15 on the answer is written as `1.152922e18`; past the largest number
a 64-bit float holds it is "too big". Dividing by zero, and a power with no real answer such as `(-8)^0.5`, give a short
line instead of an answer.

## Sending a message that starts with `=`

- A message starting with `==` is sent as you typed it (`==> look here` goes out as is).
- A lone `=` is sent as normal text.
- Anything else after one `=` is the calculator's. A message it can't read, such as `=)` or `=D`, gets the
  "can't read" line and is **not** sent: write `==)` if you meant to send a face.

The sum you typed stays in the chat box's history (the up arrow brings it back), as any message does. With the
**Chat** option at "Commands Only" or "Hidden" the game drops chat before mods hear it, so the calculator does nothing.

## Settings

**Options > Mods > Chat Calculator:** the mod's on/off switch, and **Show stacks** (on): the `(2 stacks + 5)` part of
an answer.

## Build

With [`cargo horizon-mod`](https://github.com/horizon-rust/horizon/tree/main/docs/modding), on the SDK commit
`Cargo.toml` pins:

```sh
cargo test                       # the parser, precedence, units, errors and the answer's wording
cargo horizon-mod build --wasm   # target/horizon-mod/chat_calculator.hmod
cargo horizon-mod check target/horizon-mod/chat_calculator.hmod
```

## Credit

An original mod, written for Horizon's current mod SDK; it is not a port of another mod.

## Licence

MIT, see `LICENSE`.
