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

The two form a **priority (firewall) stack** for the display name:

1. User Combat Name Rules win — a matching rule names the combat (and may append
   its own `(Elite)` etc. via additional-info rules).
2. Otherwise the **detected** map name is used, with the difficulty appended
   (e.g. `Hive Space (Elite)`) — see `Combat::name` / `Combat::detected_name`.
3. Otherwise `"Combat"`.

So a user rule "shadows" the detected name (the editor flags this for the
selected combat). The **difficulty** itself always comes from detection,
regardless of naming, and drives the Compare filter.

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

Combat::name() ─▶ user rules matched? ─▶ join their names   (they win)
                  else detected_name() ─▶ "<map> (<difficulty>)"  (fallback)
                  else "Combat"
```

The Combat Name Rules editor (`app/settings/analysis::CombatNameRules`) lists the
curated maps read-only below the user rules and flags overlaps two ways:

- **Static** (`rule_map_overlaps`): each enabled user rule is tested against every
  curated identifier's unique name (`curated_map_identifiers()`); a match means
  the rule shadows that map, listed as "rule → map(s)".
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
malformed override is logged and ignored. Schema mirrors OSCR's three tables:

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
