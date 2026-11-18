# Working on this Horizon mod (SDK package, `api = "0.2"`)

> **Build slots (v2):** every build/test/lint step goes through the campaign runner
> (`../tools/build-slot.sh`), which **blocks in the kernel** until a slot is free — never
> poll, never background a build. It **forwards compile steps to the remote box**, whose
> root is not this campaign's, so wrap the command and run locally:
>
> ```sh
> BUILD_LOCAL=1 ../tools/build-slot.sh run --job incontrol-sdk --port ports/incontrol \
>     --step test -- bash -c 'cd ports/incontrol && CARGO_INCREMENTAL=0 cargo test --offline'
> ```
>
> (and the `cd` must be inside the wrapped command: the remote root has no `ports/`).
> Read `../_shared/build-slots-usage.md` before running anything. Observe with `status`,
> `watch`, `events`. Never pass `--target-dir`, never set a private `BUILD_SLOTS_DIR`.

This port is **one flat `.hmod`** for Horizon 26.3 at the pin in `horizon-mod.toml`:

```text
horizon-mod.toml   the only manifest and the only identity (id, version, api, side, [native] pins)
Cargo.toml         the crate; its only dependency is horizon-mod-api from .horizon/sdk/
src/lib.rs         the one #[mod_api::horizon_mod] impl Hooks (glue only)
src/runtime.rs     the pure spawn.json/loot.json engines and their tests
tools/e2e.sh       the in-game scenario (needs a launcher-built game)
incontrol.hmod     the delivered package
```

Rules that bite:

- **`src/runtime.rs` is byte-identical to the retired `.hnmod` file** (sha256
  `8dc2ef1c…1638`; the 0.1.0 copy is in
  `../_shared/evidence/incontrol/retired-hnmod-0.1.0/mixins/incontrol/runtime.rs`). Do not
  reformat it: `cargo fmt` would rewrite three over-wide lines, so `src/lib.rs` declares
  `#[rustfmt::skip] pub mod runtime;`. Prove any edit with `cmp` and with
  `unzip -p incontrol.hmod src/runtime.rs | sha256sum`.
- **All behaviour logic goes in `runtime.rs`**, with a test. `src/lib.rs` only translates
  between SDK types and the runtime's; if you are tempted to compute something there, it
  belongs in the runtime.
- **No logger.** Diagnostics are `/incontrol status` and the `/incontrol reload` warning
  lines; keep their text stable (the e2e greps it).
- **The rule files are the mod's own config folder** (`Host::config_files()`):
  `ConfigPath::new("incontrol/spawn.json")` is upstream's `config/incontrol/spawn.json`;
  a bare `spawn.json` would be `config/spawn.json` and be refused as unowned.
- **Run the SDK ladder, not the retired one** (there is no `tools/horizon.py`):
  ```sh
  cargo test --offline
  cargo clippy --all-targets --offline -- -D warnings
  cargo fmt --check          # NOT --all: that formats .horizon/sdk
  rm -f Cargo.lock           # a packed lock changes the artifact's sha256
  cargo horizon-mod build    # -> target/horizon-mod/incontrol.hmod
  cargo horizon-mod check incontrol.hmod
  cargo horizon-mod fits incontrol.hmod --build nightly
  ```
  Copy the built file to `./incontrol.hmod`: the deliverable lives in the port, never in
  `target/`, and `target/` is cleaned after a verified job.
- **No in-game claim without a launcher build.** The mod's code is compiled into the
  game, so `tools/e2e.sh` needs `GAME=<horizon-launcher native-build …>`; until that run
  exists, `notes.md` records the gap.
- Keep the port separate from `horizon/` and `horizon-mixins/` (read-only pins), and do
  not add Mojang files or player data. Licence: MIT; local-only until the maintainer
  approves publishing.
