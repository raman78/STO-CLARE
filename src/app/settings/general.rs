use eframe::Frame;
use eframe::egui::*;
use rfd::FileDialog;

use crate::custom_widgets::slider_text_edit::SliderTextEdit;

use super::Settings;
use crate::custom_widgets::tooltip::CloseTooltip;

#[derive(Default)]
pub struct GeneralTab;

impl GeneralTab {
    pub fn show(
        &mut self,
        modified_settings: &mut Settings,
        detected_owner: Option<&str>,
        ui: &mut Ui,
        frame: &Frame,
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
        });
        TextEdit::singleline(&mut modified_settings.analysis.combatlog_file)
            .desired_width(f32::MAX)
            .show(ui);

        // A log to come back to. Reading someone else's run, or a single fight
        // saved out of the way, means pointing this at another file — and
        // finding your own again meant walking the file dialog back to it every
        // time.
        ui.horizontal(|ui| {
            let current = modified_settings.analysis.combatlog_file.clone();
            let default = modified_settings.general.default_combatlog_file.clone();
            let is_default = default.as_deref() == Some(current.as_str());

            if ui
                .add_enabled(!current.is_empty() && !is_default, Button::new("Remember"))
                .hover("Remember the file above as the one to come back to.")
                .clicked()
            {
                modified_settings.general.default_combatlog_file = Some(current);
            }

            if let Some(default) = &default {
                if ui
                    .add_enabled(!is_default, Button::new("Go back to default"))
                    .clicked()
                {
                    modified_settings.analysis.combatlog_file = default.clone();
                }
                if ui.button("Forget").clicked() {
                    modified_settings.general.default_combatlog_file = None;
                }
            }
        });

        // The path gets a line to itself, under a heading of its own. A combat
        // log path is long enough that sharing a row with the buttons cut it
        // off, which left the one thing this has to say — where it goes back
        // to — as something you had to hover a button to find out.
        ui.label("Default combatlog path:");
        ui.label(
            RichText::new(
                modified_settings
                    .general
                    .default_combatlog_file
                    .as_deref()
                    .unwrap_or("none yet — Remember stores the file above"),
            )
            .weak(),
        );

        ui.checkbox(
            &mut modified_settings.analysis.consolidate_combatlog,
            "Merge rotating combat logs into one file",
        )
        .hover(
            "STO starts a new combat log every hour unless the launcher is given \
             -NoAutoRotateLogs, which scatters your fights across many files. When enabled, \
             CLARE keeps a single combatlog.log up to date in the same folder (merging completed \
             logs, deleting the merged originals to save space). Open combatlog.log for the live \
             overlay and all combats in one place; open a specific log to review just that file.",
        );

        ui.separator();

        // Whose figures the combats list shows. Worked out from the log on
        // every start, so this is only ever filled in to overrule it.
        ui.label("My handle");
        ui.horizontal(|ui| {
            let mut handle = modified_settings
                .general
                .my_handle
                .clone()
                .unwrap_or_default();
            if TextEdit::singleline(&mut handle)
                .hint_text(detected_owner.unwrap_or("@handle"))
                .desired_width(200.0)
                .show(ui)
                .response
                .changed()
            {
                modified_settings.general.my_handle =
                    Some(handle).filter(|handle| !handle.trim().is_empty());
            }
            if modified_settings.general.my_handle.is_some()
                && ui
                    .button("Work it out from the log")
                    .hover("Go back to reading the handle off the log itself.")
                    .clicked()
            {
                modified_settings.general.my_handle = None;
            }
        });
        ui.label(
            RichText::new(match detected_owner {
                Some(handle) => format!(
                    "Worked out from your log: {handle}. Fill the box in only if that is not you."
                ),
                None => "No log read so far says who it belongs to — every player in it fought \
                         the same number of its combats. Fill in your handle and the combats \
                         list will show your figures."
                    .to_owned(),
            })
            .weak(),
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
        .hover(
            "Shows more numbers after the decimal point in the tables of the different tabs and the overlay",
        );

        ui.checkbox(
            &mut modified_settings.general.split_shield_hull_columns,
            "Show Hull and Shield as separate columns",
        )
        .hover(
            "Metrics that split into a hull and a shield half (damage, hits, healing, ticks) get \
             their own Hull and Shield columns next to the total, so you can see how much of each \
             ability went where. When off, the halves only show in the hover tooltip.",
        );
    }
}
