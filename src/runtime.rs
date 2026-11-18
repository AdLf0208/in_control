// InControl's `spawn.json` and `loot.json` rule engines, pure and testable.
//
// The game glue in `support.rs` loads `config/incontrol/spawn.json` and calls
// [`decide`] for every fresh mob, then loads `config/incontrol/loot.json` and
// calls [`loot_ops`] for every set of rolled drops. This module owns the JSON
// parser, the rule models and the match/precedence semantics, so none of it
// needs a world.
//
// Subset of upstream 1.21-10.3.0 (`SpawnRule`/`GenericRuleEvaluator` and
// `LootRule`/`InControlLootModifier`, see `notes.md`): for spawns `when`
// (`position`/`onjoin`), `mob`, `mod`, `dimension`, `minheight`, `maxheight`,
// `minlight`, `maxlight`, `minlight_sky`, `maxlight_sky`, `result` and
// `continue`; for loot the `mob`/`mod`/`block`/`dimension` conditions and the
// brief's `drops`/`add`/`replace`/`remove`/`result`/`continue` actions plus
// upstream's `item`/`removeall` aliases. Every other key makes the rule inert:
// a rule this port cannot evaluate in full never matches, which can only ever
// deny fewer spawns than upstream, never more, and can only ever leave the
// rolled drops alone.

use std::fmt;

/// The lifecycle moment a rule is written for.
///
/// `Position` is upstream's default: a natural spawn attempt, checked before the
/// mob joins the level. `OnJoin` covers every fresh entity, commands and spawn
/// eggs included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// A natural spawn attempt (`MobSpawnEvent.PositionCheck`).
    Position,
    /// Any fresh entity entering the level (`EntityJoinLevelEvent`).
    OnJoin,
}

/// What a matching rule asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Keep the game's own decision and stop evaluating later rules.
    Default,
    /// Allow the spawn and stop evaluating later rules.
    Allow,
    /// Cancel the spawn.
    Deny,
}

/// The block and sky light at a spawn position, each 0 to 15.
///
/// The values are raw `LightLayer.BLOCK` and `LightLayer.SKY` readings, as
/// upstream's `minlight`/`maxlight` compare them. `minlight_full` is not
/// modelled: upstream's `getMaxLocalRawBrightness` subtracts the level's
/// `skyDarken`, which this pin exposes no probe for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Light {
    /// Block light (`LightLayer.BLOCK`), 0 to 15.
    pub block: u8,
    /// Sky light (`LightLayer.SKY`), 0 to 15.
    pub sky: u8,
}

/// One parsed `spawn.json` entry.
///
/// A rule with a non-empty [`Rule::unsupported`] list never matches: the port
/// cannot honour a condition it cannot read.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Position in the file, for `/incontrol reload` reports.
    pub index: usize,
    /// The lifecycle moment this rule belongs to.
    pub when: When,
    /// Entity ids or tags that match, any of them.
    pub mobs: Option<Vec<String>>,
    /// Entity-id namespaces that match, any of them.
    pub mods: Option<Vec<String>>,
    /// Dimension ids that match, any of them.
    pub dimensions: Option<Vec<String>>,
    /// Lowest matching block y, inclusive.
    pub min_height: Option<i32>,
    /// Highest matching block y, inclusive.
    pub max_height: Option<i32>,
    /// Lowest matching block light, inclusive.
    pub min_light: Option<i32>,
    /// Highest matching block light, inclusive.
    pub max_light: Option<i32>,
    /// Lowest matching sky light, inclusive.
    pub min_light_sky: Option<i32>,
    /// Highest matching sky light, inclusive.
    pub max_light_sky: Option<i32>,
    /// The action to take.
    pub outcome: Outcome,
    /// Upstream's `continue`: keep evaluating after this match.
    pub again: bool,
    /// Keys and values this port cannot evaluate; they make the rule inert.
    pub unsupported: Vec<String>,
}

/// The rules of one successfully parsed file.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// Parsed rules in file order.
    pub rules: Vec<Rule>,
    /// One line per skipped rule or inert condition, for the reload report.
    pub warnings: Vec<String>,
}

/// A JSON or rule-model error, with the place it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    /// 1-based line.
    pub line: usize,
    /// 1-based byte column.
    pub column: usize,
    /// What was wrong.
    message: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "line {} column {}: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for LoadError {}

/// A minimal JSON value: enough for `spawn.json`, no dependency.
#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// The keys a rule cannot honour. A rule with any is inert.
trait Inert {
    fn unsupported_keys(&self) -> &[String];
}

impl Inert for Rule {
    fn unsupported_keys(&self) -> &[String] {
        &self.unsupported
    }
}

impl Inert for LootRule {
    fn unsupported_keys(&self) -> &[String] {
        &self.unsupported
    }
}

/// Parses one `spawn.json` file.
///
/// # Errors
///
/// Returns [`LoadError`] when the text is not well-formed JSON or its top level
/// is not the array of rules upstream expects.
pub fn load(text: &str) -> Result<Loaded, LoadError> {
    let (rules, warnings) = load_rule_array(text, parse_rule)?;
    Ok(Loaded { rules, warnings })
}

/// Parses one `loot.json` file.
///
/// # Errors
///
/// Returns [`LoadError`] when the text is not well-formed JSON or its top level
/// is not the array of rules upstream expects.
pub fn load_loot(text: &str) -> Result<LootLoaded, LoadError> {
    let (rules, warnings) = load_rule_array(text, parse_loot_rule)?;
    Ok(LootLoaded { rules, warnings })
}

/// The shared shape of both rule files: a top-level array of rule objects,
/// parsed in file order. A malformed rule is skipped with a warning; an inert
/// one is kept but never matches.
fn load_rule_array<T: Inert>(
    text: &str,
    parse: impl Fn(usize, Vec<(String, Json)>) -> Result<T, String>,
) -> Result<(Vec<T>, Vec<String>), LoadError> {
    let root = parse_json(text)?;
    let Json::Array(entries) = root else {
        return Err(LoadError {
            line: 1,
            column: 1,
            message: "the top level must be an array of rules".to_owned(),
        });
    };
    let mut rules = Vec::with_capacity(entries.len());
    let mut warnings = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        let Json::Object(keys) = entry else {
            warnings.push(format!("rule {index}: not an object, skipped"));
            continue;
        };
        match parse(index, keys) {
            Ok(rule) => {
                if !rule.unsupported_keys().is_empty() {
                    warnings.push(format!(
                        "rule {index}: unsupported {}, so it never matches",
                        rule.unsupported_keys().join(", ")
                    ));
                }
                rules.push(rule);
            }
            Err(reason) => warnings.push(format!("rule {index}: {reason}, skipped")),
        }
    }
    Ok((rules, warnings))
}

/// `ResourceLocation.parse`: bare names are `minecraft:` and ids are lowercase.
fn normalize_id(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.contains(':') {
        trimmed
    } else {
        format!("minecraft:{trimmed}")
    }
}

fn parse_rule(index: usize, keys: Vec<(String, Json)>) -> Result<Rule, String> {
    let mut rule = Rule {
        index,
        when: When::Position,
        mobs: None,
        mods: None,
        dimensions: None,
        min_height: None,
        max_height: None,
        min_light: None,
        max_light: None,
        min_light_sky: None,
        max_light_sky: None,
        outcome: Outcome::Default,
        again: false,
        unsupported: Vec::new(),
    };
    // Later duplicates win, as upstream's attribute map overwrites.
    for (key, value) in collapse_duplicates(keys) {
        match key.as_str() {
            "when" => match value {
                Json::String(name) => match name.to_ascii_lowercase().as_str() {
                    "position" => rule.when = When::Position,
                    "onjoin" => rule.when = When::OnJoin,
                    other => rule.unsupported.push(format!("when={other}")),
                },
                _ => return Err("`when` must be a string".to_owned()),
            },
            "mob" => {
                let mut ids = Vec::new();
                for id in string_list(&key, value)? {
                    if let Some(tag) = id.trim().strip_prefix('#') {
                        let tag = normalize_id(tag);
                        rule.unsupported.push(format!("mob tag #{tag}"));
                    } else {
                        ids.push(normalize_id(&id));
                    }
                }
                rule.mobs = Some(ids);
            }
            "mod" => {
                let mods: Vec<String> = string_list(&key, value)?
                    .into_iter()
                    .map(|name| name.trim().to_ascii_lowercase())
                    .collect();
                rule.mods = Some(mods);
            }
            "dimension" => {
                let dimensions = string_list(&key, value)?
                    .into_iter()
                    .map(|id| normalize_id(&id))
                    .collect();
                rule.dimensions = Some(dimensions);
            }
            "minheight" | "maxheight" => {
                let Json::Number(number) = value else {
                    return Err(format!("`{key}` must be a number"));
                };
                if key == "minheight" {
                    rule.min_height = Some(number as i32);
                } else {
                    rule.max_height = Some(number as i32);
                }
            }
            "minlight" | "maxlight" | "minlight_sky" | "maxlight_sky" => {
                let Json::Number(number) = value else {
                    return Err(format!("`{key}` must be a number"));
                };
                let bound = number as i32;
                match key.as_str() {
                    "minlight" => rule.min_light = Some(bound),
                    "maxlight" => rule.max_light = Some(bound),
                    "minlight_sky" => rule.min_light_sky = Some(bound),
                    _ => rule.max_light_sky = Some(bound),
                }
            }
            "result" => {
                let Json::String(name) = value else {
                    return Err("`result` must be a string".to_owned());
                };
                let lower = name.to_ascii_lowercase();
                rule.outcome = if lower == "default" || lower.starts_with("def") {
                    Outcome::Default
                } else if lower == "allow" || lower == "true" {
                    Outcome::Allow
                } else {
                    // Upstream's else branch: `deny_with_actions` and any typo deny.
                    Outcome::Deny
                };
            }
            "continue" => {
                let Json::Bool(value) = value else {
                    return Err("`continue` must be true or false".to_owned());
                };
                rule.again = value;
            }
            other => rule.unsupported.push(other.to_owned()),
        }
    }
    Ok(rule)
}

