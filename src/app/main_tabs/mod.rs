use eframe::egui::*;

use eframe::Frame;
use rfd::FileDialog;

use crate::{
    analyzer::{Combat, HealGrouping},
    app::settings::{Settings, TableKind},
};

use self::{damage_tab::DamageTab, heal_tab::HealTab, summary_tab::SummaryTab};
use crate::custom_widgets::toggle::Toggle;

mod common;
mod damage_tab;
// Exposed so the compare view can reuse the same charts.
pub(crate) mod diagrams;
pub mod export;
mod heal_tab;
mod summary_tab;
// Exposed so the compare view can reuse the tables' column separator.
pub(crate) mod tables;

pub struct MainTabs {
    pub identifier: String,
    pub summary_tab: SummaryTab,
    pub damage_out_tab: DamageTab,
    pub damage_in_tab: DamageTab,
    pub heal_ally_tab: HealTab,
    pub heal_received_tab: HealTab,
    pub heal_self_tab: HealTab,

    active_tab: MainTab,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MainTab {
    #[default]
    Summary,
    DamageDealt,
    DamageTaken,
    SelfHealing,
    HealingAlly,
    HealingReceived,
}

impl MainTab {
    /// What the tab is called on screen, reused as the sheet's name in an
    /// export and in the log line that records one.
    pub fn name(self) -> &'static str {
        match self {
            MainTab::Summary => "Summary",
            MainTab::DamageDealt => "Damage Dealt",
            MainTab::DamageTaken => "Damage Taken",
            MainTab::SelfHealing => "Self Healing",
            MainTab::HealingAlly => "Healing Ally",
            MainTab::HealingReceived => "Healing Received",
        }
    }
}

/// What each healing tab holds. Shown as tooltips, because the three pools are
/// only unambiguous once you know that they are disjoint.
const HEALING_ALLY_INFO: &str = "Healing you did to somebody else — teammates, allied NPCs and your own pets.\n\
     Grouped by ability, then by who received it.\n\
     Healing you did to yourself is not in here; it is under Self Healing.";
const HEALING_RECEIVED_INFO: &str = "Healing somebody else did to you.\n\
     Grouped by ability, then by who healed you.\n\
     Your own self heals are not in here; they are under Self Healing.";
const SELF_HEALING_INFO: &str = "Healing you did to yourself: self-buffs, your own trait and gear procs, and \
     your own consoles healing you.\n\
     Always grouped by ability — there is nobody else involved, so there is\n\
     nothing else to group by. A heal that came through a pet or a hologram\n\
     shows it underneath the ability.\n\
     Counted here only — it is deliberately left out of the other two tabs, so \
     the three add up without counting anything twice.";

impl MainTabs {
    pub fn empty() -> Self {
        Self {
            identifier: String::new(),
            damage_out_tab: DamageTab::empty(|p| &p.damage_out),
            damage_in_tab: DamageTab::empty(|p| &p.damage_in),
            heal_ally_tab: HealTab::empty(|p| &p.heal_ally, Some("Person")),
            heal_received_tab: HealTab::empty(|p| &p.heal_received, Some("Person")),
            // Self healing has no other party, so there is nothing to group by:
            // it is always the ability, with whatever pet or console carried the
            // heal underneath it.
            heal_self_tab: HealTab::empty(|p| &p.heal_self, None),
            active_tab: Default::default(),
            summary_tab: SummaryTab::empty(),
        }
    }

    /// Which columns the open tab shows. Only the tab on screen is offered:
    /// the menu is about what is in front of the user, and a list of every
    /// column of every tab would be sixty entries long.
    fn show_column_picker(&mut self, settings: &mut Settings, ui: &mut Ui) {
        let (kind, names) = self.active_columns();
        let hidden = settings.columns.hidden_count(kind, &names);
        let label = if hidden == 0 {
            "Columns ⏷".to_string()
        } else {
            format!("Columns ⏷ ({hidden} hidden)")
        };
        ui.menu_button(label, |ui| {
            let mut changed = false;
            for name in names.iter() {
                let mut shown = settings.columns.is_shown(kind, name);
                if ui.checkbox(&mut shown, *name).changed() {
                    settings.columns.set_shown(kind, name, shown);
                    changed = true;
                }
            }
            ui.separator();
            if ui.button("Show all").clicked() {
                settings.columns.show_all(kind);
                changed = true;
            }
            if changed {
                settings.save();
            }
        })
        .response
        .on_hover_text(
            "Which columns this table shows. The two damage tabs share their choice, and so do \
             the three healing ones.",
        );
    }

