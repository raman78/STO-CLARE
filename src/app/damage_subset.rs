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
        AnalysisGroup, DamageGroup, DamageMetrics, HealGroup, HealMetrics, HealTick, HealTicks,
        HealTicksManager, Hit, Hits, HitsManager, MaxOneHit, NameHandle, NameManager, Player,
        ShieldHullOptionalValues, ShieldHullValues,
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
    hits_manager: &HitsManager,
    excluded: &FxHashSet<NameHandle>,
    combat_total: &ShieldHullValues,
) -> DamageGroup {
    let kept_hits = group
        .sub_groups()
        .values()
        .filter(|sub| !excluded.contains(&sub.name()))
        .flat_map(|sub| sub.hits.get(hits_manager).iter().copied())
        .collect();
    let mut kept = subset_group(
        kept_hits,
        metrics_duration(&player.combat_time),
        combat_total,
    );
    // The row keeps its name and the rows under it: the reader is looking at
    // the same player, with part of their tree set aside, and a nameless row
    // that cannot be opened is not that.
    kept.segment = group.segment;
    // Every row stays under it, the ticked and the unticked alike. They are
    // what the ticks are of: dropping the unticked ones would take their tick
    // boxes with them, and the row above would lose the count it needs to say
    // whether all, some or none of them are in.
    kept.sub_groups = group.sub_groups().clone();
    kept
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

/// A player's healing with the given rows left out, by group handle; zeroes
/// when every row is.
///
/// The heal side of [`player_damage_without`], and for the same reason: an
/// average heal and a crit rate are ratios of tick counts, so they have to be
/// worked out from the ticks that are left rather than subtracted.
pub fn player_heal_without(
    player: &Player,
    group: &HealGroup,
    ticks_manager: &HealTicksManager,
    excluded: &FxHashSet<NameHandle>,
    pool_total: &ShieldHullValues,
) -> HealGroup {
    let kept_ticks: Vec<HealTick> = group
        .sub_groups()
        .values()
        .filter(|sub| !excluded.contains(&sub.name()))
        .flat_map(|sub| sub.ticks.get(ticks_manager).iter().cloned())
        .collect();

    let mut heal_metrics = HealMetrics::default();
    heal_metrics.calc_and_apply(&kept_ticks);
    // Healing is measured against the time anything happened, which is what
    // `Player::recalculate_metrics` divides the heal pools by.
    heal_metrics.recalculate_time_based_metrics(metrics_duration(&player.active_time));

    let mut kept = HealGroup {
        heal_percentage: ShieldHullOptionalValues::percentage(&heal_metrics.total_heal, pool_total),
        heal_metrics,
        ticks: HealTicks::Leaf(kept_ticks),
        ..Default::default()
    };
    kept.segment = group.segment;
    // Every row stays under it, the ticked and the unticked alike. They are
    // what the ticks are of: dropping the unticked ones would take their tick
    // boxes with them, and the row above would lose the count it needs to say
    // whether all, some or none of them are in.
    kept.sub_groups = group.sub_groups().clone();
    kept
}

/// The values of the rows that are kept, pooled — or `None` when none of them
/// are here. The same rule as [`subset_hits`], for ticks as well as hits.
pub fn subset_values<'a, V: Clone + 'a>(
    rows: impl Iterator<Item = (&'a str, &'a [V])>,
    excluded: &FxHashSet<String>,
) -> Option<Vec<V>> {
    let kept: Vec<&[V]> = rows
        .filter(|(name, _)| !excluded.contains(*name))
        .map(|(_, values)| values)
        .collect();
    if kept.is_empty() {
        return None;
    }
    Some(kept.into_iter().flatten().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{BaseHit, HealTick, NameFlags, SpecificHit, ValueFlags};

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

    fn crit(damage: f64) -> Hit {
        Hit {
            hit: BaseHit {
                damage,
                flags: ValueFlags::CRITICAL,
                specific: SpecificHit::Hull {
                    base_damage: damage,
                },
            },
            time_millis: 0,
        }
    }

    /// Every row kept is the whole group again — to the last digit of every
    /// figure, not only the total. This is the property the whole feature rests
    /// on: a subset is worked out the way the analyzer works out a branch, so
    /// the subset of everything has to equal what the analyzer said.
    #[test]
    fn keeping_every_row_gives_the_analyzer_s_own_figures() {
        let hits = vec![hit(100.0), crit(300.0), hit(50.0)];
        let whole = subset_group(hits.clone(), 10.0, &combat_total(1_000.0));
        let kept = subset_group(
            subset_hits(
                [("Beams", &hits[..2]), ("Torpedo", &hits[2..])].into_iter(),
                &Default::default(),
            )
            .unwrap(),
            10.0,
            &combat_total(1_000.0),
        );

        assert_eq!(whole.total_damage.all, kept.total_damage.all);
        assert_eq!(whole.dps.all, kept.dps.all);
        assert_eq!(whole.damage_metrics.hits.all, kept.damage_metrics.hits.all);
        assert_eq!(whole.critical_percentage, kept.critical_percentage);
        assert_eq!(whole.average_hit.all, kept.average_hit.all);
        assert_eq!(whole.max_one_hit.damage, kept.max_one_hit.damage);
        assert_eq!(whole.damage_percentage.all, kept.damage_percentage.all);
    }

    /// Taking a row out takes exactly that row out: what is left equals the
    /// same rows counted on their own, and the two halves add back up to the
    /// whole. Nothing is lost between them and nothing is counted twice.
    #[test]
    fn what_is_left_plus_what_was_taken_is_the_whole() {
        let beams = [hit(100.0), crit(300.0)];
        let torpedo = [hit(50.0)];
        let rows = || [("Beams", beams.as_slice()), ("Torpedo", torpedo.as_slice())].into_iter();
        let total = combat_total(1_000.0);

        let whole = subset_group(
            subset_hits(rows(), &Default::default()).unwrap(),
            10.0,
            &total,
        );
        let without_torpedo = subset_group(
            subset_hits(rows(), &excluded(&["Torpedo"])).unwrap(),
            10.0,
            &total,
        );
        let only_torpedo = subset_group(
            subset_hits(rows(), &excluded(&["Beams"])).unwrap(),
            10.0,
            &total,
        );

        assert_eq!(400.0, without_torpedo.total_damage.all);
        assert_eq!(50.0, only_torpedo.total_damage.all);
        assert_eq!(
            whole.total_damage.all,
            without_torpedo.total_damage.all + only_torpedo.total_damage.all,
            "the two parts add back up to the whole"
        );
        assert_eq!(
            whole.damage_metrics.hits.all,
            without_torpedo.damage_metrics.hits.all + only_torpedo.damage_metrics.hits.all
        );
        assert_eq!(
            whole.dps.all,
            without_torpedo.dps.all + only_torpedo.dps.all,
            "DPS is additive over the same duration"
        );
    }

    /// A ratio is of what is left, not of what there was: one crit in two hits
    /// is 50%, and taking the crit out makes it 0% rather than leaving it where
    /// it was. This is why a subset cannot be a subtraction.
    #[test]
    fn a_ratio_follows_the_rows_that_are_left() {
        let crits = [crit(300.0)];
        let plain = [hit(100.0)];
        let rows = || [("Crits", crits.as_slice()), ("Plain", plain.as_slice())].into_iter();
        let total = combat_total(1_000.0);

        let whole = subset_group(
            subset_hits(rows(), &Default::default()).unwrap(),
            10.0,
            &total,
        );
        assert_eq!(Some(50.0), whole.critical_percentage);

        let without_crits = subset_group(
            subset_hits(rows(), &excluded(&["Crits"])).unwrap(),
            10.0,
            &total,
        );
        assert_eq!(Some(0.0), without_crits.critical_percentage);
        assert_eq!(Some(100.0), without_crits.average_hit.all);
    }

    /// The share of the fight follows the subset as well: half the damage of a
    /// run is half of what that run was of the fight.
    #[test]
    fn the_share_of_the_fight_follows_the_subset() {
        let a = [hit(500.0)];
        let b = [hit(500.0)];
        let rows = || [("A", a.as_slice()), ("B", b.as_slice())].into_iter();
        let total = combat_total(2_000.0);

        let whole = subset_group(
            subset_hits(rows(), &Default::default()).unwrap(),
            10.0,
            &total,
        );
        let half = subset_group(
            subset_hits(rows(), &excluded(&["B"])).unwrap(),
            10.0,
            &total,
        );

        assert_eq!(Some(50.0), whole.damage_percentage.all);
        assert_eq!(Some(25.0), half.damage_percentage.all);
    }

    fn combat_total(all: f64) -> ShieldHullValues {
        ShieldHullValues {
            all,
            shield: 0.0,
            hull: all,
        }
    }

    fn excluded(names: &[&str]) -> FxHashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn tick(amount: f64, crit: bool) -> HealTick {
        use crate::analyzer::{BaseHealTick, SpecificHealTick};
        HealTick {
            tick: BaseHealTick {
                amount,
                flags: if crit {
                    ValueFlags::CRITICAL
                } else {
                    ValueFlags::NONE
                },
                specific: SpecificHealTick::Hull,
            },
            time_millis: 0,
        }
    }

    /// Healing answers the same way damage does: the kept rows' ticks are what
    /// the figures are worked out from, so the parts add back up to the whole
    /// and a crit rate is of what is left.
    #[test]
    fn heal_ticks_are_pooled_the_same_way_hits_are() {
        let big = [tick(300.0, true)];
        let small = [tick(100.0, false)];
        let rows = || [("Big", big.as_slice()), ("Small", small.as_slice())].into_iter();

        let whole = subset_values(rows(), &Default::default()).unwrap();
        assert_eq!(2, whole.len());

        let kept = subset_values(rows(), &excluded(&["Big"])).unwrap();
        assert_eq!(1, kept.len());
        assert_eq!(100.0, kept[0].amount);

        let mut metrics = HealMetrics::default();
        metrics.calc_and_apply(&kept);
        metrics.recalculate_time_based_metrics(10.0);
        assert_eq!(100.0, metrics.total_heal.all);
        assert_eq!(10.0, metrics.hps.all, "100 over ten seconds");
        assert_eq!(
            Some(0.0),
            metrics.critical_percentage,
            "the crit went with the row it was in"
        );

        // Nothing of this player's kept: `None`, not a zero.
        assert!(subset_values(rows(), &excluded(&["Big", "Small"])).is_none());
    }

    /// Every row unticked leaves zeroes, not the figures the player started
    /// with: the reader asked for none of it, and answering with the whole
    /// would read as a control that does nothing.
    #[test]
    fn unticking_every_row_leaves_zeroes() {
        let all = [hit(100.0), hit(300.0)];
        let rows = [("Beams", all.as_slice())].into_iter();
        assert!(subset_hits(rows, &excluded(&["Beams"])).is_none());

        let empty = subset_group(Vec::new(), 10.0, &combat_total(1_000.0));
        assert_eq!(0.0, empty.total_damage.all);
        assert_eq!(0.0, empty.dps.all);
        assert_eq!(0, empty.damage_metrics.hits.all);
        assert_eq!(None, empty.critical_percentage, "no hits, no rate");
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
