#!/usr/bin/env bash
# In-game check for InControl: `spawn.json` deny rules and `loot.json` drops.
#
# One launch in The End (an empty level on a stone floor at y=63), then:
#   1. rules `[]`                  -> `/summon zombie` appears (no rule, no veto)
#   2. onjoin zombie deny          -> the zombie is gone, a cow still spawns
#   3. edit back to `[]` + reload  -> the zombie appears again (reload works)
#   4. pig deny in the_nether      -> allowed in the End, denied in the Nether
#   5. sheep deny minheight 70..80 -> denied at y=75, allowed at y=64
#   6. default (position) zombie deny -> a `/summon` is NOT denied (upstream
#      checks `position` only on natural spawn attempts; `onjoin` covers commands)
#   7. a broken file leaves the last good rules in force
#   8. `maxlight: 0` deny -> a zombie in a dark box is denied, one in a lit box
#      (glowstone) is allowed; removing the rule keeps the dark-box zombie
#   9. `minlight: 1` deny -> the inverse: dark box allowed, lit box denied;
#      removing the rule keeps the lit-box zombie
#  10. no loot rule          -> breaking stone drops cobblestone
#  11. loot `replace`        -> a killed zombie drops exactly a diamond, no
#      rotten flesh
#  12. loot `add`            -> a killed sheep drops its wool and mutton plus a
#      diamond
#  13. loot `remove: true`   -> a killed sheep drops nothing at all
#  14. edited loot + reload  -> stone drops an emerald; rule removed -> stone
#      drops cobblestone again
#
# Every phase reloads the files with `/incontrol reload` (the player chat path:
# `ctl command` bypasses the native `run_mod_command` hook).
# COUNTERFACTUAL=1 (alias SELFTEST=1) keeps spawn.json at `[]` in phase 2 and
# asserts the zombie is NOT denied, so the phase-2 assertion can fail. The run
# must then exit 0 early: the inverted expectation is the fail-capable check.
#
# SDK layout (0.2.0): GAME must be a **launcher-built** horizon-client carrying
# this package (`horizon-launcher native-build <port> … --trust-native-code`),
# because the mod's code is compiled into the game; no `--dev-mod` is needed and
# the package carries no content of its own. `CONTROL_GAME=` optionally names a
# game **without** the package for a "the mod is what does it" leg: there the
# commands are unknown and a summoned zombie stays.
#
# The rule files are the mod's own config folder (`Host::config_files`), i.e.
# `<instance>/config/incontrol/{spawn,loot}.json`. The instance root differs
# between builds, so both known spellings are written unless CONFIG_ROOT pins
# one; each write goes to every root so the leg does not depend on the guess.
set -euo pipefail

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/incontrol
# The control binary is shared at the campaign pin it was published for.
CTL=${CTL:-$CLONE/ports/_shared/bin/horizon-1d63e0a5/horizon-ctl}
GAME=${GAME:-}
CONTROL_GAME=${CONTROL_GAME:-}
RES=${RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}
SOCK=${SOCK:-/tmp/opencode/incontrol-e2e.sock}
COUNTERFACTUAL=${COUNTERFACTUAL:-${SELFTEST:-0}}

[ -n "$GAME" ] || {
    echo "set GAME to the launcher-built horizon-client carrying incontrol.hmod" >&2
    echo "(horizon-launcher native-build $PORT --instance <i> --trust-native-code)" >&2
    exit 1
}
[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }
[ -f "$PORT/horizon-mod.toml" ] || { echo "missing package manifest" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/incontrol-home.XXXXXX)
# The game folder is the working directory and holds `config/`; XDG_CONFIG_HOME
# is the other spelling a build may use for the same folder.
CONFIG_ROOTS=${CONFIG_ROOT:-"$SCRATCH/game/config $SCRATCH/config"}
CFG=$SCRATCH/spawn.json
LOOT=$SCRATCH/loot.json
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
for root in $CONFIG_ROOTS; do
    mkdir -p "$root/incontrol"
