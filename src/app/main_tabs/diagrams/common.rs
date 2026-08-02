use std::{ops::RangeInclusive, sync::Arc};

use educe::Educe;
use eframe::egui::{TextStyle, Ui};
use egui_plot::*;

use crate::{
    analyzer::{Combat, HealTick, Hit, SpecificHealTick, SpecificHit, ValueFlags},
    helpers::number_formatting::NumberFormatter,
};

#[derive(Clone)]
pub struct PreparedDataSet<T: PreparedValue> {
    pub name: String,
    pub total_value: f64,
    pub values: Arc<[PreparedPoint<T>]>,
    pub start_time_s: f64,
    pub duration_s: f64,
}

pub type PreparedDamageDataSet = PreparedDataSet<PreparedHitValue>;
pub type PreparedHealDataSet = PreparedDataSet<PreparedHealValue>;

#[derive(Educe)]
#[educe(Deref, DerefMut)]
pub struct PreparedPoint<T: PreparedValue> {
    #[educe(Deref, DerefMut)]
    pub value: T,
    pub time_millis: u32, // offset to start of combat
}

pub type PreparedHit = PreparedPoint<PreparedHitValue>;
pub type PreparedHealTick = PreparedPoint<PreparedHealValue>;

#[derive(Clone, Copy)]
pub struct PreparedHitValue {
    pub damage: f64,
    pub hull_damage: f64,
    pub shield_damage: f64,
    pub base_damage: f64,
    pub drain_damage: f64,
    /// How much hull damage the shield stopped. Needed for the damage
    /// resistance chart, which uses the same formula as the table column.
    pub damage_prevented_to_hull: f64,
    pub hits_count: u64,
}

#[derive(Clone, Copy)]
pub struct PreparedHealValue {
    pub heal: f64,
    pub hull_heal: f64,
    pub shield_heal: f64,
    pub heals_count: u64,
    pub hull_heals_count: u64,
    pub shield_heals_count: u64,
}

/// Which halves of a heal a chart is showing. Both on by default, which sums
/// them into the single line the chart used to draw.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HealComponents {
    pub hull: bool,
    pub shield: bool,
}

impl Default for HealComponents {
    fn default() -> Self {
        Self {
            hull: true,
            shield: true,
        }
    }
}

impl HealComponents {
    /// Everything, for charts that have no component picker (all the damage
    /// ones).
    pub const ALL: Self = Self {
        hull: true,
        shield: true,
    };
}

