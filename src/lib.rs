//! InControl for Horizon — one flat SDK package: the mod's code, no content.
//!
//! Ported from InControl 1.21-10.3.0 (McJtyMods/InControl, MIT); see `notes.md`
//! for the upstream evidence, the helper → hook table, the deviations and the
//! verification. The pure rule engine lives in [`runtime`] and is unit-tested by
//! `cargo test`; this file is the glue — the one `impl Hooks`, the state the
//! rules load into, and the SDK calls that read a level and take a mob back out.
//!
//! Five moments, the same five the retired bundle injected:
//!
//! - `server_about_to_start`: read `config/incontrol/spawn.json` and
//!   `config/incontrol/loot.json`, keeping the rules already in force when a
//!   file cannot be read or parsed.
//! - `mob_spawned`: decide the fresh mob against the spawn rules; a deny takes
//!   it out of the level with no death and no loot.
//! - `loot_drops`: rewrite the rolled drops through the loot rules.
//! - `register_commands` / `run_mod_command`: `/incontrol reload` and
//!   `/incontrol status` at permission level 1, with the 0.1.0 readout text
//!   unchanged.
//!
//! The rule files are the calling mod's own config folder (`Host::config_files`,
//! which owns `config/<mod id>/`), so no raw filesystem path is read. The port
//! has no logger: diagnostics are the command readout and the skipped-rule
//! counts it reports (recorded in `notes.md`).
//!
//! `#[rustfmt::skip]` on the module is deliberate: `src/runtime.rs` must stay
//! byte-identical to the retired copy, and a few of its lines are wider than
//! rustfmt's default, so formatting it would break the campaign's carry rule for
//! a cosmetic diff (`ports/xnet/notes.md` trap 27 records the same situation).
#[rustfmt::skip]
pub mod runtime;

use std::sync::Mutex;
use std::sync::MutexGuard;

use mod_api::serde_json::Value;
use mod_api::{
    self, Command, Component, Compound, ConfigFileError, ConfigFiles, ConfigPath, EntityRef, Flow,
    Hooks, Host, Identifier, ItemKey, ItemStack, LevelRef, LightLayer, LootDropsEvent,
    MobSpawnedEvent, RandomSource, RegisterCommandsEvent, RunModCommandEvent,
    ServerAboutToStartEvent, ServerHost, Tag, Text,
};

use crate::runtime::{
    Decision, Light, Loaded, LootLoaded, LootOp, LootQuery, Spawn, decide, load, load_loot,
    loot_ops, validate_loot_items,
};

/// The mod's folder under `config/`, as upstream (`RulesManager.readAllRules`).
const RULE_DIR: &str = "incontrol";
/// The spawn rules.
const SPAWN_FILE: &str = "spawn.json";
/// The loot rules.
const LOOT_FILE: &str = "loot.json";
/// The folder the readout names; the API hands no filesystem path.
const RULE_DIR_DISPLAY: &str = "config/incontrol";
/// How far a mob may have drifted from the spawn position the hook reports and
/// still be the mob that spawned (the 0.1.0 glue required an exact match).
const SPAWN_POSITION_TOLERANCE: f64 = 1.0e-6;

/// The outcome of one load, for the command feedback.
struct Report {
    /// One line for the caller.
    message: String,
    /// One line per skipped rule, inert condition or unknown item.
    warnings: Vec<String>,
}

/// The rules in force and how the last load went.
#[derive(Default)]
struct Core {
    /// The spawn rules in force; `None` until a file has been read.
    spawn: Option<Loaded>,
    /// The loot rules in force; `None` until a file has been read.
    loot: Option<LootLoaded>,
    /// Whether the loot items have been checked against the item registry,
    /// which is only joined once a world opens (not at `about_to_start`).
    loot_checked: bool,
    /// How many spawn rules the last load skipped or found inert.
    skipped: usize,
    /// How many loot rules or item entries the last load skipped.
    loot_skipped: usize,
    /// The last failure to read or parse a file.
    error: Option<String>,
}

