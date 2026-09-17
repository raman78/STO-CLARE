# Build diff — design note

Status: a proof of concept exists on `poc/build-diff`, in two modules and two
ignored tests that run them over a real log and print a report. Nothing draws
either; there is no UI.

| Module | Unit | Driver |
|---|---|---|
| `app/compare/build_diff` | one run against one run | `build_diff_on_a_real_log` |
| `app/compare/build_groups` | every run of one build against every run of another, grouped by note | `build_groups_on_a_real_log` |

What running them found is in "What running it showed" at the end. Four of those
findings have already changed what is written above, one of them a correction to
the arithmetic of how many runs are needed.

## Purpose

The compare view (`app/compare`, see `ARCHITECTURE.md` "Comparing several
combats") puts several runs side by side and marks where they disagree. It
stops there: the reader still has to work out *what kind* of difference they
are looking at and *which of the two builds is ahead*.

That last step is the one this note is about. A player swaps one thing between
runs — say a bridge officer slot holding either a target-resistance debuff or a
discrete damage source — and wants a sentence, not a table:

```
Hellbore ahead by 3%. It adds one 11.2k DPS line; the other run instead
made every target 12 points softer, worth 9.4k DPS spread over ten weapon
rows. Every Hellbore run beat every other run, with nothing in between — but
at three runs a side the cleanest possible result is still 1 in 10, so four
a side is the least that can settle it.
```

Everything in that sentence is derivable from what the analyzer already
records. None of it is derivable from the ability list, because the log does
not carry one — see "What the program cannot know".

## Context

```
  combat log ──► analyzer ──► DamageGroup tree (per player, per combat)
                                      │
                                      ▼
                          Comparison (app/compare)
                             │              │
                   existing  │              │  proposed
                             ▼              ▼
                   Spread / vs rest    Build diff
                   (per-row figures)   (a verdict over the whole run)
```

The build diff is a consumer of the same `CompareNode` tree the table already
builds. It adds no parsing and needs no new field in `DamageMetrics`; §"Inputs"
lists what it reads and confirms each one exists today.

## What the program cannot know

The log records damage, not loadout. An ability that deals no damage of its own
— a resistance debuff being the case that matters here — produces no row in the
tree, so the program cannot see that it was slotted, let alone that it caused
anything.

The consequence shapes the whole design: **the output states effects, never
causes.** "Targets were 12 points softer, consistently across ten rows" is a
claim about the log. "Attack Pattern Beta was responsible" is not, and must not
appear. The reader makes that step; it is the easy one.

Which run is which build is likewise the reader's to say. The combat note
(`CombatNotes`, surfaced in the compare view through `slot_notes` and shown in
the column headers) is the label, and the analysis groups runs by it. A
comparison whose runs carry no notes gets a per-run diff instead of a per-build
one.

## Invariants

- **B1 — Effects, not causes.** No output names an ability, a console or a
  trait as the reason for a measured change. See "What the program cannot know".
- **B2 — The waterfall closes.** The steps between one run's DPS and another's
  sum to the difference exactly, with no residual term attributed by hand. This
  is why the existing midpoint split (`split_dps_difference`) is reused rather
  than a log-ratio decomposition; see "Decisions and trade-offs".
- **B3 — The factor identity is exact on the hull channel only.** `base_dps *
  (1 - resistance)` is what the hull would have taken. Damage dealt to shields
  carries no base damage (`SpecificHit::Shield` has no `base_damage`) and is
  therefore outside the identity. Any figure derived from it is labelled as a
  hull figure.
- **B4 — Uncertainty is shown, never inferred away.** Where the evidence cannot
  settle a question, the output says so and says what would settle it. A run
  count that makes significance unreachable is stated as such.
- **B5 — Agreement across rows is evidence about one run, not about
  repeatability.** Ten weapon rows agreeing on a common multiplier is strong
  evidence that one cause acted in that run. It says nothing about whether the
  next run will look the same. The two are reported separately and never summed
  into one confidence figure.

## Inputs

Every field below exists today. `DamageMetrics` (`analyzer/damage.rs`) is
reachable from any `DamageGroup` through its `Deref`, and therefore from every
`CompareNode` slot.

| Field | Meaning | Used for |
|---|---|---|
| `hits_per_second` | hits per second, per shield/hull/all | cadence factor |
| `total_base_damage`, `base_dps` | damage before target mitigation | potency factor |
| `damage_resistance_percentage` | target hull resistance, `damage_resistance_percentage()` | efficiency factor |
| `total_damage_prevented_to_hull_by_shields` | the other half of the resistance numerator | hull-channel bookkeeping |
| `average_crit_hit`, `average_non_crit_hull_hit`, `critical_percentage`, `crits` | the crit split already kept per group | crit normalisation |
| `total_damage`, `dps`, `average_hit` | the plain figures | the waterfall's end points |

## The four stages

### Stage 1 — Lift the crit multiplier out as a factor of its own

Crits are **not** noise to be removed. Crit chance and crit severity are things
a player fits — consoles, traits and gear raise both — so a run that crits more
may be a better build rather than a luckier one, and a stage that quietly
normalised them away would erase the very difference the report exists to find.
The crit contribution is therefore made a named factor and shown, not cancelled.

The observed mean hull hit factors exactly:

```
mean_hull_hit = neutral_hit * crit_multiplier

  crit_multiplier = 1 + p * (S - 1)
  p = critical_percentage / 100
  S = average_crit_hit / average_non_crit_hull_hit      (severity ratio)
```

`p * crit_avg + (1 - p) * non_crit_avg` is `non_crit_avg * (1 + p(S - 1))`, so
this is an identity over fields the analyzer already keeps, not an estimate.

`S` is a ratio of two post-mitigation averages, so target resistance cancels
out of it — but only when the crit and non-crit hits of that row went to the
same population of targets. See "Failure modes" for what it looks like when
they did not.

**Measured, and it decides where the factor sits.** Whether the game's logged
base damage already contains crit severity is not documented — the reference
says only "base damage of the shot" (`COMBATLOG_FORMAT.md`). Measured over the
last 40 MB of a real 128 MB log with `awk` on the raw lines (not through this
program's code): across 16 weapons with at least 500 crit and 500 non-crit hull
hits each, crit-flagged hits carry a mean base damage **2.42x** that of
non-crit hits of the same weapon, per-weapon ratios running about 2.2 to 2.9.
Were severity applied after the base figure, the ratio would sit at 1.0.

Two consequences follow, and both matter:

- Crit severity is **inside** `total_base_damage`, so `base_dps` is not a
  crit-free measure of offence. The crit factor is split out of potency, not
  layered on top of it.
- The per-weapon ratios are not equal. Whether that is sampling scatter or real
  per-weapon severity is an open calibration question, and it is exactly the
  kind of thing the stage-3 classifier is built to surface.

**The luck question is separate, and answerable.** Whether a difference in crit
*rate* between two runs is build or luck does not need a toggle: a crit rate is
a binomial proportion over thousands of hits, so its sampling error is small and
computable from the hit count the analyzer already has. The report can state
whether the observed gap is larger than luck would plausibly produce, rather
than leaving the reader to guess. The counterfactual view — both runs restated
at a pooled crit rate — stays available as a toggle, **off by default**, because
the default has to be that crits count.

### Stage 2 — Split the difference into four factors

On the hull channel (B3):

```
hull potential DPS = cadence * neutral potency * crit multiplier * efficiency

  cadence          = hits per second
  neutral potency  = base damage per non-crit hit    (before mitigation)
  crit multiplier  = 1 + p * (S - 1)                 (stage 1)
  efficiency       = 1 - target resistance
```

Each factor answers a different question, and which one moved says what kind of
change it was:

| factor moved | what that is |
|---|---|
| efficiency | the targets were softer — a debuff on them |
| neutral potency | your shots were bigger before mitigation — a damage buff |
| crit multiplier | you critted more often, or harder — a crit-stat change |
| cadence | you fired more often — a firing-cycle change |

The split into shares is the midpoint pairing already used for the `ΔDPS
breakdown` columns (`split_dps_difference`, documented at `DpsBreakdown`),
extended from two factors to four. It is chosen over the obvious alternative
for the reason in "Decisions and trade-offs".

### Stage 3 — Name the shape of each row's change

For every row present in both runs, take the ratio of the stage-1 figures, then
take the **median** of those ratios across rows and their spread. The median
rather than the mean for the same reason `typicality` uses one: a handful of
rows and one odd ratio drags a mean far enough to hide everything else.

| shape | test | reads as |
|---|---|---|
| `New` / `Gone` | the row is in one run only | a discrete source added or removed |
| `GlobalLift` | median ratio away from 1, ratios tightly clustered | one cause acting on everything |
| `Single` | one row's ratio far from the median | the change touched that row alone |
| `Noise` | the change is inside the run-to-run spread | say nothing |

This is where the small-sample problem is answered, and the answer is not a
statistical test. With ten weapon rows, **each row is a separate measurement of
the same multiplier.** Ten rows agreeing on 1.12x is strong evidence from two
runs, because the evidence is the agreement, not the difference in totals. When
the ratios scatter instead, the spread says so directly and the shape is not
`GlobalLift`.

Subject to B5: the rows share a run and share targets, so they are not
independent samples. The agreement establishes a common cause *within that run*.
Repeatability is a separate claim needing separate runs, and the output keeps
the two apart.

The classifier belongs to the family of multi-dimensional root-cause
localisation algorithms (Adtributor, Squeeze, RiskLoc). Squeeze is the closest
prior art because it is the one that handles *derived* measures — quotients and
products — which is what resistance and `cadence * potency` are. What is
borrowed is the central idea: cluster the per-element deviations and look for
the cluster that moved together.

### Stage 4 — Report

A waterfall from one run's DPS to the other's, closing exactly (B2):

```
  Run #1  "APB"                                    132 400 DPS
    - Hellbore, absent here                         -11 200
    + targets 12 points softer  (10 of 10 rows)      +9 400
    - fired less often                               -2 100
  Run #2  "Hellbore"                               128 500 DPS
```

Below it, in prose: which build is ahead and by how much, what the run-to-run
spread within a build is, and — when the run count cannot settle it — what
would. The honest statement at these sample sizes is not a p-value from a table
but a counting fact, and the count is **two-sided**, because the reader is
asking which of two builds is better rather than confirming a direction picked
beforehand. Of the six ways four runs split two and two, two separate them
completely, one per direction — so the best attainable result is 1 in 3. Three a
side reaches 1 in 10, four a side 1 in 35 (`rank_test`, and the test
`four_runs_a_side_is_the_first_count_that_can_settle_anything`). **Four runs per
build is therefore the first count whose best possible outcome clears 0.05**;
below it the question is unanswerable however the runs fall, and the output says
so rather than reporting a lead.

**The stacking counterfactual.** Take one run, add the other's `New` rows, apply
the other's `GlobalLift`. This is the figure a reader asking "what if I could
fit both?" wants. It rests on the two effects being independent, which in this
game they often are not — damage multipliers do not always compose linearly — so
it is labelled a prediction under a stated assumption and kept visually apart
from the measured part of the report.

## Failure modes

| Symptom | Cause | Where to look |
|---|---|---|
| Every row classified `Noise` | run-to-run spread swamps the effect; or one run is much shorter | the runs' durations, and the count per build |
| `GlobalLift` on a comparison of unrelated maps | different enemies have different base resistance, so efficiency moves for a reason that is not the build | the detected map and difficulty per run |
| Efficiency moved but potency did too | a teammate's debuff, or your own buff state differed | `base_dps` per run: if it matches, offence was the same and the whole difference is mitigation |
| Waterfall does not close | shield damage leaked into a hull-channel figure | B3 |
| One row's severity ratio `S` far off the rest | its crit and non-crit hits went to targets of different resistance, so resistance did not cancel out of the ratio — a damage-over-time row ticking on a mixed group is the usual case | compare that row's ratio of *base* damage against its ratio of *actual* damage: they agree when the target population is the same and part company when it is not |

## Decisions and trade-offs

**Midpoint split over log-ratio decomposition.** Taking logs makes the factors
additive in percentage terms, which is tidier to compute. It was rejected
because the shares then sum in log space and not in DPS, and a waterfall that
does not close invites arithmetic that is wrong (B2). The midpoint pairing
already in `split_dps_difference` sums to the whole difference exactly.

**Median and spread over standard deviation.** At two to five runs a standard
deviation is one number with nothing behind it, and printing a sigma implies a
measurement that was not made. The spread of the per-row ratios is a real
observation and carries the same information for this purpose.

**No machine learning.** At four combats a model cannot learn anything
arithmetic does not already give, and would wrap the result in confidence it has
no basis for — a direct conflict with B4. The problem is variance and
confounders, not an unknown functional form: the form is known in advance and is
written out in stage 2.

**Grouping by combat note rather than by a new "build" concept.** The note
already exists, is already shown in the compare view's headers and the chart
legend, and is already how the reader tells runs apart. A second labelling
mechanism would be a second thing to keep in step.

## Where the report lives

**A headline in the compare view, the report itself in a window.**

The compare view is a `Splitter::horizontal` carrying two panes that both want
height — the tree table and the chart — with a draggable boundary between them.
A third pane there would take height from both for something the reader consults
occasionally rather than scans continuously, and the `Damage by type` window
exists because that same pressure already came up once. The build diff is the
same kind of object as that summary: a derived read over the whole comparison,
not a column of the tree. It gets the same treatment — a toolbar toggle, a
window sized to its content, centred, closed by Escape.

The exception is the one line that answers the question the reader arrived
with: which run is ahead, by how much, and whether that is inside the run-to-run
spread. That goes where the hint line under the toolbar sits, always visible,
and clicking it opens the full report. A verdict nobody can find because they
did not know the button was there is a verdict that was not delivered.

## Calibration left open

**What counts as "tightly clustered" for `GlobalLift`.** This cannot be chosen
in advance; it has to be fitted against real logs. Two ways to get there, and
the plan is to do them in order:

1. A slider over a narrow range, defaulted from whatever the first real
   comparisons show, sitting with the other tuned numbers listed in
   `ARCHITECTURE.md`. Shipping this first means the classifier can be used while
   the right value is still being learnt.
2. Better, if the data supports it: no fixed threshold at all. Each row's ratio
   carries its own sampling error, set by how many hits it is built from — a row
   with 200 hits scatters more than one with 20 000. Comparing the observed
   scatter against the scatter those hit counts alone would produce makes the
   test self-calibrating, and removes the magic number. Whether hit count
   actually explains the observed scatter is the thing to measure first; the
   per-weapon severity ratios noted in stage 1, which range about 2.2 to 2.9,
   are a ready test case.

Either way the report shows the evidence and not only the label: "10 of 10 rows
within 2% of 1.12x" lets the reader judge the call the program made.

## What running it showed

Run over two combats of a real log — the same map, the same difficulty, both
solo, one noted `HBL` and one noted `APB`, which is the labelling the design
rests on. Both figures below come from the code on the branch, not from a
separate script.

The classifier picked the resistance effect out on its own, knowing nothing
about any ability: **`targets softer`, median 1.075x, 16 of 19 rows agreeing**,
worth +96k of a +243k difference. The discrete source showed up opposite it, as
one row present in the other run only: `Hellbore Light - Ignition`, 33k. That is
the shape the design predicted, arriving without being told to look for it.

Three things the run taught that the design did not have:

1. **An aggregate factor moves when the *mix* of rows moves, not only when the
   rows move.** The common core's crit multiplier rose 8.7% while the rows' own
   median rose 2.6%: most of that +95k was the proportions of the weapons
   firing, not any weapon critting harder. Presenting it as a crit finding would
   have been an artefact reported as a result. The report now prints the
   aggregate change and the rows' own median side by side and marks the gap.
2. **The tool catches the reader changing more than one thing.** The two runs
   also swapped a weapon — one array present in each run and not the other,
   +46k and -35k. Neither is a bridge officer slot, and nothing else would have
   said so.
3. **The defaults discriminate on the evidence available.** `DEFAULT_TOLERANCE`
   0.05 with `DEFAULT_AGREEMENT` 0.8 fires on the `APB`/`HBL` pair and stays
   silent on a second pair whose resistance genuinely did not differ (median
   0.997x, 16 of 19 agreeing — agreement about nothing happening). Two pairs is
   thin, and these remain provisional for the reason in "Calibration left open".

## What the grouped runs showed

Six runs, three noted `APB` and three noted `HBL`, all solo on the same map at
the same difficulty — checked by the report rather than assumed, which is why
`Group::maps` exists. Figures from `build_groups` on the branch.

| | APB | HBL |
|---|---|---|
| median hull potential DPS | 1 226 577 | 1 101 198 |
| its own runs | 1 159 074 – 1 332 003 | 1 089 378 – 1 110 857 |
| pairings won | 9 of 9 | 0 of 9 |

**Every `APB` run beat every `HBL` run, with nothing in between**, for a median
lead of 11.4%. And the mechanism came out unanimous: `targets softer` moved the
same way in all nine pairings, median 0.913x — `HBL`'s targets about 9.5%
harder. One pair of runs could not have told that from a good session; nine
pairings agreeing can.

Four findings, three of which changed the design above.

1. **A range is not a noise floor, and using one as a floor contradicted the
   rank test.** The first version compared the difference of medians against the
   larger group's own spread and reported "inside the noise" — while the line
   below it said every run of one build had beaten every run of the other. Both
   were computed from the same six numbers. A range grows with the run count and
   one good run stretches it, so it cannot be a threshold; whether the two sets
   of runs *overlap* can, and that is what `RankTest::complete` now answers.
   The spread is still printed, as context rather than as a verdict.
2. **The rank test has to be two-sided, which changes how many runs are needed.**
   The reader is asking which build is better, not confirming a direction chosen
   in advance, so both complete separations count. Three a side therefore reaches
   1 in 10 and not 1 in 20: **four runs per build** is the first count whose best
   possible result clears 0.05. An earlier version of this document said three,
   and was wrong.
3. **A one-sided `p` reported against a fixed group is actively misleading.**
   With `APB` ahead 9-0 the figure came out as 1.000 — "nothing here" for the
   strongest evidence in the sample. `RankTest::favours` names the direction the
   runs point and `p` is taken in it.
4. **The drift list is not a footnote. It is the gate.** `Thoron Infused Polaron
   Array - Fire at Will III`, worth 45.8k, is in every `APB` run and only two of
   three `HBL` runs — **more than the 34.8k of `Hellbore Light - Ignition`, the
   row that is actually the build difference.** Part of the measured lead is a
   weapon that was not held constant. Nothing else in the program would have said
   so, and a reader eyeballing damage rows would not have noticed a row that is
   *present* in five runs out of six.

That last one also invalidated a grouping. Pooling the earlier runs noted
`Attack Patern Beta` and `Hellbore` with these gives four a side, which is the
count that could settle the question — but the drift list then lists whole
weapon arrays present in one run of four, the largest worth 172.9k. Those
sessions were flown on a different loadout, so the pooled comparison measures
the loadout and not the bridge officer slot. **The check that says a comparison
is worth reading has to come before the verdict, not after it.**
