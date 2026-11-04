# Warp Lite

Server warps for Play Strawberry servers. Staff save named places; players travel to them with a
command or from a menu. Each warp has its own permission, and travel waits a few seconds so a
player cannot escape a fight with it.

## Commands

| Command | What it does | Permission |
| --- | --- | --- |
| `/setwarp <name>` | Saves (or replaces) a warp at your position and facing | `warp-lite:manage` |
| `/delwarp <name>` | Deletes a warp | `warp-lite:manage` |
| `/warp <name>` | Travels to a warp | `warp-lite:use` and `warp-lite:warp.<name>` |
| `/warp`, `/warps menu`, `/warpmenu` | Opens the warp menu | `warp-lite:use` |
| `/warps`, `/warps list` | Lists the warps you can use | `warp-lite:use` |

Warp names are 1 to 32 letters, digits, `_` or `-`, and are not case-sensitive.

## Permissions

| Permission | Default |
| --- | --- |
| `warp-lite:use` | everyone |
| `warp-lite:manage` | operators (level 2) |
| `warp-lite:warp.<name>`, for example `warp-lite:warp.spawn` | not given by the plugin; give it for each warp |

`warp-lite:manage` does not let a player use a warp without that warp's own permission.

## Settings

`plugins/data/warp-lite/config.properties` is created on the first start:

```properties
delay_seconds=3
cooldown_seconds=10
max_warps=128
```

- `delay_seconds` (0 to 60): wait before travel. Moving, taking damage, changing world or leaving
  cancels it; looking around does not.
- `cooldown_seconds` (0 to 3600): wait between two arrivals.
- `max_warps` (1 to 256): the most warps. Lowering it never deletes saved warps.

Restart or reload the plugin after a change. A value out of range stops the plugin from loading
and the console names it.

Warps are saved in `plugins/data/warp-lite/warps.tsv`.

## Safety

Before travel the plugin checks that the destination has a solid floor and room for the player.
If the destination chunk is not loaded, the player is sent to the saved place without that check,
so keep warp platforms intact.

## Languages

English, French, Spanish and Turkish, chosen by each player's game language (`lang/`).

## Install on a Play Strawberry server

1. Add Warp Lite to your server in the Play Strawberry panel.
2. Restart the server. The plugin creates `plugins/data/warp-lite/` on the first start.
3. Give your players the permissions they need.

Play Strawberry servers load only plugins signed by Play Strawberry, so a file you build yourself
from this source loads only on your own Play Strawberry developer server.

## Build from source

You need Rust with the `wasm32-wasip2` target and the Strawberry plugin SDK
(`strawberry-plugin-api`), which Play Strawberry gives to its plugin developers.
`Cargo.toml` takes it from the SDK registry named `strawberry`; add that registry to
your Cargo configuration (for example `~/.cargo/config.toml`):

To get the SDK, accept its licence at https://panel.playstrawberry.net/strawberry/sdk; the page gives you a
token for `cargo login --registry strawberry`.

```toml
[registries.strawberry]
index = "sparse+https://sdk.playstrawberry.net/index/"
```

```text
cargo test
cargo build --release --target wasm32-wasip2
```

The plugin file is `target/wasm32-wasip2/release/warp_lite.wasm`.

## License

Source available under the Strawberry Source-Available License 1.0 ([LICENSE](LICENSE)). In short: you may read, build and
change this code for your own use, and run what you build only on Play Strawberry, without making money from it unless we
approve it in writing. The build Play Strawberry offers may be used on any of your Play Strawberry servers, also ones that
earn money. Selling the code or anything built from it is not allowed. Forks are welcome for sending us changes (pull
requests and issues).
