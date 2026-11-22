mod activity_chart;
mod common;
mod damage_resistance_chart;
mod summary_chart;
mod value_per_second_graph;
mod values_chart;

pub use crate::app::main_tabs::diagrams::common::DiagramType;
use crate::app::settings::Settings;
pub use activity_chart::{
    ActiveText, ActiveTime, DEFAULT_ACTIVITY_GAP_S, active_time, series_windows,
};
pub use common::HealComponents;
pub use common::PreparedDamageDataSet;
pub use common::PreparedHealDataSet;
// The compare view builds its averaged series point by point, because averaging
// scales the hit count as well as the damage and a hit carries a whole count.
pub use common::PreparedHit;
pub use common::combat_duration_seconds;
use eframe::egui::Ui;
use itertools::Itertools;
pub use summary_chart::SummaryChart;
pub use value_per_second_graph::ValuePerSecondGraph;

use crate::analyzer::*;

use self::{
    activity_chart::*, damage_resistance_chart::*, value_per_second_graph::*, values_chart::*,
};

pub struct DamageDiagrams {
    dps_graph: DpsGraph,
    damage_chart: DamageChart,
    damage_resistance_chart: DamageResistanceChart,
    hits_per_second_graph: HitsPerSecondGraph,
    hits_count_chart: HitsChart,
    activity_chart: DamageActivityChart,
}

pub struct HealDiagrams {
    hps_graph: HpsGraph,
    heal_chart: HealChart,
    ticks_per_second: HealTicksPerSeondGraph,
    ticks_count_chart: HealTicksCountChart,
    activity_chart: HealActivityChart,
}

impl DamageDiagrams {
    pub fn empty() -> Self {
        Self {
            dps_graph: ValuePerSecondGraph::empty(DiagramType::Dps),
            damage_chart: ValuesChart::empty(DiagramType::Damage),
            damage_resistance_chart: DamageResistanceChart::empty(),
            hits_per_second_graph: HitsPerSecondGraph::empty(DiagramType::HitsPerSecond),
            hits_count_chart: HitsChart::empty(DiagramType::HitsCount),
            activity_chart: ActivityChart::empty(DiagramType::HitsCount),
        }
    }

