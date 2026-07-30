# Map & Difficulty Detection

How the analyzer figures out **which STO map** a combat is and **at what
difficulty** (Advanced / Elite / …), independent of the combat's display name.

## What the combat log does *not* contain (verified 2026-07-30)

Checked against a real 765,692-line log, because "the log has no map marker" is
the premise everything here rests on:

- **Every single line has the same 12-field shape.** No header lines, no
  differently-shaped records, nothing to mark a map load, a combat start, or a
  combat end.
- The type field only ever holds damage kinds (`Shield`, `Phaser`, `Kinetic`,
  `HitPoints`, …) — there are no system events.
- Not one occurrence of "map", "instance" or "loading". The only hits for "Zone"
  are `Gravimetric Detonation Zone`, a player kit power.

So there is **no map name and no map id anywhere in the log**, which is why
entity anchors are the only route to a map name — and why OSCR does the same.

### A map-boundary signal does exist, in another file

`CLIENTSERVERCOMM.log`, written by the game next to `combatlog.log`, logs server
transfers — i.e. actual map changes — with timestamps **in UTC**. Against the
2026-07-30 combats these line up closely:

| combat (combatlog, local time) | transfer logged |
|---|---|
| Into the Hive ends 12:10:02 | 12:10:22 |
| Undine Assault 12:17:00–12:23:07 | 12:15:56 (in), **12:23:08** (out) |
| Undine Infiltration ends 12:46:47 | 12:46:52 |

It carries **no map names** — only "disconnected from our gameserver for
transfer" — so it cannot help identify a map. What it could give is exact combat
*boundaries*, replacing the `combat_separation_time_seconds` heuristic. That
heuristic has a known failure: at 90 s a Defense of Starbase One run merged with
a Rescue and Search run into one combat, which briefly made the Normal HP bands
look like they overlapped (see `DETECTION_SAMPLES.md`).

Not implemented. It would add a dependency on a second file whose presence and
format we do not control (unverified on Windows), plus timezone conversion.

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
   (mod.rs)        ├─ source or target is NonPlayer? ─▶ critters[unique_name]
   (mod.rs)        │     exists  ("present", even with no damage record at all)
   (mod.rs)        └─ target is NonPlayer? ─▶ critters[unique_name].deaths += KILL
                                              + hull damage per instance (by id)

                   (tier phases, in order: death_counts ─▶ hull_counts ─▶
                    hull_any ─▶ global ship-class table, which has the last word)

Combat::update() ─▶ Combat::update_detection()
                       ├─ build view: unique_name &str ─▶ &CritterMeta
                       └─ detection::detect(&DETECTION_RULES, &view) ─▶ Detected
                              stored on Combat.detected_map / .detected_difficulty

AnalysisHandler::latest_info() ─▶ AnalysisInfo::Refreshed { difficulties, .. }
   ─▶ App.combat_difficulties ─▶ CompareView difficulty filter

Combat::name() ─▶ base = user rules ? join names : detected_map ?? "Combat"
                  append_detected_combat_type(base, detected_combat_type) ─▶ "... (Ground)"
                  append_detected_difficulty(base, detected_difficulty) ─▶ "... [Elite]"
