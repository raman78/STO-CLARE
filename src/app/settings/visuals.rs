use eframe::egui::{ComboBox, Context, Ui};

use crate::{
    app::{overlay::{MIN_OPACITY, Overlay}, theme},
    custom_widgets::slider_text_edit::SliderTextEdit,
};

use super::Settings;

#[derive(Default)]
pub struct VisualsTab {}

impl VisualsTab {
    pub fn show(&mut self, modified_settings: &mut Settings, ui: &mut Ui) {
        let visuals = &mut modified_settings.visuals;
        ui.label("Theme");
        ComboBox::from_id_salt("theme combo box")
            .selected_text(visuals.theme.display())
            // Straight from the registry, so a theme added there shows up here.
            .show_ui(ui, |ui| {
                for entry in theme::THEMES.iter() {
                    if ui
                        .selectable_value(&mut visuals.theme, entry.theme, entry.name)
                        .changed()
                    {
                        Self::set_theme(ui.ctx(), visuals.theme);
                    }
                }
            });

        ui.add_space(10.0);
        ui.separator();

        ui.label("Overlay Opacity");
        SliderTextEdit::new(
            &mut visuals.overlay_opacity,
            MIN_OPACITY..=1.0,
            "overlay opacity slider",
        )
        .clamp_min(MIN_OPACITY)
        .clamp_max(1.0)
        .step_by(0.05)
        .display_precision(3)
        .desired_text_edit_width(40.0)
        .show(ui)
        .on_hover_text(
            "How solid the overlay is over the game. Only the overlay is affected.",
        );

        ui.add_space(10.0);
        ui.separator();

        ui.label("UI Scale");
        let response = SliderTextEdit::new(&mut visuals.ui_scale, 0.5..=3.0, "ui scale slider")
            .clamp_to_range(false)
            .clamp_min(0.5)
            .clamp_max(10.0)
            .step_by(0.1)
            .display_precision(4)
            .desired_text_edit_width(40.0)
            .show(ui);
        if response.drag_stopped() || response.lost_focus() {
            Self::set_ui_scale(
                ui.ctx(),
                ui.ctx().native_pixels_per_point(),
                visuals.ui_scale,
            );
        }
    }

    pub fn update_visuals(
        &mut self,
        ctx: &Context,
        native_pixels_per_point: Option<f32>,
        settings: &Settings,
    ) {
        let visuals = &settings.visuals;
        Self::set_theme(ctx, visuals.theme);
        Self::set_ui_scale(ctx, native_pixels_per_point, visuals.ui_scale);
    }

    fn set_theme(ctx: &Context, selected: theme::Theme) {
        theme::apply(ctx, selected);
        Overlay::request_repaint(ctx);
    }

    fn set_ui_scale(ctx: &Context, native_pixels_per_point: Option<f32>, ui_scale: f64) {
        ctx.set_pixels_per_point(native_pixels_per_point.unwrap_or(1.0) * ui_scale as f32);
    }
}
