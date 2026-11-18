# InControl — re-pin audit (Horizon `83ad384f` / mixins `e8cd0aa`)

Read-only audit, 2026-11-05. No port code, manifests or builds were changed;
this file is the only write.

| | |
|---|---|
| New Horizon pin | `83ad384f48d0742746d518737511882680aab5b7` (427 hooks, 159 helpers), `ports/_shared/pins/horizon-83ad384f` |
| New mixins pin | `e8cd0aa38cffef49cb2e088ef716baa107148c02` |
| Port still pinned at | `c9d474dd…` / `d9c83d7…` (`compatibility.json`, `mixins/incontrol/horizon-mixin.toml`) |
| Sources read | `ports/incontrol/notes.md`, ARCHITECTURE item 24; upstream `SpawnRule.java`, `GenericRuleEvaluator.java`, `SpawnWhen.java`, `ForgeEventHandlers.java` at `9b95c726…`; pin `native_hooks.rs`, `native_mobs.rs`, `native_living_refs.rs`, `native_entity_data.rs`, `native_entity_refs.rs`, `native_entity_motion.rs`, `native_world_read.rs`, `native_server.rs`, `items.rs`/`item_data.rs`, `native-hooks.json` |
| Method | Each deferred condition/action/file was searched in `native-hooks.json` and the pin's client/world sources; classified **usable** (writable out / cancel / catalogued helper), **hook-only** (listen), **pin-internal** (reachable but uncatalogued), or **absent**. |

## Summary

