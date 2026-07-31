# Detection Samples & Decisions

Empirical record behind `src/analyzer/detection_rules.json`: the real-log combats
each map's **anchor** (identifier) and **tier thresholds** were derived from, plus
open questions. Append here as new samples arrive so we can compare and refine.

Hull numbers are **median hull damage suffered** per entity (what `detect` uses).
Rule of thumb: hull thresholds sit between the Advanced and Elite medians, with the
`HULL_VARIANCE = 0.20` margin (a tier matches when `median > threshold * 0.8`).

**Anchoring rules learned:**
- Never anchor on `Device_*` — player-carried devices, random per run.
- **"Too generic" is a measurement, not an intuition.** Count how many distinct
  combats an entity appears in across the whole log before rejecting it. This has
  now been wrong twice in the same direction: `Space_Federation_Cruiser_Galaxy`
  (written off as ubiquitous, actually 7 combats, all one patrol) and the Pahvo
  rank-and-file (written off as shared Terran troops, actually one fight).
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
- **Display names are not entity names.** Bug Hunt's friendly NPC shows in the log
  as "Lt. VanDerveer" but its entity is `Msn_Dlt_Bluegill_Hunt_Ground_Demolitions_Expert`;
  Iuppiter Iratus's "Data Thief" is `Space_Federation_Frigate_Mirror`. Anchors key
  on the entity name, so always grep the `C[id name]` field, never the label.
- **Presence covers source and target, not `indirect_source`** — deliberately.
  Measured: 138 entities appear *only* in that column and every one is a player
  effect (`Warp_Plasma_*`, `Gravity_Well*`, `Stationmod_*` consoles, torpedo
  rifts), never a map ally. Including it would add noise, not anchors. Allies that
  act through it, like VanDerveer, are still seen because they also take damage.
- When the **tier signal** entity varies by faction but its HP does not, list every
  variant under `hull_any` (any-of) with the same band.
- **Tier only on entities that die** (`deaths > 0`): their median hull damage ≈ HP.
  A `deaths = 0` entity's hull is just the damage we *happened to deal* to it
  (player-dependent), not its HP — fine as an anchor, useless as a tier threshold.
- **A sweep for friendly anchors was done 2026-07-31** across all 27 maps with
  samples in the log (`/tmp` scratch analysis, method: group every combat, keep
  entities that never die and appear in *every* run of that map, then check the
  entity occurs in no other map). It found usable allies for only three maps —
  most already anchor on a mission object, and the rest field nothing but shared
  faction ships. Results:
  - Trouble Over Terrh gains `Space_Romulan_Colony_Escort` (5/5 runs) beside its
    Elachi enemy anchor.
  - Bug Hunt gains `Msn_Dlt_Bluegill_Hunt_Ground_Demolitions_Expert` (2/2 runs),
    which is present far longer in each fight than the boss it sits next to.
  - Battle at the Binary Stars gains `Space_Federation_Cruiser_Dsc_Shenzhou`.
  - Azure Nebula Rescue has **no** ally to offer: only the generic
    `Space_Tholian_Cruiser_Web` appears in all 5 runs. Worth noting the Lleiset,
    once dropped from Azure for being shared with Terrh, turns out to occur in
    Terrh alone — but it does not appear in Azure's runs at all, so it is no help
    there either.
- **Look for the friendly ships first.** Enemy rosters are shared between maps far
  more often than the allies escorting them, and allies also tend to be present
  from the first minute, where a boss or a late objective is not. Iuppiter Iratus
  is the clearest case: its own `*_S25_Tfo` ships appear only in the last three
  minutes, while the allied Voyager covers nearly the whole fight.
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

### Ground maps — a global rank table does NOT work

Ground scales ~1.54x between tiers rather than ~4.4x, which is why ground entities
are excluded from the global table. The open question was whether a table keyed on
**rank** (Ens/Lt/Cdr/Capt) could work anyway.

**With three Advanced samples and one Elite it looked promising** — Cdr showed a
1.95x margin, Capt 1.53x — and this document recommended building one on the
senior ranks. **A second Elite map killed that.** Devil's Heart (Elite) has
weaker troops than Bug Hunt has on *Advanced*:

| rank | Advanced across 4 maps | Elite across 2 maps | verdict |
|---|---|---|---|
| Capt | 13,842–21,807 | 22,087–33,332 | x1.01 — noise |
| Cdr | 4,536–8,236 | 7,095–16,077 | **overlap** |
| Lt | 2,514–3,890 | 4,346–5,114 | x1.12 |
| Ens | 1,873–2,470 | 2,740–3,756 | x1.11 |

