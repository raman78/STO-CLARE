# Detection Samples & Decisions

Empirical record behind `src/analyzer/detection_rules.json`: the real-log combats
each map's **anchor** (identifier) and **tier thresholds** were derived from, plus
open questions. Append here as new samples arrive so we can compare and refine.

Hull numbers are **median hull damage suffered** per entity (what `detect` uses).
Rule of thumb: hull thresholds sit between the Advanced and Elite medians, with the
`HULL_VARIANCE = 0.20` margin (a tier matches when `median > threshold * 0.8`).

**Anchoring rules learned:**
- Never anchor on `Device_*` — player-carried devices, random per run.
- Patrols/TFOs randomize the **enemy** faction → anchor on a stable, *specific*
  ally / mission object, not the enemy, and not a generic ally
  (`Space_Federation_Cruiser_Galaxy` is too broad — appears everywhere).
- When the **tier signal** entity varies by faction but its HP does not, list every
  variant under `hull_any` (any-of) with the same band.
- **Tier only on entities that die** (`deaths > 0`): their median hull damage ≈ HP.
  A `deaths = 0` entity's hull is just the damage we *happened to deal* to it
  (player-dependent), not its HP — fine as an anchor, useless as a tier threshold.
- Non-dying **"friend"/objective ships** make good anchors, but individually they
  recur across maps (only *narrowing* the choice). The same *combination* of two
  rarely repeats — so anchor on the **set together** via `identifiers_all` (all-of),
  not `identifiers` (any-of). E.g. To Die With Honor = Mrek **and** Lrell; Unwanted
  Guests = both Aetherian ships.

---

## [Patrol] Trouble Over Terrh — Space — DONE (anchor + tier)
- **Anchor:** `Space_Elachi_Frigate` (Elachi enemy). Shares the rescued Lleiset
  (`Mission_Space_Romulan_Colony_Flagship_Lleiset`) with Azure Nebula Rescue, so
  disambiguated by **enemy faction** (Elachi vs Tholian), not the Lleiset.
- **Tier:** `hull_counts` on `Space_Elachi_Frigate` (Adv 80k / Elite 300k) and
  `Space_Elachi_Escort` (Adv 350k / Elite 1.4M).

| sample | tier | Frigate | Escort | Battleship_V1 |
|---|---|---|---|---|
| 2026-07-25 21:03 | Advanced | 106,615 | 486,256 | 620,136 |
| 2026-07-25 12:08 | Elite | 490,858 | 2,279,671 | 2,835,426 |

---

## [TFO] Azure Nebula Rescue — Space — anchor only (no tier yet)
- **Anchor:** `Space_Tholian_Cruiser_Web` (re-anchored off the shared Lleiset onto a
  Tholian ship, so Elachi/Terrh runs no longer false-match Azure).
- **Tier:** none yet. Data below would support hull_counts on Cruiser_Web /
  Battleship if wanted.

| sample | tier | Tholian_Cruiser_Web | Tholian_Battleship |
|---|---|---|---|
| 2026-07-25 21:43 | Advanced | 355,249 | 652,603 |
| 2026-07-25 21:22 | Elite | 1,700,205 | 2,888,013 |

---

## [Patrol] Rescue and Search — Space — DONE (anchor + tier)
- **Anchor:** `Space_Klingon_Cruiser_Dsc_Mokai` (Mokai enemy).
- **Tier:** `hull_counts` on Mokai Cruiser + Battleship (Adv 300k / Elite 1.2M).
- Samples (earlier session): Advanced Cruiser ~363k / Battleship ~338k; Elite
  Cruiser ~1.49M / Battleship ~1.54M.

---

## [Patrol] Unwanted Guests — Space — DONE (anchor + any-of tier)
- **Enemies randomized** across ~3 Borg factions: regular (`Space_Borg_*`), Mirror
  (`Space_Borg_*_Mirror`), Control (`Space_Borg_*_Control`).
- **Anchor:** allied `Space_Aetherian_Cruiser` **and** `Space_Aetherian_Dreadnought`
  together (`identifiers_all`; stable across factions; NOT the Borg, NOT the
  `Device_*` team buff).
- **Tier:** `hull_any` on the dreadnought — HP is faction-independent, only the
  entity name changes. Bands: Advanced 1.5M / Elite 4.0M; variants listed:
  `Space_Borg_Dreadnought_Mirror` / `_Control` / plain `Space_Borg_Dreadnought`.

| sample | tier | faction | dreadnought median |
|---|---|---|---|
| 2026-07-26 11:46 | Advanced | Mirror | 1,954,225 |
| 2026-07-26 11:51 | Elite | Control | 8,568,353 |
| 2026-07-26 12:02 | Elite | regular | 8,300,426 |

Open: only Mirror seen at Advanced — a regular/Control **Advanced** sample would
confirm the dreadnought's Advanced band is truly faction-independent.

---

## [Patrol] To Die With Honor — Space — DONE (anchor + tier)
- Fixed-faction **Klingon** patrol (Forcas system). Distinct from the *TFO* "To
  Hell With Honor".
- **Anchor:** named mission dreadnoughts `Space_Klingon_Dreadnought_Mrek` **and**
  `Space_Klingon_Dreadnought_Ktinga_Lrell` together (`identifiers_all` — both
  deaths=0; the pair pins the map even if either recurs alone elsewhere).
- **Tier:** `hull_counts` on ships that **die** — `Space_Klingon_Battlecruiser`
  (Adv 250k / Elite 800k) + `Space_Klingon_Raider` (Adv 90k / Elite 350k).
