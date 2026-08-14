//! Damage figures for part of a player's tree, worked out the way the analyzer
//! works out the whole of it.
//!
//! Both places that let a reader tick rows off — the comparison's Total and the
//! main window's player row — ask the same question: what would this player's
//! figures be if only these rows counted? The answer cannot be got by adding
//! the columns up. A percentage has no meaning added to another percentage, and
//! resistance, crit rate and accuracy are ratios of hit counts, not sums.
//!
//! So the hits themselves are pooled and put back through
//! [`DamageMetrics::calc_and_apply_delta`] and
//! [`DamageMetrics::recalculate_time_based_metrics`] — the same pass
//! `DamageGroup::recalculate_metrics` makes over a branch. What comes out is
//! what the analyzer would have said about a group holding exactly those rows.

use std::ops::Range;

use chrono::NaiveDateTime;
use rustc_hash::FxHashSet;

use crate::{
    analyzer::{
        AnalysisGroup, DamageGroup, DamageMetrics, Hit, Hits, HitsManager, MaxOneHit, NameHandle,
        NameManager, Player, ShieldHullOptionalValues, ShieldHullValues,
    },
    helpers::time_range_to_duration_or_zero,
};

/// A player's outgoing damage with the named rows left out, or `None` when
/// nothing is left.
///
/// Used by the compare view's Total; the main window's player row is next.
///
/// `None` is not the same as zero: a player with none of the kept rows has
/// nothing to say, and showing a zero would read as a run that did nothing
/// rather than one that did something else.
#[allow(dead_code)] // wired into the main window's tables next
pub fn player_damage_without(
    player: &Player,
    group: &DamageGroup,
    name_manager: &NameManager,
    hits_manager: &HitsManager,
    excluded: &FxHashSet<String>,
    combat_total: &ShieldHullValues,
) -> Option<DamageGroup> {
    let rows = group
        .sub_groups()
        .values()
        .map(|sub| (sub.name().get(name_manager), sub.hits.get(hits_manager)));
    Some(subset_group(
        subset_hits(rows, excluded)?,
        metrics_duration(&player.combat_time),
        combat_total,
    ))
}

/// The hits of the rows that are kept, pooled — or `None` when none of them are
/// here at all.
///
/// A branch's hits are the whole branch's, so a row goes in with everything
/// under it.
pub fn subset_hits<'a>(
    rows: impl Iterator<Item = (&'a str, &'a [Hit])>,
    excluded: &FxHashSet<String>,
) -> Option<Vec<Hit>> {
    let kept: Vec<&[Hit]> = rows
        .filter(|(name, _)| !excluded.contains(*name))
        .map(|(_, hits)| hits)
        .collect();
    if kept.is_empty() {
        return None;
    }
    Some(kept.into_iter().flatten().copied().collect())
}

/// A damage group standing for a set of hits, with every metric recalculated
/// from them the way [`DamageGroup::recalculate_metrics`] does it, so the rest
/// of a table can read it like any other group.
pub fn subset_group(hits: Vec<Hit>, duration: f64, combat_total: &ShieldHullValues) -> DamageGroup {
    let mut damage_metrics = DamageMetrics::default();
    damage_metrics.calc_and_apply_delta(&hits);
    damage_metrics.recalculate_time_based_metrics(duration);
    let mut max_one_hit = MaxOneHit::default();
    max_one_hit.update_from_hits(NameHandle::UNKNOWN, &hits);
    DamageGroup {
        damage_percentage: ShieldHullOptionalValues::percentage(
            &damage_metrics.total_damage,
            combat_total,
        ),
        damage_metrics,
        max_one_hit,
        hits: Hits::Leaf(hits),
        ..Default::default()
    }
}

/// The combat duration the analyzer measures a player's outgoing damage
/// against: the time they were in combat.
///
/// Not the length of the fight (`active_time`, which the charts use) — dividing
/// by that would give a DPS no other part of the program states.
pub fn metrics_duration(combat_time: &Option<Range<NaiveDateTime>>) -> f64 {
    time_range_to_duration_or_zero(combat_time)
        .num_milliseconds()
        .max(0) as f64
        / 1e3
}