pub trait PreparedValue: Clone + 'static {
    fn value(&self, diagram_type: DiagramType, components: HealComponents) -> f64;
    fn merge(&mut self, other: &Self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagramType {
    Dps,
    Damage,
    HitsPerSecond,
    HitsCount,
    Heal,
    Hps,
    HealTicksPerSecond,
    HealTicksCount,
    DamageResistance,
}

impl DiagramType {
    pub const fn name(&self) -> &'static str {
        match self {
            DiagramType::Dps => "DPS",
            DiagramType::Damage => "Damage",
            DiagramType::HitsPerSecond => "Hits per Second",
            DiagramType::HitsCount => "Hits count",
            DiagramType::Heal => "Heal",
            DiagramType::Hps => "HPS",
            DiagramType::HealTicksPerSecond => "Heal Ticks per Second",
            DiagramType::HealTicksCount => "Heal Ticks count",
            DiagramType::DamageResistance => "Damage Resistance",
        }
    }

    pub const fn value_name(&self) -> &'static str {
        match self {
            DiagramType::Dps => "DPS",
            DiagramType::Damage => "Damage",
            DiagramType::HitsPerSecond => "Hits per Second",
            DiagramType::HitsCount => "Hits count",
            DiagramType::Heal => "Heal",
            DiagramType::Hps => "HPS",
            DiagramType::HealTicksPerSecond => "Ticks per Second",
            DiagramType::HealTicksCount => "Ticks count",
            DiagramType::DamageResistance => "%",
        }
    }

    pub const fn tooltip(&self) -> &'static str {
        match self {
            DiagramType::Dps => {
                "Shows Damage Per Second (DPS) with an applied gauss filter (meaning the lines gets smoothed out)."
            }
            DiagramType::Damage => "Shows Damage amount for a given time slice.",
            DiagramType::HitsPerSecond => {
                "Shows Hits per Second with an applied gauss filter (meaning the lines gets smoothed out).\nNote that every damage number that shows up, counts as one hit.\nThis means for an attack, that hits the shields of an enemy, 2 Hits will be counted. One for the shield Hit and one for the hull Hit."
            }
            DiagramType::HitsCount => {
                "Shows outgoing Hits count for a given time slice.\nNote that every damage number that shows up, counts as one hit.\nThis means for an attack, that hits the shields of an enemy, 2 Hits will be counted. One for the shield Hit and one for the hull Hit."
            }
            DiagramType::Heal => "Shows Heal amount for a given time slice.",
            DiagramType::Hps => {
                "Shows Heal amount Per Second (HPS) with an applied gauss filter (meaning the lines gets smoothed out)."
            }
            DiagramType::HealTicksPerSecond => {
                "Shows Heal Ticks per Second with an applied gauss filter (meaning the lines gets smoothed out)."
            }
            DiagramType::HealTicksCount => "Shows Heal Ticks count for a given time slice.",
            DiagramType::DamageResistance => {
                "Shows how much damage resistance was present for given time slice."
            }
        }
    }
}

impl<T: PreparedValue> PreparedDataSet<T> {
    /// `combat_duration_s` is how long the combat ran, from its first log
    /// record to its last. Every series is anchored to that window rather than
    /// to its own first value, so a player who only started healing a minute in
    /// still draws from the start of the fight, and several series on one chart
    /// share a time base and bucket boundaries.
    pub fn base_new(
        name: &str,
        total_value: f64,
        values: impl Iterator<Item = impl Into<PreparedPoint<T>>>,
        combat_duration_s: f64,
    ) -> Self {
        let mut values = Vec::from_iter(values.map(|h| h.into()));
        values.sort_unstable_by_key(|h| h.time_millis);
        values.dedup_by(|h1, h2| {
            if h1.time_millis != h2.time_millis {
                return false;
            }

            h2.merge(h1);
            true
        });

        // Values carry an offset from the start of the combat, so the combat
        // starts at 0 by construction. Fall back to the series' own extent when
        // no duration is known, and never end before the last value.
        let end_time_s = values.iter().map(|h| h.time_millis).max().unwrap_or(0) as f64 / 1e3;

        Self {
            name: name.to_string(),
            total_value,
            values: Arc::from(values),
            start_time_s: 0.0,
            duration_s: combat_duration_s.max(end_time_s),
        }
    }
}

impl PreparedDamageDataSet {
    pub fn new<'a>(
        name: &str,
        total_damage: f64,
        hits: impl Iterator<Item = &'a Hit>,
        combat_duration_s: f64,
    ) -> Self {
        Self::base_new(
            name,
            total_damage,
            hits.filter(|h| !h.flags.contains(ValueFlags::IMMUNE)),
            combat_duration_s,
        )
    }
}

impl PreparedHealDataSet {
    pub fn new<'a>(
        name: &str,
        total_heal: f64,
        ticks: impl Iterator<Item = &'a HealTick>,
        combat_duration_s: f64,
    ) -> Self {
        Self::base_new(name, total_heal, ticks, combat_duration_s)
    }
}

/// How long a combat ran, first record to last, in seconds. Charts use it as
/// their x range so every series covers the whole fight.
pub fn combat_duration_seconds(combat: &Combat) -> f64 {
    combat
        .active_time
        .end
        .signed_duration_since(combat.active_time.start)
        .num_milliseconds()
        .max(0) as f64
        / 1e3
}