| | |
|---|---:|
| Deferred feature groups audited | **11** |
| Newly realizable at this pin | **5** |
| Partially realizable | **1** |
| Still blocked | **5** |
| Highest-value next step | **`finalize` rules** with the now-present write helpers (`native_living_set_equipment`, `native_set_mob_health`, `native_entity_merge_nbt`, `native_entity_add_tag`, `NativeSpawn::name`) evaluated at `mob_spawned`; pair with **`itemcount`** (killer's Looting) and **item `nbt`** on added drops, which share the loot path. E2E: a finalize rule gives a zombie a helmet, 30 health and `NoAI`, and a differential without the rule shows none. |

The repin is mechanical (heyberryshutup's `REPIN.md` recipe), then slice 4 can
take the three gaps the notes list as "A" (`finalize`, `itemcount`, `nbt`) in
one job; `events.json`/`spawner.json`/area/click rules are a second job.

## Deferred feature audit

Legend: **class** = usable / hook-only / pin-internal / absent; **verdict** =
realizable now / partial / blocked. Line numbers are in the pin root above.

| # | Deferred feature (notes / ARCHITECTURE) | New-pin symbol(s) (file:line) | Class | Verdict | Exact next step |
|---|---|---|---|---|---|
| 1 | **`biome` / `biometags` conditions** (notes: "no biome read") | No catalogued biome helper. Pin-internal only: `Game::biome_name_at` (`game/fishing.rs:460`, `pub(super)`) and `MobData::biome_has` (`mobs/data.rs:117`). `native_biome_colors` (`native_hooks.rs:2872`) is client colour data, not a world read. | pin-internal / absent as API | **blocked** | File an API request for `native_biome(pos)` + `native_biome_in_tag`; do not rely on the `pub(super)` reads unless the campaign accepts uncatalogued helpers. Rule stays inert meanwhile. |
| 2 | **`mob: "#tag"` entity-type tag condition** (notes: "no entity-type tag query") | No catalogued entity-tag helper. Pin-internal only: `MobData::entity_has` (`mobs/data.rs:112`), used by `commands.rs:1019`/`loot.rs:1190`. | pin-internal / absent as API | **blocked** | File an API request for `native_entity_in_tag(kind, tag)`; keep the rule inert. `native_entity_tags` (`native_entity_data.rs:48`) is **scoreboard tags**, not entity-type tags — do not confuse them. |
| 3 | **`minlight_full`/`maxlight_full`, `light` expression** (notes: `skyDarken` absent) | `native_light` (`:8516`, raw block+sky), `native_weather` (`:8527`, rain/thunder levels), `native_day_time` (`:8533`). No `skyDarken` probe. | usable reads, no exact probe | **blocked (exact)** | Either keep inert, or add an approximation guarded by a config switch and document the difference. `minlight`/`maxlight`/`_sky` (already shipped) remain exact. |
| 4 | **`finalize` rules** — equipment, health, AI, NBT, custom name, attributes, potions, scoreboard tags, give/drop/setblock/command/message | `native_mob_spawned` (`:7221`, `&mut self`), `native_remove_mob` (`native_mobs.rs:302`), `native_set_mob_health` (`:276`), `native_living_set_equipment` (`native_living_refs.rs:122`) with `native_entity`/`native_living` (`native_entity_refs.rs:41`, `native_living_refs.rs:66`), `native_entity_merge_nbt` (`native_entity_data.rs:32`), `native_entity_add_tag` (`:53`), `NativeSpawn::name` (`native_mobs.rs:128`), `native_summon` (`native_entities.rs:141`), `native_give_items` (`native_server.rs:682`), `native_set_block` (`:8464`), `native_run_command` (`native_server.rs:479`), `native_potential_spawns` (`:5821`). | usable | **realizable now** | Add `When::Finalize` to the engine, evaluated at `mob_spawned` after the mob is found (`newest_mob`). Apply: gear via `living_set_equipment`, health via `set_mob_health`, AI/attributes/NBT/potions/name via `entity_merge_nbt` (or `NativeSpawn.name` when spawning), tags via `entity_add_tag`, commands via `native_run_command`, drops via `native_summon(minecraft:item)`. Deny removes the just-joined mob. **Deviation:** upstream runs before `FinalizeSpawnEvent` joins; the port applies at join (no AI tick yet). For exact natural-spawn pre-finalize, use `potential_spawns` + `native_summon` with the customized NBT — document if chosen. |
| 5 | **`despawn` rules** (`MobDespawnEvent`: allow/deny/default) | No despawn hook in the catalog (only `item_despawn`). `native_entity_tick_pre` (`:7336`, cancel) can stop a tick but is not a despawn decision and would also stop AI/gravity. | absent | **blocked** | File an API request for a `mob_despawn`/`allow_despawn` hook. Do not approximate with `entity_tick_pre`; it changes tick semantics. |
| 6 | **`itemcount`** — looting-scaled counts, e.g. `"5/7-10/20-30"` | `NativeLootDrops.player` (`mod_api.rs:153`), `tool` (`:155`); **`tool` is `None` for mob deaths** (`creatures.rs:1700`). Recover the killer's weapon: `native_server_players` (`native_server.rs:204`) → `native_entity` (`native_entity_refs.rs:41`) → `native_living` (`native_living_refs.rs:66`) → `native_living_hand_item` (`:105`) → `ItemStack::enchantment_level("minecraft:looting")` (`horizon-items/src/item_data.rs:296`). The host is not in `native_server_players`; use `native_held`/the local player for it. | usable (via player lookup) | **realizable now** | Resolve `context.player` to a `NativeServerPlayer` (guests) or the host's held stack, read Looting, then pick the `/`-separated tier. Blocks can use `context.tool` directly (set for block breaks). Test with a Looting III sword vs a bare hand on the same mob. |
| 7 | **Item `nbt` on added drops** | `ItemStack::data_mut` (`horizon-items/src/item_data.rs:280`), `native_set_component` (`native_components.rs:49`), `ItemStack::extra`. | usable | **realizable now** | Apply the `nbt` object to the added stack's data (or `minecraft:custom_data` if only that is exposed), then push it. Test with a named/enchanted added drop and assert the component on the dropped entity. |
| 8 | **`events.json`, `spawner.json` (summon-aid), `effects.json`, area rules, right/left-click, place, break rules** | `native_mob_killed` (`:7437`), `native_destroy_block` (`:6763`), `native_place_block` (`:6775`), `native_right_click_block` (`:6790`), `native_left_click_block` (`:2533`), `native_server_tick` (`:2456`), `native_run_command` (`native_server.rs:479`), `native_spawn_mob` (`native_mobs.rs:166`)/`native_summon` (`native_entities.rs:141`), `native_potential_spawns` (`:5821`), `native_server_players` (`:204`). | usable | **realizable now** | Slice: events (`mob_killed` + block broken + custom queue in `server_tick`), area rules (own config + positions), click/place/break rules (cancel then act), `effects.json` via `/effect give` through `native_run_command`. Summon-aid: add `SpawnEntry`s in `potential_spawns` for vanilla mobs; custom NPCs stay out. |
| 9 | **Other conditions** — time, distance, weather, cave/seesky, block/blocktest, equipment, scoreboard tags, counts/daycount/number/phase | `native_day_time` (`:8533`), `native_entity_distance_to_sqr` (`native_entity_motion.rs:119`) + `native_server_players` (`:204`), `native_weather` (`:8527`), `native_light` (`:8516`) + `native_block` (`:9041`), `native_block_property` (`:9063`), `native_block_name_in_tag` (`native_world_read.rs:99`), `native_living_equipment` (`native_living_refs.rs:73`), `native_entity_tags` (`native_entity_data.rs:48`), `native_mobs` (`native_mobs.rs:223`), own phase/number state. | usable for most; absent for difficulty read, spawn type, `canspawnhere`, `notcolliding`, slime, structure, biome | **partial** | Implement the listed keys in the existing evaluator. Keep blocked keys inert and reported (`difficulty` can be cached from `native_difficulty_changed`, `:6919`, if the initial value is primed). `seesky` can use sky light 15 as an approximation; document it. |
| 10 | **Experience rules** (`experience.json`: setxp/multxp/addxp) | `native_block_exp` (`:1187`) modifies **block** XP only. No mob `LivingExperienceDropEvent` hook; `native_player_xp_change` (`:4633`) is a broad player-XP listener, not a per-mob drop. | absent | **blocked** | File an API request for `native_living_experience_drop(entity, kind, amount)`. Do not cancel/re-give player XP as a substitute (it would affect orbs and other sources). |
| 11 | **More subcommands** (`killmobs`, `list`, `info`, `days`, `phases`, `numbers`, `area`) | `native_register_commands` (`:3899`), `native_perform_command` (`:3909`), `native_feedback` (`native_server.rs:436`), `native_server_players` (`:204`), `native_mobs` (`native_mobs.rs:223`), `native_day_time` (`:8533`), own phase/number storage. | usable | **realizable now** | Add the verbs on the existing engine state; `killmobs` uses `native_mobs` + `native_remove_mob`/`native_kill_mob`, `list`/`info` use rule metadata, `days`/`phases`/`numbers` use own state. No new hook needed. |

## Requested-hook checks

- **`living_equipment_change` / `living_equipment_listened`** (`native_hooks.rs:7188`,
  `:7174`): **usable**, but InControl does not need the *change* stream; its
  equipment **conditions** (`helmet`, `chestplate`, `leggings`, `boots`,
  `helditem`, `offhanditem`, `bothhandsitem`) need **reads**. Those exist:
  `native_living_equipment` (`native_living_refs.rs:73`) and
  `native_living_hand_item` (`:105`) on a player `LivingRef` (find the nearest
  player through `native_server_players`; the host is not in that list, add
  `game.player`/`native_acting_player` separately). Upstream checks the
  **nearest player's** gear, not the mob's.
- **`player_velocity` / `push_player`** (`native_hooks.rs:5307`,
  `native_motion.rs:31`): `player_velocity` is listen-only; `push_player` is
  client-local; `entity_set_velocity` (`native_entity_motion.rs:51`) refuses
  players. **Not used by InControl's rules**; no deferred condition/action
  needs player motion.
- **`raid_state_change`** (`native_hooks.rs:5909`): **hook-only** (listen,
  raid id/pos/state). Upstream InControl has no raid condition or action
  (`GenericRuleEvaluator`/`RuleKeys` checked): raid mobs already arrive
  through `mob_spawned` with `natural=false`, so denial needs no raid hook.
  **Not applicable** to this port; it is the hook utilitarian's no-raider
  feature needs.
- **`player_name_entity`** (`native_hooks.rs:5227`): **hook-only** (listen,
  player/entity id/name). InControl's `finalize` `customname` action needs a
  **write**, which this hook cannot do: use `native_entity_merge_nbt`
  (`native_entity_data.rs:32`, `CustomName`) or `NativeSpawn::name`
  (`native_mobs.rs:128`) when summoning. **Not applicable** as a write path.

## Pin state / repin prerequisites

The port is still on `c9d474d` / `d9c83d7`. Before the slices above:

1. `compatibility.json`: new `horizon_revision`, `game.rs` hash `996432b9…`,
   new mixins revision; `mixin_schema` stays 6.
2. `mixins/incontrol/horizon-mixin.toml`: same 40-hex revision.
3. `tools/horizon.py`: embedded tests now `--lib`; `horizon-ctl` is in
   `horizon-client-lib` (`-p horizon-client-lib --bin horizon-ctl`).
4. One-time `cargo fetch --locked` for `zune-jpeg`; then
   `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2`.
5. The existing 68 tests and e2e should re-run unchanged; the new features
   should add their own differentials (rule-on/rule-off in the same run).

## Evidence

- `native-hooks.json`: 427 hooks / 159 helpers; the hooks cited above are
  present at v1. `native_player_velocity`, `native_raid_state_change`,
  `native_player_name_entity` all have `contract: "listen"` with no out field.
- `NativeLootDrops.tool` is `None` in the mob-death builder
  (`creatures.rs:1693-1706`), so `itemcount` must resolve the killer's hand
  rather than read `tool`.
- `native_entity_tags` is documented as `Entity.getTags()` (scoreboard tags);
  it is not an entity-type tag registry.
- Read-only: nothing under `horizon/`, `horizon-mixins/` or
  `ports/_shared/pins/` was modified.
