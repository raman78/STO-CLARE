//! A button that opens a small window under itself.
//!
//! The button is a toggle: it stays lit while its window is up, and pressing it
//! again puts the window away. Without that, the only way to close one was to
//! click somewhere else entirely, and nothing on screen said which button the
//! window belonged to.

use std::hash::Hash;

use eframe::egui::{Button, Id, InnerResponse, Ui, WidgetText, Window};

pub struct PopupButton {
    title: WidgetText,
    id: Option<Id>,
}

#[derive(Default, Clone, Copy, Debug)]
struct PopupButtonState {
    open: bool,
}

impl PopupButton {
    pub fn new(title: impl Into<WidgetText>) -> Self {
        let title = title.into();
        Self { title, id: None }
    }

    #[allow(dead_code)]
    pub fn with_id_source(mut self, source: impl Hash) -> Self {
        self.id = Some(Id::new(source));
        self
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<Option<R>> {
        let Self { title, id } = self;
        let id = id.unwrap_or(ui.next_auto_id()).with(module_path!());
        let mut state = PopupButtonState::load(ui, id);

        let button_response = ui.add(Button::new(title).selected(state.open));
        if button_response.clicked() {
            state.open = !state.open;
        }

        if !state.open {
            state.store(ui, id);
            return InnerResponse::new(None, button_response);
        }

        let inner = Window::new("")
            .id(id.with("__popup_window"))
            .title_bar(false)
            .collapsible(false)
            .auto_sized()
            .resizable(false)
            .constrain(true)
            .default_pos([button_response.rect.min.x, button_response.rect.max.y])
            .show(ui.ctx(), add_contents)
            .unwrap();

        if !button_response.clicked()
            && inner.response.clicked_elsewhere()
            && let Some(cursor_pos) = ui.input(|i| i.pointer.latest_pos())
            && !inner.response.rect.contains(cursor_pos)
        {
            // TODO find a way not to close when something inside was clicked (e.g. a combo box)
            state.open = false;
        }

        state.store(ui, id);
        InnerResponse::new(Some(inner.inner.unwrap()), button_response)
    }
}

impl PopupButtonState {
    fn load(ui: &mut Ui, id: Id) -> Self {
        ui.ctx()
            .data_mut(|d| d.get_temp::<PopupButtonState>(id).unwrap_or_default())
    }

    fn store(self, ui: &mut Ui, id: Id) {
        ui.ctx().data_mut(|d| d.insert_temp(id, self));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, Event, PointerButton, Pos2, RawInput, Vec2};

    /// Draws the button and reports whether its window was up this frame.
    fn frame(ctx: &Context, click_at: Option<Pos2>) -> (bool, Pos2) {
        let mut input = RawInput::default();
        if let Some(position) = click_at {
            input.events.push(Event::PointerMoved(position));
            for pressed in [true, false] {
                input.events.push(Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
        }
        let mut open = false;
        let mut center = Pos2::ZERO;
        let _ = ctx.run_ui(input, |ui| {
            let response = PopupButton::new("⛭").with_id_source("test").show(ui, |ui| {
                ui.label("contents");
            });
            open = response.inner.is_some();
            center = response.response.rect.center();
        });
        (open, center)
    }

    /// The icon is a toggle: the first press opens its window, the second puts
    /// it away again. It used to only ever open, so the window could be closed
    /// only by clicking somewhere else entirely.
    #[test]
    fn pressing_the_icon_opens_and_closes_it() {
        let ctx = Context::default();
        let (open, button) = frame(&ctx, None);
        assert!(!open, "closed to begin with");

        // Away from the button first, so the pointer is somewhere before the
        // click lands on it.
        let (open, _) = frame(&ctx, Some(button + Vec2::splat(200.0)));
        assert!(!open);

        let (open, _) = frame(&ctx, Some(button));
        assert!(open, "the first press opens it");

        let (open, _) = frame(&ctx, Some(button));
        assert!(!open, "the second press closes it");

        let (open, _) = frame(&ctx, Some(button));
        assert!(open, "and it opens again");
    }
}