    /// Writes the whole combat to a spreadsheet, a sheet per tab.
    fn export(&self, combat: &Combat, frame: &Frame) {
        let Some(path) = FileDialog::new()
            .set_title("Export Combat")
            .add_filter("Excel workbook", &["xlsx"])
            .set_file_name(crate::app::export::default_file_name(combat))
            .set_parent(frame)
            .save_file()
        else {
            return;
        };
        let sheets = export::all_sheets(combat, self.heal_grouping());
        match crate::app::export::write(&path, &sheets) {
            Ok(()) => log::info!("exported the combat to {}", path.display()),
            Err(error) => log::error!("failed to export to {}: {error}", path.display()),
        }
    }

    pub fn update(&mut self, settings: &Settings, combat: &Combat) {
        self.identifier = combat.identifier();
        self.summary_tab.update(settings, combat);
        self.damage_out_tab.update(settings, combat);
        self.damage_in_tab.update(settings, combat);
        self.heal_ally_tab.update(settings, combat);
        self.heal_received_tab.update(settings, combat);
        self.heal_self_tab.update(settings, combat);
    }

    /// Which nesting the healing tabs are showing, which the export follows.
    fn heal_grouping(&self) -> HealGrouping {
        match self.active_tab {
            MainTab::SelfHealing => self.heal_self_tab.grouping(),
            MainTab::HealingAlly => self.heal_ally_tab.grouping(),
            MainTab::HealingReceived => self.heal_received_tab.grouping(),
            _ => HealGrouping::ByAbility,
        }
    }

    /// Which column set the open tab draws from. The two damage tabs share one
    /// and the three healing tabs share another, so hiding a column in one of a
    /// pair hides it in the other.
    fn active_columns(&self) -> (TableKind, Vec<&'static str>) {
        match self.active_tab {
            MainTab::Summary => (TableKind::Summary, tables::SummaryTable::column_names()),
            MainTab::DamageDealt | MainTab::DamageTaken => {
                (TableKind::Damage, tables::damage_column_names())
            }
            _ => (TableKind::Heal, tables::heal_column_names()),
        }
    }

    pub fn show(
        &mut self,
        settings: &mut Settings,
        combat: Option<&Combat>,
        frame: &Frame,
        ui: &mut Ui,
    ) {
        ui.horizontal(|ui| {
            ui.steady_toggle_value(&mut self.active_tab, MainTab::Summary, "Summary");

            ui.steady_toggle_value(&mut self.active_tab, MainTab::DamageDealt, "Damage Dealt")
                .on_hover_text("Damage you dealt to others.");
            ui.steady_toggle_value(&mut self.active_tab, MainTab::DamageTaken, "Damage Taken")
                .on_hover_text("Damage others dealt to you.");

            ui.steady_toggle_value(&mut self.active_tab, MainTab::SelfHealing, "Self Healing")
                .on_hover_text(SELF_HEALING_INFO);
            ui.steady_toggle_value(&mut self.active_tab, MainTab::HealingAlly, "Healing Ally")
                .on_hover_text(HEALING_ALLY_INFO);
            ui.steady_toggle_value(
                &mut self.active_tab,
                MainTab::HealingReceived,
                "Healing Received",
            )
            .on_hover_text(HEALING_RECEIVED_INFO);

            self.show_column_picker(settings, ui);

            // The export belongs to the whole combat rather than to the tab
            // that happens to be open, so it sits away from the tabs, at the
            // far end of their row.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(combat.is_some(), Button::new("Export XLSX 🖹"))
                    .on_hover_text(
                        "Save the whole combat as a spreadsheet: one sheet per tab, every \
                         player, every row of the breakdown, and every metric — not only the \
                         columns on screen.",
                    )
                    .clicked()
                    && let Some(combat) = combat
                {
                    self.export(combat, frame);
                }
            });
        });

        match self.active_tab {
            MainTab::Summary => self.summary_tab.show(settings, ui),
            MainTab::DamageDealt => self.damage_out_tab.show(settings, ui),
            MainTab::DamageTaken => self.damage_in_tab.show(settings, ui),
            MainTab::SelfHealing => self.heal_self_tab.show(settings, ui),
            MainTab::HealingAlly => self.heal_ally_tab.show(settings, ui),
            MainTab::HealingReceived => self.heal_received_tab.show(settings, ui),
        }
    }
}
