//! Tooltips that sit close to what they explain.
//!
//! egui puts a tooltip four points below the widget it belongs to, and the
//! figure is written into `Tooltip::for_widget` rather than taken from the
//! style, so there is no way to ask for less. Across a table of numbers that
//! gap reads as a box floating between two rows: near enough to be in the way,
//! far enough to leave which row it is about in doubt.
//!
//! [`CloseTooltip`] is `Response::on_hover_text` and its two siblings with the
//! gap closed to [`GAP`]. Everything else — the delay, the width, when a
//! disabled widget explains itself — is egui's own and is left alone; the
//! bodies below are its own, with one number changed.

use eframe::egui::{Label, Response, Tooltip, Ui, WidgetText};

/// How far a tooltip sits from the widget it is about. Not zero: the tooltip
/// draws its own frame, and a frame flush against a table row's edge reads as
/// part of the row rather than as something laid over it.
const GAP: f32 = 1.0;

pub trait CloseTooltip {
    /// [`Response::on_hover_text`], drawn close to the widget.
    fn hover(self, text: impl Into<WidgetText>) -> Response;

    /// [`Response::on_disabled_hover_text`], drawn close to the widget.
    fn disabled_hover(self, text: impl Into<WidgetText>) -> Response;

    /// [`Response::on_hover_ui`], drawn close to the widget.
    fn hover_ui(self, add_contents: impl FnOnce(&mut Ui)) -> Response;
}

impl CloseTooltip for Response {
    fn hover(self, text: impl Into<WidgetText>) -> Response {
        self.hover_ui(|ui| label(ui, text))
    }

    fn hover_ui(self, add_contents: impl FnOnce(&mut Ui)) -> Response {
        Tooltip::for_enabled(&self).gap(GAP).show(add_contents);
        self
    }

    fn disabled_hover(self, text: impl Into<WidgetText>) -> Response {
        Tooltip::for_disabled(&self)
            .gap(GAP)
            .show(|ui| label(ui, text));
        self
    }
}

/// The label a text tooltip holds. The width is pinned as egui pins it: an
/// `Area` sizing itself to its contents shrinks a tooltip whose text changes
/// (egui#5167).
fn label(ui: &mut Ui, text: impl Into<WidgetText>) {
    ui.set_max_width(ui.spacing().tooltip_width);
    ui.add(Label::new(text));
}
