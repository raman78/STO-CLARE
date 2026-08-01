use super::*;
use educe::Educe;

#[derive(Clone, Copy, Debug)]
pub struct BaseHit {
    pub damage: f64,
    pub flags: ValueFlags,
    pub specific: SpecificHit,
}

#[derive(Clone, Copy, Debug, Educe)]
#[educe(Deref, DerefMut)]
pub struct Hit {
    #[educe(Deref, DerefMut)]
    pub hit: BaseHit,
    pub time_millis: u32, // offset to start of combat
}

#[derive(Clone, Copy, Debug)]
pub enum SpecificHit {
    Shield { damage_prevented_to_hull: f64 },
    ShieldDrain,
    Hull { base_damage: f64 },
}

#[derive(Clone, Debug, Default)]
pub struct DamageMetrics {
    pub hits: ShieldHullCounts,
    pub hits_per_second: ShieldHullValues,
    pub misses: u64,
    pub accuracy_percentage: Option<f64>,
    pub total_damage: ShieldHullValues,
    pub total_crit_damage: f64,
    pub total_non_crit_hull_damage: f64,
    pub total_shield_drain: f64,
    pub total_damage_prevented_to_hull_by_shields: f64,
    pub total_base_damage: f64,
    pub base_dps: f64,
    pub dps: ShieldHullValues,
    pub average_hit: ShieldHullOptionalValues,
    pub average_crit_hit: Option<f64>,
    pub average_non_crit_hull_hit: Option<f64>,
    pub critical_percentage: Option<f64>,
    pub flanking: Option<f64>,
    pub damage_resistance_percentage: Option<f64>,
    pub crits: u64,
    pub flanks: u64,
}

#[derive(Clone, Debug, Default)]
pub struct DamageMetricsDelta {
    pub hits: ShieldHullCounts,
    pub misses: u64,
    pub total_damage: ShieldHullValues,
    pub total_crit_damage: f64,
    pub total_non_crit_hull_damage: f64,
    pub total_shield_drain: f64,
    pub total_damage_prevented_to_hull_by_shields: f64,
    pub total_base_damage: f64,
    pub crits: u64,
    pub flanks: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MaxOneHit {
    pub name: NameHandle,
    pub damage: f64,
}

impl MaxOneHit {
    pub fn update_from_hits(&mut self, name: NameHandle, hits: &[Hit]) {
        hits.iter().for_each(|h| self.update(name, h.damage));
    }

    pub fn update(&mut self, name: NameHandle, damage: f64) {
        if self.damage < damage {
            self.damage = damage;
            self.name = name;
        }
    }
}

impl BaseHit {
    pub fn shield(damage: f64, flags: ValueFlags, damage_prevented_to_hull: f64) -> Self {
        Self {
            damage: damage.abs(),
            flags,
            specific: SpecificHit::Shield {
                damage_prevented_to_hull: damage_prevented_to_hull.abs(),
            },
        }
    }

    pub fn shield_drain(damage: f64, flags: ValueFlags) -> Self {
        Self {
            damage: damage.abs(),
            flags,
            specific: SpecificHit::ShieldDrain,
        }
    }

    pub fn hull(damage: f64, flags: ValueFlags, base_damage: f64) -> Self {
        Self {
            damage: damage.abs(),
            flags,
            specific: SpecificHit::Hull {
                base_damage: base_damage.abs(),
            },
        }
    }