Devil's Heart on **Elite** fields a Cdr of 7,095 against Bug Hunt's 7,803 on
**Advanced** — 9% *weaker* one tier up. No threshold can separate those.

**Conclusion: ground HP is a property of the map, not of the rank.** Unlike space,
where one table covers every map, ground tiers can only ever come from per-map
tables — which needs an Advanced+Elite pair for each map individually.

### Update (2026-07-31): the 1.54x ground multiplier is confirmed

A Devil's Heart Advanced run arrived, giving a **second** ground map with both
tiers. Its Elite/Advanced ratio across nine entities is a median of **1.53x**,
against Bug Hunt's **1.54x** — so the multiplier is a property of ground content,
not of Bug Hunt. That was the one thing needed to make extrapolation from a single
sample defensible.

| entity | Advanced | Elite | ratio |
|---|---|---|---|
| Miniboss | 13,693 | 22,087 | 1.61x |
| Aetherian Hacker | 5,202 | 8,396 | 1.61x |
| Aetherian Commander | 4,780 | 7,095 | 1.48x |
| Borg Cdr (Anchor) | 4,749 | 7,021 | 1.48x |
| Borg Ens (Range) | 2,169 | 3,316 | 1.53x |

Four more pairs followed the same day. Five independent measurements:

| map | median ratio | spread within the map |
|---|---|---|
| Devil's Heart | 1.53x | 1.34–2.18x |
| Bug Hunt | 1.54x | 1.33–2.06x |
| Pahvo Dissension | 1.58x | 1.42–2.04x |
| Into the Hive | 1.67x | 1.49–1.74x |
| Brotherhood of the Sword | **1.74x** | 1.39–2.09x |

**The between-map spread grew with every sample: 3% at three maps, 9% at four,
14% at five (1.53–1.74x).** A ground multiplier exists, but it is an
approximation per map rather than the near-constant the space ratio turned out to
be — and each new map has so far widened it rather than settling it.

**Normal→Advanced is a bigger step than Advanced→Elite.** Khitomer in Stasis is
the only ground map with that pair (its queue has no Elite) and its median ratio
is **2.11x**, against 1.53–1.74x for Elite/Advanced. Worth knowing if a Normal
band is ever needed elsewhere: there is more room there, not less.

One entity is a striking exception — `Mission_Borgraid03_Borg_Power_Node` sits at
**1.08x**, essentially unchanged between tiers. Mission objects apparently have
fixed HP while the troops around them scale, so they make poor tier signals even
though they are excellent anchors. It is excluded from the bands.

Consequence: extrapolating a threshold from a single sample is *plausible* but
would carry ~15% error against tiers only ~60% apart, leaving little room. The
space table works because its 4.4x step dwarfs that kind of error; here it does
not. All five maps carry measured bands, so none relies on the multiplier — and
on this evidence, waiting for a real pair remains the better call.

Note this does **not** revive the rank table: 1.53x describes *the same entity*
across tiers, whereas the rank table compared *different maps' entities* of the
same rank, which is where the overlap came from.

### Normal bands on ground maps (extrapolated, 2026-07-31)

**Every ground TFO offers Normal** (Raman), so any map tiered only for
Advanced/Elite can receive a Normal run. Such a run does *not* misreport — its HP
falls well short of the Advanced threshold, so nothing matches and the combat
simply shows no tier — but that is incomplete rather than correct.

**The extrapolation was then checked against reality.** A measured Bug Hunt Normal
run arrived the same day: its boss came in at 241,116 against a predicted
threshold of 135,708 (firing above 108,566), and its Cdr at 3,523 against 3,698
(firing above 2,958) — both comfortably detected. Bug Hunt now carries the
measured band; the rest keep the derived one.

That run also gives a second Normal→Advanced ratio: **2.07x**, against Khitomer's
2.11x. Unlike the Elite/Advanced multiplier, which widened with every sample, this
one is holding.

⚠ But note *which* entities carry it. Bug Hunt's boss scales only **1.19x**
Normal→Advanced and its Ensign 1.15x, while Capt/Cdr/Lt sit at 2.07–2.24x. Bosses
and rank-and-file both compress at the bottom of the tier range — the same effect
seen in Khitomer's power node at 1.08x. Mid ranks are the reliable tier signal.