impl<'a> From<&'a Hit> for PreparedHit {
    fn from(hit: &'a Hit) -> Self {
        match hit.specific {
            SpecificHit::Shield {
                damage_prevented_to_hull,
            } => Self {
                value: PreparedHitValue {
                    damage: hit.damage,
                    shield_damage: hit.damage,
                    hull_damage: 0.0,
                    base_damage: 0.0,
                    drain_damage: 0.0,
                    damage_prevented_to_hull,
                    hits_count: 1,
                },
                time_millis: hit.time_millis,
            },
            SpecificHit::ShieldDrain => Self {
                value: PreparedHitValue {
                    damage: hit.damage,
                    shield_damage: hit.damage,
                    hull_damage: 0.0,
                    base_damage: 0.0,
                    drain_damage: hit.damage,
                    damage_prevented_to_hull: 0.0,
                    hits_count: 1,
                },
                time_millis: hit.time_millis,
            },
            SpecificHit::Hull { base_damage } => Self {
                value: PreparedHitValue {
                    damage: hit.damage,
                    shield_damage: 0.0,
                    hull_damage: hit.damage,
                    base_damage,
                    drain_damage: 0.0,
                    damage_prevented_to_hull: 0.0,
                    hits_count: 1,
                },
                time_millis: hit.time_millis,
            },
        }
    }
}

impl PreparedValue for PreparedHitValue {
    fn merge(&mut self, other: &Self) {
        self.damage += other.damage;
        self.shield_damage += other.shield_damage;
        self.hull_damage += other.hull_damage;
        self.base_damage += other.base_damage;
        self.drain_damage += other.drain_damage;
        self.damage_prevented_to_hull += other.damage_prevented_to_hull;
        self.hits_count += other.hits_count;
    }

    fn value(&self, diagram_type: DiagramType, _: HealComponents) -> f64 {
        match diagram_type {
            DiagramType::Dps => self.damage,
            DiagramType::Damage => self.damage,
            DiagramType::HitsPerSecond => self.hits_count as _,
            DiagramType::HitsCount => self.hits_count as _,
            _ => unreachable!(),
        }
    }
}

impl<'a> From<&'a HealTick> for PreparedHealTick {
    fn from(tick: &'a HealTick) -> Self {
        Self {
            value: match tick.specific {
                SpecificHealTick::Hull => PreparedHealValue {
                    heal: tick.amount,
                    hull_heal: tick.amount,
                    shield_heal: 0.0,
                    heals_count: 1,
                    hull_heals_count: 1,
                    shield_heals_count: 0,
                },
                SpecificHealTick::Shield => PreparedHealValue {
                    heal: tick.amount,
                    hull_heal: 0.0,
                    shield_heal: tick.amount,
                    heals_count: 1,
                    hull_heals_count: 0,
                    shield_heals_count: 1,
                },
            },
            time_millis: tick.time_millis,
        }
    }
}

impl PreparedValue for PreparedHealValue {
    fn merge(&mut self, other: &Self) {
        self.heal += other.heal;
        self.hull_heal += other.hull_heal;
        self.shield_heal += other.shield_heal;
        self.heals_count += other.heals_count;
        self.hull_heals_count += other.hull_heals_count;
        self.shield_heals_count += other.shield_heals_count;
    }

    fn value(&self, diagram_type: DiagramType, components: HealComponents) -> f64 {
        // With both halves on this is the plain total, so the default chart is
        // unchanged; turning one off drops its share of the line.
        let heal = match (components.hull, components.shield) {
            (true, true) => self.heal,
            (true, false) => self.hull_heal,
            (false, true) => self.shield_heal,
            (false, false) => 0.0,
        };
        let ticks = match (components.hull, components.shield) {
            (true, true) => self.heals_count,
            (true, false) => self.hull_heals_count,
            (false, true) => self.shield_heals_count,
            (false, false) => 0,
        };
        match diagram_type {
            DiagramType::Heal | DiagramType::Hps => heal,
            DiagramType::HealTicksPerSecond | DiagramType::HealTicksCount => ticks as _,
            _ => unreachable!(),
        }
    }
}

