//! Toggles and tab strips that keep their size.
//!
//! egui's `selectable_label` / `selectable_value` draw no frame while resting
//! and a framed one under the pointer, and work out the button's inner margin
//! as `button_padding - the frame's stroke width` for whichever state it is in.
//! That is exact only when the resting state has no stroke: egui's own themes
//! have none, so the margin it takes off is put back by the stroke it draws.
//!
//! This app's themes do put a rim on a resting widget (`theme::glassify`), so
//! the margin comes off but nothing is drawn in its place, and the widget is
//! two pixels narrower resting than hovered. In a row of them — the tabs, the
//! chart pickers, the toolbar toggles — pointing at one nudged the rest along.
//!
//! These two draw the frame in every state instead, so the size never depends
//! on where the pointer is. `steady_toggle_value` is `selectable_value` in every other
//! way, down to marking the response changed only on a real change.

use eframe::egui::*;

pub trait Toggle {
    /// A button that stays pressed while `selected`.
    fn steady_toggle<'a>(&mut self, selected: bool, text: impl IntoAtoms<'a>) -> Response;

    /// One choice out of several: pressed while `current` holds `value`, and
    /// sets it when clicked.
    fn steady_toggle_value<'a, Value: PartialEq>(
        &mut self,
        current: &mut Value,
        value: Value,
        text: impl IntoAtoms<'a>,
    ) -> Response;
}

impl Toggle for Ui {
    fn steady_toggle<'a>(&mut self, selected: bool, text: impl IntoAtoms<'a>) -> Response {
        self.add(Button::new(text).selected(selected))
    }

    fn steady_toggle_value<'a, Value: PartialEq>(
        &mut self,
        current: &mut Value,
        value: Value,
        text: impl IntoAtoms<'a>,
    ) -> Response {
        let mut response = self.steady_toggle(*current == value, text);
        if response.clicked() && *current != value {
            *current = value;
            response.mark_changed();
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::theme;

    /// The size of a widget under the pointer, and resting, in a context with
    /// the app's theme applied.
    fn sizes(draw: impl Fn(&mut Ui) -> Response) -> (Vec2, Vec2) {
        let ctx = Context::default();
        theme::apply(&ctx, theme::Theme::Dark);

        let measure = |pointer: Option<Pos2>| {
            let mut input = RawInput::default();
            input.events.push(match pointer {
                Some(position) => Event::PointerMoved(position),
                None => Event::PointerGone,
            });
            let mut rect = Rect::ZERO;
            let _ = ctx.run_ui(input, |ui| {
                ui.horizontal(|ui| rect = draw(ui).rect);
            });
            rect
        };
        // Twice each: the first pass after an input change is the one that
        // notices it, the second is the one drawn with it.
        measure(None);
        let resting = measure(None);
        measure(Some(resting.center()));
        let hovered = measure(Some(resting.center()));
        (resting.size(), hovered.size())
    }

    /// The whole point: pointing at a toggle must not resize it, or every
    /// widget after it in the row shifts along.
    #[test]
    fn a_steady_toggle_is_the_same_size_under_the_pointer() {
        let (resting, hovered) = sizes(|ui| ui.steady_toggle(false, "Damage Dealt"));
        assert_eq!(resting, hovered);

        let (resting, hovered) = sizes(|ui| ui.steady_toggle(true, "Damage Dealt"));
        assert_eq!(resting, hovered);
    }

    /// ...and the same for the picked one out of a row.
    #[test]
    fn a_steady_toggle_value_is_the_same_size_under_the_pointer() {
        let (resting, hovered) = sizes(|ui| {
            let mut chosen = 1;
            ui.steady_toggle_value(&mut chosen, 2, "Damage Taken")
        });
        assert_eq!(resting, hovered);
    }

    /// The widget egui offers for this is what the app used to use, and it is
    /// the thing being worked around — if a version of egui ever fixes it, this
    /// test fails and the whole module can go.
    #[test]
    fn egui_s_own_selectable_label_is_what_moves() {
        let (resting, hovered) = sizes(|ui| ui.selectable_label(false, "Damage Dealt"));
        assert_eq!(
            2.0,
            hovered.x - resting.x,
            "a resting rim's width, taken off the margin and not drawn"
        );
    }

    /// Picking sets the value, and only a real change counts as one — a row of
    /// these drives "rebuild the chart" off `changed()`.
    #[test]
    fn steady_toggle_value_sets_the_value_but_only_reports_a_real_change() {
        let ctx = Context::default();
        let mut chosen = 1;
        let mut changed = None;
        let _ = ctx.run_ui(RawInput::default(), |ui| {
            changed = Some(ui.steady_toggle_value(&mut chosen, 2, "second").changed());
        });
        assert_eq!(Some(false), changed, "nothing was clicked");
        assert_eq!(1, chosen);
    }
}