    pub fn from_damage_groups<'a>(
        groups: impl Iterator<Item = &'a DamageGroup>,
        combat: &Combat,
        // The time each player's Active share is of, by the player's name.
        active_base_s: impl Fn(&str) -> f64,
        filter: f64,
        damage_time_slice: f64,
        activity_gap_s: f64,
    ) -> Self {
        let combat_duration_s = combat_duration_seconds(combat);
        let data = groups.map(|g| {
            let name = g.name().get(&combat.name_manager);
            PreparedDamageDataSet::new(
                name,
                g.total_damage.all,
                g.hits.get(&combat.hits_manger).iter(),
                combat_duration_s,
            )
            .with_active_base(active_base_s(name))
        });

        Self::from_data(data, filter, damage_time_slice, activity_gap_s)
    }

    pub fn from_data(
        data: impl Iterator<Item = PreparedDamageDataSet>,
        filter: f64,
        damage_time_slice: f64,
        activity_gap_s: f64,
    ) -> Self {
        let data = data.collect_vec();
        Self {
            dps_graph: DpsGraph::from_data(DiagramType::Dps, data.iter().cloned(), filter),
            damage_chart: DamageChart::from_data(
                DiagramType::Damage,
                data.iter().cloned(),
                damage_time_slice,
            ),
            damage_resistance_chart: DamageResistanceChart::from_data(
                data.iter().cloned(),
                damage_time_slice,
            ),
            hits_per_second_graph: HitsPerSecondGraph::from_data(
                DiagramType::HitsPerSecond,
                data.iter().cloned(),
                filter,
            ),
            hits_count_chart: HitsChart::from_data(
                DiagramType::HitsCount,
                data.iter().cloned(),
                damage_time_slice,
            ),
            activity_chart: ActivityChart::from_data(
                DiagramType::HitsCount,
                data.into_iter(),
                activity_gap_s,
            ),
        }
    }

    pub fn add_data(
        &mut self,
        data: PreparedDamageDataSet,
        filter: f64,
        time_slice: f64,
        activity_gap_s: f64,
    ) {
        self.activity_chart.add_lane(data.clone(), activity_gap_s);
        self.dps_graph.add_line(data.clone(), filter);
        self.damage_chart.add_bars(data.clone(), time_slice);
        self.damage_resistance_chart
            .add_bars(data.clone(), time_slice);
        self.hits_per_second_graph.add_line(data.clone(), filter);
        self.hits_count_chart.add_bars(data, time_slice);
    }

    /// The row the series are the parts of, for the Activity chart's Total:
    /// a chart opened on a row draws, with Total on, that row's own lane.
    pub fn with_activity_whole(mut self, data: PreparedDamageDataSet) -> Self {
        self.activity_chart.set_whole(data);
        self
    }

    /// The row a chart opened on a row's parts was opened on, if it was.
    pub fn activity_whole(&self) -> Option<PreparedDamageDataSet> {
        self.activity_chart.whole()
    }

    pub fn remove_data(&mut self, data: &str) {
        self.dps_graph.remove_line(data);
        self.damage_chart.remove_bars(data);
        self.damage_resistance_chart.remove_bars(data);
        self.hits_per_second_graph.remove_line(data);
        self.hits_count_chart.remove_bars(data);
        self.activity_chart.remove_lane(data);
    }

    pub fn update(&mut self, filter: f64, time_slice: f64, activity_gap_s: f64) {
        self.activity_chart.update(activity_gap_s);
        self.dps_graph.update(filter);
        self.damage_chart.update(time_slice);
        self.damage_resistance_chart.update(time_slice);
        self.hits_per_second_graph.update(filter);
        self.hits_count_chart.update(time_slice);
    }

    /// `activity_total` draws the Activity chart as one lane for all its rows.
    pub fn show(
        &mut self,
        settings: &Settings,
        ui: &mut Ui,
        active_diagram: DiagramType,
        activity_total: bool,
    ) {
        match active_diagram {
            DiagramType::Damage => self.damage_chart.show(settings, ui),
            DiagramType::Dps => self.dps_graph.show(settings, ui),
            DiagramType::DamageResistance => self.damage_resistance_chart.show(settings, ui),
            DiagramType::HitsCount => self.hits_count_chart.show(settings, ui),
            DiagramType::HitsPerSecond => self.hits_per_second_graph.show(settings, ui),
            DiagramType::Activity => self.activity_chart.show(settings, ui, activity_total),
            _ => unreachable!(),
        }
    }
}

impl HealDiagrams {
    pub fn empty() -> Self {
        Self {
            hps_graph: HpsGraph::empty(DiagramType::Hps),
            heal_chart: HealChart::empty(DiagramType::Heal),
            ticks_per_second: HealTicksPerSeondGraph::empty(DiagramType::HealTicksPerSecond),
            ticks_count_chart: HealTicksCountChart::empty(DiagramType::HealTicksCount),
            activity_chart: ActivityChart::empty(DiagramType::HealTicksCount),
        }
    }

