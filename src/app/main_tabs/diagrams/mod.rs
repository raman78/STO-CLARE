mod common;
mod damage_resistance_chart;
mod summary_chart;
mod value_per_second_graph;
mod values_chart;

pub use crate::app::main_tabs::diagrams::common::DiagramType;
use crate::app::settings::Settings;
use crate::app::theme;
use eframe::egui::Color32;
pub use common::HealComponents;
pub use common::PreparedDamageDataSet;
pub use common::PreparedHealDataSet;
pub use common::combat_duration_seconds;
use eframe::egui::Ui;
use itertools::Itertools;
pub use summary_chart::SummaryChart;
pub use value_per_second_graph::ValuePerSecondGraph;

use crate::analyzer::*;

use self::{damage_resistance_chart::*, value_per_second_graph::*, values_chart::*};

pub struct DamageDiagrams {
    dps_graph: DpsGraph,
    damage_chart: DamageChart,
    damage_resistance_chart: DamageResistanceChart,
    hits_per_second_graph: HitsPerSecondGraph,
    hits_count_chart: HitsChart,
}

pub struct HealDiagrams {
    hps_graph: HpsGraph,
    heal_chart: HealChart,
    ticks_per_second: HealTicksPerSeondGraph,
    ticks_count_chart: HealTicksCountChart,
}

impl DamageDiagrams {
    /// The colour the series called `name` is drawn in, or `None` when no
    /// series goes by that name.
    ///
    /// Read off the chart rather than worked out again by the caller: the
    /// colours follow the order the series ended up sorted into (by total,
    /// largest first), which is data-dependent. Every chart here sorts the same
    /// way, so one answer holds for all five of them.
    pub fn series_color(&self, name: &str) -> Option<Color32> {
        self.dps_graph
            .series_names()
            .position(|series| series == name)
            .map(theme::series_color)
    }

    pub fn empty() -> Self {
        Self {
            dps_graph: ValuePerSecondGraph::empty(DiagramType::Dps),
            damage_chart: ValuesChart::empty(DiagramType::Damage),
            damage_resistance_chart: DamageResistanceChart::empty(),
            hits_per_second_graph: HitsPerSecondGraph::empty(DiagramType::HitsPerSecond),
            hits_count_chart: HitsChart::empty(DiagramType::HitsCount),
        }
    }

    pub fn from_damage_groups<'a>(
        groups: impl Iterator<Item = &'a DamageGroup>,
        combat: &Combat,
        filter: f64,
        damage_time_slice: f64,
    ) -> Self {
        let combat_duration_s = combat_duration_seconds(combat);
        let data = groups.map(|g| {
            PreparedDamageDataSet::new(
                g.name().get(&combat.name_manager),
                g.total_damage.all,
                g.hits.get(&combat.hits_manger).iter(),
                combat_duration_s,
            )
        });

        Self::from_data(data, filter, damage_time_slice)
    }

    pub fn from_data(
        data: impl Iterator<Item = PreparedDamageDataSet>,
        filter: f64,
        damage_time_slice: f64,
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
                data.into_iter(),
                damage_time_slice,
            ),
        }
    }

    pub fn add_data(&mut self, data: PreparedDamageDataSet, filter: f64, time_slice: f64) {
        self.dps_graph.add_line(data.clone(), filter);
        self.damage_chart.add_bars(data.clone(), time_slice);
        self.damage_resistance_chart
            .add_bars(data.clone(), time_slice);
        self.hits_per_second_graph.add_line(data.clone(), filter);
        self.hits_count_chart.add_bars(data, time_slice);
    }

    pub fn remove_data(&mut self, data: &str) {
        self.dps_graph.remove_line(data);
        self.damage_chart.remove_bars(data);
        self.damage_resistance_chart.remove_bars(data);
        self.hits_per_second_graph.remove_line(data);
        self.hits_count_chart.remove_bars(data);
    }

    pub fn update(&mut self, filter: f64, time_slice: f64) {
        self.dps_graph.update(filter);
        self.damage_chart.update(time_slice);
        self.damage_resistance_chart.update(time_slice);
        self.hits_per_second_graph.update(filter);
        self.hits_count_chart.update(time_slice);
    }

    pub fn show(&mut self, settings: &Settings, ui: &mut Ui, active_diagram: DiagramType) {
        match active_diagram {
            DiagramType::Damage => self.damage_chart.show(settings, ui),
            DiagramType::Dps => self.dps_graph.show(settings, ui),
            DiagramType::DamageResistance => self.damage_resistance_chart.show(settings, ui),
            DiagramType::HitsCount => self.hits_count_chart.show(settings, ui),
            DiagramType::HitsPerSecond => self.hits_per_second_graph.show(settings, ui),
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
        }
    }

    pub fn from_heal_groups<'a>(
        groups: impl Iterator<Item = &'a HealGroup>,
        combat: &Combat,
        filter: f64,
        damage_time_slice: f64,
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

        Self::from_data(data, filter, damage_time_slice, components)
    }

    /// `components` has to be handed in rather than left at its default: a
    /// chart built while the picker says "hull only" would otherwise draw both
    /// halves until the next time anything else moved.
    pub fn from_data(
        data: impl Iterator<Item = PreparedHealDataSet>,
        filter: f64,
        heal_time_slice: f64,
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
        };
        diagrams.update(filter, heal_time_slice, components);
        diagrams
    }

    pub fn add_data(&mut self, data: PreparedHealDataSet, filter: f64, time_slice: f64) {
        self.hps_graph.add_line(data.clone(), filter);
        self.heal_chart.add_bars(data.clone(), time_slice);
        self.ticks_per_second.add_line(data.clone(), filter);
        self.ticks_count_chart.add_bars(data.clone(), time_slice);
    }

    pub fn remove_data(&mut self, data: &str) {
        self.hps_graph.remove_line(data);
        self.heal_chart.remove_bars(data);
        self.ticks_per_second.remove_line(data);
        self.ticks_count_chart.remove_bars(data);
    }

    /// `components` selects which halves of a heal the lines add up. Applied
    /// here rather than by filtering the data, so toggling redraws without
    /// rebuilding anything.
    pub fn update(&mut self, filter: f64, time_slice: f64, components: HealComponents) {
        self.hps_graph.set_components(components);
        self.heal_chart.set_components(components);
        self.ticks_per_second.set_components(components);
        self.ticks_count_chart.set_components(components);
        self.hps_graph.update(filter);
        self.heal_chart.update(time_slice);
        self.ticks_per_second.update(filter);
        self.ticks_count_chart.update(time_slice);
    }

    pub fn show(&mut self, settings: &Settings, ui: &mut Ui, active_diagram: DiagramType) {
        match active_diagram {
            DiagramType::Heal => self.heal_chart.show(settings, ui),
            DiagramType::Hps => self.hps_graph.show(settings, ui),
            DiagramType::HealTicksPerSecond => self.ticks_per_second.show(settings, ui),
            DiagramType::HealTicksCount => self.ticks_count_chart.show(settings, ui),
            _ => unreachable!(),
        }
    }
}
