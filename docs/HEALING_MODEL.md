# Healing model

How the analyzer decides which of the three healing pools a record belongs to,
and why each pool is stored in two grouping orders.

Damage is unaffected by everything in this document.

## Purpose

The STO combat log has no notion of "healing done" versus "healing received" —
it emits one record per heal tick, carrying a source, an optional indirect
source (a console or a pet), and a target. Any of the three may be the player.
The analyzer has to derive the direction, and until it did so explicitly, a
player healing themselves was recorded **twice**: once because they were the
source, once because they were the target.

The scale of that double count on a real 212 MB log, for one player:

| pool | ticks | total | hull | shield |
|---|---|---|---|---|
| received from others | 859 | 778 635 | 478 k | 300 k |
| done to others | 2 076 | 353 042 | 90 k | 260 k |
| self | 5 528 | 54 553 454 | 300 k | 54 253 k |

54.55 M of 55.68 M was self healing, dominated by a single gear proc
(`Shield Absorptive Frequency Generator Proc`, 53.5 M). Counting it in both
directional pools made both of them read as that one proc, burying the 0.78 M
that actually came from teammates.

## What counts as a heal at all

`RecordValue::new` (`src/analyzer/parser.rs:268`) classifies a record before any
of this applies:

| condition | result |
|---|---|
| `value_type == "HitPoints"`, `value1 < 0` | hull heal |
| `value_type == "Shield"`, `value2 == 0`, no `ShieldBreak`, `value1 < 0` | shield heal **or** damage — see below |
| `value_type == "Shield"`, `value2 == 0`, no `ShieldBreak`, `value1 > 0` | **shield drain damage**, not a heal |

The third row surprises: a positive shield value with a zero second value is a
drain, and lands in the damage trees.

The second row is the one no single line can settle. The shield half of an
attack sometimes records a zero base magnitude, which makes it byte-identical to
a shield restore:

```text
Reflexive Emitters,      Shield, , -2000,    0     <- a genuine shield heal
Chain Conduit Capacitor, Shield, , -856.734, 0     <- an attack on a Borg's shields
```

Only the rest of the shot separates them: an attack also writes a damage line at
the same instant, on the same target, for the same ability; a heal does not.
`Parser::resolve_ambiguous_shield_line` (`src/analyzer/parser.rs`) reads ahead
over the remainder of the timestamp to look for it, and passes the answer into
`RecordValue::new`. Details and the caveats in `docs/COMBATLOG_FORMAT.md`.

On the reference log this moves **2 348** records out of healing and into shield
damage — over half of one player's Healing Ally, which is why abilities like
`Chain Conduit Capacitor` used to show up as "healing" enemy drones.

The converse rule would be wrong and is not applied: a shield line with *no*
companion is not necessarily a heal. 9 364 lines in the reference log are
attacks whose shields absorbed the whole shot, so no hull line exists
(`Quad Disruptor Cannons`, `Disruptor Turret`, `Plasma Storm`). Only lines with
a zero base magnitude are ever in question.

## Invariants

- **H1 — disjoint.** Every heal tick lands in exactly one of `heal_ally`,
  `heal_received`, `heal_self`. Summing all three double counts nothing.
- **H2 — order-equivalent.** `HealPool::by_person` and `HealPool::by_ability`
  hold the same ticks. Any aggregate (total, tick count, HPS) is identical
  between them; only the nesting differs.
- **H3 — damage untouched.** The routing change applies to `RecordValue::Heal`
  only. Self-directed *damage* still counts as incoming damage, as before.

## Routing

`Analyzer::process_next_record` (`src/analyzer/mod.rs:180`) calls
`Player::add_out_value` for records the player sources, and
`Player::add_in_value` for records that target them. For heals the two are
gated:

```
                       heal record
                            │
              ┌─────────────┴─────────────┐
      source is this player        target is this player
              │                            │
   heal_lands_on_self()?            source is this player?
        │           │                  │           │
       yes          no                yes          no
        │           │                  │           │
   heal_self    heal_ally          (dropped —    heal_received
                                 already counted
                                  by the source
                                     branch)
```

`Player::heal_lands_on_self` (`src/analyzer/mod.rs:585`) decides the left side;
`source_is_self` (`src/analyzer/mod.rs:667`) gates the right side so a self heal
is not counted a second time.

### Record shapes and where they go

Every shape below was observed in a real log. `me` is the player whose pools are
being filled.

