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
    let mut kept = subset_group(
        subset_hits(rows, excluded)?,
        metrics_duration(&player.combat_time),
        combat_total,
    );
    // The row keeps its name and the rows under it: the reader is looking at
    // the same player, with part of their tree set aside, and a nameless row
    // that cannot be opened is not that.
    kept.segment = group.segment;
    kept.sub_groups = group
        .sub_groups()
        .iter()
        .filter(|(_, sub)| !excluded.contains(sub.name().get(name_manager)))
        .map(|(handle, sub)| (*handle, sub.clone()))
        .collect();
    Some(kept)
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

/// A player's outgoing damage holding only what was dealt in the picked types,
/// or `None` when they dealt none of them.
///
/// A row's own figures are then of the type: `Polaron Beam Array` under `Cold`
/// is the few hundred its Frostbite proc did, not the beams around it.
pub fn damage_of_types(
    player: &Player,
    group: &DamageGroup,
    name_manager: &NameManager,
    hits_manager: &HitsManager,
    types: &FxHashSet<String>,
    combat_total: &ShieldHullValues,
) -> Option<DamageGroup> {
    let filter = TypeFilter {
        name_manager,
        hits_manager,
        types,
        duration: metrics_duration(&player.combat_time),
    };
    let mut kept = filter.keep(group)?;
    // The percentages the tables show are of the whole fight for the top row
    // and of the parent below it, the same as the analyzer states them.
    kept.damage_percentage =
        ShieldHullOptionalValues::percentage(&kept.damage_metrics.total_damage, combat_total);
    set_child_percentages(&mut kept);
    Some(kept)
}

/// What a walk down the tree needs to keep the picked types out of it.
struct TypeFilter<'a> {
    name_manager: &'a NameManager,
    hits_manager: &'a HitsManager,
    types: &'a FxHashSet<String>,
    duration: f64,
}

impl TypeFilter<'_> {
    /// This group with everything outside the picked types taken out of it, or
    /// `None` when it dealt none of them.
    ///
    /// A group of one picked type is kept whole — nothing in it is of another
    /// type, so there is nothing to recompute. A group of several is rebuilt
    /// from the sub-groups that survive, since that is the only level the log
    /// separates them at: a hit carries no damage type of its own, the group it
    /// lands in does.
    fn keep(&self, group: &DamageGroup) -> Option<DamageGroup> {
        let mut own = group.damage_types.iter().map(|t| t.get(self.name_manager));
        let picked = own.clone().filter(|t| self.types.contains(*t)).count();
        let all = own.by_ref().count();

        if picked == 0 {
            return None;
        }
        if picked == all {
            return Some(group.clone());
        }

        let sub_groups: Vec<DamageGroup> = group
            .sub_groups
            .values()
            .filter_map(|sub| self.keep(sub))
            .collect();
        if sub_groups.is_empty() {
            return None;
        }

        let hits: Vec<Hit> = sub_groups
            .iter()
            .flat_map(|sub| sub.hits.get(self.hits_manager).iter().copied())
            .collect();
        let mut kept = subset_group(hits, self.duration, &Default::default());
        kept.segment = group.segment;
        kept.damage_types = group
            .damage_types
            .iter()
            .copied()
            .filter(|t| self.types.contains(t.get(self.name_manager)))
            .collect();
        kept.sub_groups = sub_groups
            .into_iter()
            .map(|sub| (sub.name(), sub))
            .collect();
        Some(kept)
    }
}

/// Each row's share of the row above it, down the tree — the figure the
/// `Damage %` column shows, which a rebuilt group has no one to have set.
fn set_child_percentages(group: &mut DamageGroup) {
    let parent_total = group.damage_metrics.total_damage;
    for sub in group.sub_groups.values_mut() {
        sub.damage_percentage =
            ShieldHullOptionalValues::percentage(&sub.damage_metrics.total_damage, &parent_total);
        set_child_percentages(sub);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{BaseHit, NameFlags, SpecificHit, ValueFlags};

    fn hit(damage: f64) -> Hit {
        Hit {
            hit: BaseHit {
                damage,
                flags: ValueFlags::NONE,
                specific: SpecificHit::Hull {
                    base_damage: damage,
                },
            },
            time_millis: 0,
        }
    }

    /// A group of one type is kept whole; a group of several is rebuilt from
    /// the sub-groups that survive, so its figures are of the picked type and
    /// not of everything it was part of. This is the Polaron Beam Array case:
    /// under `Cold` it is the Frostbite proc, not the beams around it.
    #[test]
    fn a_type_filter_rebuilds_a_mixed_group_from_what_survives() {
        let mut names = NameManager::default();
        let cold = names.insert("Cold", NameFlags::NONE);
        let polaron = names.insert("Polaron", NameFlags::NONE);
        let hits_manager = HitsManager::default();

        let leaf = |damage: f64, damage_type| {
            let mut group = DamageGroup {
                hits: Hits::Leaf(vec![hit(damage)]),
                ..Default::default()
            };
            group.damage_types.insert(damage_type);
            group.damage_metrics.total_damage.all = damage;
            group
        };
        let beams = leaf(4_000.0, polaron);
        let frostbite = leaf(60.0, cold);

        let mut group = DamageGroup::default();
        group.damage_types.insert(polaron);
        group.damage_types.insert(cold);
        group.sub_groups.insert(beams.name(), beams);
        group.sub_groups.insert(frostbite.name(), frostbite);

        let picked: FxHashSet<String> = ["Cold".to_string()].into_iter().collect();
        let filter = TypeFilter {
            name_manager: &names,
            hits_manager: &hits_manager,
            types: &picked,
            duration: 10.0,
        };

        let kept = filter.keep(&group).expect("the group dealt some cold");
        assert_eq!(60.0, kept.total_damage.all, "the proc, not the beams");
        assert_eq!(
            1,
            kept.sub_groups.len(),
            "the beams are gone with their type"
        );
        assert_eq!(6.0, kept.dps.all, "60 damage over 10 seconds");

        // A group with none of the picked types drops out entirely.
        let tetryon: FxHashSet<String> = ["Tetryon".to_string()].into_iter().collect();
        let filter = TypeFilter {
            name_manager: &names,
            hits_manager: &hits_manager,
            types: &tetryon,
            duration: 10.0,
        };
        assert!(filter.keep(&group).is_none());
    }
}
