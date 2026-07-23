//! Builds and renders the side-by-side comparison of the outgoing damage ability
//! tree of a chosen player across up to a few combats.
//!
//! The trees are aligned by ability name (name handles differ per combat, so we
//! key on the resolved name string). Rows are sorted by the first (reference)
//! combat's DPS, and every value in combats 2+ carries a colored +/- delta
//! against the reference.

use std::sync::Arc;

use eframe::egui::*;
use rustc_hash::FxHashMap;

use crate::{
    analyzer::{AnalysisGroup, Combat, DamageGroup, NameHandle, NameManager},
    app::settings::Settings,
    custom_widgets::table::*,
    helpers::number_formatting::NumberFormatter,
};

use super::CompareMetric;

const ROW_HEIGHT: f32 = 25.0;
// Two lines: the metric name on top, the combat number below.
const HEADER_HEIGHT: f32 = 34.0;

/// Delta color when the metric moved in the better direction.
const IMPROVE: Color32 = Color32::from_rgb(0x5c, 0xb8, 0x5c);
/// Delta color when the metric moved in the worse direction.
const WORSE: Color32 = Color32::from_rgb(0xd9, 0x53, 0x4f);

struct Slot {
    /// Index in the combats list (shown in the legend as combat 1/2/3).
    #[allow(dead_code)]
    index: usize,
    combat: Arc<Combat>,
    player: NameHandle,
}

pub struct Comparison {
    slots: Vec<Slot>,
    nodes: Vec<CompareNode>,
    columns: Vec<CompareMetric>,
}

struct CompareNode {
    name: String,
    /// One entry per slot; `None` when that combat's player has no such node.
    cells: Vec<Option<SlotCell>>,
    /// Reference (first slot) DPS, used to sort rows; `-inf` when absent.
    sort_key: f64,
    sub_nodes: Vec<CompareNode>,
    open: bool,
}

struct SlotCell {
    /// One entry per configured column.
    metrics: Vec<MetricCell>,
}

struct MetricCell {
    text: String,
    /// Delta versus the reference combat (only for slots after the first).
    delta: Option<DeltaCell>,
}

struct DeltaCell {
    text: String,
    improvement: bool,
}

impl Comparison {
    pub fn new(fetched: Vec<(usize, Arc<Combat>)>, columns: &[CompareMetric]) -> Self {
        let slots: Vec<Slot> = fetched
            .into_iter()
            .map(|(index, combat)| {
                let player = top_dps_player(&combat);
                Slot {
                    index,
                    combat,
                    player,
                }
            })
            .collect();
        let mut comparison = Self {
            slots,
            nodes: Vec::new(),
            columns: columns.to_vec(),
        };
        comparison.rebuild();
        comparison
    }

    fn rebuild(&mut self) {
        let parents: Vec<Option<&DamageGroup>> = self
            .slots
            .iter()
            .map(|s| s.combat.players.get(&s.player).map(|p| &p.damage_out))
            .collect();
        let name_managers: Vec<&NameManager> =
            self.slots.iter().map(|s| &s.combat.name_manager).collect();

        // Top row is the player's overall total (root of the damage tree); the
        // ability groups hang under it, expanded by default.
        let cells = build_cells(&parents, &self.columns);
        let sub_nodes = build_level(&parents, &name_managers, &self.columns);
        let sort_key = parents
            .first()
            .and_then(|p| *p)
            .map(|g| g.dps.all)
            .unwrap_or(f64::NEG_INFINITY);
        self.nodes = vec![CompareNode {
            name: "Total".to_string(),
            cells,
            sort_key,
            sub_nodes,
            open: true,
        }];
    }