done
mkdir -p "$(dirname "$SOCK")"
rm -f "$SOCK"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
# Mob commands and mod commands both arrive through the player's chat path.
say() { ctl chat "/$1"; }
# `ctl entities` filters by type already; mobs report kind "mob", so count the
# filtered reply instead of matching a kind.
count() { ctl entities 16 "$1" | jq '.entities | length'; }
# Writes a rule file to every config root the build may read, then reloads.
publish_rules() {
    local file=$1 text=$2 root
    printf '%s\n' "$text" > "$file"
    for root in $CONFIG_ROOTS; do
        cp "$file" "$root/incontrol/$(basename "$file")"
    done
    say "incontrol reload"
    ctl wait 5
}
rules() { publish_rules "$CFG" "$1"; }
loot() { publish_rules "$LOOT" "$1"; }
expect() {
    local type=$1 want=$2 label=$3 got
    got=$(count "$type")
    echo "$label: $type = $got (expected $want)"
    printf '%s\t%s\t%s\n' "$label" "$type" "$got" >> "$SCRATCH/spawn-table.tsv"
    [ "$got" -eq "$want" ] || fail "$label: expected $want $type, got $got"
}
# Dropped item entities, summed per item id (`minecraft:item` entities).
item_count() {
    ctl entities 16 minecraft:item \
        | jq -r --arg i "$1" '[.entities[] | select(.item.item == $i) | .item.count] | add // 0'
}
wool_count() {
    ctl entities 16 minecraft:item \
        | jq -r '[.entities[] | select(.item.item | endswith("_wool")) | .item.count] | add // 0'
}
# A sheep's wool colour is random, so assert whether any wool dropped.
check_item() {
    local label=$1 item=$2 want=$3 got
    got=$(item_count "$item")
    echo "$label: $item = $got (expected $want)"
    printf '%s\t%s\t%s\n' "$label" "$item" "$got" >> "$SCRATCH/loot-table.tsv"
    [ "$got" -eq "$want" ] || fail "$label: expected $want $item, got $got"
}
check_wool() {
    local label=$1 want=$2 got
    got=$(wool_count)
    echo "$label: wool = $got (expected $want)"
    printf '%s\t%s\t%s\n' "$label" "wool" "$got" >> "$SCRATCH/loot-table.tsv"
    [ "$got" -eq "$want" ] || fail "$label: expected $want wool, got $got"
}
clear_items() { ctl command "kill @e[type=item]"; ctl wait 5; }

cleanup() {
    ctl quit >/dev/null 2>&1 || true
    rm -rf "$SCRATCH"
}
trap cleanup EXIT

echo "== launch =="
printf '[]\n' > "$CFG"
printf '[]\n' > "$LOOT"
for root in $CONFIG_ROOTS; do
    cp "$CFG" "$root/incontrol/spawn.json"
    cp "$LOOT" "$root/incontrol/loot.json"
done
cd "$SCRATCH"

# The "the mod is what does it" leg: a game without the package knows neither
# command and leaves a summoned zombie alone. Optional; it costs one launch.
if [ -n "$CONTROL_GAME" ]; then
    echo "== control: a game without the package =="
    rm -f "$SOCK"
    "$CTL" --socket "$SOCK" launch \
        --game "$CONTROL_GAME" --world InControlControl --seed 20261104 --creative --headless --wait \
        --resources "$RES"
    ctl wait-ready 120
    ctl command "difficulty normal"
    ctl command "execute in minecraft:the_end run tp @s 0 100 0"
    ctl wait 40
    ctl command "fill -4 63 -4 8 63 4 minecraft:stone"
    ctl command "execute in minecraft:the_end run tp @s 0.5 64 3.5"
    ctl wait 20
    ctl chat "/incontrol reload"
    ctl wait 5
    if ctl messages 10 | jq -r '.chat[]' | grep -q "Reloaded InControl rules"; then
        fail "the control game answered /incontrol reload: the mod is installed there"
    fi
    ctl command "summon minecraft:zombie 2 64 0"
    ctl wait 20
    got=$(count zombie)
    echo "control (no mod): zombie = $got (expected 1)"
    [ "$got" -eq 1 ] || fail "control: expected the zombie to stay, got $got"
    ctl quit >/dev/null 2>&1 || true
    rm -f "$SOCK"
fi

"$CTL" --socket "$SOCK" launch \
    --game "$GAME" --world InControlE2E --seed 20261104 --creative --headless --wait \
    --resources "$RES"
ctl wait-ready 120
ctl command "time set noon"
ctl command "difficulty normal"
ctl command "kill @e[type=!player]"

