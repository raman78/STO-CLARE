use educe::Educe;

use super::*;

/// One healing pool, kept in **both** grouping orders so the heal tabs can flip
/// between them instantly instead of re-parsing the log.
///
/// - `by_person` — person → ability: who healed you (or who you healed), and
///   with what. The order the heal tabs open in.
/// - `by_ability` — ability → person: what was used, and on/from whom. The
///   order the damage tabs use.
///
/// Both hold the same ticks, so any total is identical between them; only the
/// nesting differs. Deref goes to `by_person`, so pool-level totals read as
/// `pool.total_heal` regardless of the order shown.
#[derive(Clone, Debug, Educe)]
#[educe(Deref, DerefMut)]
pub struct HealPool {
    #[educe(Deref, DerefMut)]
    pub by_person: HealGroup,
    pub by_ability: HealGroup,
}

/// Which nesting a heal tab is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealGrouping {
    ByPerson,
    ByAbility,
}

impl HealPool {
    pub(super) fn new(name: NameHandle) -> Self {
        Self {
            by_person: HealGroup::new_branch(GroupPathSegment::Group(name)),
            by_ability: HealGroup::new_branch(GroupPathSegment::Group(name)),
        }
    }

    pub(super) fn recalculate_metrics(
        &mut self,
        duration: f64,
        ticks_manager: &mut HealTicksManager,
    ) {
        self.by_person
            .recalculate_metrics(duration, ticks_manager, &mut |_| {});
        self.by_ability
            .recalculate_metrics(duration, ticks_manager, &mut |_| {});
    }

    pub(super) fn recalculate_percentages(
        &mut self,
        total_heal: &ShieldHullValues,
        parent_ticks: &ShieldHullCounts,
    ) {
        self.by_person
            .recalculate_percentages(total_heal, parent_ticks);
        self.by_ability
            .recalculate_percentages(total_heal, parent_ticks);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BaseHealTick {
    pub amount: f64,
    pub flags: ValueFlags,
    pub specific: SpecificHealTick,
}

#[derive(Clone, Copy, Debug, Educe)]
#[educe(Deref, DerefMut)]
pub struct HealTick {
    #[educe(Deref, DerefMut)]
    pub tick: BaseHealTick,
    pub time_millis: u32, // offset to start of combat
}

#[derive(Clone, Copy, Debug)]
pub enum SpecificHealTick {
    Shield,
    Hull,
}

#[derive(Clone, Default, Debug)]
pub struct HealMetrics {
    pub ticks: ShieldHullCounts,
    pub ticks_per_second: ShieldHullValues,
    pub total_heal: ShieldHullValues,
    pub hps: ShieldHullValues,
    pub average_heal: ShieldHullOptionalValues,
    pub critical_percentage: Option<f64>,
    pub crits: u64,
}

#[derive(Clone, Default, Debug)]
pub struct HealMetricsDelta {
    pub ticks: ShieldHullCounts,
    pub total_heal: ShieldHullValues,
    pub crits: u64,
}

impl BaseHealTick {
    pub fn shield(amount: f64, flags: ValueFlags) -> Self {
        Self {
            amount: amount.abs(),
            flags,
            specific: SpecificHealTick::Shield,
        }
    }

    pub fn hull(amount: f64, flags: ValueFlags) -> Self {
        Self {
            amount: amount.abs(),
            flags,
            specific: SpecificHealTick::Hull,
        }
    }

    pub fn to_tick(self, time_millis: u32) -> HealTick {
        HealTick {
            tick: self,
            time_millis,
        }
    }
}

impl HealMetrics {
    pub fn calc_and_apply(&mut self, delta_ticks: &[HealTick]) -> HealMetricsDelta {
        let mut delta = HealMetricsDelta::default();

        for tick in delta_ticks.iter() {
            match tick.specific {
                SpecificHealTick::Shield => {
                    delta.ticks.shield += 1;
                    delta.total_heal.shield += tick.amount;
                }
                SpecificHealTick::Hull => {
                    delta.ticks.hull += 1;
                    delta.total_heal.hull += tick.amount;
                }
            }

            if tick.flags.contains(ValueFlags::CRITICAL) {
                delta.crits += 1;
            }
        }

        delta.ticks.all = delta.ticks.shield + delta.ticks.hull;
        delta.total_heal.all = delta.total_heal.shield + delta.total_heal.hull;

        self.apply_delta(&delta);

        delta
    }

    pub fn apply_delta(&mut self, delta: &HealMetricsDelta) {
        self.ticks += delta.ticks;
        self.total_heal += delta.total_heal;
        self.crits += delta.crits;

        self.average_heal = ShieldHullOptionalValues::average(
            &self.total_heal,
            self.ticks.shield,
            self.ticks.hull,
            self.ticks.all,
        );

        self.critical_percentage = percentage_u64(self.crits, self.ticks.hull);
    }

    pub fn recalculate_time_based_metrics(&mut self, active_duration: f64) {
        self.ticks_per_second =
            ShieldHullValues::per_seconds(&self.ticks.to_values(), active_duration);

        self.hps = ShieldHullValues::per_seconds(&self.total_heal, active_duration);
    }
}