| source | indirect | target | example | pool |
|---|---|---|---|---|
| me | — | — | `Reflexive Emitters`, `Brace for Impact III` | `heal_self` |
| me | my console | me | `Bio-Molecular Shield Generator Fabrication` | `heal_self` |
| me | — | me | explicit self-target | `heal_self` |
| me | — / console | other player | `Rally Cry V` → `Sirak` | `heal_ally` |
| me | — / console | NPC | → `U.S.S. Birmingham`, `Security Escort III` | `heal_ally` |
| me | my pet | — | `Motivated Strikes` through `Security Escort III` | `heal_ally`, credited to the pet |
| other player | — / console | me | `Shield Generator Energy Matrix` from `Bor'ar` | `heal_received` |
| NPC | — | me | ally (or oddly-signed enemy) ability | `heal_received` |

The pet row is deliberate: `src=me, ind=my pet, tgt=-` heals the *pet*, not the
player, so it is healing done with the pet as recipient — which is what
`add_out_value`'s existing recipient fallback (`target.name()` else
`indirect_source.name()`) already produced.

`src=NPC, ind=me` (an NPC healing while the player is the indirect source)
stays in `heal_received`, matching the pre-split behaviour. It occurred 8 times
in a 212 MB log; it was not worth a rule of its own.

## Grouping orders

`HealGroup::add_heal` (`src/analyzer/groups.rs:394`) reads a path **back to
front**: the last segment becomes the top level of the tree, `path[0]` becomes
the leaf. `Player::add_heal_to_pool` (`src/analyzer/mod.rs:716`) exploits that to
file each tick twice from one path:

```
build_grouping_path() ──► [ Value(ability), Group(indirect)?, Group(custom)? ]
                                        │
                    ┌───────────────────┴───────────────────┐
              push(person)                            insert(0, person)
                    │                                       │
            person → ability                        ability → person
           (HealPool::by_person)                 (HealPool::by_ability)
```

`insert(0, ..)` mirrors what the damage tree already does for its target
(`add_out_value`), so the second order matches the layout of the damage tabs.

Both orders are built during analysis rather than pivoted in the UI, because the
tree carries per-node metrics and percentages computed by
`HealPool::recalculate_metrics` and `HealPool::recalculate_percentages`
(`src/analyzer/heal.rs`). Re-nesting in the UI would mean recomputing those; the
alternative — re-parsing on every toggle — costs 3.3 s on a 212 MB log. The cost
paid instead is one duplicate copy of the heal ticks, roughly 9 MB for a
212 MB log. Heal ticks are a small fraction of the record volume, so this was
the cheaper trade.

`HealTab` (`src/app/main_tabs/heal_tab.rs`) builds a table for each order up
front and `show_grouping_picker` (`src/app/main_tabs/heal_tab.rs:78`) selects
which one is drawn. Switching drops `selection_diagrams`, because the two tables
track their own expansion and selection state and a chart built from one no
longer corresponds to what is on screen.

## Metric bases

| metric | denominator | source |
|---|---|---|
| DPS | player `combat_time` (first to last damage dealt) | `Player::recalculate_metrics` |
| HPS, ticks/s | player `active_time` (first to last action of any kind) | same |

The two healing rates therefore use a different time base than DPS in the
neighbouring tabs. This predates the split and was left alone; the column
tooltips in `src/app/main_tabs/tables/heal_table.rs` now state it.

`HealMetrics::critical_percentage` (`src/analyzer/heal.rs`) divides the crit
count by **hull** ticks while counting crits across all ticks. That is correct
in practice: a scan of a 212 MB log found 433 critical hull heals and **zero**
critical shield heals — STO does not crit shield healing. It is not enforced
structurally the way the damage side enforces it
(`src/analyzer/damage.rs:144`), so a future game change would surface as a
critical percentage above 100.

## Verification

Two independent implementations agree on the split. `dump_heal_pools`
(`src/analyzer/mod.rs`, `#[ignore]`) totals the pools straight from the
analyzer:

```
CLA_TEST_COMBATLOG=<path> cargo test dump_heal_pools -- --ignored --nocapture
```

On the 212 MB reference log it reports `received 778635 / done 353042 /
self 54553454`, matching a standalone count of the raw records to the unit.

Unit coverage, all in `src/analyzer/mod.rs`:

| test | asserts |
|---|---|
| `healing_is_split_into_three_disjoint_pools` | H1, plus the hull/shield split per pool |
| `a_heal_between_two_players_lands_in_opposite_pools` | a cross-player heal is done for one side, received for the other, self for neither |
| `a_heal_pool_holds_both_grouping_orders` | H2, and that each order nests the way it claims |

## Related

- `docs/COMBATLOG_FORMAT.md` — what the two numeric fields mean per line kind,
  the sources for it, and the hull resistance formula.
- `docs/DIFFICULTY_DETECTION.md` — what the combat log does and does not carry,
  including the verification that it holds no map marker.