```

The Combat Name Rules editor (`app/settings/analysis::CombatNameRules`) lists the
curated maps read-only below the user rules and flags overlaps two ways:

- **Per rule** (`overlapping_maps`): a ⚠ on the rule's row, tooltip naming the
  shadowed map(s). An enabled rule overlaps a curated map two ways, unioned: (1)
  **entity** — the rule matches the map's identifying NPC (unique name,
  `curated_map_identifiers()`); (2) **name** — the map's name *appears in* the
  rule's own name, ignoring the `[TFO]`/`[Patrol]` category prefix on either side
  (`strip_category_prefix`), e.g. a rule named "Trouble Over Terrh" vs the
  "[Patrol] Trouble Over Terrh" map. The name check catches rules written against
  *display* names, which the entity check (unique names only) cannot see. Both are
  combat-independent. Wired via `GroupRulesTable::with_row_warning`.

  The name check is **containment, not equality**. Users annotate their rules
  (a real one is `[Patrol] The Ninth Rule [M]`), and the trailing tag is not a
  category prefix, so `strip_category_prefix` leaves it in place and an exact
  comparison silently produced *no* warning — the failure mode is invisible,
  which is the worst kind. No curated map name is a substring of another (203
  names, verified), so containment introduces no ambiguity.

  Note the entity check only sees **unique** names, so a rule keyed on a
  `SourceOrTargetName` (display name, e.g. "U.S.S. Birmingham") can only ever be
  flagged by the name check — the rules file carries no display names to compare
  against.
- **Per-combat**: when the selected combat is auto-detected but a rule renamed it.

The curated rules are never copied into the user's settings — they are rendered
from `detection::curated_map_names()` / `curated_map_identifiers()` — so refreshing
them is a JSON swap that leaves user rules untouched.

`critters` is accumulated live as records stream in (once each), so it is
complete by the time `update()` runs, and it grows monotonically for live
combats — it is never cleared/recomputed like `hits_manger`.

### The global ship-class tier (`global_tier`)

A map-independent Advanced/Elite split, applied **after** the per-map tables and
overriding them. Rationale and the measured numbers live in
`DETECTION_SAMPLES.md`; in short, space HP is a property of the ship *class*, so
one table tiers maps we never sampled — 18 of the 37 detectable maps have an
anchor but no tier tables of their own.

```
GlobalTier { exclude: Vec<String>, classes: Vec<ShipClassBand> }
ShipClassBand {
    match: String,                    // case-insensitive substring of the unique name
    threshold: f64,                   // above this  -> Elite
    normal_threshold: Option<f64>,    // at or below -> Normal (absent = never Normal)
}
```

Each entity that **died** and is not excluded votes Normal/Advanced/Elite against
its class thresholds; a strict majority wins, and a tie yields `None` (ambiguous —
fall back to the map's tables). `classes` is an ordered list, not a map:
`Battlecruiser` must be tested before `Cruiser`, which is a substring of it.

`normal_threshold` is **experimental**, with margins of ~1.6x rather than the
~4.4x that makes the Advanced/Elite split safe. It is optional per class precisely
so it can be dropped again without touching code.

**Per-faction bands.** Because `classes` is matched in order, a faction-specific
entry placed before the generic one overrides it — `Elachi_Battleship` wins over
`Battleship`. This is how the ~2.3x HP spread *between factions within a class* is
handled, and it needed no code change: it is the same ordering rule that already
puts `Battlecruiser` before `Cruiser`. Elachi have their own bands because their
Normal ships are as tough as other factions' Advanced ones. Add further factions
the same way when a map misreads.

It deliberately has the **last word** rather than acting as a fallback, so that a
disagreement with a hand-verified per-map table surfaces instead of being masked.
Disagreements are logged via `warn!`. Against the live log the two never
disagreed on the 19 tabled maps.

⚠ Unit tests built with `hull_critter` have `deaths = 0`, so the global tier does
not vote in them; use `dead_hull_critter` to exercise it.

### Presence vs. tier data (why the two branches differ)

The two branches above answer different questions, and conflating them was a bug:

- **Presence** (`identifiers` / `identifiers_all`) — "was this entity on the map?"
  Recorded for a non-player in **either** role. Many maps anchor on a
  non-combatant ally or objective, and such an entity may fire a single shot and
  never be hit, or only ever be healed. Keying presence off damage *taken* made
  those runs undetectable (e.g. a Ninth Rule run whose allied Galaxy cruiser fires
  once and is never targeted).
- **Tier data** (`hull_counts` / `hull_any` / `death_counts`) — "how tough was it?"
  Still recorded **only** from damage dealt *to* the entity, since that is the
  only thing that approximates HP.

An entity that is present but never damaged therefore has an empty
`hull_damage_per_instance`, so `median_hull_damage()` returns `0.0` and it matches
no tier threshold. Presence is widened; tier resolution is unchanged.

### Environment in the name (`combat_type`)

Every catalogued map carries a curated `combat_type` — "Space" (156), "Ground"
(44) or "Shuttle" (3) — taken from the STO wiki. The log states no such thing, so
this is metadata, not detection. It rides along in `Detected::combat_type` and is
appended in parentheses: `[TFO] Into the Hive (Ground) [Advanced]`.

It is **not** folded into `MapDef::display_name`, even though that would be
shorter. That string is what the settings editor matches user naming rules
against (`overlapping_maps`), so appending an environment there would stop a rule
named after the map from being recognized as overlapping it.

`append_detected_combat_type` skips the suffix when the base name already
mentions the environment, so a user rule called "Bug Hunt Ground" does not become
"Bug Hunt Ground (Ground)".

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
      "identifiers": ["<unique_name>", ...],        // any-of: any one present identifies the map
      "identifiers_all": ["<unique_name>", ...],    // all-of: the whole set must be present together
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

1. **Existence** — find a map that `MapDef::is_present`: any one of its any-of
   `identifiers` is present, or its whole all-of `identifiers_all` set is present
   together. No match ⇒ `map = None` ("Combat"). A matching map that pins a
   non-`Any` `difficulty` returns immediately (e.g. Winter Invasion ⇒ Normal).
   (`identifiers_all` anchors on a *combination* of non-dying friend/objective
   ships that recur individually but not as a set.)
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
