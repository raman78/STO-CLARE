# Detection Samples & Decisions

Empirical record behind `src/analyzer/detection_rules.json`: the real-log combats
each map's **anchor** (identifier) and **tier thresholds** were derived from, plus
open questions. Append here as new samples arrive so we can compare and refine.

Hull numbers are **median hull damage suffered** per entity (what `detect` uses).
Rule of thumb: hull thresholds sit between the Advanced and Elite medians, with the
`HULL_VARIANCE = 0.20` margin (a tier matches when `median > threshold * 0.8`).

**Anchoring rules learned:**
- Never anchor on `Device_*` — player-carried devices, random per run.
- Patrols/TFOs randomize the **enemy** faction → anchor on a stable ally /
  mission object, not the enemy.
- **Measure "generic" before rejecting it.** A plain-looking ally name is not
  automatically too broad: scan the whole log and count how many *distinct*
  combats contain it. `Space_Federation_Cruiser_Galaxy` was written off here as
  "appears everywhere", but it occurs in 7 of 62 combats and all 7 are The Ninth
  Rule — non-combatant allies are rarer in logs than their names suggest.
- **A non-combatant ally is only logged when it trades damage**, so its absence
  from a run does *not* mean it was absent from the map. That makes any single
  ally anchor lossy; pairing two of them any-of covers each other's gaps
  (The Ninth Rule: Hofmann ∪ Galaxy = 8/8 runs, neither alone > 5/8).
- **Presence now counts either role** (fixed 2026-07-29). `update_critters` used
  to register an NPC only as the *target* of a damage record, so an ally that
  fires once and is never hit — or is only ever healed — was invisible to
  detection no matter what the rules said. It now registers a non-player in
  source **or** target position as present, while hull/death tier data still
  comes only from damage dealt to it. Verified against the real log: of 59
  combats exactly one changed (the 2026-07-28 22:18 Ninth Rule run), no
  regressions, no measurable parse cost. If a map still goes unrecognized with a
  correct-looking anchor, check whether the anchor appears in the log **at all**
  before blaming the rules.
- When the **tier signal** entity varies by faction but its HP does not, list every
  variant under `hull_any` (any-of) with the same band.
- **Tier only on entities that die** (`deaths > 0`): their median hull damage ≈ HP.
  A `deaths = 0` entity's hull is just the damage we *happened to deal* to it
  (player-dependent), not its HP — fine as an anchor, useless as a tier threshold.