echo "== The End: an empty level with a floor =="
ctl command "execute in minecraft:the_end run tp @s 0 100 0"
ctl wait 40
dim=$(ctl status | jq -r '.dimension')
echo "dimension: $dim"
[ "$dim" = "minecraft:the_end" ] || fail "expected the_end, got $dim"
ctl command "fill -4 63 -4 8 63 4 minecraft:stone"
ctl command "execute in minecraft:the_end run tp @s 0.5 64 3.5"
ctl wait 20

echo "== phase 1: no rule, /summon zombie appears =="
rules '[]'
ctl messages 10 | jq -r '.chat[]' | grep -q "Reloaded InControl rules: 0 spawn rules" \
    || fail "the empty rule file did not load"
ctl command "kill @e[type=zombie]"
ctl command "summon minecraft:zombie 2 64 0"
ctl wait 20
expect zombie 1 "phase1 no rule"

echo "== phase 2: onjoin deny for minecraft:zombie =="
ctl command "kill @e[type=zombie]"
ctl command "kill @e[type=cow]"
if [ "$COUNTERFACTUAL" = 1 ]; then
    # Counterfactual: the same setup, but no rule at all.
    rules '[]'
else
    rules '[{"mob": "minecraft:zombie", "when": "onjoin", "result": "deny"}]'
fi
ctl command "summon minecraft:zombie 2 64 0"
ctl command "summon minecraft:cow 4 64 0"
ctl wait 20
if [ "$COUNTERFACTUAL" = 1 ]; then
    expect zombie 1 "phase2 counterfactual (no rule)"
else
    expect zombie 0 "phase2 deny"
fi
expect cow 1 "phase2 control"

if [ "$COUNTERFACTUAL" = 1 ]; then
    ctl quit >/dev/null 2>&1 || true
    trap - EXIT
    echo "spawn table ($SCRATCH):" >&2
    cat "$SCRATCH/spawn-table.tsv"
    rm -rf "$SCRATCH"
    echo "COUNTERFACTUAL PASS: without the deny rule the summoned zombie stays (the phase-2 assertion can fail)"
    exit 0
fi

echo "== phase 3: edit back to [] and reload -> the zombie appears =="
ctl command "kill @e[type=zombie]"
rules '[]'
ctl command "summon minecraft:zombie 2 64 0"
ctl wait 20
expect zombie 1 "phase3 reload to empty"

echo "== phase 4: dimension scope (pig denied in the_nether only) =="
rules '[{"mob": "minecraft:pig", "dimension": "minecraft:the_nether", "when": "onjoin", "result": "deny"}]'
ctl command "kill @e[type=pig]"
ctl command "summon minecraft:pig 2 64 0"
ctl wait 20
expect pig 1 "phase4 end allows"
ctl command "execute in minecraft:the_nether run tp @s 0 70 0"
ctl wait 40
dim=$(ctl status | jq -r '.dimension')
[ "$dim" = "minecraft:the_nether" ] || fail "expected the_nether, got $dim"
# A floor at the counting height, so the player does not fall out of range.
ctl command "fill -4 69 -4 8 69 4 minecraft:netherrack"
ctl command "execute in minecraft:the_nether run tp @s 0.5 70 3.5"
ctl wait 20
ctl command "kill @e[type=pig]"
ctl command "kill @e[type=cow]"
ctl command "summon minecraft:pig 2 70 0"
ctl command "summon minecraft:cow 4 70 0"
ctl wait 20
expect pig 0 "phase4 nether denies"
expect cow 1 "phase4 nether control"
ctl command "execute in minecraft:the_end run tp @s 0.5 64 3.5"
ctl wait 40

echo "== phase 5: height box (sheep denied 70..80) =="
rules '[{"mob": "minecraft:sheep", "minheight": 70, "maxheight": 80, "when": "onjoin", "result": "deny"}]'
ctl command "kill @e[type=sheep]"
ctl command "summon minecraft:sheep 2 75 0"
ctl wait 20
expect sheep 0 "phase5 inside box"
ctl command "summon minecraft:sheep 2 64 0"
ctl wait 20
expect sheep 1 "phase5 outside box"
# Differential: the same y=75 spot with no rule keeps the sheep.
rules '[]'
ctl command "summon minecraft:sheep -2 75 0"
ctl wait 20
expect sheep 2 "phase5 box removed"