/// A multi-valued string condition: one string, or a list of strings.
fn string_list(key: &str, value: Json) -> Result<Vec<String>, String> {
    match value {
        Json::String(text) => Ok(vec![text]),
        Json::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Json::String(text) => Ok(text),
                _ => Err(format!("`{key}` must hold strings")),
            })
            .collect(),
        _ => Err(format!("`{key}` must be a string or a list of strings")),
    }
}

/// Later duplicates win, as upstream's attribute map overwrites.
fn collapse_duplicates(keys: Vec<(String, Json)>) -> Vec<(String, Json)> {
    let mut seen: Vec<(String, Json)> = Vec::with_capacity(keys.len());
    for (key, value) in keys {
        if let Some(slot) = seen.iter_mut().find(|(seen, _)| *seen == key) {
            slot.1 = value;
        } else {
            seen.push((key, value));
        }
    }
    seen
}

impl Json {
    /// A compact JSON form, for the `nbt` action the glue turns into
    /// `minecraft:custom_data`.
    fn to_json(&self) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => {
                if value.fract() == 0.0 && value.is_finite() {
                    format!("{value:.0}")
                } else {
                    value.to_string()
                }
            }
            Self::String(text) => {
                let mut out = String::with_capacity(text.len() + 2);
                out.push('"');
                for ch in text.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        other => out.push(other),
                    }
                }
                out.push('"');
                out
            }
            Self::Array(items) => {
                let inner: Vec<String> = items.iter().map(Self::to_json).collect();
                format!("[{}]", inner.join(","))
            }
            Self::Object(fields) => {
                let inner: Vec<String> = fields
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}:{}",
                            Self::String(key.clone()).to_json(),
                            value.to_json()
                        )
                    })
                    .collect();
                format!("{{{}}}", inner.join(","))
            }
        }
    }
}

/// Parses upstream's looting tiers, `"5/7-10/20-30"`: one entry per Looting
/// level, a fixed count or an inclusive `min-max` range.
fn parse_count_tiers(text: &str) -> Option<Vec<(u32, u32)>> {
    let mut tiers = Vec::new();
    for part in text.split('/') {
        let part = part.trim();
        let (min, max) = match part.split_once('-') {
            Some((min, max)) => (parse_count(min)?, parse_count(max)?),
            None => {
                let count = parse_count(part)?;
                (count, count)
            }
        };
        if max < min {
            return None;
        }
        tiers.push((min, max));
    }
    (!tiers.is_empty()).then_some(tiers)
}

fn parse_count(text: &str) -> Option<u32> {
    let count = text.trim().parse::<u32>().ok()?;
    (count <= MAX_COUNT).then_some(count)
}

/// One spawn decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// No rule denies this spawn; the game keeps its own decision.
    Allow,
    /// The spawn is denied, by the rule at this index in `spawn.json`.
    Deny { rule: usize },
}

/// What one fresh mob looks like to the engine.
#[derive(Debug, Clone, Copy)]
pub struct Spawn<'a> {
    /// The dimension id (`minecraft:overworld`).
    pub dimension: &'a str,
    /// The entity id (`minecraft:zombie`).
    pub mob: &'a str,
    /// The spawn position.
    pub pos: [f64; 3],
    /// Block and sky light at the spawn position, or `None` when the level
    /// could not be read there (upstream's invalid-chunk case).
    pub light: Option<Light>,
    /// True for a natural spawn attempt; false for a command, spawn egg,
    /// spawner, breed or raid.
    pub natural: bool,
}

/// Decides one spawn against the rules, in file order.
///
/// The first matching rule decides; `continue` keeps evaluating and a deny
/// cannot be undone by a later rule, as upstream's cancelled spawn cannot.
#[must_use]
pub fn decide(rules: &[Rule], spawn: Spawn<'_>) -> Decision {
    let mut denied = None;
    for rule in rules {
        if !rule.matches(spawn) {
            continue;
        }
        if rule.outcome == Outcome::Deny && denied.is_none() {
            denied = Some(Decision::Deny { rule: rule.index });
        }
        if !rule.again {
            break;
        }
    }
    denied.unwrap_or(Decision::Allow)
}

impl Rule {
    /// Whether every condition of this rule matches `spawn`.
    fn matches(&self, spawn: Spawn<'_>) -> bool {
        if !self.unsupported.is_empty() {
            return false;
        }
        if self.when == When::Position && !spawn.natural {
            return false;
        }
        let mob = normalize_id(spawn.mob);
        let dimension = normalize_id(spawn.dimension);
        if let Some(mobs) = &self.mobs
            && !mobs.is_empty()
            && !mobs.contains(&mob)
        {
            return false;
        }
        let namespace = namespace(&mob);
        if let Some(mods) = &self.mods
            && !mods.is_empty()
            && !mods.iter().any(|name| name == namespace)
        {
            return false;
        }
        if let Some(dimensions) = &self.dimensions
            && !dimensions.is_empty()
            && !dimensions.contains(&dimension)
        {
            return false;
        }
        let y = spawn.pos[1] as i32;
        if self.min_height.is_some_and(|min| y < min) {
            return false;
        }
        if self.max_height.is_some_and(|max| y > max) {
            return false;
        }
        // Upstream's light checks read the chunk first and fail the condition
        // when it is not loaded, so a missing read never matches.
        if self.min_light.is_some()
            || self.max_light.is_some()
            || self.min_light_sky.is_some()
            || self.max_light_sky.is_some()
        {
            let Some(light) = spawn.light else {
                return false;
            };
            let block = i32::from(light.block);
            let sky = i32::from(light.sky);
            if self.min_light.is_some_and(|min| block < min)
                || self.max_light.is_some_and(|max| block > max)
                || self.min_light_sky.is_some_and(|min| sky < min)
                || self.max_light_sky.is_some_and(|max| sky > max)
            {
                return false;
            }
        }
        true
    }
}

/// The namespace of a normalised id (`minecraft` for `minecraft:zombie`).
fn namespace(id: &str) -> &str {
    id.split_once(':').map_or(id, |(namespace, _)| namespace)
}

// ---- loot.json ----

/// One item a loot rule adds, with its chance and count range.
///
/// `chance` is the probability that the entry drops at all (0 to 1); `min` and
/// `max` bound the count, inclusive. `max == min` means a fixed count.
#[derive(Debug, Clone, PartialEq)]
pub struct LootItem {
    /// The normalised item id (`minecraft:diamond`).
    pub item: String,
    /// The probability of dropping, 0 to 1.
    pub chance: f64,
    /// The lowest count, inclusive.
    pub min: u32,
    /// The highest count, inclusive.
    pub max: u32,
    /// Upstream `itemcount` tiers: the `(min, max)` count for Looting 0, 1,
    /// 2, …; the last tier repeats for a higher level.
    pub counts: Option<Vec<(u32, u32)>>,
}

