# Combat log format

What the two numeric fields of a combat log line mean, per line kind, and what
follows for the metrics derived from them.

The game ships no specification. Everything below is either quoted from the
community reference or measured against a real 212 MB log; each claim says
which.

## Source

The community reference is the r/stobuilds wiki page `math/log_reading`,
compiled by Mastajdog from a 2015 thread. Reddit, `stowiki.net` and the redlib
mirrors all refuse automated requests, so fetch it from the archive:

```
curl --compressed \
  "http://web.archive.org/web/20250806033015/https://www.reddit.com/r/stobuilds/wiki/math/log_reading/"
```

Shield mechanics come from the sibling page `math/shields` (by /u/Jayiie), the
hull resistance formula from `stowiki.net`'s "Damage resistance" article.

## Line shape

```
timestamp :: owner_display, owner_internal,
             source_display, source_internal,     <- the pet / console, if any
             target_display, target_internal,
             event_display, event_internal,
             type, flags, magnitude, base_magnitude
```

`Parser::parse_from_line` (`src/analyzer/parser.rs`) reads them in this order.
Note that what the code calls `source` is the *owner* and what it calls
`indirect_source` is the reference's *source* — the naming is inherited from
upstream, the field order is correct.

Fields 12 and 13 are generically "actual magnitude inflicted" and "some relevant
number to pair with 12". Their meaning depends on the line:

| line kind     | magnitude             | base magnitude                                                    |
|---------------|-----------------------|-------------------------------------------------------------------|
| hull heal     | −(amount healed)      | −(heal before Sensor Analysis / healing reductions), or 0 if same |
| shield heal   | −(amount healed)      | −(heal before reductions), or 0 if same                           |
| shield damage | −(damage to shields)  | −(damage prevented to hull)                                       |
| hull damage   | **+**(damage to hull) | base damage of the shot                                           |

Values carry 6 significant figures, so compare with a tolerance rather than for
equality.

Hull damage is the only one of the four that is positive. The reference's author
flags this as unexplained. Measured confirmation that a negative shield line
really is the shield *losing* points: all **37 951** `ShieldBreak` lines in the
212 MB log are negative and none positive — a shield cannot be broken by
healing.

## Lines per shot

One shot writes one *or two* lines; there is never a zero-filler.

| situation                                       | lines                         |
|-------------------------------------------------|-------------------------------|
| target has a shield facing up                   | shield line **and** hull line |
| no shield facing, or the attack ignores shields | hull line only                |
| shield absorbed everything                      | shield line only              |
| a drain is logged                               | shield line only              |

Two complications the reference calls out:

- **Overshields** produce a *second* shield line for the same shot, always at 0
  resistance. Summing "damage prevented" across both double counts.
- A shot's shield and hull lines may carry **different `source` fields**, so
  pairing them cannot key on that field.

## The one line kind the fields cannot classify

A shield line with a negative magnitude and a **zero** base magnitude is shaped
identically whether it restored a shield or an attack took one down: the table
above gives "0 if same" for a heal and "damage prevented to hull" — which can
also be 0 — for damage.

```text
Reflexive Emitters,      Shield, , -2000,    0     <- shield heal
Chain Conduit Capacitor, Shield, , -856.734, 0     <- attack on a Borg's shields
```

`Parser::resolve_ambiguous_shield_line` (`src/analyzer/parser.rs`) settles it
from the rest of the shot: it reads ahead over the remainder of the timestamp
and looks for a damage line with the same owner, target and ability. Found means
attack; not found means heal. A negative `HitPoints` line does **not** count —
abilities that restore hull and shields together (`Mudd's Time Bracelet`) write
one beside the shield line, and it is a second heal, not the shot's damage line.
The key omits the source field, because a shot's two lines may carry different
sources.

Lines read ahead are queued and served by the following calls, so nothing is
parsed twice and byte ranges stay exact.

Measured on the reference log: 2 348 records reclassified, 0.09 s added to a
3.3 s parse.

Two limits worth knowing:

- **The converse does not hold.** A shield line with no companion is not
  necessarily a heal — 9 364 lines are attacks the shields absorbed whole, so no
  hull line exists. Only zero-base-magnitude lines are ever in question.
- **The tail of a growing log.** If the log ends mid-timestamp, no companion is
  found and the line is taken as a heal; the rest arrives on a later refresh, by
  which time the record is already in. Bounded to the final timestamp of a log
  that stops growing.