    pub fn to_hit(self, time_millis: u32) -> Hit {
        Hit {
            hit: self,
            time_millis,
        }
    }
}

impl DamageMetrics {
    pub fn calc_and_apply_delta(&mut self, delta_hits: &[Hit]) -> DamageMetricsDelta {
        let mut delta = DamageMetricsDelta::default();

        for hit in delta_hits.iter() {
            match hit.specific {
                SpecificHit::Shield { .. } | SpecificHit::ShieldDrain => delta.hits.shield += 1,
                SpecificHit::Hull { .. } => delta.hits.hull += 1,
            }

            if hit.flags.contains(ValueFlags::IMMUNE) {
                continue;
            }

            match hit.specific {
                SpecificHit::Shield {
                    damage_prevented_to_hull,
                } => {
                    delta.total_damage.shield += hit.damage;
                    delta.total_damage_prevented_to_hull_by_shields += damage_prevented_to_hull;
                }
                SpecificHit::Hull { base_damage } => {
                    delta.total_damage.hull += hit.damage;
                    delta.total_base_damage += base_damage;
                    // Criticals are counted per hull hit only, to stay consistent
                    // with the crit metrics (crit %, average crit/non-crit hull
                    // hit), which are all hull-based. Counting shield crits here
                    // would let `crits` exceed `hits.hull` and underflow below.
                    if hit.flags.contains(ValueFlags::CRITICAL) {
                        delta.total_crit_damage += hit.damage;
                        delta.crits += 1;
                    } else {
                        delta.total_non_crit_hull_damage += hit.damage;
                    }
                }
                SpecificHit::ShieldDrain => {
                    delta.total_damage.shield += hit.damage;
                    delta.total_shield_drain += hit.damage;
                }
            }

            if hit.flags.contains(ValueFlags::FLANK) {
                delta.flanks += 1;
            }

            if hit.flags.contains(ValueFlags::MISS) {
                delta.misses += 1;
            }
        }

        delta.hits.all = delta.hits.shield + delta.hits.hull;
        delta.total_damage.all = delta.total_damage.hull + delta.total_damage.shield;

        self.apply_delta(&delta);
        delta
    }

    pub fn apply_delta(&mut self, delta: &DamageMetricsDelta) {
        self.hits += delta.hits;
        self.total_damage += delta.total_damage;
        self.total_base_damage += delta.total_base_damage;
        self.total_damage_prevented_to_hull_by_shields +=
            delta.total_damage_prevented_to_hull_by_shields;
        self.total_shield_drain += delta.total_shield_drain;
        self.total_crit_damage += delta.total_crit_damage;
        self.total_non_crit_hull_damage += delta.total_non_crit_hull_damage;
        self.crits += delta.crits;
        self.flanks += delta.flanks;
        self.misses += delta.misses;

        self.critical_percentage = percentage_u64(self.crits, self.hits.hull);

        self.flanking = percentage_u64(self.flanks, self.hits.hull);
        self.accuracy_percentage = percentage_u64(self.misses, self.hits.hull).map(|m| 100.0 - m);

        self.damage_resistance_percentage = damage_resistance_percentage(
            self.total_damage.hull,
            self.total_damage_prevented_to_hull_by_shields,
            self.total_base_damage,
        );
    }