/// The mod's session state. One `Mod` lives for the session (the generated
/// `hooks()` boxes one `Default`), and `Hooks` is `Send + Sync`, so the state is
/// a `Mutex` and not a `RefCell`.
#[derive(Default)]
struct Mod {
    core: Mutex<Option<Core>>,
}

impl Mod {
    fn core(&self) -> MutexGuard<'_, Option<Core>> {
        self.core
            .lock()
            .expect("the incontrol rule-state lock is not held across a panic")
    }

    /// Reads both rule files and puts them in force.
    ///
    /// A read or parse failure keeps the previous rules of that file and records
    /// the error; the other file still reloads. Item ids are checked on the first
    /// drop, when the item registry is joined.
    fn reload(&self, files: &dyn ConfigFiles) -> Report {
        let spawn_outcome = read_rules(files, SPAWN_FILE, |text| {
            load(text).map_err(|error| error.to_string())
        });
        let loot_outcome = read_rules(files, LOOT_FILE, |text| {
            load_loot(text).map_err(|error| error.to_string())
        });

        let mut guard = self.core();
        let core = guard.get_or_insert_with(Core::default);
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        match spawn_outcome {
            Ok(Some(mut loaded)) => {
                core.skipped = loaded.warnings.len();
                for warning in loaded.warnings.drain(..) {
                    warnings.push(format!("spawn.json {warning}"));
                }
                core.spawn = Some(loaded);
            }
            // No file yet: upstream reads none and spawns stay vanilla.
            Ok(None) => {
                core.skipped = 0;
                core.spawn = Some(Loaded {
                    rules: Vec::new(),
                    warnings: Vec::new(),
                });
            }
            Err(reason) => errors.push(reason),
        }
        match loot_outcome {
            Ok(Some(mut loaded)) => {
                core.loot_skipped = loaded.warnings.len();
                for warning in loaded.warnings.drain(..) {
                    warnings.push(format!("loot.json {warning}"));
                }
                core.loot = Some(loaded);
                core.loot_checked = false;
            }
            Ok(None) => {
                core.loot_skipped = 0;
                core.loot = Some(LootLoaded {
                    rules: Vec::new(),
                    warnings: Vec::new(),
                });
                core.loot_checked = true;
            }
            Err(reason) => errors.push(reason),
        }

        let spawn_count = core.spawn.as_ref().map_or(0, |loaded| loaded.rules.len());
        let loot_count = core.loot.as_ref().map_or(0, |loaded| loaded.rules.len());
        let message = if errors.is_empty() {
            format!(
                "Reloaded InControl rules: {spawn_count} spawn rules ({} skipped), {loot_count} loot rules ({} skipped)",
                core.skipped, core.loot_skipped
            )
        } else {
            format!(
                "InControl reload failed: {} (keeping {spawn_count} spawn rules, {loot_count} loot rules)",
                errors.join("; ")
            )
        };
        core.error = (!errors.is_empty()).then(|| errors.join("; "));
        Report { message, warnings }
    }

    /// The end of a fresh mob's spawn: a deny rule takes it back out.
    fn veto(&self, host: &mut dyn ServerHost, event: &MobSpawnedEvent<'_>) {
        let pos = event.pos;
        let at = [pos[0] as i32, pos[1] as i32, pos[2] as i32];
        // The mob is already in a loaded level, so the light probe can read it
        // (upstream's position with Java's `(int)` cast). A read the level cannot
        // answer fails the light conditions, as upstream's invalid chunk does.
        let light = host.level(event.dimension).and_then(|level| {
            Some(Light {
                block: u8::try_from(level.brightness(host, LightLayer::Block, at)?).ok()?,
                sky: u8::try_from(level.brightness(host, LightLayer::Sky, at)?).ok()?,
            })
        });
        let decision = {
            let guard = self.core();
            let Some(loaded) = guard.as_ref().and_then(|core| core.spawn.as_ref()) else {
                return;
            };
            decide(
                &loaded.rules,
                Spawn {
                    dimension: event.dimension,
                    mob: event.kind,
                    pos,
                    light,
                    natural: event.natural,
                },
            )
        };
        if let Decision::Deny { .. } = decision {
            discard_newest(host, event.dimension, event.kind, pos);
        }
    }

    /// Drops are being rolled: rewrite them through `loot.json`.
    ///
    /// A block break names its block and a mob death names its type, exactly the
    /// two sources a loot rule can match. A drop set with no loot table (a leashed
    /// mob's lead) is left alone, as upstream's loot modifier only sees tables.
    fn rewrite_drops(&self, host: &mut dyn ServerHost, event: &mut LootDropsEvent<'_>) {
        let context = event.context;
        if context.table.is_empty() {
            return;
        }
        let block = context
            .block
            .and_then(|(_, state)| host.block_state_string(state))
            .map(|state| {
                // `minecraft:stone[property=value]` → `minecraft:stone`.
                state
                    .split_once('[')
                    .map_or(state.clone(), |(id, _)| id.to_owned())
            });
        self.ensure_loot_items(host);
        let looting = context
            .weapon_item(host)
            .and_then(|stack| stack.get(&Looting))
            .unwrap_or(0);
        let level = host.level(context.dimension);
        let ops = {
            let guard = self.core();
            let Some(loot) = guard.as_ref().and_then(|core| core.loot.as_ref()) else {
                return;
            };
            if loot.rules.is_empty() {
                return;
            }
            let query = LootQuery {
                mob: context.entity.map(|(_, kind)| kind),
                block: block.as_deref(),
                dimension: context.dimension,
                looting,
            };
            loot_ops(&loot.rules, query, &mut |bound| roll(level, host, bound))
        };
        apply_loot_ops(host, event.drops, ops);
    }

    /// Drops the loot entries naming an item the game does not have.
    ///
    /// The item registry is only joined once a world opens, so this runs on the
    /// first set of drops rather than at load time; it reports once and is then
    /// marked done until the next reload.
    fn ensure_loot_items(&self, host: &mut dyn ServerHost) {
        let keys: &dyn Host = host;
        let mut guard = self.core();
        let Some(core) = guard.as_mut() else {
            return;
        };
        if core.loot_checked {
            return;
        }
        if let Some(loot) = core.loot.as_mut() {
            validate_loot_items(loot, |id| item_key(keys, id).is_some());
        }
        core.loot_skipped = core.loot.as_ref().map_or(0, |loot| loot.warnings.len());
        core.loot_checked = true;
    }

    /// The readout `/incontrol status` prints, one line per entry.
    fn status_lines(&self) -> Vec<String> {
        let guard = self.core();
        let Some(core) = guard.as_ref() else {
            return vec!["InControl is not initialised.".to_owned()];
        };
        let spawn = core.spawn.as_ref().map_or(0, |loaded| loaded.rules.len());
        let loot = core.loot.as_ref().map_or(0, |loaded| loaded.rules.len());
        let mut lines = vec![format!("InControl: {spawn} spawn rules, {loot} loot rules")];
        if core.skipped + core.loot_skipped > 0 {
            lines.push(format!(
                "skipped: {} spawn conditions, {} loot entries",
                core.skipped, core.loot_skipped
            ));
        }
        lines.push(format!("rules: {RULE_DIR_DISPLAY}"));
        if let Some(error) = &core.error {
            lines.push(format!("last error: {error}"));
        }
        lines
    }
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    /// Declares `/incontrol reload` and `/incontrol status` at permission level 1
    /// (upstream's reload level, `CmdReload.java:16-40`).
    fn register_commands(&self, _host: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        event.commands.push(Command::op("incontrol reload", 1));
        event.commands.push(Command::op("incontrol status", 1));
    }

    /// Before the server starts: read both rule files.
    fn server_about_to_start(
        &self,
        host: &mut dyn ServerHost,
        _event: &mut ServerAboutToStartEvent,
    ) {
        let _ = self.reload(host.config_files());
    }

    /// A fresh mob of `kind` joined `dimension` at `pos`: the spawn rules decide.
    fn mob_spawned(&self, host: &mut dyn ServerHost, event: &mut MobSpawnedEvent<'_>) {
        self.veto(host, event);
    }

    /// The drops a mob death or a block break rolled: `loot.json` rewrites them.
    fn loot_drops(&self, host: &mut dyn ServerHost, event: &mut LootDropsEvent<'_>) {
        self.rewrite_drops(host, event);
    }

    /// Runs the two commands; another usage is left to the game.
    fn run_mod_command(
        &self,
        host: &mut dyn ServerHost,
        event: &mut RunModCommandEvent<'_>,
    ) -> Flow {
        let literals: Vec<&str> = event.command.literals.iter().map(String::as_str).collect();
        match literals.as_slice() {
            ["incontrol", "reload"] => {
                let report = self.reload(host.config_files());
                feedback(host, report.message);
                for warning in report.warnings.iter().take(5) {
                    feedback(host, warning.clone());
                }
                if report.warnings.len() > 5 {
                    feedback(host, format!("and {} more", report.warnings.len() - 5));
                }
                Flow::Cancel
            }
            ["incontrol", "status"] => {
                for line in self.status_lines() {
                    feedback(host, line);
                }
                Flow::Cancel
            }
            _ => Flow::Continue,
        }
    }
}