    pub fn from_heal_groups<'a>(
        groups: impl Iterator<Item = &'a HealGroup>,
        combat: &Combat,
        filter: f64,
        damage_time_slice: f64,
        activity_gap_s: f64,
        components: HealComponents,
    ) -> Self {
        let combat_duration_s = combat_duration_seconds(combat);
        let data = groups.map(|g| {
            PreparedHealDataSet::new(
                g.name().get(&combat.name_manager),
                g.total_heal.all,
                g.ticks.get(&combat.heal_ticks_manger).iter(),
                combat_duration_s,
            )
        });

        Self::from_data(data, filter, damage_time_slice, activity_gap_s, components)
    }

    /// `components` has to be handed in rather than left at its default: a
    /// chart built while the picker says "hull only" would otherwise draw both
    /// halves until the next time anything else moved.
    pub fn from_data(
        data: impl Iterator<Item = PreparedHealDataSet>,
        filter: f64,
        heal_time_slice: f64,
        activity_gap_s: f64,
        components: HealComponents,
    ) -> Self {
        let data = data.collect_vec();
        let mut diagrams = Self {
            hps_graph: HpsGraph::from_data(DiagramType::Hps, data.iter().cloned(), filter),
            heal_chart: HealChart::from_data(
                DiagramType::Heal,
                data.iter().cloned(),
                heal_time_slice,
            ),
            ticks_per_second: HealTicksPerSeondGraph::from_data(
                DiagramType::HealTicksPerSecond,
                data.iter().cloned(),
                filter,
            ),
            ticks_count_chart: HealTicksCountChart::from_data(
                DiagramType::HealTicksCount,
                data.iter().cloned(),
                heal_time_slice,
            ),
            activity_chart: ActivityChart::from_data(
                DiagramType::HealTicksCount,
                data.into_iter(),
                activity_gap_s,
            ),
        };
        diagrams.update(filter, heal_time_slice, activity_gap_s, components);
        diagrams
    }

    pub fn add_data(
        &mut self,
        data: PreparedHealDataSet,
        filter: f64,
        time_slice: f64,
        activity_gap_s: f64,
    ) {
        self.activity_chart.add_lane(data.clone(), activity_gap_s);
        self.hps_graph.add_line(data.clone(), filter);
        self.heal_chart.add_bars(data.clone(), time_slice);
        self.ticks_per_second.add_line(data.clone(), filter);
        self.ticks_count_chart.add_bars(data.clone(), time_slice);
    }

    /// The row the series are the parts of, for the Activity chart's Total:
    /// a chart opened on a row draws, with Total on, that row's own lane.
    pub fn with_activity_whole(mut self, data: PreparedHealDataSet) -> Self {
        self.activity_chart.set_whole(data);
        self
    }

    /// The row a chart opened on a row's parts was opened on, if it was.
    pub fn activity_whole(&self) -> Option<PreparedHealDataSet> {
        self.activity_chart.whole()
    }

    pub fn remove_data(&mut self, data: &str) {
        self.hps_graph.remove_line(data);
        self.heal_chart.remove_bars(data);
        self.ticks_per_second.remove_line(data);
        self.ticks_count_chart.remove_bars(data);
        self.activity_chart.remove_lane(data);
    }

    /// `components` selects which halves of a heal the lines add up. Applied
    /// here rather than by filtering the data, so toggling redraws without
    /// rebuilding anything.
    pub fn update(
        &mut self,
        filter: f64,
        time_slice: f64,
        activity_gap_s: f64,
        components: HealComponents,
    ) {
        self.hps_graph.set_components(components);
        self.heal_chart.set_components(components);
        self.ticks_per_second.set_components(components);
        self.ticks_count_chart.set_components(components);
        self.activity_chart.set_components(components);
        self.activity_chart.update(activity_gap_s);
        self.hps_graph.update(filter);
        self.heal_chart.update(time_slice);
        self.ticks_per_second.update(filter);
        self.ticks_count_chart.update(time_slice);
    }

    /// `activity_total` draws the Activity chart as one lane for all its rows.
    pub fn show(
        &mut self,
        settings: &Settings,
        ui: &mut Ui,
        active_diagram: DiagramType,
        activity_total: bool,
    ) {
        match active_diagram {
            DiagramType::Heal => self.heal_chart.show(settings, ui),
            DiagramType::Hps => self.hps_graph.show(settings, ui),
            DiagramType::HealTicksPerSecond => self.ticks_per_second.show(settings, ui),
            DiagramType::HealTicksCount => self.ticks_count_chart.show(settings, ui),
            DiagramType::Activity => self.activity_chart.show(settings, ui, activity_total),
            _ => unreachable!(),
        }
    }
}