pub fn seconds_to_millis(seconds: f64) -> u32 {
    (seconds * 1e3).round() as _
}

pub fn millis_to_seconds(millis: u32) -> f64 {
    millis as f64 * (1.0 / 1e3)
}

/// Widest y axis label a chart reserves room for: `1'000'000`, nine characters
/// with the thousands marks. A per-second or per-slice figure does not
/// realistically go past a few million.
const WIDEST_Y_LABEL_CHARS: f32 = 9.0;

/// Room to keep for the y axis labels, so the plot area starts at the same
/// place on every chart and stops sliding sideways when the numbers change
/// magnitude — switching a healing chart between hull and shield moves them by
/// an order of magnitude, and the whole plot used to jump with them.
///
/// Measured from the font in use rather than fixed, so it holds at any UI
/// scale. It is a *minimum*: a label that needs more still gets it.
pub fn y_axis_width(ui: &Ui) -> f32 {
    let digit = ui.fonts_mut(|fonts| fonts.glyph_width(&TextStyle::Body.resolve(ui.style()), '0'));
    digit * WIDEST_Y_LABEL_CHARS + ui.spacing().item_spacing.x * 2.0
}

pub fn format_axis(mark: GridMark, _: &RangeInclusive<f64>) -> String {
    if mark.value < 0.0 {
        return String::new();
    }
    let mut formatter = NumberFormatter::new();
    formatter.format(mark.value, 0)
}

pub fn format_element(bar: &Bar, _: &BarChart, more_decimals: bool) -> String {
    let mut formatter = NumberFormatter::new();
    if bar.name.is_empty() {
        return format!(
            "{}",
            formatter.format(bar.value, if more_decimals { 2 } else { 0 })
        );
    }
    format!(
        "{}\n{}",
        bar.name,
        formatter.format(bar.value, if more_decimals { 2 } else { 0 })
    )
}