echo "== phase 6: a default (position) rule does not veto /summon =="
rules '[{"mob": "minecraft:zombie", "result": "deny"}]'
ctl command "kill @e[type=zombie]"
ctl command "summon minecraft:zombie 2 64 0"
ctl wait 20
expect zombie 1 "phase6 position rule skips a command spawn"

echo "== phase 7: a bad file keeps the rules in force =="
rules '[{"mob": "minecraft:cow", "when": "onjoin", "result": "deny"}]'
ctl command "kill @e[type=cow]"
ctl command "summon minecraft:cow 2 64 0"
ctl wait 20
expect cow 0 "phase7 good cow deny"
rules '[{"mob": "minecraft:cow", "when": "onjoin", "result": "deny"},]'
feedback=$(ctl messages 10 | jq -r '.chat[]' | grep -m1 "InControl reload failed" || true)
echo "feedback: $feedback"
case "$feedback" in
    "InControl reload failed:"*) ;;
    *) fail "a broken file did not report the failure" ;;
esac
ctl command "kill @e[type=cow]"
ctl command "summon minecraft:cow 2 64 0"
ctl wait 20
# The broken file was refused, so the cow deny from the last good file still holds.
expect cow 0 "phase7 broken file keeps the rules"

echo "== phase 8: maxlight 0 denies a dark spawn and allows a lit one =="
# Two sealed 3x3x3 boxes on the floor: one unlit, one lit by glowstone in the
# ceiling. Block light is 0 in the dark box and about 14 at the lit box floor.
ctl command "fill 4 64 -2 8 67 2 minecraft:stone"
ctl command "fill 5 64 -1 7 66 1 minecraft:air"
ctl command "fill -5 64 -2 -1 67 2 minecraft:stone"
ctl command "fill -4 64 -1 -2 66 1 minecraft:air"
ctl command "setblock -3 66 0 minecraft:glowstone"
rules '[{"mob": "minecraft:zombie", "maxlight": 0, "when": "onjoin", "result": "deny"}]'
ctl command "kill @e[type=zombie]"
ctl command "summon minecraft:zombie 6 64 0"
ctl wait 20
expect zombie 0 "phase8 dark box denied"
ctl command "summon minecraft:zombie -3 64 0"
ctl wait 20
expect zombie 1 "phase8 lit box allowed"
# Differential: the same dark box with no rule keeps the zombie.
rules '[]'
ctl command "kill @e[type=zombie]"
ctl command "summon minecraft:zombie 6 64 0"
ctl wait 20
expect zombie 1 "phase8 dark box without rule"

echo "== phase 9: minlight 1 is the inverse (dark allowed, lit denied) =="
rules '[{"mob": "minecraft:zombie", "minlight": 1, "when": "onjoin", "result": "deny"}]'
ctl command "kill @e[type=zombie]"
ctl command "summon minecraft:zombie 6 64 0"
ctl wait 20
expect zombie 1 "phase9 dark box allowed"
ctl command "kill @e[type=zombie]"
ctl command "summon minecraft:zombie -3 64 0"
ctl wait 20
expect zombie 0 "phase9 lit box denied"
# Differential: the same lit box with no rule keeps the zombie.
rules '[]'
ctl command "summon minecraft:zombie -3 64 0"
ctl wait 20
expect zombie 1 "phase9 lit box without rule"

echo "== loot: survival gear for block breaks =="
# Creative breaks roll no loot at this pin, and stone needs the right tool.
ctl command "gamemode survival"
ctl command "give @s minecraft:diamond_pickaxe"
ctl wait 10
ctl select minecraft:diamond_pickaxe
reset_drops() {
    ctl command "kill @e[type=zombie]"
    ctl command "kill @e[type=sheep]"
    ctl wait 10
    clear_items
}

echo "== phase 10: no loot rule, stone drops cobblestone =="
loot '[]'
ctl break 0 63 2 | jq -c '{block_before,block,drops}'
check_item "phase10 stone no rule" minecraft:cobblestone 1
check_item "phase10 no diamond" minecraft:diamond 0
clear_items

echo "== phase 11: a replace rule denies the mob drops it replaces =="
loot '[{"mob": "minecraft:zombie", "replace": true, "drops": [{"item": "minecraft:diamond"}]}]'
reset_drops
ctl command "summon minecraft:zombie 4 64 0"
ctl wait 20
ctl command "kill @e[type=zombie]"
ctl wait 20
check_item "phase11 zombie replace diamond" minecraft:diamond 1
check_item "phase11 no rotten flesh" minecraft:rotten_flesh 0
clear_items

