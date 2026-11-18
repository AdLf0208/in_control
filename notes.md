# InControl — port notes (P3 logic tier, server-only)

Ported from **InControl 1.21-10.3.0** (`McJtyMods/InControl`, MIT) for
Minecraft Java 1.21.1 NeoForge. Local-only port until the maintainer approves
publishing (ARCHITECTURE.md §8).

Status: `ported-partial` — **slice 1: the `spawn.json` deny engine** for
`when`/`mob`/`mod`/`dimension`/`minheight`/`maxheight`/`result`/`continue`,
plus **slice 2: the `minlight`/`maxlight`/`_sky` light conditions**, plus
**slice 3: the `loot.json` drop rules** (`drops`/`add`/`replace`/`remove`/
`result` for a mob or a block, through the `loot_drops` hook). Both files are
loaded from `config/incontrol/`, with `/incontrol reload` and
`/incontrol status`. Summon-aid, event, spawner, area, phase and
finalize/despawn rules are deferred (table below).

**Package format (2026-11-18):** the retired `.hnmod` container was replaced by
Horizon's flat SDK package — one `incontrol.hmod` with `horizon-mod.toml`
(`api = "0.2"`, pinned to `0b14f8a`/`e59da909`), `src/lib.rs` (one
`impl mod_api::Hooks`), `src/runtime.rs` (this port's pure rules, byte-identical
to the retired file) and no `assets/`/`data/` (the mod carries no content).
Version **0.2.0**, a first publication. See
[`## Migration to the SDK format`](#migration-to-the-sdk-format). The 0.1.0
in-game runs below are historical: the launcher that builds an SDK package is
not installed in this environment, so **no 0.2.0 in-game leg is claimed**.

## Behaviour implemented

- **Rule file**: `config/incontrol/spawn.json` (upstream's path,
  `RulesManager.java:173-214`), a JSON array of rule objects in file order.
  Read at server start and on `/incontrol reload` (upstream: first level load +
  `/incontrol reload`).
- **Conditions** (all must match, `GenericRuleEvaluator.java:1402-1409`):
  - `when`: `position` (default; natural spawn attempts only) or `onjoin`
    (every fresh mob: natural, spawner, spawn egg, `/summon`, breed, raid).
  - `mob`: one entity id or a list (OR); bare ids gain `minecraft:`.
  - `mod`: one namespace or a list (OR).
  - `dimension`: one dimension id or a list (OR); bare ids gain `minecraft:`.
  - `minheight`/`maxheight`: block y, inclusive; `y` is the Java `(int)` cast
    of the spawn y, as upstream (`GenericRuleEvaluator.java:920-934`).
  - `minlight`/`maxlight` (slice 2): block light at the spawn block, inclusive
    (`GenericRuleEvaluator.java:858-866`, `:885-893`); upstream reads
    `world.getBrightness(LightLayer.BLOCK, pos)` and compares `>=`/`<=`.
  - `minlight_sky`/`maxlight_sky` (slice 2): sky light, inclusive, same
    comparisons on `LightLayer.SKY` (`GenericRuleEvaluator.java:867-875`,
    `:894-902`).
  - A light read at a position whose chunk is not loaded fails the condition
    (`TestingTools.isChunkInvalid`, `TestingTools.java:429-435`, returns false
    from every check); the port models that as `Spawn.light == None`, which
    never matches a light rule. The glue always supplies `Some` because
    `mob_spawned` only fires for an entity already in a loaded level.
  - The light block is the spawn position with Java's `(int)` cast
    (`SpawnRule.java:93-95` for the position query; the port uses the same
    cast as `minheight`). At `onjoin` upstream uses `entity.blockPosition()`
    (`SpawnRule.java:187-189`), which floors negative coordinates; the port's
    truncation differs only for negative x/z/y.
- **Precedence**: rules are evaluated top to bottom; the first matching rule
  decides and later rules are skipped unless `"continue": true`
  (`ForgeEventHandlers.java:97-126`, `:217-255`). `default`/`allow` stop and
  keep/allow the spawn; `deny` and `deny_with_actions` remove it. A deny found
  while `continue` runs stays denied (upstream's cancel is not reversible).
  No matching rule = vanilla.
- **Result words**: `default` (and any `def…` prefix), `allow`/`true`, anything
  else denies — upstream's exact mapping, typos included
  (`SpawnRule.java:416-428`).
- **Veto**: at `mob_spawned` the port finds the just-added mob
  (`native_mobs(dimension)` by kind and exact position, highest id) and
  `native_remove_mob`s it: out of the world with no death and no loot
  (`native_mobs.rs:206-214`). The mob is never ticked and a `/summon` of it
  fails like a cancelled spawn.
- **Reload safety** (deviation): a file that cannot be read or parsed keeps the
  rules already in force and reports the error; upstream clears and ends with
  no rules (silently un-denying spawns until the file is fixed).
- **Commands**: `/incontrol reload` and `/incontrol status` at permission
  level 1 (upstream's reload level, `CmdReload.java:16-40`). Feedback names the
  rule count and every skipped condition.
- **Inert rules** (deviation): a rule carrying a condition this port cannot
  read is kept but never matches, and reported. Upstream honours it; matching
  it without its other conditions could only deny *more* than the author
  intended, so the conservative direction is chosen.

## Slice 3: loot rules (`loot.json`)

- **Rule file**: `config/incontrol/loot.json` (`RulesManager.java:180` reads it
  with the other rule files; upstream parses it as `LootRule::parse`,
  `LootRule.java:211-218`), a JSON array of rule objects in file order.
- **Conditions** (all must match): `mob` (entity id or list), `block` (block id
  or list), `mod` (namespace of whichever id the drop has), `dimension` (id or
  list), each exactly as in `spawn.json` (bare ids gain `minecraft:`). A rule
  with neither `mob` nor `block` is a wildcard. A `#tag` on `mob` or `block`
  makes the rule inert (no entity-tag query at the pin; block tags exist but
  the engine holds no registry).
- **Actions**, applied in this order for one matching rule:
  - `drops` (or its alias `add`): a list of `{"item", "chance", "min", "max"}`
    entries appended to the rolled drops. `chance` defaults to 1, `min` to 1,
    `max` to `min`; `chance` is drawn in 1000 steps, `count` in
    `min..=max` (one stack, no max-stack-size splitting).
  - `remove`: `true` clears every rolled stack; a string or list takes those
    item ids out and leaves the rest.
  - `replace: true` clears first and then adds the `drops` list.
  - `result`: `deny` clears (like `remove: true`); `allow`/`default` change
    nothing; any other word makes the rule inert.
  - `removeall: true` is upstream's alias for clearing; `item` is upstream's
    add alias (one of each), and `itemcount` (a looting-indexed count list such
    as `"5/7-10/20-30"`) and a rule-level `nbt` object are read too: the count
    tier comes from the killer's weapon and the `nbt` rides on every added
    stack as `minecraft:custom_data`.
- **Precedence**: rules are evaluated top to bottom; the first matching rule
  applies and later ones are skipped unless it says `"continue": true`
  (the spawned-rule order model; see deviations for upstream's loot
  exception).
- **Sources**: `mob` matches `NativeLootDrops::entity`'s type (`minecraft:zombie`),
  `block` matches the broken block's registry name (`game.res.facts()`), and a
  drop set with no loot table (a leashed mob's lead) is left alone — upstream's
  loot modifier only sees tables (`InControlLootModifier.java:42-46`).
- **Item ids**: resolved against the item registry on the first drop after a
  reload (the registry is not joined at `server_about_to_start`); an unknown id
  is dropped from the rule with a warning, never pushed into a stack.
- **RNG**: chance and count draws come from the level's own random
  (`game.world.random`, the same source bhc's drop hook uses), so a replay of
  the same world is deterministic. Upstream uses a private `java.util.Random`
  (`LootRule.java:98`).

## Hooks

*(This table is the retired 0.1.0 `.hnmod` bundle's — one helper file per
injection. The 0.2.0 SDK package's typed `Hooks` methods are in
[`## Migration to the SDK format`](#migration-to-the-sdk-format) below; the same
five moments, no helper files.)*

| Hook | Helper | What it does |
|---|---|---|
| `server_about_to_start` (v1, API 44) | `about.rs` | Reads `config/incontrol/spawn.json` and `loot.json`. |
| `mob_spawned` (v1, API 40) | `spawned.rs` | Reads `native_light` at the spawn block, decides the fresh mob; a deny removes it with `native_remove_mob`. |
| `loot_drops` (v1, API 75) | `loot.rs` | Maps `NativeLootDrops` (entity/block/dimension/table) to a `LootQuery`, applies the `loot.json` operations to `drops`, and adds plain stacks. |
| `register_commands` (v1, API 8) | `register.rs` | Declares `/incontrol reload` and `/incontrol status` at level 1. |
| `perform_command` (v1, API 8) | `perform.rs` | Runs them and reports to the player. |

## Why `mob_spawned` and not `entity_join_level`

The pin's `entity_join_level` (API 47) only hears **item entities and
experience orbs** — its doc says so (`native_hooks.rs:122-123`, `:2597-2617`)
and its only callers are the item/orb/loaded-save paths (`game.rs:3584`,
`:12837`, `commands.rs:2071`, `persist.rs:340,360`). A mob deny rule cannot be
implemented there. `mob_spawned` (API 40) hears every fresh mob with
`dimension`/`kind`/`pos`/`natural`, including `/summon` (`natural=false`), and
the mob is already in the level's list (`native_hooks.rs:2569-2575`), so the
veto is one `native_remove_mob` at head of the hook — zero ticks of life, not
one.

Two consequences are recorded as pin facts, not port choices:

- `onjoin` here never re-fires for a mob loaded from disk or changing level
  (`native_hooks.rs:2573-2574`), while upstream's `EntityJoinLevelEvent` does;
  the port therefore behaves like upstream's newer `firstjoin` for denies
  (no double-deny on chunk reload). Recorded as a deviation.
- A denied natural spawn can still leave a jockey: `spawn_jockeys` runs after
  `native_mobs_joined` and finds the mob by id without checking `removed`
  (`creatures.rs:413-417`, `passengers.rs:307-345`). Rare (the second mob of a
  jockey pair); upstream's pre-spawn veto has no such window.

## Gaps (probed at pin `c9d474d`)

| Upstream behaviour | Status | Evidence |
|---|---|---|
| `minlight`/`maxlight`/`_sky` | **implemented in slice 2** | block/sky probes at `native_light(pos)` (`native_hooks.rs:6650-6656`); upstream comparisons recorded above |
| `minlight_full`/`maxlight_full`, `light` expression | **deferred**: `getMaxLocalRawBrightness` subtracts `skyDarken`, which the pin has no native probe for | upstream `GenericRuleEvaluator.java:876-884`, `:903-911`; rule stays inert |
| `biome`, `biometags` | **blocked**: no biome read | the only biome native is client-side colour refresh (`native_biome_colors`, `native_hooks.rs:1487`); in-crate `biome_facts` is not an API |
| `mob: "#tag"` | **blocked**: no entity-type tag query | block tags exist (`native_block_in_tag`), entity tags do not; rule inert |
| `finalize` rules (equipment/health/AI/NBT) | **blocked**: actions need entity-write APIs (`native_set_mob_health` exists, gear/NBT do not) | `SpawnRule.java:232-378` action list; check the pin's `native_mob_*` set |
| `despawn` rules | out of scope (despawn hook absent) | `ForgeEventHandlers.java:258-296` |
| `phases`, `kubejs`, `random`, counts, distance, difficulty, weather, cave/slime, structure, area, block tests, scoreboard | out of slice | `GenericRuleEvaluator.java:72-176` |
| `loot.json` Conditions | **incl. `mob`/`block`/`mod`/`dimension` in slice 3** | `LootRule.java:100-197` registers the full condition set; the other keys make the rule inert |
| `loot.json` actions | **`drops`/`add`/`replace`/`remove`/`result` in slice 3**, plus `item`/`removeall` aliases | `LootRule.java:220-233`, `InControlLootModifier.java:56-93` |
| `itemcount` (looting-scaled counts) | **implemented**: the tiers are parsed and the killer's Looting level picks one (`runtime::loot_ops`'s `query.looting`; `itemcount_tiers_scale_with_looting`). *(This row said "inert" until the 0.2.0 migration; the engine and the script's phases 15–16 prove otherwise — the artefact won.)* | `InControlLootModifier.java:48-54` reads it from `ATTACKING_ENTITY`'s weapon |
| item `nbt` on added drops | **implemented**: the object is applied as `minecraft:custom_data` on every added stack (the e2e's phase 17; corrected here like the row above) | `LootRule.java:307-315` |
| `block` loot rules | **implemented as an extension**: upstream's modifier returns early when there is no `THIS_ENTITY`, so a block break is never touched (`InControlLootModifier.java:42-46`); the pin's `NativeLootDrops::block` makes it possible | `native_hooks.rs:670-705` |
| summon-aid (`spawner.json`), events, experience/effects/break/place/click rules | out of slice (other files) | ARCHITECTURE §3 gap table |

## Pin mapping

- `native_config_dir() -> Result<PathBuf, String>` (API 26,
  `native_hooks.rs:3824`): `<game dir>/config`; the e2e's cwd is the scratch
  root, so the file is `<cwd>/config/incontrol/spawn.json`.
- `native_mobs(dimension) -> Vec<NativeMob>` and
  `native_remove_mob(dimension, id) -> bool` (API 37, `native_mobs.rs:118-130`,
  `:206-214`): the level in place only.
- `native_light(pos: [i32; 3]) -> (u8, u8)` (API 97,
  `native_hooks.rs:6650-6656`): `(block, sky)` for the level in place, each
  0..15; `(0, 0)` in a chunk that is not loaded. The glue calls it on the
  spawn's block position and passes the pair as `Spawn.light`.
- `NativeCommand::op(usage, level)` (`native_server.rs:61-120`),
  `native_feedback(Text)` (`native_server.rs:449`), `Text::literal`.
- `native_loot_drops(&mut self, context: &NativeLootDrops<'_>, drops: &mut Vec<ItemStack>)`
  (API 75, `native_hooks.rs:2624-2646`): the hook runs before the caller drops
  the stacks. `NativeLootDrops` (`native_hooks.rs:670-705`) carries `table`,
  `dimension`, `origin`, `block: Option<([i32; 3], BlockStateId)>`,
  `entity: Option<(u32, &str)>`, `player`, `tool`, `explosion`, `used`.
  `ItemStack` is `horizon_items::items::ItemStack { item: Arc<str>, count,
  damage, extra }`; the glue builds a default-component stack and checks ids
  against `game.res.items().get(id)` (`items.rs:921`).
- Hook target module: `crate::game::incontrol` (pure engine) and
  `crate::game::incontrol_state` (glue).

## Deviations

1. A broken `spawn.json` keeps the previous rules instead of clearing them.
2. A rule with an unreadable key/condition is inert and reported (upstream
   applies the conditions it understands; an unknown key there blanks *all*
   conditions and makes the rule match everything — `GenericAttributeMapFactory.java:48-57`).
3. `deny_with_actions` denies without running actions (no action engine yet).
4. `onjoin` does not fire for saved/transferring mobs at this pin (see above).
5. `/incontrol` has `reload` and `status` only; the other subcommands
   (`killmobs`, `list`, `info`, `days`, `phases`, `numbers`, `area`) are out of
   slice 1.
6. Feedback goes to the caller only; upstream's `killmobs`/`stats` broadcasts
   are not part of this slice.
7. `minlight_full`/`maxlight_full` and the `light` expression stay inert:
   upstream's `getMaxLocalRawBrightness` subtracts the level's `skyDarken`,
   which the pin exposes no native probe for, so the port cannot reproduce the
   comparison exactly.
8. Light and height use Java's `(int)` cast of the spawn position. At `onjoin`
   upstream uses `entity.blockPosition()` (floor), so negative coordinates can
   address the neighbouring block; this only affects spawns at negative
   x/y/z and keeps the port consistent with its existing height check.
9. **The `loot.json` schema follows the campaign brief, not upstream 1.21_neo.**
   The brief specifies `drops` entries with `item`/`chance`/`min`/`max`, and
   `replace`/`remove`/`result`; upstream has none of those. Upstream's loot
   actions are `item` (+`itemcount`), `remove` and `removeall`
   (`LootRule.java:220-233`, `InControlLootModifier.java:63-90`), with no
   per-entry chance (`chance` is a loot-table feature, not InControl's). The
   port implements the brief's keys and keeps upstream's `item`, `remove` (as
   a list) and `removeall` as exact aliases; `itemcount` and `nbt` are read as
   upstream reads them (a looting-indexed count tier and a `custom_data` object
   on the added stack).
10. **Loot rules are first-match/`continue`, as the brief asks; upstream applies
    every matching loot rule** (`InControlLootModifier.java:56-93`: a `for`
    loop with no break, and the docs say loot is the exception to first-match).
    The port's order means a rule without `"continue": true` hides later loot
    rules.
11. **Block loot rules are an extension.** Upstream's loot modifier returns
    when `THIS_ENTITY` is null (`InControlLootModifier.java:42-46`), so a block
    break never reaches a loot rule; the pin's `NativeLootDrops::block` lets
    the port match the broken block's registry name.
12. `chance` has no upstream counterpart and is drawn in `CHANCE_STEPS` (1000)
    steps; the draws come from the level's random, not a private
    `java.util.Random` (`LootRule.java:98`).
13. Unknown item ids are reported and dropped on the first set of drops after
    a reload (the item registry is not joined at `server_about_to_start`);
    upstream resolves ids at parse time (`LootRule.java:303-305`).
14. An added count is one stack; upstream splits a count above the item's max
    stack size into several stacks (`InControlLootModifier.java:79-89`).

## Upstream source

**InControl 1.21-10.3.0** — <https://github.com/McJtyMods/InControl>, branch
`1.21_neo`, analysed at commit `9b95c7260af38feccdc993953ef6791831adfc4d`
(2026-09-01; `gradle.properties` `version=1.21-10.3.0`; the following commits
only link the docs in the README and then bump to 10.3.1). MIT; local-only until
approved. The analysis clone under `ports/_shared/upstream/incontrol/` was
deleted after the findings were recorded; the behaviour table and file:line
evidence are above.

**Areas read:** `rules/SpawnRule.java`, `rules/RulesManager.java`,
`rules/support/{GenericRuleEvaluator,RuleKeys,SpawnWhen,ICResult}.java`,
`ForgeEventHandlers.java`, `commands/{ModCommands,CmdReload,CmdInfo}.java`,
`tools/typed/{AttributeMap,GenericAttributeMapFactory}.java`,
`tools/rules/TestingTools.java`, `spawner/PositionCheck.java`,
`tools/varia/JSonTools.java`, plus the 1.20.1+ documentation pages (rule order,
result table, `onjoin`/`firstjoin` notes).

**Slice-2 light analysis:** `GenericRuleEvaluator.java:96-102` registers all
seven light keys; `:858-866` is `minlight` on `LightLayer.BLOCK` (`>=`),
`:885-893` is `maxlight` (`<=`), `:867-875`/`:894-902` are the `_sky`
variants on `LightLayer.SKY`, and `:876-884`/`:903-911` are `_full` on
`getMaxLocalRawBrightness` (deferred). Every check first calls
`TestingTools.isChunkInvalid` (`:429-435`) and returns false when the chunk is
absent or not `FULL`, so a missing read never matches. The spawn positions are
`new BlockPos((int) x, (int) y, (int) z)` for `MobSpawnEvent.PositionCheck`
(`SpawnRule.java:93-95`) and `entity.blockPosition()` for `EntityJoinLevelEvent`
(`:187-189`). `CmdInfo.java:51-53` confirms the three readings the mod exposes:
block, sky and full.

**Slice-3 loot analysis:** the loot rules are a NeoForge global loot modifier,
not an entity event: `InControlLootModifier.doApply` (`:42-95`) receives the
generated stacks, returns them untouched when `LootContextParams.THIS_ENTITY`
is null (`:43-46`), reads the attacker's Looting level (`:48-54`), then walks
every rule in order (`:57`): `removeall` clears (`:63-64`), `remove` filters
each stack (`:66-73`), `item` adds stacks split at the item's max size
(`:76-90`). `LootRule` parses a rule with the same condition factory as
`spawn.json` (`:211-218`), registers the shared conditions (`:100-197`), and
maps the actions `item`/`itemcount`/`remove`/`removeall` (`:220-233`);
`itemcount` is a looting-indexed list such as `"5/7-10/20-30"` (`:247-295`).
The rules load with the other files in `RulesManager.java:180`. The event
query (`:47-96`) takes the position and entity from `THIS_ENTITY` and the
player from `ATTACKING_ENTITY`, which is why a block break has no rule-visible
position. The 1.20.1+ docs' "Loot Control" section confirms the actions and
says every loot rule is evaluated, in contrast with every other rule file.

**Areas read (slice 3):** `rules/LootRule.java`,
`events/InControlLootModifier.java`, `rules/RulesManager.java` (loot loading),
and the docs' "Loot Control" section. The clone was made from
`McJtyMods/InControl` branch `1.21_neo` at the pinned commit
`9b95c726…`, then deleted.

Skills: superpowers/using-superpowers,brainstorming,writing-plans,test-driven-development,systematic-debugging,verification-before-completion; BeautifulRust/rust-best-practices,rust-port

## Verification

Verified 2026-11-04 at pin `c9d474dd9116e84c243f8992d75c11314df56f06`
(mixins `d9c83d7`, plan schema 6); logs under
`/tmp/opencode/incontrol-slice3/`.

| Check | Result |
|---|---|
| `cargo fmt --all --check` | `fmt_exit=0` |
| `cargo test --locked --offline` | `test_exit=0`, **68 passed; 0 failed** |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | `clippy_exit=0`, **0 warnings** |
| `tools/horizon.py stage` | `stage_exit=0` (`stage_changed=true`, 62.0 s) |
| `tools/horizon.py check` | `cargo_exit=0` (43.2 s Cargo), 0 warnings in `horizon-check.log` |
| `tools/horizon.py test` | `cargo_exit=0`, **68 passed; 0 failed; 531 filtered out** as `game::incontrol::tests` |
| `tools/horizon.py build` | `build_exit=0` |
| `tools/e2e.sh` (positive) | `e2e_exit=0`, `PASS` |
| `tools/e2e.sh` (`COUNTERFACTUAL=1`) | `e2e_cf_exit=0`, `COUNTERFACTUAL PASS` |
| staged `cargo clippy` on `horizon-client` | `staged_clippy_exit=0`; its 43 warnings are pre-existing Horizon lints, none in `incontrol` sources |

The `.hnmod` bundle: `target/incontrol-0.1.0.hnmod`, 21,539 B, SHA-256
`030ab30a7d2a7a42e2f2890c3ad7e9f459b1bb246ad148ef82184b915db9da8a`
(`horizon-native.json` + the nine bundle files, `loot.rs` included).

**In-game loot table** (positive run; the same launch as the spawn phases, then
survival with a diamond pickaxe on the End stone floor):

| Phase | Rule | Source | Expected | Got |
|---|---|---|---|---:|
| 10 no rule | `[]` | break stone | cobblestone 1, diamond 0 | 1 / 0 |
| 11 replace | `{"mob":"minecraft:zombie","replace":true,"drops":[diamond]}` | kill zombie | diamond 1, rotten flesh 0 | 1 / 0 |
| 12 add | `{"mob":"minecraft:sheep","drops":[diamond]}` | kill sheep | diamond 1 **and** wool 1 | 1 / 1 |
| 13 remove | `{"mob":"minecraft:sheep","remove":true}` | kill sheep | wool 0, mutton 0, diamond 0 | 0 / 0 / 0 |
| 14 edited | stone `replace` emerald + reload | break stone | emerald 1, cobblestone 0 | 1 / 0 |
| 14 rule removed | `[]` + reload | break stone | cobblestone 1 | 1 |

Each loot row has a differential inside the same run: phase 10 breaks the same
stone with no rule (cobblestone), phase 14 breaks it with a rule (emerald) and
then again after removing the rule (cobblestone); phases 12 and 13 kill the
same sheep with an add and with a remove rule. A hook that did nothing would
fail phase 11 (diamond 0) and phase 14 (cobblestone instead of emerald), and a
hook that always cleared would fail phases 10 and 12.

**In-game spawn table** (positive run; `/summon` + `/incontrol reload`, The
End with a stone floor at y=63, then the Nether; phases 8–9 use two sealed
3×3×3 stone boxes on that floor, the lit one with glowstone in its ceiling):

| Phase | Entity | Count | Expected | Proves |
|---|---|---:|---:|---|
| 1 no rule | zombie | 1 | 1 | no rule = vanilla |
| 2 `onjoin` zombie deny | zombie | 0 | 0 | the rule vetoes the mob |
| 2 control | cow | 1 | 1 | the veto is mob-scoped |
| 3 back to `[]` + reload | zombie | 1 | 1 | reload applies the edited file |
| 4 pig deny in `the_nether`, End | pig | 1 | 1 | dimension scope allows elsewhere |
| 4 same rule, Nether | pig | 0 | 0 | dimension scope denies there |
| 4 Nether control | cow | 1 | 1 | only the pig is denied |
| 5 sheep deny 70..80 at y=75 | sheep | 0 | 0 | height box blocks inside |
| 5 same rule at y=64 | sheep | 1 | 1 | height box allows outside |
| 5 rule removed, y=75 | sheep | 2 | 2 | the box, not the height, denied |
| 6 default (`position`) zombie deny | zombie | 1 | 1 | position rules leave `/summon` alone |
| 7 `onjoin` cow deny | cow | 0 | 0 | good file loaded |
| 7 broken file refused | cow | 0 | 0 | previous rules kept |
| 8 `maxlight: 0`, dark box | zombie | 0 | 0 | the light condition denies |
| 8 same rule, lit box | zombie | 1 | 1 | block light above 0 allows |
| 8 dark box, rule removed | zombie | 1 | 1 | the light, not the box, denied |
| 9 `minlight: 1`, dark box | zombie | 1 | 1 | the inverse bound allows dark |
| 9 same rule, lit box | zombie | 0 | 0 | the inverse bound denies lit |
| 9 lit box, rule removed | zombie | 1 | 1 | the light, not the box, denied |

`COUNTERFACTUAL=1` reruns phases 1–2 with `spawn.json` = `[]` and prints
`phase2 counterfactual (no rule): zombie = 1` where the positive run prints
`phase2 deny: zombie = 0` — identical setup, so the phase-2 assertion can fail.
Phases 8–9 carry their own differentials: the same dark box and the same lit box
keep the zombie once the light rule is removed, so each light row can fail.

Harness notes found while validating (script, not port, changes):

- `ctl chat "/incontrol reload"` is the path that reaches the native
  `perform_command` hook; `ctl command` bypasses it.
- `/difficulty normal` was issued at launch so `/summon` of a monster is not
  refused by a peaceful world.
- In the Nether the player must stand on a floor at the counting height before
  summoning, or it falls out of `ctl entities`' radius and the count reads 0 for
  the wrong reason.
- A creative break rolls no loot and does not reach `loot_drops`
  (`native_hooks.rs:2639-2640`: "nor a creative break"), so the loot phases
  switch to survival and hold a diamond pickaxe; stone needs the right tool to
  drop anything.
- A summoned sheep's wool colour is random, so the script asserts that *some*
  `*_wool` dropped (and how many) rather than a fixed item id.
- `/kill @e[type=…]` now rolls that mob's loot, so every loot phase clears item
  entities and the other mob kinds first.

## Deviations

See the numbered list above and `README.md`. The behaviours that change most
from upstream are the reload-failure policy (keep the rules), inert unsupported
rules, the brief's loot schema with first-match/`continue` (upstream's loot is
all-match and uses `item`/`itemcount`/`remove`/`removeall`), and block loot
rules (an extension; upstream only modifies entity loot).


## Migration to the SDK format (2026-11-18)

Migrated from the retired `.hnmod` container (Horizon's mod-API merge #16941, launcher
≥ 0.2.70; PR #17992 cleans up the old files) to the flat SDK package at nightly
**`0b14f8acfd97fa2c750e42795249dcc981178ca4`** / mixins
**`e59da9094f0c2fdda6c627fe9eff22dec6a906f0`** (SDK `package_format` 6, `content_api`
"0.2"), launcher 0.2.78. This is the port's **first publication**, so there is no closed
PR to supersede. The reference recipes were `ports/imfast/notes.md` (the 12-step original)
and `ports/naturesaura/notes.md` (same shape: config files, no content); this port's own
traps are below.

**Done in this order:** the retired paths were moved into
`ports/_shared/evidence/incontrol/retired-hnmod-0.1.0/` **before** the first
`cargo horizon-mod build` (the tool refuses a root holding retired package paths), then
`src/runtime.rs` was copied with `cp` and proved with `cmp` + `sha256sum`, `cargo test`
ran on the new crate **before any glue existed** — `69 passed; 0 failed`, the retired file
byte for byte (sha256 `8dc2ef1cec9947bffdb8e0a3230d095aad2321c61270f175aba0cb2181f81638`)
— and only then was `src/lib.rs` written. `runtime.rs` is 69 tests, not the 68 the slice
records: the file carries one more than the last table counted (same file, same sha256).

### File map

| Old `.hnmod` layout | New SDK package |
|---|---|
| `horizon-mixin.toml` (`[mod]`, `[compatibility]`, 2 `[[module]]`, 5 `[[injection]]`) | `horizon-mod.toml` (`id`/`name`/`version`/`description`/`authors`/`api`/`side`/`capabilities` + `[native]` game/horizon/mixins) |
| `Cargo.toml` (`[lib] path = "mixins/incontrol/runtime.rs"`, `[workspace] members = ["tools/audit"]`) | `Cargo.toml` (`[lib]` implied `src/lib.rs`; one dependency, `horizon-mod-api` from `.horizon/sdk/`) |
| `mixins/incontrol/runtime.rs` (module + crate lib) | `src/runtime.rs` — **byte-identical**, `pub mod runtime;` (with `#[rustfmt::skip]`, trap 1) |
| `mixins/incontrol/support.rs` (module `incontrol_state`, 457 lines) | `src/lib.rs` — the one `impl Hooks` plus the state, 548 lines |
| `about.rs`, `spawned.rs`, `loot.rs`, `register.rs`, `perform.rs` | methods of `impl Hooks`; no per-hook files |
| `compatibility.json`, `rust-toolchain.toml`, `horizon-native.json` | **gone**: `[native]` in `horizon-mod.toml` is the pin, and the launcher decides the toolchain |
| `tools/horizon.py`, `tools/package.py`, `tools/test_stage.py`, `tools/test_template.py`, `tools/audit/`, `tools/__pycache__` | **gone** (moved to the evidence folder); the ladder is `cargo test`/`clippy`/`fmt` + `cargo horizon-mod build/check/fits` |
| `.github/workflows/ci.yml` (clones horizon-mixins, reads `compatibility.json`, runs `tools/test_template.py`) | **gone** (moved to evidence): none of those exist any more |
| `Cargo.lock` (3,260 B) | deleted, `/Cargo.lock` in `.gitignore`: a packed lock resolves newer crates than the SDK's own and changes the artifact (trap 5) |
| `incontrol.hnmod` (21,539 B, 0.1.0) | `incontrol.hmod` (35,742 B, 8 entries) — **one file, code only**; the port carries no content, so there is no `assets/`/`data/` and nothing to embed |

**Kept and updated:** `tools/e2e.sh` (the scenario's assertions are format-independent; its
launch and config paths now name a launcher-built game, see the header and *Gaps*),
`notes.md`, `README.md` (rewritten for the flat layout), `VALIDATION.md`, `REPIN-AUDIT.md`,
`LICENSE`, and the new `LICENSE-NOTICE.md` (both travel inside the package).
**Retired into evidence:** `mixins/`, `compatibility.json`, `rust-toolchain.toml`,
`.github/`, `tools/{audit,horizon.py,package.py,test_stage.py,test_template.py}` — the
retired tree is evidence, not deleted.

### Helper → hook table

Everything the retired `support.rs` did per hook, and where it lives now. A row with no
home would be a recorded gap; there is none.

| Retired injection / helper | SDK (0.2) | Mapping notes |
|---|---|---|
| `server_about_to_start` (`about.rs`) → `support::on_server_about_to_start` | `Hooks::server_about_to_start` | same hook, same moment (before the worlds load); it reads both files and keeps the previous rules on a bad one |
| `Game::native_config_dir()` + `std::fs::read_to_string("config/incontrol/*.json")` | `Host::config_files()` + `ConfigPath::new("incontrol/spawn.json")` | the service owns `config/<mod id>/`; `ConfigPath::owned_by` accepts `<id>/<path>` (`content/config_files.rs:252-262`), so upstream's exact path is kept and no raw filesystem path is read |
| `mob_spawned` (`spawned.rs`) → `support::on_mob_spawned` | `Hooks::mob_spawned` | same hook (`dimension`/`kind`/`pos`/`natural`), still a **listen** hook: the veto is the removal below, as in 0.1.0 |
| `Game::native_light(pos) -> (u8, u8)` | `LevelRef::brightness(host, LightLayer::{Block,Sky}, pos)` | two reads, `Option<i32>` each; a read the level cannot answer leaves `Spawn.light = None`, which fails the light conditions (trap 12) |
| `Game::native_mobs(dimension)` + `native_remove_mob(dimension, id)` | `LevelRef::entities_in(host, min, max)` → `EntityRef::{kind, position, id}` → `EntityRef::discard(host)` | the hook names the mob by kind and position only; the newest entity of that kind at exactly `pos` in a ±1 box is the one, and `discard` is vanilla's `Entity.discard` (no death, no loot) |
| `loot_drops` (`loot.rs`) → `support::on_loot_drops` | `Hooks::loot_drops` | a **collect** hook: `event.drops: &mut Vec<ItemStack>`; same `table.is_empty()` skip |
| `game.res.facts().get(state).name` for the broken block | `Host::block_state_string(state)` | answers `minecraft:stone[property=value]`; the port cuts at `[` |
| `game.res.items().get(id).is_some()` (unknown-id check) | `Host::item_key(&Identifier::parse(id)).is_some()` | same lazy pass on the first drop after a reload |
| `ItemStack { item: Arc<str>, count, damage, extra }` | `ItemStack::new(ItemKey, count) -> Option` | a `0` count is `None` (an empty stack), so a `min: 0` entry that draws 0 adds nothing — the old code pushed a 0-count stack that did not drop |
| `stack.data_mut().set_other("minecraft:custom_data", …)` | `stack.set_components(compound)` with the tag under `minecraft:custom_data` | the rule's `nbt` object still rides on the added stack |
| `horizon_client_core::scoreboard::json_to_tag(&Value)` | a local `json_tag`/`tag_of` (30 lines) | `horizon_core::json_nbt` is **not** exported; the same rules (narrowest signed tag, `Double` bits, `Byte` for booleans, object/array nesting, `null` refused) |
| `hand_item(...).enchantment_level("minecraft:looting")` | `LootDrops::weapon_item(host)` + a local `Component` over `minecraft:enchantments` | the weapon the death recorded (or the live melee stack) replaces the hand lookup, and the enchantment is read as the component it is |
| `game.world.random.next_int_bound(bound)` | `LevelRef::with_random(host, \|r\| r.next_int_bound(bound))` | the drop's own level stream, so a replay stays deterministic; 0 where the level is not open |
| `game.native_feedback(Text::literal(..))` | `host.commands().feedback(Text::literal(..))` | routed to the command's own source, as before |
| `register_commands` (`register.rs`) + `NativeCommand::op` | `Hooks::register_commands` + `Command::op` | same two usages, permission level 1 |
| `perform_command` (`perform.rs`, string words) | `Hooks::run_mod_command` (`Flow::Cancel` when handled) | `event.command.literals` is `["incontrol","reload"]`; the readout text is byte-identical to 0.1.0 (`Reloaded InControl rules: …`, `InControl reload failed: …`, `InControl: …`, `skipped: …`, `last error: …`) |
| `static CORE: Mutex<Option<Core>>` | `Mod { core: Mutex<Option<Core>> }` | the macro boxes one `Default` `Mod` per session and `Hooks` is `Send + Sync`; no `RefCell` (trap 16 of the campaign list) |
| `log::warn!("[incontrol] …")` | **dropped** | the package has no logger; `/incontrol status` carries the rule counts, the skipped counts and the last error, and `/incontrol reload` prints up to five warnings |

### Manifest

`id = "incontrol"` (also the resource namespace), `name = "InControl!"`,
`version = "0.2.0"` (the port's version, not upstream's 10.3.x), `api = "0.2"` (exact),
`side = "both"` (an integrated server belongs to the *client* installation, so a
single-player world and a LAN host need it; every hook here is server-side),
`capabilities = ["registries"]` (the schema's one value — required even for a mod that
registers nothing), `[native] game = "26.3"`, `horizon`/`mixins` the two 40-hex commits.
`authors = ["McJty", "AdLf0208"]`. No `license`/`homepage`/`issues` field: the schema has
none, and the packed manifest is the one written (no field was stripped).

### Gaps (0.2.0)

| Gap | Shape | Status |
|---|---|---|
| **No 0.2.0 in-game run** | `horizon-launcher native-build` is not installed here, so no game carrying the package can be built; the 0.1.0 runs are the behaviour's evidence for a `runtime.rs` that did not change | recorded, not claimed (see *Verification*) |
| `tools/e2e.sh` needs a launcher-built game | `GAME=` must name it (and `CONTROL_GAME=` a game without the package, for the "the mod is what does it" leg); the script writes the rule files to both known config roots unless `CONFIG_ROOT` pins one | the script is updated but **not run**; its first run is the next slice |
| `entity_join_level` is now a **pre-join veto** | API v2 makes it cancellable and it hears every kind, mobs included — the genuine `EntityJoinLevelEvent` upstream's `onjoin` rules hook. It carries no `natural` flag, so `when: position` rules would need `mob_spawn_position_check` as a second hook | **not used in 0.2.0**: the 0.1.0 route (`mob_spawned` + discard) is kept so the 69 runtime tests and the proven e2e still describe the same behaviour. The two-hook redesign is the next slice's improvement, and it would also close the jockey window below |
| A denied natural spawn can still leave a jockey | `spawn_jockeys` runs after the mob join and finds the mob by id without checking `removed` (`creatures.rs:413-417`) | unchanged from 0.1.0; the pre-join veto above would close it |
| `biome`/`biometags`, `mob: "#tag"`, `finalize`, `despawn`, phases, area, spawner and event rules | unchanged from 0.1.0 (no read at the pin; see the gap table above) | still inert and reported |
| The status readout names a relative folder | the API hands no filesystem path, so `rules: config/incontrol` replaces 0.1.0's absolute path | cosmetic; the e2e greps the reload message, not this line |
| A light read at an unloaded chunk | 0.1.0's `native_light` answered `(0,0)` there, which could match `maxlight: 0`; the SDK's `brightness` answers `None` and the runtime then never matches a light rule | a strict improvement in the fail-closed direction; a `mob_spawned` mob is always in a loaded chunk, so no case in the e2e changes |
| The veto's entity lookup | the hook names the mob by dimension/kind/position, so the port finds it with `LevelRef::entities_in` (a ±1 box, kind matched by id, position within `1e-6`, highest entity id wins) and calls `EntityRef::discard`. If the level cannot answer the query the spawn is left alone — 0.1.0's `native_mobs` always answered | untested in game (no launcher); the search is the same shape the 0.1.0 e2e proved, one handle call instead of a level-wide list |

`## Upstream defects fixed`: none found in this migration. The 0.1.0 corrections and
deviations recorded above (loot schema, first-match/`continue`, block loot rules, the
reload-failure policy) stand unchanged; the migration moved no behaviour.

### Traps this migration hit (new to this port)

1. **`cargo fmt --check` rewrites `src/runtime.rs`.** The retired file has three call
   sites wider than rustfmt's default (one `vec![LootStack { … }]` line, one `vec![…]` in
   a test), so formatting the crate would break the byte-identical carry rule. Fix:
   `#[rustfmt::skip]` on `pub mod runtime;` in `src/lib.rs` — rustfmt then leaves the
   whole module alone, and `cmp` proves the file unchanged after running `cargo fmt`
   (`ports/xnet/notes.md` trap 27 records the same problem from the other direction).
2. **`build-slot.sh` forwards the command to the remote runner, which syncs the port to
   its own root** — so the brief's `cd ports/incontrol` fails there with
   `No such file or directory` (exit 1, not a build failure). Every step here ran with
   `BUILD_LOCAL=1` and the `cd` inside the wrapped command.
3. **`Host::config_files()` owns `<mod id>/<path>`.** The ownership test is
   `owned("fd/a/b.json")` (`content/config_files.rs:252-262`), so
   `ConfigPath::new("incontrol/spawn.json")` is upstream's `config/incontrol/spawn.json`;
   a bare `"spawn.json"` would be `config/spawn.json` and refused.
4. **`entity_discard` is a handle method, not a host method.** The first `cargo check`
   failed `E0599: no method named entity_discard found for &mut dyn ServerHost`; the call
   is `entity.discard(host)` on the `EntityRef` (the red run is
   `evidence/incontrol/sdk-check-compile.log`).
5. **`ItemStack` is no longer a struct literal.** `ItemStack::new(ItemKey, count)` is
   fallible and `0` is `None`; the item comes from `Host::item_key(&Identifier::parse(id))`.
6. **There is no `enchantment_level` on a stack.** The Looting level is the
   `minecraft:enchantments` component (a flat compound of id → level), read through a
   local `Component` impl; `mod_api::JavaString` stays unnamed (`"key".into()`,
   `Tag::get(&str)`).
7. **`mod_api::serde_json` is the only JSON the SDK exports.** `horizon_core::json_nbt`
   (the converter 0.1.0 called) is not in the surface, so the `nbt` → `custom_data`
   conversion is local; `Tag::{Float,Double}` hold bit patterns, not numbers.
8. **Clippy 1.97 refused the 0.1.0 comparison style.** `ids.iter().any(|id| held == *id)`
   is `manual_contains` under `-D warnings`; `ids.contains(&held)` is the fix.
9. **`cargo horizon-mod check --uses` prints the same three lines as `check`.** The
   per-use list is the package's own `uses.json` (`unzip -p incontrol.hmod uses.json`),
   269 entries here (133 types, 70 handles, 25 methods, 19 hook entries, 15 traits). It is
   a surface report, not a call graph or a subscription list: it names `Hooks::blocks`,
   `Hooks::items`, `Hooks::command`, `ConfigFiles::write` and `ConfigFiles::list`, which
   this mod does not implement or call. The `impl Hooks` in `src/lib.rs` is the ground
   truth for what the mod subscribes to (five methods).
10. **The mod has no content, so the package has no `assets/`/`data/`.** Eight entries:
    `horizon-mod.toml`, `Cargo.toml`, `LICENSE`, `LICENSE-NOTICE.md`, `README.md`,
    `src/lib.rs`, `src/runtime.rs`, `uses.json` — no `Cargo.lock` (trap: it changes the
    sha256) and no `icon.png` (optional).

### Build and verify (measured 2026-11-18)

Every command ran in a `ports/tools/build-slot.sh` lease with `BUILD_LOCAL=1`, directly
in the port (`CARGO_INCREMENTAL=0`, no `--target-dir`):

```sh
rm -f Cargo.lock && cargo horizon-mod build        # -> target/horizon-mod/incontrol.hmod
cargo test --offline                              # 69 passed; 0 failed (runtime only, before glue)
cargo test --offline                              # 69 passed; 0 failed (with the glue)
cargo clippy --all-targets --offline -- -D warnings   # exit 0, 0 warnings
cargo fmt --check                                 # clean
cargo horizon-mod check incontrol.hmod            # exit 0
cargo horizon-mod fits incontrol.hmod --build nightly  # exit 0
unzip -p incontrol.hmod src/runtime.rs | sha256sum     # 8dc2ef1c…1638 = the retired file
```

`check` output:

```
incontrol 0.2.0 (native-mod, format native-1/0b14f8acfd97fa2c750e42795249dcc981178ca4/e59da9094f0c2fdda6c627fe9eff22dec6a906f0, for 26.3)
  35742 bytes, SHA-256 4c1a0d9d6aed240202443dea443a34f9865c25fd1a4b3a97fcc1984271682c20, unsigned
  native source: unsandboxed; not executed by the registry check. Compile and test against the exact pinned Horizon and Mixins revisions before review.
```

`fits` output:

```
InControl!:
0b14f8a  made for
```

The package is **35,742 bytes**, SHA-256
`4c1a0d9d6aed240202443dea443a34f9865c25fd1a4b3a97fcc1984271682c20`, **8 entries** — the two
manifests, `LICENSE`, `LICENSE-NOTICE.md`, `README.md`, `src/lib.rs`, `src/runtime.rs` and
`uses.json`; no `Cargo.lock`. Built **twice** (with `rm -f Cargo.lock` before each), the two
archives are byte-identical (`cmp`, `evidence/incontrol/sdk-build-second.log`).

### Not re-run in this migration

- **The in-game e2e** — no `horizon-launcher` here to build a game carrying the package.
  The 0.1.0 runs stand as the behaviour's evidence for a `runtime.rs` whose sha256 did not
  change; they lived under `/tmp/opencode/incontrol-slice3/` (now gone) and are
  **historical**: the tables above are the record, the launch path and `perform_command`
  route they used no longer exist. The script's **phases 15–17** (`itemcount` tiers at
  Looting 0 and III, an `nbt` object on an added drop) are in `tools/e2e.sh` but were
  added after the recorded run's table and are **not** in it; the first 0.2.0 run is the
  next slice's check.
- **The staged ladder** (`tools/horizon.py stage/check/test/build`) — deleted with the
  retired format.
- **`mod_api`'s own tests** — the SDK ships them; this port runs `cargo test` on its own
  crate only.

### Changelog for the first publication (0.2.0)

> **First publication.** InControl for Horizon 0.2.0 is the mod's first release and the
> first SDK package: there is no earlier submission to supersede. It is built for Horizon
> nightly `0b14f8acfd97fa2c750e42795249dcc981178ca4` and mixins
> `e59da9094f0c2fdda6c627fe9eff22dec6a906f0` (launcher 0.2.78, mod API `0.2`) and runs on
> Minecraft 26.3. For `api = "0.2"` the retired `.hnmod` container is gone: the five old
> injections are five typed `Hooks` methods (`server_about_to_start`, `mob_spawned`,
> `loot_drops`, `register_commands`, `run_mod_command`), the rule files are read through
> the SDK's own config service instead of a raw filesystem path, the spawn veto finds the
> fresh mob through the level's entity list and removes it with `Entity.discard`, and the
> Looting level comes from the killer's weapon component. The mod's pure rules
> (`src/runtime.rs`) and their 69 tests are unchanged, byte for byte. Package: 8 entries,
> **35,742 B**, sha256 `4c1a0d9d6aed240202443dea443a34f9865c25fd1a4b3a97fcc1984271682c20`;
> `check` and `fits` exit 0, and two builds are byte-identical. No 0.2.0 in-game run: the
> launcher is not installed in this environment.

### Publication route (campaign ruling 2026-11-18)

The submission form is gone; publishing is `horizon-rust/horizon-mods#1147` — the artifact
is attached to a release of the author's own repository under the entry's exact `file`
name, a PR adds the `[[versions]]` record and links the file, and a **maintainer** runs
`scripts/stage.sh` (a red `check` before that is the expected `fetch.sh` failure). This
job owns no submission; the orchestrator's `publish-ready.py` does the bookkeeping.

Skills: superpowers/using-superpowers,brainstorming,writing-plans,test-driven-development,systematic-debugging,verification-before-completion; BeautifulRust/rust-best-practices,rust-port