pub fn time_slices<'a, T: PreparedValue>(
    data: &'a PreparedDataSet<T>,
    time_slice: f64,
) -> impl Iterator<Item = (f64, &'a [PreparedPoint<T>])> + 'a {
    let time_slice_m = seconds_to_millis(time_slice);
    // Round the series start down to a bucket boundary. This used to divide
    // without multiplying back, mixing a bucket index into a millisecond value,
    // which shifted every bucket centre by start/time_slice milliseconds and
    // emitted a run of empty buckets before the data.
    let first_time_slice = (seconds_to_millis(data.start_time_s) / time_slice_m) * time_slice_m;
    let last_time_m = seconds_to_millis(data.start_time_s + data.duration_s);
    let mut time_slice_end = first_time_slice + time_slice_m;
    let mut values = &*data.values;
    let sliced_values = std::iter::from_fn(move || {
        // Keep emitting (empty) buckets to the end of the combat, so a series
        // that stops early still shows the rest of the fight as nothing.
        if values.len() == 0 && time_slice_end > last_time_m + time_slice_m {
            return None;
        }
        let slice_end = values
            .iter()
            .take_while(|v| v.time_millis < time_slice_end)
            .count();
        let slice = &values[0..slice_end];
        let center = millis_to_seconds(time_slice_end - time_slice_m / 2);
        values = &values[slice_end..];
        time_slice_end += time_slice_m;
        Some((center, slice))
    });

    sliced_values
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heal_series(times_s: &[u32], combat_duration_s: f64) -> PreparedHealDataSet {
        let values: Vec<PreparedHealTick> = times_s
            .iter()
            .map(|s| PreparedPoint {
                value: PreparedHealValue {
                    heal: 100.0,
                    hull_heal: 100.0,
                    shield_heal: 0.0,
                    heals_count: 1,
                    hull_heals_count: 1,
                    shield_heals_count: 0,
                },
                time_millis: s * 1000,
            })
            .collect();
        PreparedDataSet::base_new(
            "test",
            100.0 * times_s.len() as f64,
            values.into_iter(),
            combat_duration_s,
        )
    }

    /// A series whose first value lands well into the fight still starts at the
    /// fight's start. Healing typically begins later than shooting, which is how
    /// this showed up: the HPS chart began at the first heal instead of at 0.
    #[test]
    fn a_series_starts_at_the_combat_start_not_at_its_first_value() {
        let data = heal_series(&[40, 41, 42], 60.0);
        assert_eq!(0.0, data.start_time_s);
        assert_eq!(60.0, data.duration_s);
    }

    /// ...and it runs to the end of the fight, not to its own last value.
    #[test]
    fn a_series_runs_to_the_combat_end() {
        let data = heal_series(&[5, 6], 90.0);
        assert_eq!(90.0, data.duration_s);
    }

    /// A combat length that is somehow shorter than the data never truncates it.
    #[test]
    fn the_series_extent_wins_over_a_too_short_combat() {
        let data = heal_series(&[10, 120], 30.0);
        assert_eq!(120.0, data.duration_s);
    }

    /// With both halves on the chart draws the plain total, which is what it
    /// drew before the picker existed; each half on its own drops the other.
    #[test]
    fn the_component_picker_selects_which_halves_add_up() {
        let value = PreparedHealValue {
            heal: 300.0,
            hull_heal: 200.0,
            shield_heal: 100.0,
            heals_count: 3,
            hull_heals_count: 2,
            shield_heals_count: 1,
        };
        let both = HealComponents {
            hull: true,
            shield: true,
        };
        let hull = HealComponents {
            hull: true,
            shield: false,
        };
        let shield = HealComponents {
            hull: false,
            shield: true,
        };

        assert_eq!(300.0, value.value(DiagramType::Hps, both));
        assert_eq!(200.0, value.value(DiagramType::Hps, hull));
        assert_eq!(100.0, value.value(DiagramType::Hps, shield));

        assert_eq!(3.0, value.value(DiagramType::HealTicksCount, both));
        assert_eq!(2.0, value.value(DiagramType::HealTicksCount, hull));
        assert_eq!(1.0, value.value(DiagramType::HealTicksCount, shield));
    }

    /// A tick carries its own half, so merging keeps the two apart.
    #[test]
    fn merging_ticks_keeps_the_halves_apart() {
        let mut hull = PreparedHealValue {
            heal: 10.0,
            hull_heal: 10.0,
            shield_heal: 0.0,
            heals_count: 1,
            hull_heals_count: 1,
            shield_heals_count: 0,
        };
        let shield = PreparedHealValue {
            heal: 4.0,
            hull_heal: 0.0,
            shield_heal: 4.0,
            heals_count: 1,
            hull_heals_count: 0,
            shield_heals_count: 1,
        };
        hull.merge(&shield);

        assert_eq!(14.0, hull.heal);
        assert_eq!(10.0, hull.hull_heal);
        assert_eq!(4.0, hull.shield_heal);
    }

    /// Buckets start at the combat start and land on whole multiples of the
    /// slice. The bucket boundary used to be computed from a bucket index that
    /// was added to a millisecond value, which offset every centre and emitted a
    /// run of empty buckets before the data.
    #[test]
    fn buckets_are_aligned_to_the_slice_and_cover_the_fight() {
        let data = heal_series(&[3, 4], 6.0);
        let slices: Vec<(f64, usize)> =
            time_slices(&data, 1.0).map(|(c, s)| (c, s.len())).collect();

        assert_eq!(
            vec![0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5],
            slices.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            "one bucket per second of the fight, centred on the half second"
        );
        assert_eq!(
            2,
            slices.iter().map(|(_, n)| n).sum::<usize>(),
            "both values are accounted for exactly once"
        );
        assert_eq!(
            (1, 1),
            (slices[3].1, slices[4].1),
            "the values sit in the buckets covering 3 s and 4 s"
        );
    }
}