echo "== phase 12: an add rule keeps the normal drops =="
loot '[{"mob": "minecraft:sheep", "drops": [{"item": "minecraft:diamond"}]}]'
reset_drops
ctl command "summon minecraft:sheep 4 64 0"
ctl wait 20
ctl command "kill @e[type=sheep]"
ctl wait 20
check_item "phase12 sheep add diamond" minecraft:diamond 1
check_wool "phase12 normal wool kept" 1
clear_items

echo "== phase 13: remove true denies every drop =="
loot '[{"mob": "minecraft:sheep", "remove": true}]'
reset_drops
ctl command "summon minecraft:sheep 4 64 0"
ctl wait 20
ctl command "kill @e[type=sheep]"
ctl wait 20
check_wool "phase13 remove wool" 0
check_item "phase13 remove mutton" minecraft:mutton 0
check_item "phase13 remove diamond" minecraft:diamond 0

echo "== phase 14: an edited loot file applies on reload =="
loot '[{"block": "minecraft:stone", "replace": true, "drops": [{"item": "minecraft:emerald"}]}]'
ctl break 1 63 2 | jq -c '{block_before,block,drops}'
check_item "phase14 edited replace emerald" minecraft:emerald 1
check_item "phase14 edited replace no cobblestone" minecraft:cobblestone 0
clear_items
# Differential: the same block with no rule drops its vanilla cobblestone.
loot '[]'
ctl command "setblock 1 63 2 minecraft:stone"
ctl wait 10
ctl break 1 63 2 | jq -c '{block_before,block,drops}'
check_item "phase14 rule removed cobblestone" minecraft:cobblestone 1
clear_items

echo "== phase 15: itemcount tiers at Looting 0 =="
# A command kill has no player, so the engine sees Looting 0 and takes the
# first tier: 5.
loot '[{"mob": "minecraft:zombie", "item": "minecraft:diamond", "itemcount": "5/9-10"}]'
reset_drops
ctl command "summon minecraft:zombie 4 64 0"
ctl wait 20
ctl command "kill @e[type=zombie]"
ctl wait 20
check_item "phase15 itemcount looting0" minecraft:diamond 5
clear_items

echo "== phase 16: itemcount tiers at Looting III =="
# The player must be the killer for the loot context to carry a player; the
# support then reads the held sword's Looting level.
loot '[{"mob": "minecraft:zombie", "item": "minecraft:diamond", "itemcount": "5/9-10"}]'
reset_drops
ctl command "item replace entity @s weapon.mainhand with minecraft:diamond_sword"
ctl wait 5
ctl command "enchant @s minecraft:looting 3"
ctl wait 5
ctl command "summon minecraft:zombie 4 64 0"
ctl wait 20
ctl command "damage @e[type=zombie,limit=1] 100 minecraft:player_attack by @s"
ctl wait 20
got=$(item_count minecraft:diamond)
echo "phase16 itemcount looting3: minecraft:diamond = $got (expected 9..10)"
[ "$got" -ge 9 ] && [ "$got" -le 10 ] || fail "phase16: expected 9..10 diamonds, got $got"
clear_items

echo "== phase 17: item nbt rides on the added drop =="
loot '[{"mob": "minecraft:zombie", "replace": true, "item": "minecraft:diamond", "nbt": {"display": {"Name": "Gem"}}}]'
reset_drops
ctl command "summon minecraft:zombie 4 64 0"
ctl wait 20
ctl command "kill @e[type=zombie]"
ctl wait 20
nbt=$(ctl command "data get entity @e[type=minecraft:item,limit=1] Item")
echo "$nbt"
case "$nbt" in
    *Gem*) ;;
    *) fail "phase17: the added diamond has no Gem custom_data: $nbt" ;;
esac
clear_items

echo "== spawn table =="
cat "$SCRATCH/spawn-table.tsv"
echo "== loot table =="
cat "$SCRATCH/loot-table.tsv"
ctl quit >/dev/null 2>&1 || true
trap - EXIT
rm -rf "$SCRATCH"
echo "PASS: spawn rules veto onjoin mobs by id, dimension, height box and block light; loot rules deny, replace and add drops for a mob and a block; reload applies edits"