    pub fn recalculate_time_based_metrics(&mut self, combat_duration: f64) {
        self.base_dps = self.total_base_damage / combat_duration.max(1.0);
        self.hits_per_second =
            ShieldHullValues::per_seconds(&self.hits.to_values(), combat_duration);

        self.dps = ShieldHullValues::per_seconds(&self.total_damage, combat_duration);
        self.average_hit = ShieldHullOptionalValues::average(
            &self.total_damage,
            self.hits.shield,
            self.hits.hull,
            self.hits.all,
        );

        self.average_crit_hit = average(self.total_crit_damage, self.crits);
        self.average_non_crit_hull_hit =
            average(self.total_non_crit_hull_damage, self.hits.hull - self.crits);
    }
}

/// The target's **hull** damage resistance, as a percentage.
///
/// Formula from the community reference (r/stobuilds wiki `math/log_reading`):
///
/// ```text
/// resistance = 1 - (damage prevented to hull + damage to hull) / base damage of shot
/// ```
///
/// The two halves of the numerator are what the log gives per shot: the hull
/// line carries the damage that landed plus the shot's base damage, and the
/// shield line carries how much the shield stopped from reaching the hull.
/// Together they are what the hull would have taken with no shields, so against
/// the base damage they isolate the hull's own mitigation.
///
/// Negative values are normal and mean the target was debuffed (or hit with
/// armor penetration) past zero resistance.
///
/// Deliberately **not** in the numerator:
/// - *damage dealt to shields* — shields mitigate through shield hardness, a
///   separate stat with its own formula and its own 75% cap, so folding it in
///   mixes two unrelated mechanics. This is what the figure used to do, which
///   understated resistance (measured on a real log: -35.9% instead of -56.6%).
/// - *shield drains* — resisted by DrainX, a third channel again, and their
///   records carry neither a hull component nor a base damage, so they cannot
///   enter either side of the fraction. No correction term is needed for them.
pub fn damage_resistance_percentage(
    total_hull_damage: f64,
    total_damage_prevented_to_hull_by_shields: f64,
    total_base_damage: f64,
) -> Option<f64> {
    if total_base_damage == 0.0 {
        return None;
    }

    let damage_the_hull_would_have_taken =
        total_hull_damage + total_damage_prevented_to_hull_by_shields;

    let res = 1.0 - damage_the_hull_would_have_taken / total_base_damage;
    Some(res * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: Option<f64>, expected: f64) {
        let actual = actual.expect("a resistance value");
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    /// Resistance is the *hull's* mitigation: what reached the hull plus what
    /// the shields kept off it, against the shot's base damage. The damage
    /// dealt to the shields themselves is a different mechanic (shield
    /// hardness) and must not enter the figure.
    #[test]
    fn resistance_counts_hull_damage_plus_what_shields_prevented() {
        // One shot: base 1000, the shield stopped 600 of it and took 900 doing
        // so, 200 landed on the hull. 1 - (200 + 600) / 1000 = 20%.
        let hits = [
            BaseHit::shield(900.0, ValueFlags::NONE, 600.0).to_hit(0),
            BaseHit::hull(200.0, ValueFlags::NONE, 1000.0).to_hit(0),
        ];
        let mut metrics = DamageMetrics::default();
        metrics.calc_and_apply_delta(&hits);

        assert_close(metrics.damage_resistance_percentage, 20.0);
        // The 900 dealt to the shields is deliberately absent: counting it (as
        // the figure used to) would have given 1 - 1100/1000 = -10%.
    }

    /// A shield drain is resisted by DrainX, a third channel again, and its
    /// record carries neither a hull component nor a base damage — so it can
    /// move neither side of the fraction.
    #[test]
    fn a_shield_drain_leaves_the_resistance_alone() {
        let shot = [
            BaseHit::shield(900.0, ValueFlags::NONE, 600.0).to_hit(0),
            BaseHit::hull(200.0, ValueFlags::NONE, 1000.0).to_hit(0),
        ];
        let mut without_drain = DamageMetrics::default();
        without_drain.calc_and_apply_delta(&shot);

        let mut with_drain = DamageMetrics::default();
        with_drain.calc_and_apply_delta(&shot);
        with_drain.calc_and_apply_delta(&[BaseHit::shield_drain(5000.0, ValueFlags::NONE).to_hit(0)]);

        assert_eq!(
            without_drain.damage_resistance_percentage, with_drain.damage_resistance_percentage,
            "a drain must not change the hull resistance"
        );
        // It is still counted as damage everywhere else.
        assert_eq!(5000.0, with_drain.total_shield_drain);
        assert_eq!(
            without_drain.total_damage.shield + 5000.0,
            with_drain.total_damage.shield
        );
    }

    /// With no shields in play the formula reduces to hull damage over base.
    #[test]
    fn resistance_without_shields_is_hull_damage_over_base() {
        let mut metrics = DamageMetrics::default();
        metrics.calc_and_apply_delta(&[BaseHit::hull(750.0, ValueFlags::NONE, 1000.0).to_hit(0)]);
        assert_close(metrics.damage_resistance_percentage, 25.0);
    }

    /// A debuffed target takes more than the base damage, so the figure goes
    /// negative. That is a normal reading, not an error.
    #[test]
    fn a_debuffed_target_reads_negative() {
        let mut metrics = DamageMetrics::default();
        metrics.calc_and_apply_delta(&[BaseHit::hull(1200.0, ValueFlags::NONE, 1000.0).to_hit(0)]);
        assert_close(metrics.damage_resistance_percentage, -20.0);
    }

    /// A critical hit on shields must not be counted as a hull crit: `crits`
    /// stays hull-only, so `hits.hull - crits` cannot underflow (regression test
    /// for a debug-mode panic / release-mode garbage metric).
    #[test]
    fn crits_are_counted_per_hull_hit_only() {
        let hits = [
            BaseHit::shield(100.0, ValueFlags::CRITICAL, 50.0).to_hit(0),
            BaseHit::hull(200.0, ValueFlags::CRITICAL, 180.0).to_hit(0),
            BaseHit::hull(150.0, ValueFlags::NONE, 140.0).to_hit(0),
        ];
        let mut metrics = DamageMetrics::default();
        metrics.calc_and_apply_delta(&hits);

        assert_eq!(metrics.hits.hull, 2);
        assert_eq!(metrics.hits.shield, 1);
        assert_eq!(metrics.crits, 1, "only the hull crit counts, not the shield crit");
        // Previously `hits.hull - crits` (2 - 3 with the shield crit) underflowed.
        metrics.recalculate_time_based_metrics(10.0);
        assert_eq!(metrics.average_non_crit_hull_hit, Some(150.0));
    }
}