/// One parsed `loot.json` entry.
///
/// A rule with a non-empty [`LootRule::unsupported`] list never matches: the
/// port cannot honour a condition or action it cannot read, so it leaves the
/// rolled drops alone.
#[derive(Debug, Clone, PartialEq)]
pub struct LootRule {
    /// Position in the file, for `/incontrol reload` reports.
    pub index: usize,
    /// Entity ids that match, any of them.
    pub mobs: Option<Vec<String>>,
    /// Namespaces of the entity or block id, any of them.
    pub mods: Option<Vec<String>>,
    /// Block ids that match, any of them.
    pub blocks: Option<Vec<String>>,
    /// Dimension ids that match, any of them.
    pub dimensions: Option<Vec<String>>,
    /// Clear every rolled stack before the adds (`replace`, `remove: true`,
    /// `removeall` or `result: deny`).
    pub clear: bool,
    /// Item ids to take out of the rolled stacks.
    pub remove: Vec<String>,
    /// Items to add, in file order.
    pub adds: Vec<LootItem>,
    /// Upstream's rule-level `itemcount` tiers, used by `item` adds that have
    /// no tiers of their own.
    pub itemcount: Option<Vec<(u32, u32)>>,
    /// Upstream's `nbt`: a JSON object stored as `minecraft:custom_data` on
    /// every added stack.
    pub nbt: Option<String>,
    /// Upstream's `continue`: keep evaluating later loot rules.
    pub again: bool,
    /// Keys and values this port cannot evaluate; they make the rule inert.
    pub unsupported: Vec<String>,
}

/// The rules of one successfully parsed `loot.json` file.
#[derive(Debug, Clone, PartialEq)]
pub struct LootLoaded {
    /// Parsed rules in file order.
    pub rules: Vec<LootRule>,
    /// One line per skipped rule, inert condition or unknown item.
    pub warnings: Vec<String>,
}

/// What one set of rolled drops looks like to the loot engine.
#[derive(Debug, Clone, Copy)]
pub struct LootQuery<'a> {
    /// The mob that died (`minecraft:zombie`), when a mob dropped these.
    pub mob: Option<&'a str>,
    /// The block that broke (`minecraft:stone`), when a block dropped these.
    pub block: Option<&'a str>,
    /// The dimension id (`minecraft:overworld`).
    pub dimension: &'a str,
    /// The killer's Looting level (0 when there is no player or weapon).
    pub looting: u32,
}

/// One stack a matching loot rule adds, with its optional `nbt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootStack {
    /// The normalised item id.
    pub item: String,
    /// The drawn count.
    pub count: u32,
    /// The rule's `nbt`, as JSON text, for `minecraft:custom_data`.
    pub nbt: Option<String>,
}

/// What one matching loot rule asks the glue to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LootOp {
    /// Drop nothing that has been rolled so far.
    Clear,
    /// Take these item ids out of the rolled stacks.
    Remove(Vec<String>),
    /// Append these stacks.
    Add(Vec<LootStack>),
}

/// The resolution of the `chance` draw: one in a thousand.
pub const CHANCE_STEPS: u32 = 1000;
/// The largest count a drop entry may ask for, a guard against a typo.
const MAX_COUNT: u32 = 1_000_000;

fn parse_loot_rule(index: usize, keys: Vec<(String, Json)>) -> Result<LootRule, String> {
    let mut rule = LootRule {
        index,
        mobs: None,
        mods: None,
        blocks: None,
        dimensions: None,
        clear: false,
        remove: Vec::new(),
        adds: Vec::new(),
        itemcount: None,
        nbt: None,
        again: false,
        unsupported: Vec::new(),
    };
    for (key, value) in collapse_duplicates(keys) {
        match key.as_str() {
            "mob" => {
                let mut ids = Vec::new();
                for id in string_list(&key, value)? {
                    if let Some(tag) = id.trim().strip_prefix('#') {
                        let tag = normalize_id(tag);
                        rule.unsupported.push(format!("mob tag #{tag}"));
                    } else {
                        ids.push(normalize_id(&id));
                    }
                }
                rule.mobs = Some(ids);
            }
            "mod" => {
                let mods: Vec<String> = string_list(&key, value)?
                    .into_iter()
                    .map(|name| name.trim().to_ascii_lowercase())
                    .collect();
                rule.mods = Some(mods);
            }
            "block" => {
                let mut blocks = Vec::new();
                for id in string_list(&key, value)? {
                    if let Some(tag) = id.trim().strip_prefix('#') {
                        let tag = normalize_id(tag);
                        rule.unsupported.push(format!("block tag #{tag}"));
                    } else {
                        blocks.push(normalize_id(&id));
                    }
                }
                rule.blocks = Some(blocks);
            }
            "dimension" => {
                let dimensions = string_list(&key, value)?
                    .into_iter()
                    .map(|id| normalize_id(&id))
                    .collect();
                rule.dimensions = Some(dimensions);
            }
            "drops" | "add" => {
                let Json::Array(entries) = value else {
                    return Err(format!("`{key}` must be a list of drop objects"));
                };
                for entry in entries {
                    let (drop, unsupported) = parse_loot_entry(entry)?;
                    rule.adds.push(drop);
                    rule.unsupported.extend(unsupported);
                }
            }
            // Upstream's loot action: `item` adds one of each, with the count
            // from `itemcount`. That count depends on the looting level, which
            // this port cannot reproduce, so `itemcount` makes the rule inert.
            "item" => {
                for id in string_list(&key, value)? {
                    rule.adds.push(LootItem {
                        item: normalize_id(&id),
                        chance: 1.0,
                        min: 1,
                        max: 1,
                        counts: None,
                    });
                }
            }
            "itemcount" => {
                let Json::String(text) = value else {
                    return Err("`itemcount` must be a string".to_owned());
                };
                match parse_count_tiers(&text) {
                    Some(tiers) => rule.itemcount = Some(tiers),
                    None => rule.unsupported.push(format!("itemcount={text}")),
                }
            }
            "nbt" => match value {
                Json::Object(_) => rule.nbt = Some(value.to_json()),
                other => rule.unsupported.push(format!("nbt={}", other.to_json())),
            },
            "replace" => {
                let Json::Bool(value) = value else {
                    return Err("`replace` must be true or false".to_owned());
                };
                rule.clear |= value;
            }
            "removeall" => {
                let Json::Bool(value) = value else {
                    return Err("`removeall` must be true or false".to_owned());
                };
                rule.clear |= value;
            }
            "remove" => match value {
                Json::Bool(value) => rule.clear |= value,
                Json::String(_) | Json::Array(_) => {
                    rule.remove.extend(
                        string_list(&key, value)?
                            .into_iter()
                            .map(|id| normalize_id(&id)),
                    );
                }
                _ => return Err("`remove` must be true, false, a string or a list".to_owned()),
            },
            "result" => {
                let Json::String(name) = value else {
                    return Err("`result` must be a string".to_owned());
                };
                let lower = name.to_ascii_lowercase();
                match lower.as_str() {
                    "deny" => rule.clear = true,
                    "allow" | "default" => {}
                    other => rule.unsupported.push(format!("result={other}")),
                }
            }
            "continue" => {
                let Json::Bool(value) = value else {
                    return Err("`continue` must be true or false".to_owned());
                };
                rule.again = value;
            }
            other => rule.unsupported.push(other.to_owned()),
        }
    }
    Ok(rule)
}

/// One `drops` entry, plus the keys inside it this port cannot read.
fn parse_loot_entry(value: Json) -> Result<(LootItem, Vec<String>), String> {
    let Json::Object(keys) = value else {
        return Err("a `drops` entry must be an object".to_owned());
    };
    let mut item = None;
    let mut chance = 1.0;
    let mut min = 1;
    let mut max = None;
    let mut counts = None;
    let mut unsupported = Vec::new();
    for (key, value) in collapse_duplicates(keys) {
        match key.as_str() {
            "item" => {
                let Json::String(id) = value else {
                    return Err("`drops.item` must be a string".to_owned());
                };
                item = Some(normalize_id(&id));
            }
            "itemcount" | "counts" => {
                let Json::String(text) = value else {
                    return Err(format!("`drops.{key}` must be a string"));
                };
                match parse_count_tiers(&text) {
                    Some(tiers) => counts = Some(tiers),
                    None => unsupported.push(format!("drops.{key}={text}")),
                }
            }
            "chance" => {
                let Json::Number(number) = value else {
                    return Err("`drops.chance` must be a number".to_owned());
                };
                if !(0.0..=1.0).contains(&number) {
                    return Err(format!(
                        "`drops.chance` must be between 0 and 1, got {number}"
                    ));
                }
                chance = number;
            }
            "min" | "max" => {
                let Json::Number(number) = value else {
                    return Err(format!("`drops.{key}` must be a number"));
                };
                let count = count_value(&key, number)?;
                if key == "min" {
                    min = count;
                } else {
                    max = Some(count);
                }
            }
            other => unsupported.push(format!("drops.{other}")),
        }
    }
    let item = item.ok_or("a `drops` entry needs an `item`")?;
    let max = max.unwrap_or(min);
    if max < min {
        return Err(format!("`drops.max` ({max}) is below `drops.min` ({min})"));
    }
    Ok((
        LootItem {
            item,
            chance,
            min,
            max,
            counts,
        },
        unsupported,
    ))
}