## Shield facings are not identifiable

A ship has four shield facings and the game clearly tracks them separately, but
the log carries no way to tell them apart. Investigated because abilities exist
that key on a single facing (e.g. one dropping to zero); the conclusion is that
those cannot be attributed from the log.

Evidence that the four-facing structure reaches the log:

- A shield heal is written as **four lines** at the same instant — 3 973 of
  4 366 heal moments in the reference log. Multiples of four (8, 12, 16, 20)
  occur too, and are several applications inside the same tenth of a second.
- Counts below four appear for the same abilities that normally emit four
  (`Automated Reinforcement II`: 738 × 4 but also one 3 and one 1), consistent
  with a facing that is already full getting no line.
- Groups of fewer than four correlate with a quieter preceding few seconds:
  averaged over the previous 5 s, moments with four lines follow 10.1 incoming
  shield hits, moments with fewer follow 6.0. Directional only — the sample is
  117 moments.

Evidence that they still cannot be told apart, and this part is conclusive:

- Across 4 009 four-line moments, all four lines are **byte-identical in every
  field** — event id, flags, both values. Two groups in the whole log differ,
  and both are two separate heals colliding in the same tenth of a second.

So the count carries a weak signal about *how many* facings were below full;
nothing ever says *which*. Elimination cannot bootstrap either: knowing a
facing's state would require attributing damage lines to facings, which is the
missing piece to begin with, and no field ever labels a facing, so even a
perfectly tracked set of four counters could not be mapped to
fore/aft/port/starboard. Had the game logged zero-damage lines for untouched
facings, the fixed four-slot order would have made position meaningful; it does
not.

What remains usable is the `ShieldBreak` flag: an explicit, countable event
meaning a facing dropped to zero (37 951 occurrences in the reference log),
attributable to a target and a moment but not to a facing.

## Mitigation is three separate channels

This is why one combined "resistance" figure cannot be right:

| what is hit         | stat                     | formula                         | cap            |
|---------------------|--------------------------|---------------------------------|----------------|
| hull                | Damage Resistance Rating | `R = 1 − (¼ + 3·(75/(150+m))²)` | 75% asymptotic |
| shields             | shield hardness          | `1 − Π(1 − hᵢ)`                 | 75% hard       |
| shields, by a drain | DrainX                   | `1/(1 + DrainX)`                | —              |

Also: bleedthrough sends a share of the pre-resistance damage straight to hull
(5% on a typical resilient shield), and kinetic damage is cut by 75% against
shields, after bleedthrough and before hardness.

## What the code computes

`damage_resistance_percentage` (`src/analyzer/damage.rs`) reports **hull**
resistance only:

```text
1 − (damage to hull + damage prevented to hull) / base damage of shot
```

Both halves of the numerator are accumulated in
`DamageMetrics::calc_and_apply_delta`: hull hits contribute `total_damage.hull`
and `total_base_damage`, shield hits contribute
`total_damage_prevented_to_hull_by_shields`.

Deliberately excluded:

- **damage dealt to shields** — a different channel (hardness). Including it is
  what the figure used to do; on the reference log that read −35.9% where the
  hull figure is −55.5%.
- **shield drains** — a third channel (DrainX). Their records carry neither a
  hull component nor a base damage, so they enter neither side of the fraction
  and need no correction term. They remain counted as damage everywhere else
  (`total_damage.shield`, DPS, hit counts).

Negative values are normal: they mean the target was debuffed, or hit with armor
penetration, past zero resistance.

### Known approximations

1. **Aggregated, not per shot.** The reference formula is per shot; the code
   sums numerator and denominator over all hits in a group and divides once.
   A shot fully absorbed by shields contributes "prevented" but no base damage,
   and overshields contribute "prevented" twice. Fixing this needs shield and
   hull lines to be paired, which the parser does not do.
2. **Missing base damage is substituted.** When a hull line's base magnitude is
   0, `RecordValue::new` (`src/analyzer/parser.rs`) uses the actual damage as
   the base, i.e. assumes zero resistance. On the reference log this inflates
   the denominator by 0.67% and pulls the result from −56.6% to −55.5%.
   Inherited from upstream; left as is.

## Related

- `docs/HEALING_MODEL.md` — the three healing pools, and why a shield line with
  a zero base magnitude cannot be told apart from a heal by its fields alone.