The remaining five tiered ground maps have a `Normal` band at **Advanced ÷ 2.11**,
the only measured Normal→Advanced ratio (Khitomer in Stasis). The margin is
generous: a threshold at `Advanced/2.11` still fires for any real ratio between
about 1.0x and 2.6x, and beyond that the failure mode is a missing tier, never a
wrong one. Verified: every existing ground verdict is unchanged.

Replace each with a measured band as Normal runs of those maps arrive.

**Undine Infiltration — DONE (2026-07-31 20:23 Elite closed the pair).** It had
been deliberately left with no bands at all while only its Advanced was sampled,
because an Advanced-only band would have made a future Elite read as Advanced.
The Elite run arrived and the map now has a measured `hull_any` pair.

| entity | Advanced | Elite | ratio |
|---|---|---|---|
| `Mission_Ground_Undine_Capt_Range_Psi_Infiltration_Boss` | 30,710 | 71,916 | 2.34x |
| `Ground_Undine_Capt_Range_Psi` | 19,476 | 31,900 | 1.64x |
| `Ground_Undine_Cdr_Range_Psi` | 7,134 | 11,207 | 1.57x |
| `Ground_Undine_Lt_Range_Psi` | 3,513 | 5,695 | 1.62x |
| `Ground_Undine_Lt_Mixed` | 3,890 | 5,734 | 1.47x |
| ~~`Ground_Undine_Cdr_Melee`~~ | 8,236 | 9,868 | **1.20x — excluded** |

Median 1.57x, inside the 1.53–1.74x ground range. `Cdr_Melee` is left out on
purpose: at 1.20x its Advanced figure (8,236) sits *above* the Elite firing
threshold (9,868 x 0.8 = 7,894), so it would light up Elite on an Advanced run.
Every other entity clears its separation.

It carries a derived Normal band (Advanced ÷ 2.11), like the other tiered ground
maps — but getting there exposed an engine bug worth recording.

### The surviving-entity bug (found here, fixed 2026-07-31)

The derived Normal band was written and immediately made the *opening fragment* of
the Advanced session read as Normal. That fragment (2026-07-30 12:35:04–12:36:29,
47 lines, cut off by a 70 s gap) contains exactly one entity — the boss, with
**deaths = 0** and a median of 14,192. That figure is not the boss's HP; it is
merely the damage dealt to it before the fragment ended, and it cleared the
derived threshold of 14,555 x 0.8 = 11,644.