/// A whole item count: not negative, not fractional, and not absurd.
fn count_value(key: &str, number: f64) -> Result<u32, String> {
    if number.fract() != 0.0 || number < 0.0 || number > f64::from(MAX_COUNT) {
        return Err(format!(
            "`drops.{key}` must be a whole number from 0 to {MAX_COUNT}, got {number}"
        ));
    }
    Ok(number as u32)
}

/// Drops the entries naming an item the game does not have, with a warning.
///
/// The engine cannot resolve item ids (it has no registry); the glue calls this
/// once the item registry is loaded.
pub fn validate_loot_items(loot: &mut LootLoaded, known: impl Fn(&str) -> bool) {
    let mut warnings = Vec::new();
    for rule in &mut loot.rules {
        let index = rule.index;
        let mut adds = Vec::with_capacity(rule.adds.len());
        for drop in rule.adds.drain(..) {
            if known(&drop.item) {
                adds.push(drop);
            } else {
                warnings.push(format!(
                    "rule {index}: unknown item '{}', entry dropped",
                    drop.item
                ));
            }
        }
        rule.adds = adds;
        let mut removals = Vec::with_capacity(rule.remove.len());
        for id in rule.remove.drain(..) {
            if known(&id) {
                removals.push(id);
            } else {
                warnings.push(format!(
                    "rule {index}: unknown item '{id}', removal dropped"
                ));
            }
        }
        rule.remove = removals;
    }
    loot.warnings.append(&mut warnings);
}

impl LootRule {
    /// Whether every condition of this rule matches `query`.
    fn matches(&self, query: LootQuery<'_>) -> bool {
        if !self.unsupported.is_empty() {
            return false;
        }
        let mob = query.mob.map(normalize_id);
        let block = query.block.map(normalize_id);
        let dimension = normalize_id(query.dimension);
        if let Some(mobs) = &self.mobs
            && !mobs.is_empty()
            && !mob.as_ref().is_some_and(|mob| mobs.contains(mob))
        {
            return false;
        }
        if let Some(blocks) = &self.blocks
            && !blocks.is_empty()
            && !block.as_ref().is_some_and(|block| blocks.contains(block))
        {
            return false;
        }
        let source_namespace = mob.as_deref().or(block.as_deref()).map(namespace);
        if let Some(mods) = &self.mods
            && !mods.is_empty()
            && !source_namespace.is_some_and(|name| mods.iter().any(|mod_id| mod_id == name))
        {
            return false;
        }
        if let Some(dimensions) = &self.dimensions
            && !dimensions.is_empty()
            && !dimensions.contains(&dimension)
        {
            return false;
        }
        true
    }
}

/// The operations of every matching loot rule, in file order.
///
/// The first matching rule decides and later rules are skipped unless it says
/// `continue`; `roll(bound)` must return a value below `bound` and is the only
/// source of randomness, so a seeded or fixed function makes the outcome
/// deterministic.
#[must_use]
pub fn loot_ops(
    rules: &[LootRule],
    query: LootQuery<'_>,
    roll: &mut dyn FnMut(u32) -> u32,
) -> Vec<LootOp> {
    let mut ops = Vec::new();
    for rule in rules {
        if !rule.matches(query) {
            continue;
        }
        if rule.clear {
            ops.push(LootOp::Clear);
        }
        if !rule.remove.is_empty() {
            ops.push(LootOp::Remove(rule.remove.clone()));
        }
        let stacks = rolled_adds(
            &rule.adds,
            rule.itemcount.as_deref(),
            query.looting,
            rule.nbt.as_deref(),
            roll,
        );
        if !stacks.is_empty() {
            ops.push(LootOp::Add(stacks));
        }
        if !rule.again {
            break;
        }
    }
    ops
}

/// The stacks this rule's adds produce, each after its chance and count draws.
fn rolled_adds(
    adds: &[LootItem],
    itemcount: Option<&[(u32, u32)]>,
    looting: u32,
    nbt: Option<&str>,
    roll: &mut dyn FnMut(u32) -> u32,
) -> Vec<LootStack> {
    let mut stacks = Vec::new();
    for add in adds {
        let threshold = chance_threshold(add.chance);
        if threshold == 0 {
            continue;
        }
        if threshold < CHANCE_STEPS && draw(roll, CHANCE_STEPS) >= threshold {
            continue;
        }
        let bounds = add
            .counts
            .as_deref()
            .or(itemcount)
            .and_then(|tiers| tiers.get(looting as usize).or_else(|| tiers.last()))
            .copied()
            .unwrap_or((add.min, add.max));
        let (min, max) = bounds;
        let span = max - min + 1;
        let count = if span > 1 {
            min + draw(roll, span)
        } else {
            min
        };
        if count > 0 {
            stacks.push(LootStack {
                item: add.item.clone(),
                count,
                nbt: nbt.map(str::to_owned),
            });
        }
    }
    stacks
}

/// A chance as a threshold in [`CHANCE_STEPS`], rounded to the nearest step.
fn chance_threshold(chance: f64) -> u32 {
    (chance.clamp(0.0, 1.0) * f64::from(CHANCE_STEPS)).round() as u32
}

/// One bounded draw, clamped so a misbehaving source can never overflow.
fn draw(roll: &mut dyn FnMut(u32) -> u32, bound: u32) -> u32 {
    roll(bound).min(bound - 1)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

fn parse_json(text: &str) -> Result<Json, LoadError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.value()?;
    parser.skip_space();
    if parser.at != parser.bytes.len() {
        return Err(parser.error("trailing characters after the value"));
    }
    Ok(value)
}