- **Lesson:** don't tier on a deaths=0 entity (Mrek) — its "hull" is damage we
  *dealt*, player-dependent, not its HP. Use killed entities (hull ~ HP).

| sample | tier | Mrek (deaths=0, not HP) | Battlecruiser | Raider | Escort |
|---|---|---|---|---|---|
| 2026-07-26 12:47 | Advanced | 1,144,742 | 357,756 | 118,931 | 206,499 |
| 2026-07-26 13:38 | Elite | 5,832,321 | 1,196,890 | 523,080 | 928,583 |

## [Patrol] Out of Control — Space — DONE (anchor + tier)
- **Fixed faction** (Borg Control in both samples — not randomized like Unwanted
  Guests). Sitor-system patrol.
- **Anchor:** `Space_Borg_Barrage_Turret_Sitor_Patrol` / `..._Caster` (mission
  barrage turrets, deaths=0, present in both tiers, Sitor-specific).
- **Tier:** `hull_counts` (all-of) on `Space_Borg_Battleship_Control`
  (Adv 300k / Elite 1.0M) + `Space_Borg_Cruiser_Control` (Adv 200k / Elite 800k).
- The Control Borg entities are shared with Unwanted Guests, but the anchors
  differ (Sitor turret vs Aetherian ally), so the maps never cross.

| sample | tier | Battleship_Control | Cruiser_Control | Frigate_Control |
|---|---|---|---|---|
| 2026-07-26 12:54 | Advanced | 421,569 | 289,338 | 117,537 |
| 2026-07-26 13:12 | Elite | 1,489,182 | 1,211,985 | 467,344 |

## [Patrol] The Ninth Rule — Space — CATALOG-ONLY (no reliable anchor)
- **Enemies fully randomized** (~3–4 factions); samples show Gorn and Orion.
- **No usable common anchor.** Entities present in both runs: only
  `Device_Event_Kobayashi_Maru_Resupply` (device, excluded) and
  `Space_Federation_Cruiser_Galaxy` (generic ally → false positives).
- `Mission_Space_Federation_Science_Hofmann` (mission-specific ally) appeared in
  the Elite run but **not** the Advanced run — not reliably present.

| sample | tier | faction | Battleship | Cruiser | Frigate |
|---|---|---|---|---|---|
| 2026-07-26 11:42 | Advanced | Gorn | 343,967 | 276,849 | 113,646 |
| 2026-07-26 12:24 | Elite | Orion | 1,513,717 | 1,192,155 | 477,575 |

Open: confirm whether `Mission_..._Hofmann` (or another fixed ally/object) is in
**every** Ninth Rule run — if yes, anchor on it (tier could then use `hull_any` on
the battleship/cruiser, which scales ~4× Adv→Elite regardless of faction).

---

## [TFO] Bug Hunt — Ground — anchor only (tier needs an Advanced sample)
- **Anchor:** `Bluegills_Ground_Boss`. Fixed faction (Bluegills / Undine), so no
  any-of needed; tier will come from a death-count / Elite-only entity or a hull
  threshold — but that requires comparing Elite vs Advanced.
- Ladder has Bug Hunt `Elite` + `None` only (no Advanced), so ladder can't help.

| sample | tier | Boss hull | notable entities (deaths) |
|---|---|---|---|
| 2026-07-26 12:58 | Elite | 451,781 | Capt 1, Cdr 23, Lt 33, Ens 60, Ens_Noautospawn 83, **Ens_Noautospawn_Queenfodder 13** |

Open: need a **Bug Hunt Advanced** run to diff against. Candidate discriminators to
check then: is `Bluegills_Ground_Ens_Noautospawn_Queenfodder` Elite-only? does the
boss hull (~452k Elite) separate the tiers? do the rank death counts differ?

## [Patrol] Jupiter Station Showdown — Ground — DONE (anchor + tier)
- **Anchor:** `Msn_Ground_Capt_Mirror_Janeway_Boss_Unkillable`.
- **Tier:** `death_counts` Elite keyed on the presence (count 0) of
  `Msn_Assimilated_Fed_Odyssey_Ground_Borg_Ens_Melee_Adapt_Elite_Only`.
- Category/type corrected to **Patrol / Ground** (wiki-confirmed; Ground also from
  the `Msn_Ground_...` entity prefix).

---

## OSCR-ported maps (thresholds from OSCR, not from our logs)
Infected: The Conduit, Cure Found, Khitomer Vortex, Hive Onslaught, Battle of
Wolf 359, Miner Instabilities — death-count and/or hull tables were ported from
OSCR. No local sample log recorded; refresh from OSCR if they change.

## OSCR ladder — exhausted (do not revisit as a source)
The OSCR DPS ladder (`https://oscr.stobuilds.com/ladder/`) only carries a small,
fixed set of **"authorized" maps** — as of 2026-07 exactly **10**: Bug Hunt,
Cure Found, Hive: Onslaught, Infected: The Conduit, Jupiter Station Showdown,
Khitomer Vortex, Miner Instabilities, Nukara Prime, Operation Wolf, Winter
Invasion. All of these are already handled (tiered, or single-difficulty). It will
**never** cover the other TFOs or any patrol, so all remaining tier tables must
come from Raman's own logs (Advanced+Elite pairs), map by map. (Pipeline, if ever
needed again: `/ladder/` → filter by `name`+`difficulty` → `/ladder-entries/` for
combatlog IDs → `/combatlog/<id>/download/` → run the entity dump.)

Marginal leftover: Bug Hunt has ladder `Elite` + `None` (no `Advanced`), so its
Advanced band can't be sourced from the ladder — needs a real Advanced run.
