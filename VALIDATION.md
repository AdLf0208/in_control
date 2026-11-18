# Validation

Validated on Linux with Rust 1.95.0 on 2026-11-04, against the exact revisions
in `compatibility.json` (Horizon `c9d474dd9116e84c243f8992d75c11314df56f06`,
horizon-mixins `d9c83d7096b7ea5b83678da2efdfd6161f74997e`, plan schema 6) and the
prepared 26.3 resources at `/home/adlf/Desktop/horizon/.local/horizon-client/26.3`.

| Check | Command | Result |
|---|---|---|
| Formatting | `cargo fmt --all --check` | `fmt_exit=0` |
| Runtime tests | `cargo test --locked --offline` | `test_exit=0`, `68 passed; 0 failed` |
| Lints | `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | `clippy_exit=0`, 0 warnings |
| Staged export | `python3 tools/horizon.py stage` | `stage_exit=0`, `stage_changed=true` |
| Compile check | `python3 tools/horizon.py check` | `cargo_exit=0` (43.2 s Cargo) |
| Embedded tests | `python3 tools/horizon.py test` | `cargo_exit=0`, `68 passed; 0 failed; 531 filtered out` (`game::incontrol::tests`) |
| Fast build | `python3 tools/horizon.py build` | `build_exit=0`, `target/horizon-cargo/fast/horizon-client` (81,700,160 B) |
| Harness | `horizon-ctl` from `target/horizon-cargo/fast` (built in an earlier stage of this pin) | every `ctl` command the e2e sent returned `"ok": true` |
| In-game scenario | `SOCK=… tools/e2e.sh` | `e2e_exit=0`, `PASS: spawn rules veto …; loot rules deny, replace and add …` + the 19-row spawn table and the 12-row loot table |
| Counterfactual | `SOCK=… COUNTERFACTUAL=1 tools/e2e.sh` | `e2e_cf_exit=0`, `COUNTERFACTUAL PASS: without the deny rule the summoned zombie stays` |

Logs: `/tmp/opencode/incontrol-slice3/{stage-final,check-final,test-final,build-final,staged-clippy,e2e-final,e2e-cf-final}.log`
and the JSON reports beside them.

## In-game spawn table (positive run)

| Phase | Entity | Count | Expected |
|---|---|---:|---:|
| 1. no rule, `/summon zombie` | zombie | 1 | 1 |
| 2. `onjoin` zombie deny | zombie | 0 | 0 |
| 2. control | cow | 1 | 1 |
| 3. edit back to `[]` + reload | zombie | 1 | 1 |
| 4. pig deny in `the_nether`, in the End | pig | 1 | 1 |
| 4. same rule, in the Nether | pig | 0 | 0 |
| 4. Nether control | cow | 1 | 1 |
| 5. sheep deny 70..80, at y=75 | sheep | 0 | 0 |
| 5. same rule, at y=64 | sheep | 1 | 1 |
| 5. rule removed, at y=75 | sheep | 2 | 2 |
| 6. default (`position`) zombie deny, `/summon` | zombie | 1 | 1 |
| 7. `onjoin` cow deny | cow | 0 | 0 |
| 7. broken file refused, rules kept | cow | 0 | 0 |
| 8. `maxlight: 0`, dark box | zombie | 0 | 0 |
| 8. same rule, lit box | zombie | 1 | 1 |
| 8. dark box, rule removed | zombie | 1 | 1 |
| 9. `minlight: 1`, dark box | zombie | 1 | 1 |
| 9. same rule, lit box | zombie | 0 | 0 |
| 9. lit box, rule removed | zombie | 1 | 1 |

## In-game loot table (positive run)

| Phase | Rule | Source | Expected |
|---|---|---|---:|
| 10. no rule | `[]` | break stone | cobblestone 1, diamond 0 |
| 11. `replace` | zombie → diamond | kill zombie | diamond 1, rotten flesh 0 |
| 12. `add` | sheep → diamond | kill sheep | diamond 1 and wool 1 |
| 13. `remove: true` | sheep | kill sheep | wool 0, mutton 0, diamond 0 |
| 14. edited + reload | stone → emerald | break stone | emerald 1, cobblestone 0 |
| 14. rule removed | `[]` | break stone | cobblestone 1 |

The loot phases run in survival with a diamond pickaxe, because a creative
break rolls no loot at this pin and stone needs the right tool. Each row has a
differential in the same run: the same stone with and without a rule, and the
same sheep under an `add` and a `remove` rule.

The counterfactual reruns phases 1–2 with `spawn.json` = `[]` and reports
`phase2 counterfactual (no rule): zombie = 1` — the same world, commands and
waits as `phase2 deny: zombie = 0`, so each assertion fails on the other run's
outcome. Phases 3, 5, 7, 8, 9, 10 and 14 are differentials inside the positive run (the
rule is removed or the file broken and the entity appears/keeps being denied).
Phases 8–9 use two sealed stone boxes on the End floor, the lit one with
glowstone in its ceiling.

## Not run / not applicable

- `python3 tools/test_template.py` is the template's tiny-fixture test: its
  fixture defines only the template example's `break_block` target and asserts
  one injection/one module, so it cannot hold for this bundle. The real bundle
  is validated by the staged ladder above, which compiles and tests the actual
  injected sources.
- `ports/tools/clean-targets.sh` is run after the job (build state, not an
  artifact).

---

## 0.2.0 — the SDK package (2026-11-18)

Everything above is the **0.1.0 `.hnmod`** record and is **historical**: the container is
retired, `compatibility.json` moved to
`../_shared/evidence/incontrol/retired-hnmod-0.1.0/`, and the staged ladder
(`tools/horizon.py`) no longer exists. The behaviour it describes is the same behaviour,
now carried by a package whose `src/runtime.rs` is byte-identical (sha256
`8dc2ef1cec9947bffdb8e0a3230d095aad2321c61270f175aba0cb2181f81638`).

Pins: Horizon `0b14f8acfd97fa2c750e42795249dcc981178ca4`, mixins
`e59da9094f0c2fdda6c627fe9eff22dec6a906f0`, mod API `0.2`, launcher 0.2.78, rustc 1.97.0
(the SDK declares 1.99.1; the package carries no `rust-toolchain.toml`).

| Check | Command | Result | Evidence |
|---|---|---|---|
| Runtime tests, **before any glue** | `cargo test --offline` | **69 passed; 0 failed** | `evidence/incontrol/sdk-test-runtime-only.log` |
| Runtime tests, with the glue | `cargo test --offline` | **69 passed; 0 failed** | `evidence/incontrol/sdk-test.log` |
| First compile of the glue | `cargo check --offline --all-targets` | **red** (`E0599` `entity_discard`), then green | `evidence/incontrol/sdk-check-compile.log` |
| Lints | `cargo clippy --all-targets --offline -- -D warnings` | exit 0, 0 warnings | `evidence/incontrol/sdk-clippy.log` |
| Formatting | `cargo fmt --check` | clean (the runtime module is `#[rustfmt::skip]`, see notes trap 1) | `evidence/incontrol/sdk-fmt.log` |
| Package | `rm -f Cargo.lock && cargo horizon-mod build` | `target/horizon-mod/incontrol.hmod` | `evidence/incontrol/sdk-build.log` |
| Registry check | `cargo horizon-mod check incontrol.hmod` | `incontrol 0.2.0 (native-mod, format native-1/0b14f8a…/e59da90…, for 26.3)`, 35,742 B, sha256 `4c1a0d9d…2c20`, **exit 0** | `evidence/incontrol/sdk-check.log`, `sdk-check-json.log` |
| Fits the pinned build | `cargo horizon-mod fits incontrol.hmod --build nightly` | `0b14f8a  made for`, exit 0 | `evidence/incontrol/sdk-fits.log` |
| Same package twice | `cargo horizon-mod build` again after `rm -f Cargo.lock` | 35,742 B, `4c1a0d9d…2c20` both times (`cmp` equal) | `evidence/incontrol/sdk-build-second.log`, `sdk-reproducible.txt` |
| `runtime.rs` untouched | `cmp` with the retired copy; `unzip -p incontrol.hmod src/runtime.rs \| sha256sum` | `8dc2ef1c…1638` in all three places | `evidence/incontrol/sdk-runtime-sha256.txt` |
| What it uses | `unzip -p incontrol.hmod uses.json` | 269 entries (19 `hook:`, 25 `method:`, 70 `handle:`, 133 `type:`, 15 `trait:`, 1 `capability:`) | `evidence/incontrol/sdk-uses-from-package.json` |

### Not run in 0.2.0

- **The in-game e2e.** It needs a game built by `horizon-launcher native-build` (the
  mod's code is compiled into the game, so no shared or per-port `horizon-client` carries
  it), and `horizon-launcher` is not installed here. `tools/e2e.sh` is updated for that
  launch (`GAME=`, optional `CONTROL_GAME=`, `COUNTERFACTUAL=1`/`SELFTEST=1`), and no
  0.2.0 run is claimed. The 0.1.0 `PASS` + `COUNTERFACTUAL PASS` runs above remain the
  behaviour's evidence for the unchanged `runtime.rs`.
- The 0.1.0 logs lived under `/tmp/opencode/incontrol-slice3/` and are gone; the tables
  above are the record.