- Non-dying **"friend"/objective ships** make good anchors, but a *generic* one
  recurs across maps (only narrows). The same *combination* rarely repeats, so
  `identifiers_all` (all-of) pins it. **BUT combinations are fragile to combat
  splitting** (a user's `combat_separation_time_seconds` — 45s — fragments a patrol
  with long lulls; a fragment may carry only part of the set → the map goes
  unrecognized). So prefer **any-of on a *named*/specific entity** (it doesn't
  recur, and one is enough in every fragment); reserve `identifiers_all` for
  genuinely generic friends on maps that don't fragment. E.g. To Die With Honor is
  any-of [Mrek, Lrell] (a 45s split left a fragment with only Lrell + Mrek's Placate
  variant); Unwanted Guests keeps the Aetherian pair (it doesn't split).

---

## The global ship-class tier (map-independent Advanced/Elite)

Derived 2026-07-29 from the nine mapped TFOs/patrols below, by dumping every
NPC's median hull damage tagged with the detected map and tier
(`dump_critter_hp_by_tier`, an ignored test).

**Finding: space HP is a property of the ship *class*, not of the map or the
faction.** For the *same entity*, Elite carries ~4.4x the hull of Advanced:

| environment | n (entity pairs) | median Elite/Advanced | quartiles |
|---|---|---|---|
| Space | 45 | **4.44x** | 4.20–4.61 |
| Ground | 7 | **1.54x** | 1.33–1.58 |
| Boss | 1 | 6.31x | — |

Because the ratio is near-constant, each class occupies the same band on every
map, and one global table can tier a map we have never sampled:

| class | Advanced observed | Elite observed | threshold used |
|---|---|---|---|
| Dreadnought | 1.68–1.95M | 8.30–8.99M | 4,000,000 |
| Battleship | 301–620k | 1.16–7.09M | 850,000 |
| Escort | 203–491k | 915k–2.28M | 670,000 |
| Battlecruiser | 273–358k | 1.20–1.21M | 650,000 |
| Cruiser | 196–465k | 875k–2.57M | 640,000 |
| Frigate | 91–167k | 418–746k | 264,000 |
| Raider | 92–155k | 399–551k | 250,000 |

Each threshold is the geometric mean of the highest Advanced and lowest Elite of
that class, i.e. ~2.1x (= √4.4) above a typical Advanced.

**Validation — leave-one-map-out** (thresholds recomputed *without* the map under
test, so the map is genuinely unseen): **217/220 entities**, and **44/44 combats**
once entities vote by majority. The three entity-level misses are all
`Space_Elachi_Escort`, whose Advanced hull (486–491k) is double every other
Advanced escort (203–223k) — Elachi "escorts" are really cruiser-rank. The combat
still resolved correctly, 5 votes to 1.

**Verified against the live log:** with the global table enabled, of 66 combats
**not one** of the 19 already-tabled maps changed its tier, and Azure Nebula
Rescue — anchored but with no tables at all — gained 4 correct tiers.

**Exclusions (by entity name), each with a measured reason:**
- `Ground` / `Crewman` / `Bluegills` — ground scales 1.54x, not 4.4x.
- `Boss` / `Queen` — own scale (`..._Sibrian_Final_Boss` is 7.8M on *Advanced*,
  which would read as Elite on any class band).
- `Device_`, `Photonic_Fleet`, `Player_`, `Distress`, `Assistance_Beacon` —
  player-carried items and player-summoned allies (`Rom_Photonic_Fleet_Battleship`
  showed 88k on Elite).
- pets/drones/torpedoes/turrets/platforms etc. — not rank-scaled.

### Normal — EXPERIMENTAL

Added 2026-07-29 on far weaker evidence than the Advanced/Elite split, and marked
as such in the rules file. Only one Normal map exists in the log (Defense of
Starbase One), so every Normal figure comes from a single map and faction.

| class | Normal observed | lowest Advanced | threshold | basis |
|---|---|---|---|---|
| Battleship | 209,618–224,842 | 300,537 | 260,000 | measured, n=10 |
| Cruiser | 169,169–181,217 | 194,197 | 188,000 | measured, n=5 |
| Raider | 60,045–71,669 | 92,360 | 81,000 | measured, n=5 |
| Frigate | 18,372–26,061 | 81,493 | 46,000 | measured, n=2 |
| Dreadnought | — | — | 1,200,000 | extrapolated |
| Battlecruiser | — | — | 195,000 | extrapolated |
| Escort | — | — | 200,000 | extrapolated |

Extrapolated classes use 0.30x the Elite threshold — the median
`normal_threshold / threshold` of the four measured ones (0.293–0.325, with
Frigate an outlier at 0.175).

**Margins are thin, and that forced per-faction bands.** Advanced/Normal is only
~1.6x apart versus ~4.4x for Elite/Advanced, while the HP spread *within* a class
across factions reaches 2.3x — larger than the step we are trying to detect. On
Advanced, Battleship runs Elachi 609k → Borg 473k → Tzenkethi 396k → Nausicaan
346k → Gorn 336k → Klingon 336k → Tholian 301k. It is a gradient, not one outlier.

Confirmed live on 2026-07-30 by three real Normal runs. Rescue and Search (4 votes
Normal) and The Ninth Rule (3 Normal, 1 Advanced — the majority carried it) were
right; **Trouble Over Terrh read Advanced**, because Elachi ships on *Normal* are
as tough as other factions on *Advanced* (battleship 292,930 vs a lowest observed
Advanced of 300,537 elsewhere).

Fixed by giving Elachi their own bands, listed **before** the generic ones — the
class list is matched in order, so this needed no code change:

| class | all factions | excluding Elachi | Elachi alone |
|---|---|---|---|
| Escort | **overlap** | x1.96 | x2.06 |
| Battleship | x1.03 | x1.34 | x2.05 |
| Frigate | x1.05 | x1.14 | x1.38 |
| Cruiser | x1.07 | x1.07 | no pair yet |
| Raider | x1.24 | x1.24 | no pair yet |

Cruiser (x1.07) and Frigate (x1.14) stay tight even without Elachi — there the
squeeze comes from Tholians at the *bottom* (Cruiser_Web 194,197 against a
Klingon Normal of 181,217). Majority voting absorbs a single wrong vote, which is
what keeps them working; add per-faction bands the same way if a map misreads.

Verified on a frozen log (62 combats): adding the Elachi bands changed **exactly
one** verdict — the Trouble Over Terrh run above — with no side effects.

**Trap found while deriving this.** The first attempt showed Normal and Advanced
bands overlapping in every class, which would have killed the idea. The cause was
a single combat: at the 90 s separation used by the manual test, a Defense of
Starbase One run and a Rescue and Search run **merged into one combat**, so
Advanced-tier `Space_Klingon_*_Dsc_Mokai` ships were tagged Normal. `_Dsc_Mokai`
at 354,749 "Normal" against 336,087–351,796 Advanced was the tell: identical HP
for the identical entity. Excluding that combat, and medians resting on fewer
than 3 kills, made the bands separate cleanly. **When a cross-map comparison shows
an impossible overlap, suspect a merged combat before doubting the model.**

**Live check:** 30 Advanced, 20 Elite, 9 Normal. The three added Normals are the
2026-07-30 runs above — the first positive confirmations that the band works on
maps that do *not* pin Normal. No Advanced or Elite verdict changed.

**Limits.** Entities whose
name carries no class word cannot vote. The bands come from one player's ~week of
logs across 9 maps. If Cryptic ever retunes the multiplier, only these seven
numbers need revisiting — that is the point of keeping it global.

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

## [TFO] Red Alert: Borg — Space — DONE (single-difficulty Normal)
- **Anchor:** any-of `Mission_Space_Borg_Battleship_7_Of_10` (named boss) plus
  `Space_Borg_Battleship_Dse` / `Space_Borg_Cruiser_Dse`. The boss only appears in
  the fight's **later phase**, so the `_Dse` rank-and-file — present throughout —
  keeps an early split fragment recognizable.
- The `_Dse` suffix (Deep Space Encounter) marks the Red Alert roster and occurs
  in this map alone across the log, so no collision with the other Borg maps
  (Unwanted Guests, Resistance of Starbase One, Out of Control, Infected), which
  all key on different Borg entities.
- **Tier:** pinned `difficulty: "Normal"`.

| sample | tier | Battleship_7_Of_10 | Battleship_Dse | Cruiser_Dse | Frigate_Dse |
|---|---|---|---|---|---|
| 2026-07-30 00:53 | Normal | 3,107,477 | 211,201 | 187,210 | 79,156 |

## [TFO] Red Alert: Tholian — Space — DONE (single-difficulty Normal)
- **Anchor:** `Space_Tholian_Dreadnought_Red_Alert` (appears in this map only,
  across the whole log).
- **Tier:** pinned `difficulty: "Normal"` — Red Alerts are **single-difficulty**
  (Raman, from the in-game queue). All five Red Alert entries in the catalog carry
  the pin; only Tholian has an anchor so far.
- ⚠ **Collides with Azure Nebula Rescue.** A Red Alert run also contains
  `Space_Tholian_Cruiser_Web`, which is Azure's anchor, so both maps match and the
  run was misreported as "[TFO] Azure Nebula Rescue [Normal]". The pin resolves it
  deterministically: `detect` returns as soon as it reaches a map with a fixed
  difficulty, regardless of the unordered map iteration. Guarded by
  `red_alert_wins_over_azure_when_both_match`. Azure's own four runs are
  unaffected. If Azure ever needs to stop relying on this, it needs a narrower
  anchor than the generic `Cruiser_Web`.
- Note `Mission_Event_Tzenkethi_Red_Alert_*` belongs to **Tzenkethi Front**, not
  to Red Alert: Tzenkethi — reused assets, see that map's entry.

| sample | tier | Dreadnought_Red_Alert | Tholian_Battleship | Tholian_Cruiser |
|---|---|---|---|---|
| 2026-07-30 00:45 | Normal | 1,619,134 | 230,750 | 176,695 |

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

## [TFO] Defense of Starbase One — Space — DONE (single-difficulty Normal)
- Distinct Discovery-era Starbase One TFO (Discovery rep). Enemies: **Discovery
  Klingons** (`Space_Klingon_*_Dsc`). Not a collision with Resistance — they use
  *different* evacuation-ship entities (see below).
- **Anchor:** `Space_Federation_Cruiser_Dsc_Tfo_Evacuation_Ship` (Discovery variant).
- **Tier:** pinned `difficulty: "Normal"` — **single difficulty**: the in-game queue
  offers only Normal (Raman confirmed he can't select higher). The wiki's `N/A/E`
  for this TFO is **out of date**. First of the "Normal-only" maps; the schema
  already supports it (`difficulty` field, like Winter Invasion / Operation Wolf).

| sample | tier | Battleship_Dsc | Cruiser_Dsc | Raider_Dsc |
|---|---|---|---|---|
| 2026-07-26 17:41 | Normal | 226,313 | 176,588 | 63,403 |

## [TFO] Resistance of Starbase One — Space — DONE (anchor + any-of tier)
- Randomized enemy group (both samples were **Mirror Borg**). **No collision with
  "Defense of Starbase One":** confirmed the two use different evacuation ships —
  Resistance `..._Vtx_Tfo_Evacuation_Ship`, Defense `..._Dsc_Tfo_Evacuation_Ship`.
- **Anchor:** allied `Space_Federation_Cruiser_Vtx_Tfo_Evacuation_Ship`
  (faction-independent; deaths=0, present in both).
- **Tier:** `hull_any` on the dreadnought (deaths=1 ⇒ hull≈HP), bands Advanced 1.4M
  / Elite 4.0M. Listed the Borg faction variants (`_Mirror` observed; `_Control` /
  plain extrapolated from the established faction-independent dreadnought HP). If a
  non-Borg-variant run appears without a tier, add its dreadnought entity.

| sample | tier | faction | Dreadnought_Mirror | Battleship_Mirror |
|---|---|---|---|---|
| 2026-07-26 14:45 | Advanced | Mirror | 1,909,801 | 504,586 |
| 2026-07-26 16:26 | Elite | Mirror | 8,804,174 | 2,352,650 |

## [Patrol] To Die With Honor — Space — DONE (anchor + tier)
- Fixed-faction **Klingon** patrol (Forcas system). Distinct from the *TFO* "To
  Hell With Honor".
- **Anchor:** named mission dreadnoughts `Space_Klingon_Dreadnought_Mrek` /
  `Space_Klingon_Dreadnought_Ktinga_Lrell` — **any-of** (each is specific enough
  alone). Was `identifiers_all`, but a 45s-separation split produced a second
  fragment (13:41–13:43) carrying only Lrell + `..._Mrek_Placate_Nonplayers`,
  which the all-of anchor missed. Lrell is in every fragment, so any-of recovers it.
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

## [Patrol] The Ninth Rule — Space — DONE (any-of anchor + per-class any-of tier)
- **Enemies fully randomized** across ≥4 factions — Gorn, Nausicaan, Orion,
  Terran/Mirror-Discovery — and a single run can mix **two** of them
  (2026-07-25 Terran+Orion, 2026-07-29 Gorn+Orion, 2026-07-23 Gorn+Nausicaan).
- **Anchor:** any-of `Mission_Space_Federation_Science_Hofmann` **or**
  `Space_Federation_Cruiser_Galaxy` (both allied, `deaths = 0`). Neither alone is
  enough — see below.
- **Tier:** `hull_any`, banded **per ship class, not per faction** (measured; see
  the invariance table). Battleship Adv 300k / Elite 1.2M · Cruiser 240k / 1.0M ·
  Escort 180k / 800k · Frigate 95k / 400k, each listed for every observed faction
  variant. `Space_Gorn_Cruiser_Phalanx` is **deliberately excluded** — its
  Advanced median (196k–215k) sits too close to the 240k cruiser band.

### Why two anchors (the earlier "no reliable anchor" verdict was wrong)
The previous note dismissed `Space_Federation_Cruiser_Galaxy` as "a generic ally →
false positives". A full scan of the 2026-07-23 → 07-29 log (705k lines,
**62 combats**) shows that is **not** true in practice: the Galaxy cruiser appears
in **7 combats, and all 7 are this patrol**. It is a *non-combatant* mission ally,
so the risk was overstated.

Conversely `Mission_..._Hofmann` is 100% specific but only *sometimes* logged —
it is a non-combatant, so it only surfaces when it happens to trade damage. Each
anchor covers the other's gap:

| run | faction | tier | Hofmann | Galaxy | map confirmed by Raman |
|---|---|---|---|---|---|
| 2026-07-23 13:22 | Gorn + Nausicaan | Advanced | — | ✅ | ❔ inferred |
| 2026-07-25 00:24 | Terran-Mirror + Orion | Elite | ✅ | ✅ | ❔ inferred |
| 2026-07-26 11:42 | Gorn | Advanced | — | ✅ | ✅ |
| 2026-07-26 12:24 | Orion | Elite | ✅ | ✅ | ✅ |
| 2026-07-28 18:38 *(split fragment)* | Gorn | Advanced | ✅ | — | ✅ |
| 2026-07-28 18:40 | Gorn | Advanced | ✅ | ✅ | ✅ |
| 2026-07-28 22:18 | Nausicaan + Gorn | Advanced | — | ✅ | ❔ inferred |
| 2026-07-29 21:56 | Gorn + Orion | Elite | ✅ | ✅ | ✅ |

The 18:38 row is a real 45s-separation fragment (61s lull before 18:40:33) that
carries Hofmann but no Galaxy — exactly the split-fragility any-of exists for.

⚠ The three **inferred** rows were never named by Raman; they are grouped here
because they carry the Galaxy anchor and match the pattern (a lone non-dying
Federation ally + a randomized 1–2 faction enemy fleet, class HP in-band). The
confirmed 2026-07-29 run *does* mix two factions (Gorn + Orion), which is what
makes the Gorn+Nausicaan pairing credible — but it is not proof. If any of them
turns out to be a different patrol, the Galaxy anchor is contaminated and must be
dropped back to Hofmann-only (which costs 3 of 8 runs).

### HP is per class, not per faction (measured)
The 2026-07-29 Elite run settles this: it spawned Gorn **and** Orion together, and
their same-class ships land within 0.4–1.7% of each other. Same for Advanced Gorn
vs Nausicaan. So one band per class covers every faction, including the ones never
yet seen at a given tier.

| class | Advanced | Elite | ratio |
|---|---|---|---|
| Battleship | Gorn 326,971 / 343,967 / 349,282 · Nausicaan 345,674 | Gorn 1,519,796 · Orion 1,513,717 · Terran 1,532,854 | ~4.4× |
| Cruiser | Gorn 265,301 / 276,849 / 279,422 | Gorn 1,205,138 · Orion 1,192,155 / 1,200,843 · Terran 1,192,785 | ~4.4× |
| Escort | Nausicaan 203,391 / 210,443 / 214,071 / 223,368 | Terran 925,069 | ~4.4× |
| Frigate | Gorn 103,119 / 107,894 / 112,073 · Nausicaan 111,100 / 115,184 | Gorn 481,527 · Orion 469,080 / 477,575 / 489,814 · Terran 476,289 | ~4.3× |

Open: Normal-tier runs (patrols offer N/A/E) fall below the Advanced band and
report no tier — consistent with the other patrols, no table written.

---

## [TFO] Tzenkethi Front — Space — DONE (anchor + any-of tier)
- **Fixed faction** (Tzenkethi). Four runs in the log, splitting cleanly into two
  Advanced and two Elite.
- **Anchor:** the mission assault objects `Msn_Tzk_Tzenkethi_Assault_Ball` /
  `..._Assault_Tzenkethi_Starbase` (deaths = 0, present in all four runs).
- ⚠ **Deliberately not anchored** on `Mission_Event_Tzenkethi_Red_Alert_Tzenkethi_*`.
  Despite appearing in every Tzenkethi Front run, those are **reused Red Alert
  assets**, and the catalog holds a separate `Red Alert: Tzenkethi` map. No Red
  Alert run exists in this log to check against, so anchoring there could merge
  the two maps. They are still used as a *tier* signal — that is read only after
  the map already matched, so it cannot cross the maps.
- **Tier:** `hull_any` (any-of, so a split fragment carrying only one ship class
  still resolves). Bands: Dreadnought 1.3M / 6.0M · Cruiser_Var2 200k / 900k ·
  Cruiser_Var1 180k / 700k · Frigate 85k / 330k.

| sample | tier | Dreadnought | Cruiser_Var2 | Cruiser_Var1 | Frigate |
|---|---|---|---|---|---|
| 2026-07-27 19:32 | Advanced | 1,787,974 | 260,424 | 226,552 | 134,227 |
| 2026-07-29 15:59 | Advanced | 1,682,031 | 248,757 | 227,332 | 101,212 |
| 2026-07-27 18:35 | Elite | 8,955,752 | 1,207,045 | 1,017,930 | 448,302 |
| 2026-07-29 22:50 | Elite | 8,424,430 | 1,286,509 | 1,017,058 | 469,240 |

`Space_Tzenkethi_Cruiser_Var1` is the steadiest signal in any map measured so far
(226,552 / 227,332 Advanced; 1,017,930 / 1,017,058 Elite). `Space_Tzenkethi_Battleship`
was **left out**: many instances survive (deaths 2–4 of 6–15), so its median is
diluted by ships we merely damaged and it drifts (301k–389k Advanced).

Open: no `Red Alert: Tzenkethi` sample yet — one would confirm whether the
`Mission_Event_..._Red_Alert_*` ships really are shared, and let that map be
anchored without risk of colliding with this one.

## [TFO] Bug Hunt — Ground — DONE (two tier signals)
- **Anchor:** `Bluegills_Ground_Boss`. Fixed faction (Bluegills / Undine).
- **Tier — two signals (user chose "both"):**
  1. **Elite marker** (`death_counts` Elite): `Bluegills_Ground_Ens_Noautospawn_Queenfodder`
     present ⇒ Elite (absent in the Advanced sample, present in Elite).
  2. **Hull bands** (`hull_counts`): Boss (Adv 200k / Elite 380k) + Cdr (Adv 5k /
     Elite 12k), double-gated. Labels Advanced too.
- **Ground scales weakly** (~1.6-2x, vs ~4x in space) → thin hull margin; that's
  why the Queenfodder marker backs it up. Both signals agreed on the samples.
- **Latent edge** (revisit with more samples): the hull phase runs after the death
  marker and could in principle downgrade a marker-Elite run to Advanced if that
  run's boss hull were Advanced-level — but real Elite runs have Elite-level hull
  (boss ~452k > the 380k band), so it doesn't happen in practice.

| sample | tier | Boss | Cdr | Queenfodder present? |
|---|---|---|---|---|
| 2026-07-26 14:18 | Advanced | 286,343 | 7,803 | no |
| 2026-07-26 12:58 | Elite | 451,781 | 16,077 | yes (13 deaths) |

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
