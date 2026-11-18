# InControl for Horizon

A clean-room Rust rewrite of **InControl 1.21-10.3.0**
([McJtyMods/InControl](https://github.com/McJtyMods/InControl), MIT) as **one
flat SDK package** for Horizon at pin `0b14f8a` (nightly 2026-11-18), targeted
at Minecraft 26.3.

It is a server-side rule engine: `config/incontrol/spawn.json` cancels mob
spawns and `config/incontrol/loot.json` rewrites the drops of a mob or a block.
The package carries no blocks, items or assets of its own — only the code (the
`runtime` rules and the one `impl Hooks` that drives them).

**Local-only**: the licence allows derivatives, but the campaign publishes only
with the maintainer's approval ([ARCHITECTURE.md](../../ARCHITECTURE.md) §8).

## Spawn rules

`config/incontrol/spawn.json` holds a JSON array of rules, evaluated top to
bottom. A rule matches when **every** condition in it matches; the first match
decides unless it says `"continue": true`.

```json
[
  {
    "mob": "minecraft:zombie",
    "dimension": "minecraft:overworld",
    "minheight": 0,
    "maxheight": 60,
    "maxlight": 7,
    "when": "onjoin",
    "result": "deny"
  }
]
```

| Key | Values | Notes |
|---|---|---|
| `when` | `position` (default) or `onjoin` | `position` = natural spawn attempts only, as upstream; `onjoin` = every fresh mob, commands and spawn eggs included |
| `mob` | id or list of ids | bare ids gain `minecraft:`; `#tag` cannot be read at this pin (the rule is then inert) |
| `mod` | namespace or list | matches the entity id's namespace |
| `dimension` | id or list | bare ids gain `minecraft:` |
| `minheight` / `maxheight` | integer block y, inclusive | the block y is the spawn y truncated, as upstream |
| `minlight` / `maxlight` | integer block light, inclusive | block light at the spawn block (`LightLayer.BLOCK`) |
| `minlight_sky` / `maxlight_sky` | integer sky light, inclusive | sky light at the spawn block (`LightLayer.SKY`) |
| `result` | `deny`, `deny_with_actions`, `allow`, `default` | any other word denies, as upstream; `deny_with_actions` denies without running actions in this port |
| `continue` | `true`/`false` | keep evaluating later rules after this match |

No matching rule leaves the spawn alone. A rule using a key this port cannot
evaluate (`biome`, `minlight_full`, `random`, `finalize`, `despawn`, …) is
**inert**: it never matches, and `/incontrol reload` says so. That is
deliberate — a rule we cannot read in full must never deny more than its author
asked.

A broken file is refused and the rules already in force stay: a typo must not
silently un-deny every spawn.

## Loot rules

`config/incontrol/loot.json` holds a JSON array of rules that deny, replace or
add to the drops of a mob or a block, evaluated top to bottom; the first match
applies and stops unless it says `"continue": true`.

```json
[
  {
    "mob": "minecraft:zombie",
    "replace": true,
    "drops": [
      { "item": "minecraft:diamond", "chance": 1.0, "min": 1, "max": 2 }
    ]
  },
  { "block": "minecraft:stone", "remove": true }
]
```

| Key | Values | Notes |
|---|---|---|
| `mob` | id or list | the entity whose drops these are |
| `block` | id or list | the broken block whose drops these are (an extension: upstream only touches entity loot) |
| `mod` | namespace or list | the namespace of the mob or block id |
| `dimension` | id or list | where the drops happened |
| `drops` / `add` | list of drop objects | append to the rolled drops |
| `replace` | `true`/`false` | clear the rolled drops first, then add `drops` |
| `remove` | `true` / item id or list | `true` clears everything; a list takes those ids out |
| `result` | `deny`, `allow`, `default` | `deny` clears; the others change nothing; any other word makes the rule inert |
| `continue` | `true`/`false` | keep evaluating later loot rules |

A drop object is `{"item": id, "chance": 0..1, "min": n, "max": n}`; `chance`
defaults to 1, `min` to 1 and `max` to `min`. `item` (one of each) and
`removeall` are upstream's aliases; `itemcount` (looting-scaled counts, read from
the killer's weapon) is supported, a per-drop `nbt` object rides on the added
stack as `minecraft:custom_data`, and an item id the game does not have is
dropped from the rule with a warning. A drop set with no loot table (a leashed
mob's lead) is left alone.

## Commands

`/incontrol reload` re-reads both files, `/incontrol status` prints what is in
force; both need permission level 1, as upstream.

## Build and validate

```sh
# the SDK is fetched into .horizon/sdk/ by the first `cargo horizon-mod` command
cargo test                       # the pure rules, 69 tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check                # NOT --all: it would format the fetched SDK

rm -f Cargo.lock                 # a packed lock changes the artifact
cargo horizon-mod build          # -> target/horizon-mod/incontrol.hmod
cargo horizon-mod check incontrol.hmod
cargo horizon-mod fits incontrol.hmod --build nightly
```

The delivered package is `incontrol.hmod` in this directory. The game that runs
it is a **launcher-built** `horizon-client` carrying the package
(`horizon-launcher native-build <this dir> … --trust-native-code`); no `--dev-mod`
is needed, because the package is the mod.

## In-game check

`tools/e2e.sh` drives the rules through `/incontrol reload` in a headless world
in The End: a `[]` baseline, an `onjoin` zombie deny with a cow control, an edit
back to `[]`, a dimension-scoped pig deny, a height-box sheep deny with an
outside control, a default `position` rule that must not touch a `/summon`, a
broken file that must keep the previous rules, the light conditions
(`maxlight: 0` denies a dark-box zombie and allows a lit-box one, `minlight: 1`
the inverse, each with a rule-removed differential), then survival loot phases:
stone without a rule drops cobblestone, `replace` makes a killed zombie drop
exactly a diamond, `add` keeps a sheep's wool while adding a diamond, `remove`
denies every drop, an edited file reloads to an emerald — plus `itemcount` tiers
at Looting 0 and III and an `nbt` object on an added drop.
`COUNTERFACTUAL=1` (alias `SELFTEST=1`) runs the same scenario with no deny rule,
so the deny assertion can fail.

It needs `GAME=<launcher-built horizon-client>`; the 0.2.0 run is not done in
this environment (the launcher is not installed here) and is recorded as a gap
in [notes.md](notes.md).

## Layout

```text
horizon-mod.toml   the only manifest: id, version, api = "0.2", side, capability, [native] pins
Cargo.toml         the crate; its only dependency is horizon-mod-api from .horizon/sdk/
src/lib.rs         the one #[mod_api::horizon_mod] impl Hooks (glue)
src/runtime.rs     the pure spawn.json / loot.json engines and their 69 tests
tools/e2e.sh       the in-game scenario (fail-capable)
incontrol.hmod     the delivered package
notes.md           upstream evidence, helper → hook table, deviations, gaps, verification
```

## Licence

MIT (the upstream mod is MIT); `LICENSE` and `LICENSE-NOTICE.md` travel inside
the package. Local-only until the campaign maintainer approves publishing.
