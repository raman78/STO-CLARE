# Map & Difficulty Detection

How the analyzer figures out **which STO map** a combat is and **at what
difficulty** (Advanced / Elite / …), independent of the combat's display name.

## Why this exists

STO combat logs carry no explicit map or difficulty marker. Two separate
mechanisms could tell them apart:

- **Combat Name Rules** (`analyzer::settings::CombatNameRule`, user-editable) —
  string-match entity names to produce a *display label*, e.g. append `(Elite)`
  when an entity's unique name ends in `Elite_Initial`. This only works for the
  handful of maps whose Elite NPCs are literally name-tagged, and never detects
  Advanced (STO does not tag advanced-tier NPCs by name).
- **Detection** (this module, `analyzer::detection`) — a data-driven port of the
  OSCR parser's approach (`STOCD/OSCR`, `detection.py` + `combat.py`). It ignores
  names entirely and derives an authoritative `(map, difficulty)` from *what
  actually happened*: which curated boss/NPC internal names appeared, how many
  times specific entities died, and (planned) how much hull damage they suffered.

The display name is **base name + detected difficulty**:

1. Base name: the user's Combat Name Rules if any match (they take priority),
   else the detected map, else `"Combat"`.
2. The detected difficulty is then appended in square brackets (e.g. `[Elite]`)
   unless the base already mentions that tier — see `Combat::name` /
   `append_detected_difficulty`.

The difficulty is computed from the log's entities, independent of naming, so the
tier shows even on combats a user rule names (only the base name is "shadowed",
never the difficulty). It also drives the Compare filter.

## Data flow

```
parse record ─▶ Combat::update_critters()      accumulate per-NPC facts
   (mod.rs)        └─ target is NonPlayer? ─▶ critters[unique_name].deaths += KILL
                                              + hull damage per instance (by id)

Combat::update() ─▶ Combat::update_detection()
                       ├─ build view: unique_name &str ─▶ &CritterMeta
                       └─ detection::detect(&DETECTION_RULES, &view) ─▶ Detected
                              stored on Combat.detected_map / .detected_difficulty

AnalysisHandler::latest_info() ─▶ AnalysisInfo::Refreshed { difficulties, .. }
   ─▶ App.combat_difficulties ─▶ CompareView difficulty filter

Combat::name() ─▶ base = user rules ? join names : detected_map ?? "Combat"
                  append_detected_difficulty(base, detected_difficulty) ─▶ "... [Elite]"
```

The Combat Name Rules editor (`app/settings/analysis::CombatNameRules`) lists the
curated maps read-only below the user rules and flags overlaps two ways:

- **Per rule** (`overlapping_maps`): a ⚠ on the rule's row, tooltip naming the
  shadowed map(s). An enabled rule overlaps a curated map two ways, unioned: (1)
  **entity** — the rule matches the map's identifying NPC (unique name,
  `curated_map_identifiers()`); (2) **name** — the rule's own name equals the
  map's name, ignoring the `[TFO]`/`[Patrol]` category prefix on either side
  (`strip_category_prefix`), e.g. a rule named "Trouble Over Terrh" vs the
  "[Patrol] Trouble Over Terrh" map. The name check catches rules written against
  *display* names, which the entity check (unique names only) cannot see. Both are
  combat-independent. Wired via `GroupRulesTable::with_row_warning`.
- **Per-combat**: when the selected combat is auto-detected but a rule renamed it.

The curated rules are never copied into the user's settings — they are rendered
from `detection::curated_map_names()` / `curated_map_identifiers()` — so refreshing
them is a JSON swap that leaves user rules untouched.

`critters` is accumulated live as records stream in (once each), so it is
complete by the time `update()` runs, and it grows monotonically for live
combats — it is never cleared/recomputed like `hits_manger`.

## The rules (`detection_rules.json`)

Bundled next to the module and embedded via `include_str!`, parsed once into the
`DETECTION_RULES` lazy static. A user override at
`<config dir>/STO_CombatLogAnalyzer/detection_rules.json` (same dir as the app
settings) **fully replaces** the bundled tables when present and valid; a
malformed override is logged and ignored. The file is **keyed by map name**, each
map carrying everything about it (`DetectionRules { maps: HashMap<String, MapDef> }`):

```jsonc
{
  "maps": {
    "<map name>": {                    // the key IS the map's bare name
      "category": "TFO" | "Patrol" | ...,           // optional; shown as a "[category] " prefix
      "combat_type": "Space" | "Ground" | "Shuttle", // optional; curated reference only, not shown
      "difficulty": "Any" | "Normal" | "Advanced" | "Elite", // optional (default Any); a pinned tier
      "identifiers": ["<unique_name>", ...],        // entities whose presence identifies the map
      "death_counts": { "Advanced": { "<unique_name>": <count> }, "Elite": { ... } },
      "hull_counts":  { "Advanced": { "<unique_name>": <threshold> }, "Elite": { ... } }, // all-of
      "hull_any":     { "Advanced": { "<unique_name>": <threshold> }, "Elite": { ... } }  // any-of
    }
  }
}
```

