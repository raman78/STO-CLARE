use eframe::egui::*;

use crate::{analyzer::Combat, app::settings::Settings};

use self::{damage_tab::DamageTab, heal_tab::HealTab, summary_tab::SummaryTab};

mod common;
mod damage_tab;
// Exposed so the compare view can reuse the same charts.
pub(crate) mod diagrams;
mod heal_tab;
mod summary_tab;
mod tables;

pub struct MainTabs {
    pub identifier: String,
    pub summary_tab: SummaryTab,
    pub damage_out_tab: DamageTab,
    pub damage_in_tab: DamageTab,
    pub heal_done_tab: HealTab,
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
    HealingDone,
    HealingReceived,
    SelfHealing,
}

/// What each healing tab holds. Shown as tooltips, because the three pools are
/// only unambiguous once you know that they are disjoint.
const HEALING_DONE_INFO: &str =
    "Healing you did to somebody else — teammates, allied NPCs and your own pets.\n\
     Grouped by ability, then by who received it.\n\
     Healing you did to yourself is not in here; it is under Self Healing.";
const HEALING_RECEIVED_INFO: &str =
    "Healing somebody else did to you.\n\
     Grouped by ability, then by who healed you.\n\
     Your own self heals are not in here; they are under Self Healing.";
const SELF_HEALING_INFO: &str =
    "Healing you did to yourself: self-buffs, your own trait and gear procs, and \
     your own consoles healing you.\n\
     Grouped by ability, then by what it came from.\n\
     Counted here only — it is deliberately left out of the other two tabs, so \
     the three add up without counting anything twice.";

impl MainTabs {
    pub fn empty() -> Self {
        Self {
            identifier: String::new(),
            damage_out_tab: DamageTab::empty(|p| &p.damage_out),
            damage_in_tab: DamageTab::empty(|p| &p.damage_in),
            heal_done_tab: HealTab::empty(|p| &p.heal_done),
            heal_received_tab: HealTab::empty(|p| &p.heal_received),
            heal_self_tab: HealTab::empty(|p| &p.heal_self),
            active_tab: Default::default(),
            summary_tab: SummaryTab::empty(),
        }
    }

    pub fn update(&mut self, settings: &Settings, combat: &Combat) {
        self.identifier = combat.identifier();
        self.summary_tab.update(settings, combat);
        self.damage_out_tab.update(settings, combat);
        self.damage_in_tab.update(settings, combat);
        self.heal_done_tab.update(settings, combat);
        self.heal_received_tab.update(settings, combat);
        self.heal_self_tab.update(settings, combat);
    }

    pub fn show(&mut self, settings: &Settings, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, MainTab::Summary, "Summary");

            ui.selectable_value(&mut self.active_tab, MainTab::DamageDealt, "Damage Dealt")
                .on_hover_text("Damage you dealt to others.");
            ui.selectable_value(&mut self.active_tab, MainTab::DamageTaken, "Damage Taken")
                .on_hover_text("Damage others dealt to you.");

            ui.selectable_value(&mut self.active_tab, MainTab::HealingDone, "Healing Done")
                .on_hover_text(HEALING_DONE_INFO);
            ui.selectable_value(
                &mut self.active_tab,
                MainTab::HealingReceived,
                "Healing Received",
            )
            .on_hover_text(HEALING_RECEIVED_INFO);
            ui.selectable_value(&mut self.active_tab, MainTab::SelfHealing, "Self Healing")
                .on_hover_text(SELF_HEALING_INFO);
        });

        match self.active_tab {
            MainTab::Summary => self.summary_tab.show(settings, ui),
            MainTab::DamageDealt => self.damage_out_tab.show(settings, ui),
            MainTab::DamageTaken => self.damage_in_tab.show(settings, ui),
            MainTab::HealingDone => self.heal_done_tab.show(settings, ui),
            MainTab::HealingReceived => self.heal_received_tab.show(settings, ui),
            MainTab::SelfHealing => self.heal_self_tab.show(settings, ui),
        }
    }
}