/// Reads one rule file, if it exists. A read error is the caller's to keep.
fn read_rules<T>(
    files: &dyn ConfigFiles,
    name: &str,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<Option<T>, String> {
    let path = ConfigPath::new(&format!("{RULE_DIR}/{name}"))
        .map_err(|error| format!("{RULE_DIR}/{name}: {error}"))?;
    let bytes = match files.read(&path) {
        Ok(bytes) => bytes,
        Err(ConfigFileError::NotFound) => return Ok(None),
        Err(error) => return Err(format!("{RULE_DIR}/{name}: {error}")),
    };
    let text = String::from_utf8_lossy(&bytes);
    parse(&text)
        .map(Some)
        .map_err(|reason| format!("{RULE_DIR}/{name}: {reason}"))
}

/// Takes the mob that just spawned back out of the level: no death, no loot, no
/// tick of life (vanilla's `Entity.discard`).
///
/// The hook names the mob by dimension, kind and position, so the newest entity of
/// that kind at exactly `pos` is the one. A mob that cannot be found leaves the
/// spawn as it is — the deny rule has nothing to act on.
fn discard_newest(host: &mut dyn ServerHost, dimension: &str, mob: &str, pos: [f64; 3]) -> bool {
    let Some(level) = host.level(dimension) else {
        return false;
    };
    // The mob's own box meets a box centred on its position.
    let min = [pos[0] - 1.0, pos[1] - 1.0, pos[2] - 1.0];
    let max = [pos[0] + 1.0, pos[1] + 1.0, pos[2] + 1.0];
    let keys: &dyn Host = host;
    let Some(entities) = level.entities_in(keys, min, max) else {
        return false;
    };
    let mut newest: Option<(u32, EntityRef)> = None;
    for entity in entities {
        let Some(kind) = entity.kind(keys) else {
            continue;
        };
        if kind.id(keys) != mob {
            continue;
        }
        let Some(entity_pos) = entity.position(keys) else {
            continue;
        };
        if (entity_pos[0] - pos[0]).abs() > SPAWN_POSITION_TOLERANCE
            || (entity_pos[1] - pos[1]).abs() > SPAWN_POSITION_TOLERANCE
            || (entity_pos[2] - pos[2]).abs() > SPAWN_POSITION_TOLERANCE
        {
            continue;
        }
        let id = entity.id();
        if newest.is_none_or(|(newest_id, _)| id > newest_id) {
            newest = Some((id, entity));
        }
    }
    newest.is_some_and(|(_, entity)| entity.discard(host))
}

/// Applies the engine's operations, in order, to the rolled stacks.
fn apply_loot_ops(host: &mut dyn ServerHost, drops: &mut Vec<ItemStack>, ops: Vec<LootOp>) {
    let keys: &dyn Host = host;
    for op in ops {
        match op {
            LootOp::Clear => drops.clear(),
            LootOp::Remove(ids) => drops.retain(|stack| {
                let held = stack.item().id(keys);
                !ids.contains(&held)
            }),
            LootOp::Add(stacks) => {
                for drop in stacks {
                    let Some(mut stack) = item_key(keys, &drop.item)
                        .and_then(|item| ItemStack::new(item, drop.count))
                    else {
                        continue;
                    };
                    if let Some(nbt) = drop.nbt.as_deref()
                        && let Some(tag) = json_tag(nbt)
                    {
                        let mut patch = Compound::new();
                        patch.insert("minecraft:custom_data".into(), tag);
                        let _ = stack.set_components(patch);
                    }
                    drops.push(stack);
                }
            }
        }
    }
}

/// One chance or count draw from the drop's own level, as 0.1.0 drew from
/// `game.world.random`: a replay of the same world stays deterministic. 0 where
/// the level is not open, which the engine treats as the lowest draw.
fn roll(level: Option<LevelRef>, host: &mut dyn ServerHost, bound: u32) -> u32 {
    let Some(level) = level else {
        return 0;
    };
    let Ok(bound) = i32::try_from(bound) else {
        return 0;
    };
    level
        .with_random(host, |random| random.next_int_bound(bound).unwrap_or(0))
        .and_then(|drawn| u32::try_from(drawn).ok())
        .unwrap_or(0)
}

/// The item a registry id names, or `None` when the game does not have it.
fn item_key(host: &dyn Host, id: &str) -> Option<ItemKey> {
    Identifier::parse(id).ok().and_then(|id| host.item_key(&id))
}

/// A stack's `minecraft:looting` level, read from its `minecraft:enchantments`
/// component (the killer's weapon, as 0.1.0 read the held stack's level).
struct Looting;

impl Component for Looting {
    type Value = u32;

    fn id(&self) -> &str {
        "minecraft:enchantments"
    }

    fn decode(&self, value: &Tag) -> Option<u32> {
        match value.get("minecraft:looting")? {
            Tag::Byte(level) => u32::try_from(*level).ok(),
            Tag::Short(level) => u32::try_from(*level).ok(),
            Tag::Int(level) => u32::try_from(*level).ok(),
            _ => None,
        }
    }

    fn encode(&self, value: u32) -> Tag {
        let mut levels = Compound::new();
        levels.insert(
            "minecraft:looting".into(),
            Tag::Int(i32::try_from(value).unwrap_or(i32::MAX)),
        );
        Tag::Compound(levels)
    }
}

/// A rule's `nbt` object as the tag of an added stack's `minecraft:custom_data`.
///
/// The retired glue handed this to the game's JSON→NBT converter
/// (`horizon_client_core::scoreboard::json_to_tag`); the SDK exports no such
/// function, so the conversion is here, with the same rules: a JSON integer takes
/// the narrowest signed tag, a non-integer is a `Double`, a boolean is a `Byte`,
/// and an object or array nests. `null` is refused (NBT has no null to hold it).
fn json_tag(text: &str) -> Option<Tag> {
    let value: Value = mod_api::serde_json::from_str(text).ok()?;
    tag_of(&value)
}

fn tag_of(value: &Value) -> Option<Tag> {
    match value {
        Value::Null => None,
        Value::Bool(flag) => Some(Tag::Byte(i8::from(*flag))),
        Value::Number(number) => Some(number_tag(number)),
        Value::String(text) => Some(Tag::String(text.as_str().into())),
        Value::Array(items) => items
            .iter()
            .map(tag_of)
            .collect::<Option<Vec<Tag>>>()
            .map(Tag::List),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| tag_of(value).map(|tag| (key.as_str().into(), tag)))
            .collect::<Option<Compound>>()
            .map(Tag::Compound),
    }
}

fn number_tag(number: &mod_api::serde_json::Number) -> Tag {
    if let Some(value) = number.as_i64() {
        if let Ok(value) = i8::try_from(value) {
            return Tag::Byte(value);
        }
        if let Ok(value) = i16::try_from(value) {
            return Tag::Short(value);
        }
        if let Ok(value) = i32::try_from(value) {
            return Tag::Int(value);
        }
        return Tag::Long(value);
    }
    Tag::Double(number.as_f64().unwrap_or(0.0).to_bits())
}

/// Sends one line to the command's own source (the caller, not a broadcast).
fn feedback(host: &mut dyn ServerHost, line: String) {
    let _ = host.commands().feedback(Text::literal(line));
}
