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

The two are complementary: naming stays user-owned and covers arbitrary
encounters; detection is objective and drives the Compare view's difficulty
filter. They do not replace each other.

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
```

`critters` is accumulated live as records stream in (once each), so it is
complete by the time `update()` runs, and it grows monotonically for live
combats — it is never cleared/recomputed like `hits_manger`.

## The rules (`detection_rules.json`)

Bundled next to the module and embedded via `include_str!`, parsed once into the
`DETECTION_RULES` lazy static. Schema mirrors OSCR's three tables:

```jsonc
{
  "map_identifiers": {                 // entity present ⇒ this map
    "<unique_name>": { "map": "...", "difficulty": "Any" | "Normal" | "Advanced" | "Elite" }
  },
  "death_counts": {                    // resolve tier by exact death counts
    "<map>": { "Advanced": { "<unique_name>": <count> }, "Elite": { ... } }
  },
  "hull_counts": {                     // tie-break by median hull damage
    "<map>": { "Advanced": { "<unique_name>": <threshold> }, "Elite": { ... } }
  }
}
```

A required death count of `0` means "must be present, any number of deaths" —
used when only an entity's *existence* distinguishes the tier (e.g. the
Advanced- vs Elite-only pet in Cure Found).

Keeping the tables as JSON (rather than hard-coded like OSCR) means they can be
refreshed when OSCR updates, ideally from a shared canonical file rather than by
scraping OSCR's Python.

## Algorithm (`detection::detect`)

Mirrors OSCR's `detect_map`:

1. **Existence** — intersect the combat's present unique names with
   `map_identifiers`. No match ⇒ `map = None` ("Combat"). A matching entity that
   pins a non-`Any` difficulty returns immediately (e.g. Winter Invasion ⇒
   Normal).
2. **Death counts** — for the identified map, test each tier low→high
   (`DIFFICULTY_ORDER = [Advanced, Elite]`); a higher tier that also matches
   overrides a lower one. A tier matches when every listed entity is present and
   every entry with count `> 0` died *exactly* that many times. If the map has
   tables but none match, the tier is `Any`.
3. **Hull tie-break** — for maps where the two tiers have identical death counts
   (e.g. Hive Space), compare each entity's **median hull damage suffered**
   (across instances) against its threshold: a tier matches when
   `threshold * (1 - HULL_VARIANCE) < median`, with `HULL_VARIANCE = 0.20`
   (OSCR's `var`). Runs after the death phase and overrides it (higher tiers
   still win); skipped when death tables existed but none matched (that stays
   `Any`).

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
  Any", a fixed-difficulty map, and the Hive Space hull tie-break (Advanced vs
  Elite from median hull damage). `bundled_rules_parse` guards the JSON.
- `analyzer::tests::detects_maps_in_real_log` (ignored) — smoke test that prints
  detected `(map, difficulty)` for each combat in the live log.