    pub fn show(&mut self, ui: &mut Ui, settings: &mut Settings) {
        if self.slots.is_empty() {
            ui.label("No combats selected.");
            return;
        }

        self.show_column_picker(ui, settings);

        // Pick up column changes from the picker (or an external settings edit).
        if self.columns != settings.compare.columns {
            self.columns = settings.compare.columns.clone();
            self.rebuild();
        }

        // Legend + per-combat player picker.
        let mut player_change: Option<(usize, NameHandle)> = None;
        for (slot_i, slot) in self.slots.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}: {}", slot_i + 1, slot.combat.identifier()));
                let current = slot.player.get(&slot.combat.name_manager).to_string();
                ComboBox::new(("compare player", slot_i), "player")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (handle, name) in players_by_dps(&slot.combat) {
                            if ui.selectable_label(handle == slot.player, name).clicked() {
                                player_change = Some((slot_i, handle));
                            }
                        }
                    });
            });
        }
        if let Some((slot_i, handle)) = player_change {
            self.slots[slot_i].player = handle;
            self.rebuild();
        }

        ui.separator();
        self.show_table(ui);
    }

    fn show_column_picker(&self, ui: &mut Ui, settings: &mut Settings) {
        ui.menu_button("Columns ▾", |ui| {
            let mut changed = false;
            for &metric in CompareMetric::ALL {
                let mut on = settings.compare.columns.contains(&metric);
                if ui.checkbox(&mut on, metric.label()).changed() {
                    changed = true;
                    if on {
                        settings.compare.columns.push(metric);
                    } else {
                        settings.compare.columns.retain(|&m| m != metric);
                    }
                }
            }
            if changed {
                // Keep a stable column order regardless of toggle order.
                settings.compare.columns.sort_by_key(|m| {
                    CompareMetric::ALL
                        .iter()
                        .position(|x| x == m)
                        .unwrap_or(usize::MAX)
                });
                settings.save();
            }
        });
    }

    fn show_table(&mut self, ui: &mut Ui) {
        let n_slots = self.slots.len();
        let n_metrics = self.columns.len();
        // Columns are grouped by metric: the metric name spans its group (shown
        // on the first column), with the combat number below each column.
        let headers: Vec<String> = self
            .columns
            .iter()
            .flat_map(|c| {
                (0..n_slots).map(move |slot_i| {
                    if slot_i == 0 {
                        format!("{}\n{}", c.label(), slot_i + 1)
                    } else {
                        format!("\n{}", slot_i + 1)
                    }
                })
            })
            .collect();
        let nodes = &mut self.nodes;

        ScrollArea::horizontal().show(ui, |ui| {
            Table::new(ui)
                .cell_spacing(10.0)
                .header(HEADER_HEIGHT, |r| {
                    r.cell(|ui| {
                        ui.label("Name");
                    });
                    for header in &headers {
                        r.cell(|ui| {
                            ui.label(header);
                        });
                    }
                })
                .body(ROW_HEIGHT, |mut t| {
                    for node in nodes.iter_mut() {
                        node.show(&mut t, 0.0, n_slots, n_metrics);
                    }
                });
        });
    }
}

impl CompareNode {
    fn show(&mut self, t: &mut TableBody, indent: f32, n_slots: usize, n_metrics: usize) {
        t.row(|r| {
            r.cell(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space(indent * 20.0);
                    let symbol = if self.open { "⏷" } else { "⏵" };
                    let can_open = !self.sub_nodes.is_empty();
                    if ui
                        .add_visible(can_open, Button::selectable(false, symbol))
                        .clicked()
                    {
                        self.open = !self.open;
                    }
                    ui.label(&self.name);
                });
            });

            // Column groups by metric: for each metric, one cell per combat.
            for metric_i in 0..n_metrics {
                for slot_i in 0..n_slots {
                    match self
                        .cells
                        .get(slot_i)
                        .and_then(|c| c.as_ref())
                        .and_then(|c| c.metrics.get(metric_i))
                    {
                        Some(metric) => {
                            r.cell_with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if let Some(delta) = &metric.delta {
                                    let color = if delta.improvement { IMPROVE } else { WORSE };
                                    ui.colored_label(color, &delta.text);
                                }
                                ui.label(&metric.text);
                            });
                        }
                        None => {
                            r.cell(|_| {});
                        }
                    }
                }
            }
        });

        if self.open {
            for sub in self.sub_nodes.iter_mut() {
                sub.show(t, indent + 1.0, n_slots, n_metrics);
            }
        }
    }
}

fn top_dps_player(combat: &Combat) -> NameHandle {
    players_by_dps(combat)
        .into_iter()
        .next()
        .map(|(handle, _)| handle)
        .unwrap_or(NameHandle::UNKNOWN)
}

fn players_by_dps(combat: &Combat) -> Vec<(NameHandle, String)> {
    let mut players: Vec<(NameHandle, f64, String)> = combat
        .players
        .iter()
        .map(|(handle, player)| {
            (
                *handle,
                player.damage_out.dps.all,
                handle.get(&combat.name_manager).to_string(),
            )
        })
        .collect();
    players.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    players
        .into_iter()
        .map(|(handle, _, name)| (handle, name))
        .collect()
}