The gap was wider than this map: `hull_any_match` and `hull_damage_match` never
checked `deaths`, while the global ship-class table has always skipped
`deaths == 0` explicitly, for exactly this reason ("for the others the hull figure
is damage we happened to deal, not the entity's HP"). Any per-map tier could
therefore be decided by an entity that merely got hurt.

Both matchers now carry the same guard. What the fix was measured against before
being made, rather than argued about:

- **On the real log it changes nothing** — 96 of 98 combats, verdict for verdict
  identical to before. Resistance of Starbase One was the one regression
  candidate (its `Space_Borg_Dreadnought` entries never die in the log) and it
  survives, because `hull_any` only needs one listed entity and its others do die.
- **11 unit tests went red**, all for the same reason: the `hull_critter` fixture
  helper left `deaths` at 0. Since a median hull figure is meaningless for an
  entity that lived, that helper was simply wrong; it now sets `deaths = 1` and
  the separate `dead_hull_critter` is folded into it.
- **The per-map tests still stand on their own.** Because the global table has the
  last word (it overwrites the per-map verdict), a fixture that starts voting in
  it could make a test pass for the wrong reason. Re-running the suite with the
  global override disabled, every per-map tier test still passed; only the three
  `global_tier_*` tests and the two that deliberately rely on it failed.

Guarded by `an_entity_that_survived_does_not_decide_the_tier`, which was itself
checked to fail without the fix (it reports `Some(Advanced)` instead of `None`).
The first version of that test passed either way and was rebuilt — with the band
removed, 14,192 no longer reached any threshold, so it proved nothing.

### Decision (2026-07-31): wait for measured pairs

Ground tiers will come from **per-map tables built from an Advanced+Elite pair of
that same map**. Two shortcuts were considered and declined:

- **Pinning a difficulty** — only valid where the queue offers one tier, as with
  Battle of Korfez. Devil's Heart is not such a map.
- **Extrapolating from the 1.54x ground multiplier**, i.e. taking a map's single
  sample, predicting the other tier and putting the threshold between them. This
  does work on Bug Hunt — a threshold derived from its Advanced alone correctly
  separates its real Elite, for all five ranks — but that multiplier *comes from*
  Bug Hunt, so the check is circular. Devil's Heart having weaker Elite troops
  than Bug Hunt's Advanced is exactly the kind of spread that would break it.

Until a pair exists, these maps show no tier. That is the honest output: a wrong
tier is worse than none, and unlike the space table there is no independent
evidence to lean on.

**What is missing, per map:**

| map | have | needs |
|---|---|---|
| Bug Hunt | Normal + Advanced + Elite | — (all three measured) |
| Devil's Heart | Advanced + Elite | — (has tables) |
| Pahvo Dissension | Advanced + Elite | — (has tables) |
| Into the Hive | Advanced + Elite | — (has tables) |
| Brotherhood of the Sword | Advanced + Elite | — (has tables) |
| Khitomer in Stasis | Normal + Advanced | — (has tables; the queue offers no Elite) |
| Undine Infiltration | Advanced + Elite | — (has tables) |
| Infected: Manus | — (anchor only) | Normal + Advanced |
| Cure Applied | — (anchor only) | Normal + Advanced |

**Which pair a ground map needs depends on the queue.** Three of them top out
below Elite — Khitomer in Stasis, Infected: Manus and Cure Applied all offer
Normal and Advanced only (confirmed by Raman, who plays them; the wiki is
unreachable, 403). For those the pair to measure is **Normal + Advanced**, and
waiting for an Elite sample would mean waiting forever. Their expected spread is
therefore the ~2.07-2.11x Normal→Advanced multiplier, not the ~4.4x space one.

A second pair would also settle whether the 1.54x multiplier is a property of
ground content or just of Bug Hunt — which is the one thing that would make the
extrapolation shortcut usable after all.

**Lesson worth keeping:** the earlier recommendation rested on a single Elite map.
One map cannot show whether a spread is a property of the tier or of that map; it
takes two to tell those apart. The same trap was avoided in space only because the
Elite samples came from four different maps early on.

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

## [TFO] Devil's Heart — Ground — DONE (anchor only, no tier)
- **Anchor:** the five entities carrying `Devils_Heart` in their name — the Borg
  miniboss, the Aetherian hacker and boss, the boss's explosive drones and the
  miniboss turret. Between them the miniboss (00:18–00:20) and the hacker
  (00:20–00:22) cover the whole fight.
- ⚠ The `Vtx_` prefix is **not** map-specific: `Space_Federation_Cruiser_Vtx_Tfo_Evacuation_Ship`
  anchors Resistance of Starbase One. It marks the episode, not the map — only
  `Devils_Heart` pins this one.
- **Tier:** none. Ground maps are excluded from the global table, and this run is
  what proved a rank-based ground table cannot work (see above).

| sample | tier | Miniboss | Aetherian_Cdr | Aetherian_Lt | Aetherian_Ens |
|---|---|---|---|---|---|
| 2026-07-31 00:18 | Elite | 22,087 | 7,095 | 5,114 | 3,756 |

## [TFO] Iuppiter Iratus — Space — DONE (anchor + global tier)
- **Anchor:** `Space_Holo_Projector_Jupiter_Tfo` (Iuppiter is Latin for Jupiter,
  so the name pins the map outright), the allied `Space_Federation_Voyager`, and
  the three `*_S25_Tfo` ships whose suffix marks them as this TFO's own set.
- **The friendly ship carries the coverage.** The `*_S25_Tfo` set only appears in
  the last three minutes (23:50–23:52 of 23:43–23:52); Voyager runs 23:43 and
  23:45–23:51, nearly the whole fight. The fight has no internal gap over 30 s so
  it cannot split today, but anchoring on the ally means it would survive one.
- ⚠ `Msn_Dyz_Az_Space_Federation_Science` looked like a good mission-object
  anchor and was **rejected by counting**: it also appears in the Breach run an
  hour earlier. A reminder that "looks map-specific" is not a measurement.
- **Rejected:** the `Space_Federation_*_Mirror` rank-and-file, which cover the
  whole fight. Note these are *not* the same entities as The Ninth Rule's
  `Space_Federation_*_Dsc_Mirror` — Terran Discovery ships are a separate set —
  but plain Mirror ships plausibly appear in other Terran content and there is
  none in this log to count against.
- ⚠ **Raman's hand-written rule matched "Data Thief"**, which is the *display*
  name of `Space_Federation_Frigate_Mirror`. Detection cannot use display names,
  so the rule gave no hint as to what to anchor on — exactly the caveat recorded
  for the archived rules.
- **Tier:** from the global ship-class table (battleship 453,593 against an 850k
  band ⇒ Advanced).

| sample | tier | Mirror_Enterprise | Battleship_Mirror | Cruiser_Mirror_2 | Frigate_Mirror |
|---|---|---|---|---|---|
| 2026-07-30 23:43 | Advanced | 4,626,261 | 453,593 | 387,758 | 173,633 |

## [TFO] Breach — Space — DONE (anchor + global tier)
- **Anchor:** the Dyson breach emplacements (`Mission_Dys_Breach_Hard_Point`,
  `Msn_Dys_Event_Breach_{Beam,Cannon}_Turret`, `..._Beam_Turret_Boss`) and the Voth
  objectives (`Msn_Space_Voth_Boss_Power_Core`, `Msn_Space_Voth_Shield_Core_3`).
- **Why both halves:** the emplacements only run 22:57–22:59 and the Voth cores
  23:04–23:06, of a fight spanning 22:55:57–23:06:56. Neither set alone covers it.
  The fight does hold together at 60 s, but the largest internal gap is **57 s** —
  three seconds of margin, so a shorter separation would split it and each half
  needs its own anchor.
- **Rejected:** plain `Space_Voth_*` ships. Voth appear across Dyson Sphere
  content, and there is no second Voth map in this log to count against.
- **Tier:** from the global ship-class table (cruiser 446,733 against a 640k band
  ⇒ Advanced). No tables of its own.

| sample | tier | Voth_Power_Core | Dreadnaught | Cruiser | Frigate |
|---|---|---|---|---|---|
| 2026-07-30 22:55 | Advanced | 6,707,080 | 3,081,268 | 446,733 | 181,219 |

## [TFO] Brotherhood of the Sword — Ground — DONE (anchor only, no tier)
- **Anchor:** the seven `Msn_Ico_Qonos_Ground*` allies — Honor Guards (Ferasan,
  Klingon, Nausicaan), MACO (Andorian, Tellarite, Vulcan) and an Orion ally. All
  seven are listed because the escort's species look randomised per run, so a
  different set may turn up next time.
- They only appear in two of the fight's six minutes (22:41–22:42 of 22:40–22:45),
  which is safe here: the fight has **no gap longer than 30 s**, so it cannot
  split at any sensible separation.
- **Rejected:** the `Ground_Herald_*` troops cover the whole fight and occur
  nowhere else in the log, but Heralds appear across the Iconian War content and
  there is no other Herald ground map here to check against — the counting rule
  cannot clear them. If a split ever loses the early part, they are the fallback.
- **Tier:** none — ground maps are excluded from the global table.

| sample | tier | Capt_Gravity | Cdr_Melee | Lieutenant | Ensign |
|---|---|---|---|---|---|
| 2026-07-30 22:40 | Advanced | 24,336 | 5,030 | 2,159 | 1,657 |

Fifth Advanced ground sample. Still no Elite pair from a second ground map, so the
rank table described above remains unbuilt.

## [TFO] Peril Over Pahvo — Space — DONE (anchor only, no tier)
- **Anchor:** `Msn_Dsc_Pahvo_Defense_Queue_System_Upgradeable_Satellite` (runs the
  whole fight) and `..._Planetary_Shield` (second half).
- ⚠ **Forced a re-anchor of Rescue and Search.** This map fields the *same*
  `Space_Klingon_*_Dsc_Mokai` ships that Rescue and Search was anchored on, so
  both matched and the winner came down to hash-map iteration order — the run was
  reported as Rescue and Search. Neither map pins a difficulty, so the early
  return that settles the Red Alert / Azure clash does not apply here.
- **Rescue and Search now anchors on the rescued Lukari ships**
  (`Msn_Space_Lukari_Science_Vessel` / `_Frigate` / `_Escort`). Measured: all 14 of
  its runs in the log carry them, in every minute of each fight, and no other map
  does. Its Mokai hull bands are unchanged — they still resolve the tier, they
  just no longer identify the map. Guarded by
  `pahvo_and_rescue_are_told_apart_by_their_mission_objects`.
- One 533-line fragment (2026-07-30 22:02) holds nothing but Mokai ships, with no
  mission object of either map, and is now correctly left unidentified rather than
  mislabelled Rescue and Search.

| sample | tier | Battleship_Mokai | Cruiser_Mokai | Raider_Mokai |
|---|---|---|---|---|
| 2026-07-30 22:05 | Advanced | 472,023 | 376,972 | 126,811 |

## [TFO] Battle of Korfez — Space — DONE (single-difficulty Elite)
- **Anchor:** `Dlt_Vaadwaur_Stf_System_Dreadnought_Boss` (27.8M hull — the STF's
  final dreadnought).
- **Tier:** pinned `difficulty: "Elite"` — Elite-only in the queue (Raman). First
  map pinned to Elite; the others pinned so far are all Normal.
- ⚠ Same shape as Pahvo Dissension: the boss only shows up at the **end**
  (17:29–17:30 of 17:22–17:30). Left as the sole anchor because the fight holds
  together comfortably — largest internal gap 32 s against a 60 s separation — and
  because `Space_Vaadwaur_*` are ordinary faction ships that Delta Quadrant
  patrols may also field. There is no such patrol in the log to check against, so
  the counting rule cannot clear them here; if a split ever loses the early part,
  add `Space_Vaadwaur_Cruiser` / `_Frigate` and watch for collisions.
- The global table would have reached Elite on its own (cruiser 2.27M against a
  1.0M band, dreadnought 27.8M against 4.0M); the pin just makes it certain.

| sample | tier | Boss | Cruiser | Battleship | Frigate |
|---|---|---|---|---|---|
| 2026-07-30 17:22 | Elite | 27,822,587 | 2,274,614 | 2,188,095 | 665,022 |

## [TFO] Khitomer in Stasis — Ground — DONE (anchor only, no tier)
- **Anchor:** `Mission_Borgraid03_Borg_Power_Node` plus the `Ground_Borg_*_Raidisode_*`
  troops and `Ground_Borg_Ens_Melee_Wolf_359`.
- ⚠ **Shares its rank-and-file with Into the Hive.** `Ground_Borg_Capt_Melee`,
  `_Cdr_Melee`, `_Ens_Melee`, `_Ens_Melee_Spawned`, `_Lt_Range`, `Borg_Anchor` and
  `_Neo_Melee_Ambush_Pet` all occur in **both** maps, so none of them can anchor
  either. The `Raidisode` variants and the Borgraid03 power node occur in this map
  alone — the counting rule earns its keep here, in the opposite direction to
  Pahvo: this time the generic-looking troops really were shared.
- **Why so many anchors:** no single one covers the fight. The Infected Romulan
  troops run 16:32–16:38, the power node 16:37–16:40, and the Wolf 359 drones
  16:39–16:42. `Infected_{Fed,Klingon,Romulan}` look like randomised variants, so
  all three are listed. Together they leave no gap. The fight held together at 60 s
  (largest internal gap 57 s), but only barely.
- `Ground_Borg_Ens_Melee_Wolf_359` is presumably a reused asset; Battle of Wolf 359
  is a *space* queue, so a ground entity of that name should not collide with it.
- **Tier:** none — ground maps are excluded from the global table.

| sample | tier | Capt_Melee | Power_Node | Cdr_Melee | Lt_Range | Ens_Melee |
|---|---|---|---|---|---|---|
| 2026-07-30 16:32 | Advanced | 14,047 | 6,401 | 5,325 | 3,795 | 3,181 |

## [TFO] Pahvo Dissension — Ground — DONE (anchor only, no tier)
- **Anchor:** any-of the two named bosses
  `Ground_Federation_Capt_Range_{Eng,Tac}_Mirror_Dsc_Pahvo_Boss` **plus** the
  rank-and-file `Ground_Federation_Ens_Range_{Tac,Eng,Sci}_Mirror_Dsc`.
- The bosses alone were not enough: they appear only in the **last minute** of a
  ~10 minute fight (16:15–16:16 of 16:06–16:16). That happens to be safe at a 60 s
  separation — the largest internal gap is 52 s, so this fight does not split —
  but a shorter separation would have left the early part unrecognized. The Ens
  ranks run the whole fight, so they close that hole.
- ⚠ **The rank-and-file were first dismissed as "generic Terran ground troops
  that would collide with other maps". That was an assumption, not a measurement,
  and measuring contradicted it:** every `Ground_Federation_*_Mirror_Dsc` entity
  occurs in this one fight and nowhere else in the whole log. The residual risk is
  that another Mirror-Discovery ground map (none in the catalog is anchored yet)
  fields the same troops — if one ever misreads as Pahvo Dissension, drop the Ens
  entries and accept the boss-only coverage.
- **Rejected anchors:** `Device_Pahvo_Tfo_Crystal_Tether` covers the whole fight
  and looks map-specific, but it is a `Device_*` (see the anchoring rules), and
  one sample is not enough to prove it is mission-issued rather than carried.
  Same for `Ground_Universal_Kit_Pahvo_Crystal_Prism_Module_Summon`, a player kit.
- **Tier:** none — ground maps are excluded from the global table.

| sample | tier | Boss_Eng | Boss_Tac | Cdr | Lt | Ens |
|---|---|---|---|---|---|---|
| 2026-07-30 16:06 | Advanced | 35,027 | 27,182 | 4,536 | 2,514 | 2,047 |

This is a **fourth Advanced ground sample**. It widens the Cdr spread across maps
to 1.81x (4,536–8,236), against a 1.95x margin to the lowest known Elite — so the
"senior ranks only" plan still holds, but with less room than three maps
suggested. Ens stays useless (1,873–2,470 across four maps).

## [TFO] Battle at the Binary Stars — Space — DONE (single-difficulty Normal)
- Was already detectable via `Space_Klingon_Dreadnought_Dsc_Sarcophagus`; this run
  only added the tier and fixed the name.
- **Tier:** pinned `difficulty: "Normal"` — Normal-only in the queue (Raman).
- ✅ **First independent check of the global Normal band.** Before pinning, the
  global ship-class table tiered this run Normal entirely on its own, on a map with
  no tier tables and no hand-tuned thresholds. Every voting entity agreed:
  Battleship_Dsc 216,081 (band 260k), Cruiser_Dsc 170,195 (188k), Escort_Dsc
  132,630 (145k), Raider_Dsc 63,031 (83k). The pin now makes it authoritative
  rather than inferred.
- **Renamed** "Battle At The Binary Stars" → "Battle at the Binary Stars" (wiki
  capitalisation). Note `Battle of the Binary Stars` on the wiki redirects to the
  *story mission*, not this TFO — they are different content.

| sample | tier | Battleship_Dsc | Cruiser_Dsc | Escort_Dsc | Raider_Dsc |
|---|---|---|---|---|---|
| 2026-07-30 13:57 | Normal | 216,081 | 170,195 | 132,630 | 63,031 |

## Red Alerts — all five DONE (single-difficulty Normal)

Raman confirmed from the in-game queue that **every Red Alert is Normal-only**, so
all five carry `difficulty: "Normal"` and never reach the tier tables. Sampled
2026-07-30 in one sitting.

| map | anchor(s) |
|---|---|
| Borg | `Mission_Space_Borg_Battleship_7_Of_10`, `Space_Borg_Battleship_Dse`, `Space_Borg_Cruiser_Dse` |
| Tholian | `Space_Tholian_Dreadnought_Red_Alert` |
| Tzenkethi | `Msn_Event_Tzenkethi_Alert_System_Satellite`, `Mission_Tzenkethi_Protomatter_Torpedo_Entity` |
| Elachi | `Mission_Space_Elachi_Frigate` |
| Na'kuhl | `Event_Nakuhl_Space_Convoy_Transport`, `Space_Federation_Frigate_Nakuhl_Red_Alert` |

**Pattern worth reusing:** a Red Alert shares its rank-and-file ships with the
regular map of the same faction, and is distinguished only by a *mission object* —
an escorted convoy, an alert satellite, a named boss. Three of the five collided
with an existing map (Tholian↔Azure Nebula Rescue, Tzenkethi↔Tzenkethi Front,
Elachi↔Trouble Over Terrh). Never anchor a Red Alert on its faction's ordinary
ships, and when adding any faction map, check whether that faction has a Red Alert
first.

The same faction is also **weaker on a Red Alert** than on its regular map (Elachi
battleship 217,336 vs 292,930 on Terrh Normal; the Tzenkethi dreadnought 1,052,092
vs 1,682,031 on Tzenkethi Front Advanced), so tier bands never transfer between a
Red Alert and its parent map.

## [TFO] Red Alert: Na'kuhl — Space — DONE (single-difficulty Normal)
- **Anchor:** any-of `Event_Nakuhl_Space_Convoy_Transport` (the escorted convoy,
  present the whole fight, deaths = 0) and
  `Space_Federation_Frigate_Nakuhl_Red_Alert` (only in the first half).
- No other Na'kuhl map exists in the catalog, so there is nothing to collide with.
- **Tier:** pinned `difficulty: "Normal"`.

| sample | tier | Dreadnought | Battleship | Cruiser | Frigate |
|---|---|---|---|---|---|
| 2026-07-30 01:33 | Normal | 977,294 | 224,627 | 175,553 | 74,208 |

## [TFO] Red Alert: Elachi — Space — DONE (single-difficulty Normal)
- **Anchor:** `Mission_Space_Elachi_Frigate` — present for the whole fight and in
  this map alone.
- ⚠ **One `Mission_` prefix away from Trouble Over Terrh's anchor**
  (`Space_Elachi_Frigate`). They are different entities and identifier lookup is
  exact — a hash-map key, not a substring — so they never cross. Guarded by
  `elachi_maps_are_told_apart_by_the_mission_prefix`, which would fail if the
  lookup ever loosened. When grepping a log for one of these, anchor the pattern
  (`C\[\d+ Space_Elachi_Frigate\]`): a bare `Space_Elachi_Frigate` also matches
  the `Mission_` one and makes a non-existent collision look real.
- The two maps **do** share `Space_Elachi_Battleship_V1/V2` and
  `Space_Elachi_Escort`, so neither is anchored on those.
- **Tier:** pinned `difficulty: "Normal"`.

| sample | tier | Battleship_V1 | Battleship_V2 | Escort | Mission_Frigate |
|---|---|---|---|---|---|
| 2026-07-30 01:25 | Normal | 217,336 | 213,382 | 211,767 | 62,531 |

Elachi on a Red Alert are weaker than on Terrh Normal (battleship 217,336 vs
292,930), so again the tier bands do not transfer between maps.

## [TFO] Red Alert: Tzenkethi — Space — DONE (single-difficulty Normal)
- **Anchor:** any-of `Msn_Event_Tzenkethi_Alert_System_Satellite` and
  `Mission_Tzenkethi_Protomatter_Torpedo_Entity`. Both run the whole fight
  (01:15–01:17) and occur in this map alone, so a split fragment still resolves.
- ✅ **Confirms the reused-asset call on Tzenkethi Front.** This run fields
  `Mission_Event_Tzenkethi_Red_Alert_*` — the very ships Tzenkethi Front was
  deliberately *not* anchored on. Had it been, this Red Alert would have been
  reported as Tzenkethi Front. Each map is anchored on something the other never
  fields, so the shared ships cannot merge them; guarded by
  `tzenkethi_front_and_red_alert_stay_apart`. Tzenkethi Front's four runs are
  unaffected.
- ⚠ `Stationmod_Universal_Priors_Satellite` also appears here and looks
  map-specific, but it occurs in 12 unrelated combats — it is a **player console**.
  `Stationmod` is already in the global-tier exclusions for the same reason.
- **Tier:** pinned `difficulty: "Normal"`.

| sample | tier | RedAlert_Dreadnought | RedAlert_Battleship | Cruiser_Var1 |
|---|---|---|---|---|
| 2026-07-30 01:15 | Normal | 1,052,092 | 191,876 | 109,986 |

Note the shared dreadnought is 1,052,092 here against 1,682,031 on Tzenkethi Front
Advanced — the same entity is weaker on a Red Alert, so its tier bands are not
transferable between the two maps.

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

## [TFO] Counterpoint — Space — DONE (anchor from day one, confirmed by sample)
- **Anchor:** `Mission_Starbase_Mirror_Ds9_Mu_Queue`, present ~3,100 lines in the
  sample. `Mission_Starbase_Ds9_Mu_Queue` (no `Mirror`) is an equally unique
  second candidate if the first ever turns out to be skippable; not needed so far.
- **Tier:** none of its own — the **global ship-class table alone** decided it.
- This map is the first blind confirmation of that table. Both the anchor and the
  category came from the very first detection commit (ported, never sampled), and
  the tier was never measured for Mirror ships at all. The 2026-07-31 14:51 run
  came back `[TFO] Counterpoint (Space) [Advanced]` with no rule change: 13 voting
  entities, **13 of 13 Advanced, zero dissent**.
- Worth noting *which* entities voted. Nothing here is Mirror-specific to the
  table — Dreadnought, Battleship, Cruiser, Escort and Frigate bands all caught
  their Mirror counterparts on the first try, e.g. `Space_Federation_Cruiser_Mirror`
  at 379,533 inside a band built with no Mirror sample in it. That is the
  per-class (not per-faction) claim holding on genuinely unseen ships.
- Six `..._Science_...` entities matched no class and simply abstained. The table
  has no `Science` band; adding one is not warranted by a single map, and
  abstention is the correct, safe behaviour.

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