impl Parser<'_> {
    fn error(&self, message: &str) -> LoadError {
        let offset = self.at.min(self.bytes.len());
        let prefix = &self.bytes[..offset];
        let line = 1 + prefix.iter().filter(|byte| **byte == b'\n').count();
        let column = match prefix.iter().rposition(|byte| *byte == b'\n') {
            Some(newline) => offset - newline,
            None => offset + 1,
        };
        LoadError {
            line,
            column,
            message: message.to_owned(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), LoadError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", byte as char)))
        }
    }

    fn value(&mut self) -> Result<Json, LoadError> {
        self.skip_space();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(_) => Err(self.error("expected a value")),
            None => Err(self.error("unexpected end of input, expected a value")),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, LoadError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("expected a value"))
        }
    }

    fn array(&mut self) -> Result<Json, LoadError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']' in a list")),
            }
        }
    }

    fn object(&mut self) -> Result<Json, LoadError> {
        self.expect(b'{')?;
        let mut fields = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.skip_space();
            self.expect(b':')?;
            let value = self.value()?;
            fields.push((key, value));
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(fields));
                }
                _ => return Err(self.error("expected ',' or '}' in an object")),
            }
        }
    }

    fn string(&mut self) -> Result<String, LoadError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| self.error("unexpected end of input in a string"))?;
            self.at += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.error("unexpected end of input in a string"))?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err(self.error("invalid escape in a string")),
                    }
                }
                _ if byte < 0x20 => return Err(self.error("a control character must be escaped")),
                _ => {
                    // The input is `&str`, so a multi-byte sequence is copied whole.
                    let start = self.at - 1;
                    while self.peek().is_some_and(|byte| (byte & 0xc0) == 0x80) {
                        self.at += 1;
                    }
                    let text = std::str::from_utf8(&self.bytes[start..self.at])
                        .map_err(|_| self.error("invalid UTF-8 in a string"))?;
                    out.push_str(text);
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, LoadError> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.error("unexpected end of input in a `\\u` escape"))?;
        let text = std::str::from_utf8(digits).map_err(|_| self.error("invalid `\\u` escape"))?;
        let code = u32::from_str_radix(text, 16).map_err(|_| self.error("invalid `\\u` escape"))?;
        self.at += 4;
        char::from_u32(code).ok_or_else(|| self.error("invalid `\\u` escape"))
    }

    fn number(&mut self) -> Result<Json, LoadError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| self.error("invalid number"))?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| self.error("invalid number"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_one(text: &str) -> Rule {
        let loaded = load(text).expect("the rule file parses");
        assert_eq!(loaded.rules.len(), 1, "warnings: {:?}", loaded.warnings);
        loaded.rules[0].clone()
    }

    #[test]
    fn empty_array_loads_no_rules() {
        let loaded = load("[]").expect("empty rule list parses");

        assert!(loaded.rules.is_empty());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn whitespace_and_nested_containers_parse() {
        let loaded = load(
            "  [\n  {\n    \"mob\": \"minecraft:zombie\",\n    \"result\": \"deny\"\n  }\n]\n",
        )
        .expect("pretty-printed rule list parses");

        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].outcome, Outcome::Deny);
    }

    #[test]
    fn string_escapes_decode() {
        let value = parse_json(r#""a\"b\\c\n\u0041""#).expect("escapes parse");

        assert_eq!(value, Json::String("a\"b\\c\nA".to_owned()));
    }

    #[test]
    fn numbers_and_booleans_parse() {
        let value = parse_json("[-1.5, 0, 42, true, false, null]").expect("scalars parse");

        assert_eq!(
            value,
            Json::Array(vec![
                Json::Number(-1.5),
                Json::Number(0.0),
                Json::Number(42.0),
                Json::Bool(true),
                Json::Bool(false),
                Json::Null,
            ])
        );
    }

    #[test]
    fn a_syntax_error_names_the_line_and_column() {
        let error = load("[\n  {\"mob\": }\n]").expect_err("a missing value is rejected");

        assert_eq!(error.line, 2, "error: {error}");
        assert!(error.column > 1, "error: {error}");
        assert!(error.to_string().contains("value"), "error: {error}");
    }

    #[test]
    fn a_non_array_file_is_rejected() {
        let error = load("{\"mob\": \"minecraft:zombie\"}").expect_err("an object is not a list");

        assert!(error.to_string().contains("array"), "error: {error}");
    }

    #[test]
    fn a_non_object_rule_is_skipped_with_a_warning() {
        let loaded = load("[\"zombie\", {\"mob\": \"minecraft:cow\", \"result\": \"deny\"}]")
            .expect("the file parses");

        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].index, 1);
        assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
        assert!(
            loaded.warnings[0].contains("rule 0"),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn a_rule_without_result_defaults_and_stops() {
        let rule = load_one("[{\"mob\": \"minecraft:zombie\"}]");

        assert_eq!(rule.outcome, Outcome::Default);
        assert!(!rule.again);
        assert_eq!(rule.when, When::Position);
    }

    #[test]
    fn result_words_map_to_outcomes() {
        assert_eq!(load_one("[{\"result\": \"deny\"}]").outcome, Outcome::Deny);
        assert_eq!(
            load_one("[{\"result\": \"deny_with_actions\"}]").outcome,
            Outcome::Deny
        );
        assert_eq!(
            load_one("[{\"result\": \"allow\"}]").outcome,
            Outcome::Allow
        );
        assert_eq!(load_one("[{\"result\": \"true\"}]").outcome, Outcome::Allow);
        assert_eq!(
            load_one("[{\"result\": \"default\"}]").outcome,
            Outcome::Default
        );
        assert_eq!(
            load_one("[{\"result\": \"not-a-result\"}]").outcome,
            Outcome::Deny
        );
    }

    #[test]
    fn bare_identifiers_gain_the_minecraft_namespace() {
        let rule =
            load_one("[{\"mob\": \"zombie\", \"dimension\": \"the_end\", \"mod\": \"Minecraft\"}]");

        assert_eq!(
            rule.mobs.as_deref(),
            Some(&["minecraft:zombie".to_owned()][..])
        );
        assert_eq!(
            rule.dimensions.as_deref(),
            Some(&["minecraft:the_end".to_owned()][..])
        );
        assert_eq!(rule.mods.as_deref(), Some(&["minecraft".to_owned()][..]));
    }

    #[test]
    fn list_valued_conditions_accept_one_value_or_an_array() {
        let single =
            load_one("[{\"mob\": \"minecraft:zombie\", \"dimension\": \"minecraft:overworld\"}]");
        let list = load_one(
            "[{\"mob\": [\"minecraft:zombie\", \"minecraft:skeleton\"], \"dimension\": [\"minecraft:overworld\", \"minecraft:the_nether\"]}]",
        );

        assert_eq!(single.mobs.as_ref().map(Vec::len), Some(1));
        assert_eq!(list.mobs.as_ref().map(Vec::len), Some(2));
        assert_eq!(list.dimensions.as_ref().map(Vec::len), Some(2));
    }

    #[test]
    fn height_bounds_parse_and_continue_is_a_boolean() {
        let rule = load_one(
            "[{\"minheight\": 60, \"maxheight\": -1, \"continue\": true, \"when\": \"ONJOIN\"}]",
        );

        assert_eq!(rule.min_height, Some(60));
        assert_eq!(rule.max_height, Some(-1));
        assert!(rule.again);
        assert_eq!(rule.when, When::OnJoin);
    }

    #[test]
    fn unsupported_conditions_make_the_rule_inert() {
        let loaded = load(
            r#"[
              {"mob": "minecraft:zombie", "biome": "minecraft:desert", "result": "deny"},
              {"mob": "minecraft:cow", "result": "deny"}
            ]"#,
        )
        .expect("the file parses");

        assert_eq!(loaded.rules.len(), 2);
        assert_eq!(loaded.rules[0].unsupported, vec!["biome".to_owned()]);
        assert!(loaded.rules[1].unsupported.is_empty());
        assert_eq!(
            loaded.warnings.len(),
            1,
            "one warning for the inert rule: {:?}",
            loaded.warnings
        );
    }

    #[test]
    fn every_deferred_condition_is_named_in_the_warning() {
        let loaded = load(
            "[{\"minlight_full\": 1, \"maxlight_full\": 7, \"random\": 0.5, \"hostile\": true, \"result\": \"deny\"}]",
        )
        .expect("the file parses");

        let unsupported = &loaded.rules[0].unsupported;
        for key in ["minlight_full", "maxlight_full", "random", "hostile"] {
            assert!(
                unsupported.iter().any(|item| item == key),
                "{unsupported:?}"
            );
            assert!(loaded.warnings[0].contains(key), "{:?}", loaded.warnings);
        }
    }

    #[test]
    fn a_despawn_rule_is_inert_not_misread_as_position() {
        let loaded =
            load("[{\"mob\": \"minecraft:zombie\", \"when\": \"despawn\", \"result\": \"deny\"}]")
                .expect("the file parses");

        assert_eq!(loaded.rules[0].unsupported, vec!["when=despawn".to_owned()]);
    }

    #[test]
    fn an_unknown_key_makes_the_rule_inert() {
        let loaded =
            load("[{\"mob\": \"minecraft:zombie\", \"nonsense\": 1, \"result\": \"deny\"}]")
                .expect("the file parses");

        assert_eq!(loaded.rules[0].unsupported, vec!["nonsense".to_owned()]);
    }

    #[test]
    fn mob_tags_are_inert_until_entity_tags_can_be_read() {
        let loaded = load("[{\"mob\": \"#minecraft:undead\", \"result\": \"deny\"}]")
            .expect("the file parses");

        assert_eq!(
            loaded.rules[0].unsupported,
            vec!["mob tag #minecraft:undead".to_owned()]
        );
    }

    #[test]
    fn a_wrongly_typed_value_is_reported_with_its_rule_index() {
        let loaded =
            load("[{\"minheight\": \"high\", \"result\": \"deny\"}]").expect("the file parses");

        assert!(loaded.rules.is_empty(), "{:?}", loaded.rules);
        assert_eq!(loaded.warnings.len(), 1);
        assert!(
            loaded.warnings[0].contains("rule 0"),
            "{:?}",
            loaded.warnings
        );
        assert!(
            loaded.warnings[0].contains("minheight"),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn duplicate_keys_use_the_last_value() {
        let rule = load_one("[{\"mob\": \"minecraft:cow\", \"mob\": \"minecraft:zombie\"}]");

        assert_eq!(
            rule.mobs.as_deref(),
            Some(&["minecraft:zombie".to_owned()][..])
        );
    }

    fn spawn<'a>(dimension: &'a str, mob: &'a str, pos: [f64; 3], natural: bool) -> Spawn<'a> {
        Spawn {
            dimension,
            mob,
            pos,
            natural,
            light: Some(Light { block: 0, sky: 0 }),
        }
    }

    fn spawn_in_light<'a>(
        dimension: &'a str,
        mob: &'a str,
        pos: [f64; 3],
        natural: bool,
        light: Option<Light>,
    ) -> Spawn<'a> {
        Spawn {
            dimension,
            mob,
            pos,
            natural,
            light,
        }
    }

    #[test]
    fn light_conditions_parse_as_block_and_sky_bounds() {
        let rule = load_one(
            "[{\"minlight\": 1, \"maxlight\": 7, \"minlight_sky\": 2, \"maxlight_sky\": 15}]",
        );

        assert_eq!(rule.min_light, Some(1));
        assert_eq!(rule.max_light, Some(7));
        assert_eq!(rule.min_light_sky, Some(2));
        assert_eq!(rule.max_light_sky, Some(15));
        assert!(rule.unsupported.is_empty(), "{:?}", rule.unsupported);
    }

    #[test]
    fn minlight_and_maxlight_bound_block_light_inclusively() {
        let min = load_one("[{\"minlight\": 5, \"when\": \"onjoin\", \"result\": \"deny\"}]");
        let max = load_one("[{\"maxlight\": 5, \"when\": \"onjoin\", \"result\": \"deny\"}]");
        let at = |rule: &Rule, block: u8| {
            decide(
                std::slice::from_ref(rule),
                spawn_in_light(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false,
                    Some(Light { block, sky: 0 }),
                ),
            )
        };

        assert_eq!(at(&min, 5), Decision::Deny { rule: 0 });
        assert_eq!(at(&min, 4), Decision::Allow);
        assert_eq!(at(&max, 5), Decision::Deny { rule: 0 });
        assert_eq!(at(&max, 6), Decision::Allow);
    }

    #[test]
    fn sky_light_conditions_read_the_sky_layer_not_block_light() {
        let min = load_one("[{\"minlight_sky\": 10, \"when\": \"onjoin\", \"result\": \"deny\"}]");
        let max = load_one("[{\"maxlight_sky\": 4, \"when\": \"onjoin\", \"result\": \"deny\"}]");
        let at = |rule: &Rule, block: u8, sky: u8| {
            decide(
                std::slice::from_ref(rule),
                spawn_in_light(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false,
                    Some(Light { block, sky }),
                ),
            )
        };

        // Block light is 15 in both cases: only the sky layer may decide.
        assert_eq!(at(&min, 15, 10), Decision::Deny { rule: 0 });
        assert_eq!(at(&min, 15, 9), Decision::Allow);
        assert_eq!(at(&max, 15, 4), Decision::Deny { rule: 0 });
        assert_eq!(at(&max, 15, 5), Decision::Allow);
    }

    #[test]
    fn a_missing_light_read_never_matches_a_light_condition() {
        let min = load_one("[{\"minlight\": 0, \"when\": \"onjoin\", \"result\": \"deny\"}]");
        let max = load_one("[{\"maxlight\": 15, \"when\": \"onjoin\", \"result\": \"deny\"}]");

        for rule in [&min, &max] {
            assert_eq!(
                decide(
                    std::slice::from_ref(rule),
                    spawn_in_light(
                        "minecraft:overworld",
                        "minecraft:zombie",
                        [0.0, 64.0, 0.0],
                        false,
                        None,
                    ),
                ),
                Decision::Allow,
                "a missing light read must fail closed"
            );
        }
    }

    #[test]
    fn light_conditions_combine_with_mob_and_dimension() {
        let rule = load_one(
            "[{\"mob\": \"minecraft:zombie\", \"dimension\": \"minecraft:overworld\", \"maxlight\": 0, \"when\": \"onjoin\", \"result\": \"deny\"}]",
        );
        let dark = Some(Light { block: 0, sky: 0 });
        let at = |mob: &str, dimension: &str| {
            decide(
                std::slice::from_ref(&rule),
                spawn_in_light(dimension, mob, [0.0, 64.0, 0.0], false, dark),
            )
        };

        assert_eq!(
            at("minecraft:zombie", "minecraft:overworld"),
            Decision::Deny { rule: 0 }
        );
        assert_eq!(at("minecraft:cow", "minecraft:overworld"), Decision::Allow);
        assert_eq!(at("minecraft:zombie", "minecraft:the_end"), Decision::Allow);
    }

    #[test]
    fn a_non_numeric_light_bound_is_reported_and_skipped() {
        let loaded =
            load("[{\"minlight\": \"dark\", \"result\": \"deny\"}]").expect("the file parses");

        assert!(loaded.rules.is_empty(), "{:?}", loaded.rules);
        assert_eq!(loaded.warnings.len(), 1);
        assert!(
            loaded.warnings[0].contains("minlight"),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn no_rule_allows_every_spawn() {
        let loaded = load("[]").expect("empty rule list parses");

        assert_eq!(
            decide(
                &loaded.rules,
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    true
                )
            ),
            Decision::Allow
        );
    }

    #[test]
    fn a_deny_matches_its_mob_only() {
        let rule = load_one(
            "[{\"mob\": \"minecraft:zombie\", \"when\": \"onjoin\", \"result\": \"deny\"}]",
        );

        assert_eq!(
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 0 }
        );
        assert_eq!(
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:overworld",
                    "minecraft:cow",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Allow
        );
    }

    #[test]
    fn a_bare_mob_id_matches_the_namespaced_kind() {
        let rule = load_one("[{\"mob\": \"zombie\", \"when\": \"onjoin\", \"result\": \"deny\"}]");

        assert_eq!(
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 0 }
        );
    }

    #[test]
    fn a_dimension_condition_must_match() {
        let rule = load_one(
            "[{\"mob\": \"minecraft:pig\", \"dimension\": \"minecraft:the_nether\", \"when\": \"onjoin\", \"result\": \"deny\"}]",
        );

        assert_eq!(
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:the_end",
                    "minecraft:pig",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Allow
        );
        assert_eq!(
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:the_nether",
                    "minecraft:pig",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 0 }
        );
    }

    #[test]
    fn height_bounds_are_inclusive_and_use_the_truncated_y() {
        let rule = load_one(
            "[{\"minheight\": 70, \"maxheight\": 80, \"when\": \"onjoin\", \"result\": \"deny\"}]",
        );
        let at = |y: f64| {
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, y, 0.0],
                    false,
                ),
            )
        };

        assert_eq!(at(70.0), Decision::Deny { rule: 0 });
        assert_eq!(at(80.0), Decision::Deny { rule: 0 });
        assert_eq!(at(69.9), Decision::Allow);
        assert_eq!(at(69.0), Decision::Allow);
        assert_eq!(at(80.9), Decision::Deny { rule: 0 });
        assert_eq!(at(81.0), Decision::Allow);
    }

    #[test]
    fn the_first_matching_rule_decides() {
        let loaded = load(
            r#"[
              {"mob": "minecraft:zombie", "when": "onjoin", "result": "allow"},
              {"mob": "minecraft:zombie", "when": "onjoin", "result": "deny"}
            ]"#,
        )
        .expect("the file parses");

        assert_eq!(
            decide(
                &loaded.rules,
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Allow
        );
    }

    #[test]
    fn continue_lets_a_later_rule_deny() {
        let loaded = load(
            r#"[
              {"mob": "minecraft:zombie", "when": "onjoin", "result": "allow", "continue": true},
              {"mob": "minecraft:zombie", "when": "onjoin", "result": "deny"}
            ]"#,
        )
        .expect("the file parses");

        assert_eq!(
            decide(
                &loaded.rules,
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 1 }
        );
    }

    #[test]
    fn a_deny_is_not_cleared_by_a_later_allow() {
        let loaded = load(
            r#"[
              {"mob": "minecraft:zombie", "when": "onjoin", "result": "deny", "continue": true},
              {"mob": "minecraft:zombie", "when": "onjoin", "result": "allow"}
            ]"#,
        )
        .expect("the file parses");

        assert_eq!(
            decide(
                &loaded.rules,
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 0 }
        );
    }

    #[test]
    fn position_rules_ignore_command_spawns_and_onjoin_rules_cover_them() {
        let position = load_one("[{\"mob\": \"minecraft:zombie\", \"result\": \"deny\"}]");
        let onjoin = load_one(
            "[{\"mob\": \"minecraft:zombie\", \"when\": \"onjoin\", \"result\": \"deny\"}]",
        );

        assert_eq!(
            decide(
                std::slice::from_ref(&position),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    true
                )
            ),
            Decision::Deny { rule: 0 }
        );
        assert_eq!(
            decide(
                std::slice::from_ref(&position),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Allow
        );
        assert_eq!(
            decide(
                std::slice::from_ref(&onjoin),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 0 }
        );
    }

    #[test]
    fn a_mod_condition_matches_the_namespace() {
        let rule =
            load_one("[{\"mod\": \"minecraft\", \"when\": \"onjoin\", \"result\": \"deny\"}]");
        let other =
            load_one("[{\"mod\": \"othermod\", \"when\": \"onjoin\", \"result\": \"deny\"}]");

        assert_eq!(
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 0 }
        );
        assert_eq!(
            decide(
                std::slice::from_ref(&other),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Allow
        );
    }

    #[test]
    fn an_inert_rule_never_matches() {
        let loaded = load(
            "[{\"mob\": \"minecraft:zombie\", \"biome\": \"minecraft:desert\", \"when\": \"onjoin\", \"result\": \"deny\"}]",
        )
        .expect("the file parses");

        assert_eq!(
            decide(
                &loaded.rules,
                spawn(
                    "minecraft:desert",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Allow
        );
    }

    #[test]
    fn a_rule_without_conditions_is_a_wildcard() {
        let rule = load_one("[{\"when\": \"onjoin\", \"result\": \"deny\"}]");

        assert_eq!(
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:the_end",
                    "minecraft:cow",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 0 }
        );
    }

    #[test]
    fn empty_lists_are_wildcards_as_upstreams_empty_attribute_sets() {
        let rule = load_one(
            "[{\"mob\": [], \"dimension\": [], \"when\": \"onjoin\", \"result\": \"deny\"}]",
        );

        assert_eq!(
            decide(
                std::slice::from_ref(&rule),
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 0 }
        );
    }

    #[test]
    fn file_order_is_preserved_in_the_indexes() {
        let loaded = load(
            r#"[
              {"mob": "minecraft:cow", "when": "onjoin", "result": "deny"},
              {"mob": "minecraft:zombie", "when": "onjoin", "result": "deny", "continue": true},
              {"mob": "minecraft:zombie", "when": "onjoin", "result": "deny"}
            ]"#,
        )
        .expect("the file parses");

        assert_eq!(
            decide(
                &loaded.rules,
                spawn(
                    "minecraft:overworld",
                    "minecraft:zombie",
                    [0.0, 64.0, 0.0],
                    false
                )
            ),
            Decision::Deny { rule: 1 }
        );
    }

    // ---- loot.json rules ----

    fn loot_one(text: &str) -> LootRule {
        let loaded = load_loot(text).expect("the loot file parses");
        assert_eq!(loaded.rules.len(), 1, "warnings: {:?}", loaded.warnings);
        loaded.rules[0].clone()
    }

    fn loot_query<'a>(
        mob: Option<&'a str>,
        block: Option<&'a str>,
        dimension: &'a str,
    ) -> LootQuery<'a> {
        LootQuery {
            mob,
            block,
            dimension,
            looting: 0,
        }
    }

    fn no_draw(_bound: u32) -> u32 {
        panic!("this rule must not draw a random number")
    }

    #[test]
    fn an_empty_loot_file_loads_no_rules() {
        let loaded = load_loot("[]").expect("empty loot list parses");

        assert!(loaded.rules.is_empty());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn a_loot_drop_parses_item_chance_and_counts() {
        let rule = loot_one(
            r#"[{"mob": "minecraft:zombie", "drops": [{"item": "diamond", "chance": 0.5, "min": 1, "max": 3}]}]"#,
        );

        assert_eq!(
            rule.mobs.as_deref(),
            Some(&["minecraft:zombie".to_owned()][..])
        );
        assert_eq!(
            rule.adds,
            vec![LootItem {
                item: "minecraft:diamond".to_owned(),
                chance: 0.5,
                min: 1,
                max: 3,
                counts: None,
            }]
        );
        assert!(!rule.clear);
        assert!(rule.remove.is_empty());
        assert!(rule.unsupported.is_empty(), "{:?}", rule.unsupported);
    }

    #[test]
    fn loot_drop_defaults_are_one_certain_item() {
        let rule = loot_one(r#"[{"mob": "minecraft:zombie", "drops": [{"item": "diamond"}]}]"#);

        assert_eq!(
            rule.adds,
            vec![LootItem {
                item: "minecraft:diamond".to_owned(),
                chance: 1.0,
                min: 1,
                max: 1,
                counts: None,
            }]
        );
    }

    #[test]
    fn replace_clears_the_rolled_drops_before_adding() {
        let rule = loot_one(
            r#"[{"mob": "minecraft:zombie", "replace": true, "drops": [{"item": "diamond"}]}]"#,
        );

        assert_eq!(
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            ),
            vec![
                LootOp::Clear,
                LootOp::Add(vec![LootStack { item: "minecraft:diamond".to_owned(), count: 1, nbt: None }])
            ]
        );
    }

    #[test]
    fn remove_true_clears_every_drop() {
        let rule = loot_one(r#"[{"mob": "minecraft:zombie", "remove": true}]"#);

        assert!(rule.clear, "a boolean remove means remove all");
        assert_eq!(
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            ),
            vec![LootOp::Clear]
        );
    }

    #[test]
    fn remove_takes_out_the_named_items_only() {
        let rule = loot_one(r#"[{"mob": "minecraft:zombie", "remove": ["rotten_flesh"]}]"#);

        assert!(!rule.clear);
        assert_eq!(
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            ),
            vec![LootOp::Remove(vec!["minecraft:rotten_flesh".to_owned()])]
        );
    }

    #[test]
    fn result_deny_clears_and_the_other_words_keep_the_drops() {
        let deny = loot_one(r#"[{"mob": "minecraft:zombie", "result": "deny"}]"#);
        let allow = loot_one(r#"[{"mob": "minecraft:zombie", "result": "allow"}]"#);
        let default = loot_one(r#"[{"mob": "minecraft:zombie", "result": "default"}]"#);
        let at = |rule: &LootRule| {
            loot_ops(
                std::slice::from_ref(rule),
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            )
        };

        assert!(deny.clear);
        assert_eq!(at(&deny), vec![LootOp::Clear]);
        assert!(at(&allow).is_empty());
        assert!(at(&default).is_empty());
    }

    #[test]
    fn an_unknown_loot_key_makes_the_rule_inert() {
        let loaded = load_loot(r#"[{"mob": "minecraft:zombie", "phase": "hard", "remove": true}]"#)
            .expect("the file parses");

        assert_eq!(loaded.rules[0].unsupported, vec!["phase".to_owned()]);
        assert!(
            loot_ops(
                &loaded.rules,
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            )
            .is_empty(),
            "an inert rule must not clear anything"
        );
        assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    }

    #[test]
    fn a_block_rule_matches_only_its_block() {
        let rule =
            loot_one(r#"[{"block": "stone", "replace": true, "drops": [{"item": "diamond"}]}]"#);
        let ops = |block: Option<&str>, mob: Option<&str>| {
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(mob, block, "minecraft:overworld"),
                &mut no_draw,
            )
        };

        assert_eq!(
            ops(Some("minecraft:stone"), None),
            vec![
                LootOp::Clear,
                LootOp::Add(vec![LootStack { item: "minecraft:diamond".to_owned(), count: 1, nbt: None }])
            ]
        );
        assert!(ops(Some("minecraft:dirt"), None).is_empty());
        assert!(ops(None, Some("minecraft:zombie")).is_empty());
    }

    #[test]
    fn a_mob_rule_never_matches_a_block_break() {
        let rule = loot_one(
            r#"[{"mob": "minecraft:zombie", "replace": true, "drops": [{"item": "diamond"}]}]"#,
        );

        assert!(
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(None, Some("minecraft:stone"), "minecraft:overworld"),
                &mut no_draw,
            )
            .is_empty()
        );
    }

    #[test]
    fn the_first_matching_loot_rule_stops_unless_continue() {
        let loaded = load_loot(
            r#"[
              {"mob": "minecraft:zombie", "drops": [{"item": "emerald"}]},
              {"mob": "minecraft:zombie", "drops": [{"item": "diamond"}]}
            ]"#,
        )
        .expect("the file parses");

        assert_eq!(
            loot_ops(
                &loaded.rules,
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            ),
            vec![LootOp::Add(vec![LootStack { item: "minecraft:emerald".to_owned(), count: 1, nbt: None }])]
        );
    }

    #[test]
    fn continue_applies_the_next_loot_rule_too() {
        let loaded = load_loot(
            r#"[
              {"mob": "minecraft:zombie", "drops": [{"item": "emerald"}], "continue": true},
              {"mob": "minecraft:zombie", "drops": [{"item": "diamond"}]}
            ]"#,
        )
        .expect("the file parses");

        assert_eq!(
            loot_ops(
                &loaded.rules,
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            ),
            vec![
                LootOp::Add(vec![LootStack { item: "minecraft:emerald".to_owned(), count: 1, nbt: None }]),
                LootOp::Add(vec![LootStack { item: "minecraft:diamond".to_owned(), count: 1, nbt: None }]),
            ]
        );
    }

    #[test]
    fn chance_zero_never_drops_and_chance_one_always_does() {
        let never = loot_one(
            r#"[{"mob": "minecraft:zombie", "drops": [{"item": "diamond", "chance": 0}]}]"#,
        );
        let always = loot_one(
            r#"[{"mob": "minecraft:zombie", "drops": [{"item": "diamond", "chance": 1}]}]"#,
        );
        let at = |rule: &LootRule| {
            loot_ops(
                std::slice::from_ref(rule),
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            )
        };

        assert!(at(&never).is_empty());
        assert_eq!(
            at(&always),
            vec![LootOp::Add(vec![LootStack { item: "minecraft:diamond".to_owned(), count: 1, nbt: None }])]
        );
    }

    #[test]
    fn a_failed_chance_draw_drops_nothing() {
        let rule = loot_one(
            r#"[{"mob": "minecraft:zombie", "drops": [{"item": "diamond", "chance": 0.5}]}]"#,
        );
        let mut bounds = Vec::new();
        let ops = loot_ops(
            std::slice::from_ref(&rule),
            loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
            &mut |bound| {
                bounds.push(bound);
                999
            },
        );

        assert_eq!(
            bounds,
            vec![CHANCE_STEPS],
            "one draw out of the chance steps"
        );
        assert!(ops.is_empty());
    }

    #[test]
    fn counts_are_rolled_between_min_and_max() {
        let rule = loot_one(
            r#"[{"mob": "minecraft:zombie", "drops": [{"item": "diamond", "min": 2, "max": 5}]}]"#,
        );
        let mut bounds = Vec::new();
        let ops = loot_ops(
            std::slice::from_ref(&rule),
            loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
            &mut |bound| {
                bounds.push(bound);
                2
            },
        );

        assert_eq!(bounds, vec![4], "the draw spans max - min + 1");
        assert_eq!(
            ops,
            vec![LootOp::Add(vec![LootStack { item: "minecraft:diamond".to_owned(), count: 4, nbt: None }])]
        );
    }

    #[test]
    fn an_add_leaves_the_rolled_drops_alone() {
        let rule = loot_one(r#"[{"mob": "minecraft:zombie", "drops": [{"item": "diamond"}]}]"#);

        assert_eq!(
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            ),
            vec![LootOp::Add(vec![LootStack { item: "minecraft:diamond".to_owned(), count: 1, nbt: None }])],
            "only an add, never a clear"
        );
    }

    #[test]
    fn the_item_key_is_an_alias_for_a_single_add() {
        let rule = loot_one(r#"[{"mob": "minecraft:zombie", "item": "diamond"}]"#);

        assert_eq!(
            rule.adds,
            vec![LootItem {
                item: "minecraft:diamond".to_owned(),
                chance: 1.0,
                min: 1,
                max: 1,
                counts: None,
            }]
        );
    }

    #[test]
    fn itemcount_tiers_scale_with_looting() {
        let rule = loot_one(
            r#"[{"mob": "minecraft:zombie", "item": "diamond", "itemcount": "5/7-10/20-30"}]"#,
        );
        let ops = |looting, roll: &mut dyn FnMut(u32) -> u32| {
            loot_ops(
                std::slice::from_ref(&rule),
                LootQuery {
                    mob: Some("minecraft:zombie"),
                    block: None,
                    dimension: "minecraft:overworld",
                    looting,
                },
                roll,
            )
        };

        assert_eq!(
            ops(0, &mut no_draw),
            vec![LootOp::Add(vec![LootStack {
                item: "minecraft:diamond".to_owned(),
                count: 5,
                nbt: None,
            }])]
        );
        assert_eq!(
            ops(1, &mut |_| 0),
            vec![LootOp::Add(vec![LootStack {
                item: "minecraft:diamond".to_owned(),
                count: 7,
                nbt: None,
            }])]
        );
        assert_eq!(
            ops(1, &mut |bound| bound - 1),
            vec![LootOp::Add(vec![LootStack {
                item: "minecraft:diamond".to_owned(),
                count: 10,
                nbt: None,
            }])]
        );
        assert_eq!(
            ops(9, &mut |_| 0),
            vec![LootOp::Add(vec![LootStack {
                item: "minecraft:diamond".to_owned(),
                count: 20,
                nbt: None,
            }])],
            "the last tier repeats for a higher Looting level"
        );
    }

    #[test]
    fn a_rule_nbt_rides_on_every_added_stack() {
        let rule = loot_one(
            r#"[{"mob": "minecraft:zombie", "item": "diamond", "nbt": {"display": {"Name": "Gem"}}}]"#,
        );

        assert_eq!(
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            ),
            vec![LootOp::Add(vec![LootStack {
                item: "minecraft:diamond".to_owned(),
                count: 1,
                nbt: Some(r#"{"display":{"Name":"Gem"}}"#.to_owned()),
            }])]
        );
    }

    #[test]
    fn block_tags_are_inert_until_block_tags_can_be_resolved() {
        let loaded = load_loot(r##"[{"block": "#minecraft:logs", "remove": true}]"##)
            .expect("the file parses");

        assert_eq!(
            loaded.rules[0].unsupported,
            vec!["block tag #minecraft:logs".to_owned()]
        );
    }

    #[test]
    fn an_unknown_item_is_dropped_with_a_warning() {
        let mut loaded = load_loot(
            r#"[{"mob": "minecraft:zombie", "remove": ["minecraft:no_such_item"], "drops": [{"item": "minecraft:no_such_item"}, {"item": "minecraft:diamond"}]}]"#,
        )
        .expect("the file parses");

        validate_loot_items(&mut loaded, |id| id == "minecraft:diamond");

        assert!(loaded.rules[0].remove.is_empty());
        assert_eq!(
            loaded.rules[0].adds,
            vec![LootItem {
                item: "minecraft:diamond".to_owned(),
                chance: 1.0,
                min: 1,
                max: 1,
                counts: None,
            }]
        );
        assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
        assert!(
            loaded.warnings[0].contains("minecraft:no_such_item"),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn a_reversed_count_range_is_rejected_with_the_rule_index() {
        let loaded = load_loot(
            r#"[{"mob": "minecraft:zombie", "drops": [{"item": "diamond", "min": 3, "max": 1}]}]"#,
        )
        .expect("the file parses");

        assert!(loaded.rules.is_empty(), "{:?}", loaded.rules);
        assert_eq!(loaded.warnings.len(), 1);
        assert!(
            loaded.warnings[0].contains("rule 0"),
            "{:?}",
            loaded.warnings
        );
        assert!(loaded.warnings[0].contains("min"), "{:?}", loaded.warnings);
    }

    #[test]
    fn empty_loot_lists_are_wildcards_as_upstreams_empty_attribute_sets() {
        let rule = loot_one(r#"[{"mob": [], "block": [], "dimension": [], "remove": true}]"#);

        assert_eq!(
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(Some("minecraft:cow"), None, "minecraft:the_end"),
                &mut no_draw,
            ),
            vec![LootOp::Clear]
        );
    }

    #[test]
    fn loot_mod_and_dimension_conditions_must_match() {
        let rule = loot_one(
            r#"[{"mod": "minecraft", "dimension": "minecraft:overworld", "remove": true}]"#,
        );
        let at = |mob: &str, dimension: &str| {
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(Some(mob), None, dimension),
                &mut no_draw,
            )
        };

        assert_eq!(
            at("minecraft:zombie", "minecraft:overworld"),
            vec![LootOp::Clear]
        );
        assert!(at("othermod:zombie", "minecraft:overworld").is_empty());
        assert!(at("minecraft:zombie", "minecraft:the_end").is_empty());
    }

    #[test]
    fn removeall_is_upstreams_alias_for_clearing() {
        let rule = loot_one(r#"[{"mob": "minecraft:zombie", "removeall": true}]"#);

        assert!(rule.clear);
    }

    #[test]
    fn a_loot_rule_without_conditions_is_a_wildcard() {
        let rule = loot_one(r#"[{"remove": true}]"#);

        assert_eq!(
            loot_ops(
                std::slice::from_ref(&rule),
                loot_query(None, Some("minecraft:stone"), "minecraft:the_end"),
                &mut no_draw,
            ),
            vec![LootOp::Clear]
        );
    }

    #[test]
    fn duplicate_loot_keys_use_the_last_value() {
        let rule =
            loot_one(r#"[{"mob": "minecraft:cow", "mob": "minecraft:zombie", "remove": true}]"#);

        assert_eq!(
            rule.mobs.as_deref(),
            Some(&["minecraft:zombie".to_owned()][..])
        );
    }

    #[test]
    fn an_unknown_result_word_makes_the_rule_inert() {
        let loaded =
            load_loot(r#"[{"mob": "minecraft:zombie", "result": "denny", "remove": true}]"#)
                .expect("the file parses");

        assert_eq!(
            loaded.rules[0].unsupported,
            vec!["result=denny".to_owned()],
            "a typo must not clear every drop"
        );
        assert!(
            loot_ops(
                &loaded.rules,
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            )
            .is_empty()
        );
    }

    #[test]
    fn an_unknown_key_inside_a_drop_makes_the_rule_inert() {
        let loaded = load_loot(
            r#"[{"mob": "minecraft:zombie", "drops": [{"item": "minecraft:diamond", "count": 2}]}]"#,
        )
        .expect("the file parses");

        assert_eq!(loaded.rules[0].unsupported, vec!["drops.count".to_owned()]);
        assert!(
            loot_ops(
                &loaded.rules,
                loot_query(Some("minecraft:zombie"), None, "minecraft:overworld"),
                &mut no_draw,
            )
            .is_empty()
        );
    }

    #[test]
    fn a_chance_outside_zero_to_one_is_rejected() {
        let loaded = load_loot(
            r#"[{"mob": "minecraft:zombie", "drops": [{"item": "minecraft:diamond", "chance": 2}]}]"#,
        )
        .expect("the file parses");

        assert!(loaded.rules.is_empty(), "{:?}", loaded.rules);
        assert_eq!(loaded.warnings.len(), 1);
        assert!(
            loaded.warnings[0].contains("chance"),
            "{:?}",
            loaded.warnings
        );
    }
}