Only `category`/`combat_type`/`difficulty`/`identifiers`/`death_counts`/`hull_counts`
that apply are written; all are `#[serde(default)]`. A map with an **empty (absent)
`identifiers`** is a **catalog-only** entry — a known map (e.g. from the STO wiki)
we cannot detect yet; it never matches. A required death count of `0` means "must
be present, any number of deaths" (e.g. the Advanced- vs Elite-only pet in Cure
Found).

**Never key `identifiers` (or tier tables) on a `Device_*` entity.** Those are
player-carried devices (Kobayashi Maru Resupply, team buffs, …) that appear only
when a player happens to equip one — random per run, not a property of the map.
Anchor on fixed mission NPCs, objects, or allied ships instead (e.g. Unwanted
Guests, whose enemies are randomly regular or Mirror Borg, is anchored on its
allied Aetherian ships).

`category` is curated **content-type metadata** (STO logs carry no such marker):
`MapDef::display_name(name)` renders `[category] <name>` (e.g. `[TFO] Azure Nebula
Rescue`); the bare key name stays the identity used everywhere else. `combat_type`
(Space/Ground/Shuttle, from the STO wiki) is reference-only and never displayed.

Keeping the tables as JSON (rather than hard-coded like OSCR) means they can be
refreshed when OSCR/the wiki updates, ideally from a shared canonical file.

## Algorithm (`detection::detect`)

Mirrors OSCR's `detect_map`, iterating `rules.maps`:

1. **Existence** — find a map whose `identifiers` intersect the combat's present
   unique names (`MapDef::is_present`). No match ⇒ `map = None` ("Combat"). A
   matching map that pins a non-`Any` `difficulty` returns immediately (e.g.
   Winter Invasion ⇒ Normal).
2. **Death counts** — for the identified map, test each tier low→high
   (`DIFFICULTY_ORDER = [Advanced, Elite]`); a higher tier that also matches
   overrides a lower one. A tier matches when every listed entity is present and
   every entry with count `> 0` died *exactly* that many times. If the map has
   tables but none match, the tier is `Any`.
3. **Hull tie-break** — for maps where the two tiers have identical death counts
   (e.g. Hive Onslaught), compare each entity's **median hull damage suffered**
   (across instances) against its threshold: a tier matches when
   `threshold * (1 - HULL_VARIANCE) < median`, with `HULL_VARIANCE = 0.20`
   (OSCR's `var`). `hull_counts` is **all-of** (every listed entity must be
   present and pass); `hull_any` is **any-of** (one present entity passing is
   enough) — used for faction-randomized maps where the tier signal is one entity
   per faction (list every variant with the same band; only whichever spawned is
   checked, e.g. Unwanted Guests' dreadnought). Both run after the death phase and
   override it (higher tiers still win); skipped when death tables existed but
   none matched (that stays `Any`).

## Shared objective entities (anchor on the enemy, not the objective)

The existence phase takes the **first** matching map; when several match, the
winner is order-dependent (`maps` is a `HashMap`), so a map must list
`identifiers` that are **unique to it**. This bites when two maps share an
*objective* entity. Example: both **[TFO] Azure Nebula Rescue** and the
**[Patrol] Trouble Over Terrh** patrol rescue the same allied ship
(`Mission_Space_Romulan_Colony_Flagship_Lleiset`), so anchoring either on the
Lleiset makes them indistinguishable. They are told apart by **enemy faction**
instead — Azure lists a Tholian ship (`Space_Tholian_Cruiser_Web`), Terrh an
Elachi one (`Space_Elachi_Frigate`) — which never co-occur, so the Lleiset is
left out of both maps' `identifiers` entirely. Both tiers are then resolved by
the hull tie-break (deaths are run-dependent), like Rescue and Search.

## What we already parse vs. what this adds

Every raw signal is already produced by the parser — no parser changes were
needed:

- `Entity::NonPlayer { unique_name, _id }` — the internal name OSCR keys on.
- `ValueFlags::KILL` — the killing blow.
- hull vs shield split at hit level (`SpecificHit::Hull`).

What was missing was a **per-NPC aggregation** (`Combat::critters`), since the
normal damage tree is grouped by player→ability→target, with kills keyed by
*display* name and scattered across the tree. `critters` is that flat table,
keyed by unique name.

## Tests

- `analyzer::detection::tests` — unit tests over synthetic critter tables:
  unknown map, Advanced by counts, Elite override, "known map but wrong counts ⇒
  Any", a fixed-difficulty map, the Hive Space hull tie-break (Advanced vs Elite
  from median hull damage), the Trouble Over Terrh hull tiers, and that Azure vs
  Terrh are told apart by enemy faction. `bundled_rules_parse` guards the JSON.
- `analyzer::tests::detects_maps_in_real_log` (ignored) — smoke test that prints
  detected `(map, difficulty)` for each combat in the live log.
