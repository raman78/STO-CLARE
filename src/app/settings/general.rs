use crate::platform::AppWindow;
use egui::*;
use rfd::FileDialog;

use crate::{
    app::analysis_handling::AnalysisHandler, custom_widgets::slider_text_edit::SliderTextEdit,
};

use super::Settings;

#[derive(Default)]
pub struct GeneralTab {
    clear_log_dialog: ClearLogDialog,
}

#[derive(Default)]
pub struct ClearLogDialog {
    is_open: bool,
    // Per-combat "delete" flags, indexed like the combats list; rebuilt when the
    // dialog opens (default: every combat except the newest).
    to_delete: Vec<bool>,
}

impl GeneralTab {
    pub fn show(
        &mut self,
        analysis_handler: &AnalysisHandler,
        modified_settings: &mut Settings,
        combats: &[String],
        ui: &mut Ui,
        frame: &AppWindow,
    ) {
        ui.horizontal(|ui| {
            ui.label("Combatlog File");
            if ui.button("Browse").clicked() {
                let mut dialog = FileDialog::new()
                    .set_title("Choose combatlog File")
                    .add_filter("combatlog", &["log"])
                    .set_parent(frame);
                // Open at the folder of the currently configured log, so Browse
                // starts where it last left off instead of the default dir.
                if let Some(dir) = std::path::Path::new(&modified_settings.analysis.combatlog_file)
                    .parent()
                    .filter(|dir| dir.is_dir())
                {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(new_combatlog_file) = dialog.pick_file() {
                    modified_settings.analysis.combatlog_file =
                        new_combatlog_file.display().to_string();
                }
            }

            self.clear_log_dialog.show(analysis_handler, combats, ui);
        });
        TextEdit::singleline(&mut modified_settings.analysis.combatlog_file)
            .desired_width(f32::MAX)
            .show(ui);

        #[cfg(target_os = "linux")]
        ui.checkbox(
            &mut modified_settings.analysis.consolidate_combatlog,
            "Merge rotating combat logs into one file",
        )
        .on_hover_text(
            "STO under Proton creates a new combat log file every so often. When enabled, CLA \
             keeps a single combatlog.log up to date in the same folder (merging completed logs, \
             deleting the merged originals to save space). Open combatlog.log for the live overlay \
             and all combats in one place; open a specific log to review just that file.",
        );

        ui.separator();

        ui.label("Combat Separation Time in seconds");
        SliderTextEdit::new(
            &mut modified_settings.analysis.combat_separation_time_seconds,
            15.0..=240.0,
            "combat separation time slider",
        )
        .clamp_to_range(false)
        .step_by(15.0)
        .desired_text_edit_width(40.0)
        .clamp_min(1.0)
        .show(ui);

        ui.separator();

        ui.checkbox(
            &mut modified_settings.auto_refresh.enable,
            "Auto Refresh when log changes (Overlay will always auto refresh)",
        );
        ui.label("Auto Refresh Interval in seconds (applies to the Overlay and when auto refresh is enabled)");
        SliderTextEdit::new(
            &mut modified_settings.auto_refresh.interval_seconds,
            0.1..=4.0,
            "auto refresh interval slider",
        )
        .clamp_to_range(false)
        .step_by(0.1)
        .display_precision(2)
        .desired_text_edit_width(40.0)
        .clamp_min(0.1)
        .show(ui);

        ui.separator();

        ui.checkbox(
            &mut modified_settings.general.more_decimals,
            "Show more decimals",
        )
        .on_hover_text(
            "Shows more numbers after the decimal point in the tables of the different tabs and the overlay",
        );
    }

    pub fn show_clear_log_dialog(
        &mut self,
        analysis_handler: &AnalysisHandler,
        combats: &[String],
        ui: &mut Ui,
    ) {
        self.clear_log_dialog.show(analysis_handler, combats, ui);
    }

    pub fn initialize(&mut self) {
        self.clear_log_dialog.initialize();
    }
}

impl ClearLogDialog {
    fn show(&mut self, analysis_handler: &AnalysisHandler, combats: &[String], ui: &mut Ui) {
        let open_response = ui.button("Clear Log File");

        let mut newly_opened = false;
        if open_response.clicked() {
            self.is_open = true;
            newly_opened = true;
            self.reset_selection(combats.len());
            // Rebuild the list from the log on open so it always shows every combat.
            analysis_handler.refresh();
        }

        if !self.is_open {
            return;
        }

        // Keep the flags aligned with the combats list if it changed while the
        // dialog was open (e.g. an auto refresh appended a new combat).
        if self.to_delete.len() != combats.len() {
            self.reset_selection(combats.len());
        }

        let mut window = Window::new("Delete Combats")
            .collapsible(false)
            .default_size([460.0, 480.0])
            .resizable(true);
        if newly_opened {
            window = window.current_pos(open_response.rect.min);
        }

        window.show(ui.ctx(), |ui| {
            ui.label("Select the combats to delete. Everything left unchecked is kept in the log.");
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                if ui.button("Select all").clicked() {
                    self.to_delete.iter_mut().for_each(|d| *d = true);
                }
                if ui.button("Unselect all").clicked() {
                    self.to_delete.iter_mut().for_each(|d| *d = false);
                }
            });

            ui.separator();
            ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                // Newest first, matching the combats dropdown.
                let newest = combats.len().wrapping_sub(1);
                for i in (0..combats.len()).rev() {
                    if let Some(flag) = self.to_delete.get_mut(i) {
                        let label = if i == newest {
                            format!("{}  (newest)", combats[i])
                        } else {
                            combats[i].clone()
                        };
                        ui.checkbox(flag, label);
                    }
                }
            });
            ui.separator();

            let delete_count = self.to_delete.iter().filter(|&&d| d).count();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        delete_count > 0,
                        Button::new(format!("Delete {delete_count} selected")),
                    )
                    .clicked()
                {
                    let keep: Vec<usize> = self
                        .to_delete
                        .iter()
                        .enumerate()
                        .filter(|&(_, &delete)| !delete)
                        .map(|(i, _)| i)
                        .collect();
                    analysis_handler.keep_combats(keep);
                    self.is_open = false;
                }

                if ui.button("Cancel").clicked() {
                    self.is_open = false;
                }
            });
        });
    }

    /// Default selection: delete every combat except the newest.
    fn reset_selection(&mut self, count: usize) {
        self.to_delete = vec![true; count];
        if let Some(newest) = self.to_delete.last_mut() {
            *newest = false;
        }
    }

    fn initialize(&mut self) {
        self.is_open = false;
    }
}