/// Build one level of the aligned tree from the parent damage groups of each
/// slot (union of child ability names), recursing into sub-groups.
fn build_level(
    parents: &[Option<&DamageGroup>],
    name_managers: &[&NameManager],
    columns: &[CompareMetric],
) -> Vec<CompareNode> {
    let n = parents.len();
    let mut order: Vec<String> = Vec::new();
    let mut index: FxHashMap<String, usize> = Default::default();
    let mut children: Vec<Vec<Option<&DamageGroup>>> = Vec::new();

    for (slot_i, parent) in parents.iter().enumerate() {
        let Some(parent) = parent else { continue };
        for sub in parent.sub_groups().values() {
            let name = sub.name().get(name_managers[slot_i]).to_string();
            let idx = *index.entry(name.clone()).or_insert_with(|| {
                order.push(name);
                children.push(vec![None; n]);
                order.len() - 1
            });
            children[idx][slot_i] = Some(sub);
        }
    }

    let mut nodes: Vec<CompareNode> = order
        .into_iter()
        .enumerate()
        .map(|(idx, name)| {
            let per_slot = &children[idx];
            let sort_key = per_slot[0].map(|g| g.dps.all).unwrap_or(f64::NEG_INFINITY);
            let cells = build_cells(per_slot, columns);
            let sub_nodes = build_level(per_slot, name_managers, columns);
            CompareNode {
                name,
                cells,
                sort_key,
                sub_nodes,
                open: false,
            }
        })
        .collect();

    nodes.sort_by(|a, b| {
        b.sort_key
            .partial_cmp(&a.sort_key)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    nodes
}

fn build_cells(per_slot: &[Option<&DamageGroup>], columns: &[CompareMetric]) -> Vec<Option<SlotCell>> {
    let mut formatter = NumberFormatter::new();

    // Raw metric values per slot, so combats 2+ can be compared to slot 0.
    let raw: Vec<Option<Vec<Option<f64>>>> = per_slot
        .iter()
        .map(|g| g.map(|g| columns.iter().map(|c| c.extract(g)).collect::<Vec<_>>()))
        .collect();
    let base = raw.first().and_then(|o| o.as_ref());

    raw.iter()
        .enumerate()
        .map(|(slot_i, values)| {
            values.as_ref().map(|values| {
                let metrics = values
                    .iter()
                    .enumerate()
                    .map(|(m, value)| {
                        let column = columns[m];
                        let text = value
                            .map(|v| formatter.format(v, column.precision()))
                            .unwrap_or_default();
                        let delta = if slot_i == 0 {
                            None
                        } else {
                            make_delta(base.and_then(|b| b[m]), *value, column, &mut formatter)
                        };
                        MetricCell { text, delta }
                    })
                    .collect();
                SlotCell { metrics }
            })
        })
        .collect()
}

/// Formatted, colored delta of `current` versus `base` for one metric. `None`
/// when either value is missing or the values are equal.
fn make_delta(
    base: Option<f64>,
    current: Option<f64>,
    metric: CompareMetric,
    formatter: &mut NumberFormatter,
) -> Option<DeltaCell> {
    let (base, current) = (base?, current?);
    let diff = current - base;
    if diff.abs() < 1e-9 {
        return None;
    }
    let improvement = if metric.higher_is_better() {
        diff > 0.0
    } else {
        diff < 0.0
    };
    let sign = if diff > 0.0 { "+" } else { "-" };
    Some(DeltaCell {
        text: format!("{}{}", sign, formatter.format(diff.abs(), metric.precision())),
        improvement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_direction_for_higher_is_better() {
        let mut f = NumberFormatter::new();
        let up = make_delta(Some(100.0), Some(120.0), CompareMetric::Dps, &mut f).unwrap();
        assert!(up.improvement);
        assert_eq!(up.text, "+20");

        let down = make_delta(Some(120.0), Some(100.0), CompareMetric::Dps, &mut f).unwrap();
        assert!(!down.improvement);
        assert_eq!(down.text, "-20");
    }

    #[test]
    fn delta_direction_for_resistance_is_inverted() {
        // Lower resistance faced is better, so a drop is an improvement (green).
        let mut f = NumberFormatter::new();
        let lower = make_delta(Some(50.0), Some(40.0), CompareMetric::Resistance, &mut f).unwrap();
        assert!(lower.improvement);
        let higher = make_delta(Some(40.0), Some(50.0), CompareMetric::Resistance, &mut f).unwrap();
        assert!(!higher.improvement);
    }

    #[test]
    fn equal_or_missing_values_have_no_delta() {
        let mut f = NumberFormatter::new();
        assert!(make_delta(Some(100.0), Some(100.0), CompareMetric::Dps, &mut f).is_none());
        assert!(make_delta(None, Some(100.0), CompareMetric::Dps, &mut f).is_none());
        assert!(make_delta(Some(100.0), None, CompareMetric::Dps, &mut f).is_none());
    }
}
