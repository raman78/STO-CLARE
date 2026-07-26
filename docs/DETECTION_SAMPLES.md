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
- **Anchor:** allied `Space_Aetherian_Cruiser` / `Space_Aetherian_Dreadnought`
  (stable across factions; NOT the Borg, NOT the `Device_*` team buff).
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
