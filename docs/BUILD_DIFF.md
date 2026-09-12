# Build diff — design note

Status: design only. Nothing described here is implemented. The branch
`feat/build-diff` exists to build and try it.

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
rows. Run-to-run spread within a build is +-5k DPS, so this lead is thinner
than the noise: three runs per build is the least that can settle it.
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

### Stage 1 — Normalise out the crit streaks

The largest source of a difference that is not a build difference is luck on
crits. Before anything is compared, each row's average hit is restated at a
crit rate common to both runs:

```
neutral_avg_hit = non_crit_avg * (1 - shared_crit_rate)
                + crit_avg     * shared_crit_rate
```

`average_non_crit_hull_hit` and `average_crit_hit` are already kept per group,
so this is arithmetic over existing fields. `shared_crit_rate` is the pooled
rate over both runs, which keeps the correction symmetric — neither run is the
reference.

Observable: with the stage on, two runs of the same build whose crit rates
differed should move closer together. If they do not, the difference was not
crit luck, which is itself worth knowing.

### Stage 2 — Split the difference into three factors

On the hull channel (B3):

```
hull potential DPS = cadence * potency * efficiency

  cadence     = hits per second
  potency     = average base damage per hit    (before mitigation)
  efficiency  = 1 - target resistance
```

Each factor answers a different question, and which one moved says what kind of
change it was:

| factor moved | what that is |
|---|---|
| efficiency | the targets were softer — a debuff on them |
| potency | your shots were bigger before mitigation — a buff on you |
| cadence | you fired more often — a firing-cycle change |

The split into shares is the midpoint pairing already used for the `ΔDPS
breakdown` columns (`split_dps_difference`, documented at `DpsBreakdown`),
extended from two factors to three. It is chosen over the obvious alternative
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
would. The honest statement at two runs per build is not a p-value but a
counting fact: with two against two there are only six orderings, so the best
attainable one-sided result is 1 in 6. Three against three reaches 1 in 20.
Below three runs per build the question is unanswerable however the data falls,
and the output says that rather than reporting a lead.

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

## Open questions

1. **Where the report lives.** A third pane in the compare view, or a window of
   its own like the damage-type summary. The type summary's window is the
   nearest precedent. Needs a decision before the UI work starts; blocks
   nothing before then.
2. **Whether stage 1 is on by default.** Crit normalisation changes displayed
   figures away from what the rest of the program shows for the same run, which
   is a reason to make it explicit. Against that, leaving it off by default
   means the first thing most readers see is the noisiest version.
3. **What counts as "tightly clustered" for `GlobalLift`.** The threshold has to
   be picked against real logs, not chosen in advance. It is the one tuned
   number here, and belongs with the others listed in `ARCHITECTURE.md`.
