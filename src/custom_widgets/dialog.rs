//! Shared dialog behaviour: Escape closes the window that is being drawn.
//!
//! Every window in the program offers the same key rather than each one
//! deciding for itself, so a reader who has learnt it in one dialog has learnt
//! it in all of them. The main window is not a dialog and is left alone.

use std::sync::LazyLock;

use eframe::egui::{Context, Id, Key, Modifiers};

/// Whether anything had the keyboard focus when the previous pass ended. Written
/// every pass a dialog is up, read on the pass Escape arrives — see below for
/// why it cannot simply be asked of egui at that moment.
static FOCUS_LAST_PASS: LazyLock<Id> = LazyLock::new(|| Id::new(module_path!()).with("focus"));

/// Whether Escape was pressed and no dialog has taken it yet this pass.
///
/// **Taken, not read.** The first caller of a pass gets it and every later one
/// sees nothing, which decides what happens with several dialogs open: a window
/// drawn *inside* another asks before the window it stands in — its contents run
/// inside the outer window's closure — so Escape puts the inner one away and
/// leaves its parent up. Two dialogs side by side are resolved by which is drawn
/// first, since egui keeps closed windows in its layer order and cannot be asked
/// which visible window is on top.
///
/// **A field being typed in is given up first**, so a rule name half written is
/// not thrown away by a key pressed to get out of the field; the next Escape
/// closes the dialog. That question has to be answered from the *previous* pass:
/// egui drops the keyboard focus itself the moment Escape arrives
/// (`Focus::begin_pass`) and passes the key on anyway, so by the time a dialog
/// asks, every field looks unfocused and the two acts are indistinguishable.
/// `TextEdit` does not take the key either.
pub fn escape_closes(ctx: &Context) -> bool {
    let had_focus = ctx
        .data(|d| d.get_temp::<bool>(*FOCUS_LAST_PASS))
        .unwrap_or(false);
    let focused_now = ctx.memory(|m| m.focused()).is_some();
    ctx.data_mut(|d| d.insert_temp(*FOCUS_LAST_PASS, focused_now));

    if !ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        return false;
    }

    !had_focus
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, RawInput, TextEdit};

    fn escape() -> RawInput {
        let mut input = RawInput::default();
        input.events.push(Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        input
    }

    /// One press is one close: the second dialog to ask sees nothing, so two
    /// windows up at once do not both go away on a single key.
    #[test]
    fn escape_is_taken_by_the_first_dialog_that_asks() {
        let ctx = Context::default();
        let mut answers = Vec::new();
        let _ = ctx.run_ui(escape(), |ui| {
            answers.push(escape_closes(ui.ctx()));
            answers.push(escape_closes(ui.ctx()));
        });

        assert_eq!(vec![true, false], answers);
    }

    /// Nothing pressed, nothing closed — the key is not remembered between
    /// passes.
    #[test]
    fn nothing_closes_without_the_key() {
        let ctx = Context::default();
        let mut closed = true;
        let _ = ctx.run_ui(RawInput::default(), |ui| {
            closed = escape_closes(ui.ctx());
        });

        assert!(!closed);
    }

    /// Escape pressed while a field is being typed in leaves the field and keeps
    /// the dialog open; the next one closes it. Without this, Escape pressed to
    /// get out of a half-written rule name would discard every change the dialog
    /// holds — egui has already dropped the focus by then, so the dialog cannot
    /// tell that pass from any other.
    #[test]
    fn the_field_is_given_up_before_the_dialog() {
        let ctx = Context::default();

        /// One pass of a dialog holding a text field, answering whether Escape
        /// closed it.
        fn pass(ctx: &Context, input: RawInput, focus: bool) -> bool {
            let mut text = String::new();
            let mut closed = false;
            let _ = ctx.run_ui(input, |ui| {
                let response = ui.add(TextEdit::singleline(&mut text));
                if focus {
                    response.request_focus();
                }
                closed = escape_closes(ui.ctx());
            });
            closed
        }

        // A pass with the field focused and no key pressed, which is what the
        // dialog is in while a name is being typed.
        pass(&ctx, RawInput::default(), true);
        assert!(
            !pass(&ctx, escape(), false),
            "the first Escape leaves the field, it does not close the dialog"
        );
        assert!(
            pass(&ctx, escape(), false),
            "with nothing focused, Escape closes the dialog"
        );
    }
}
