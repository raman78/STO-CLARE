//! The numbers that were **chosen** rather than derived: sizes, limits and the
//! room the window leaves for things.
//!
//! They are gathered here because they are the ones somebody comes looking for.
//! A number that falls out of the code — an index, a divisor, a count of
//! something — stays where it is used; a number that was settled by measuring
//! the program, or by taste, lives in this file with the reason it has the
//! value it has. Changing one here changes it everywhere it is read.
//!
//! `MAX_NOTE_CHARS` is the exception to "defined here": it truncates what is
//! *stored*, so it belongs beside the store that enforces it
//! ([`crate::app::settings::CombatNotes::set`]). It is re-exported so that
//! everything a maintainer might want to change can still be reached through
//! one module.

use eframe::egui::{TextStyle, Ui, Vec2, vec2};

pub use crate::app::settings::MAX_NOTE_CHARS;

// ── The list of fights ──────────────────────────────────────────────────────

/// The height of one row of the combats table, and of its heading.
pub const ROW_HEIGHT: f32 = 25.0;
pub const HEADER_HEIGHT: f32 = 25.0;

/// The narrowest the combats panel may be dragged. It still holds the map name
/// and the DPS beside it; below that the table would be a column of ellipses.
pub const PANEL_MIN_WIDTH: f32 = 260.0;

/// How wide the combats panel will make *itself* to fit its table. Enough for
/// the longest map the program knows — "[TFO] Nukara Prime: Transdimensional
/// Tactics" — beside every other column, including the two a comparison adds
/// and the room a note is given (measured; see `print_column_widths`). Past
/// this it stops widening on its own, because a list that takes the whole
/// window leaves nothing to read a fight in.
pub const PANEL_AUTO_WIDTH: f32 = 1200.0;

/// How much of the window the reader may **drag** the combats panel across.
///
/// Widening itself and being widened are two different things: the panel stops
/// growing on its own at [`PANEL_AUTO_WIDTH`], because nobody asked for a list
/// that swallows the window, but a reader who wants the whole note column and
/// every player of a team run open is asking for exactly that and should get
/// it. Three quarters leaves the tabs beside it readable — enough to see which
/// fight is open and what its Total says — which is what stops the panel being
/// dragged over the thing it is for.
///
/// A quarter of a narrow window can be less than [`PANEL_MIN_WIDTH`]; the
/// minimum wins there, since a panel below it is a column of ellipses.
pub const PANEL_MAX_SHARE: f32 = 0.75;

/// The gap either side of a column's contents in the combats table. Narrower
/// than the tables' own default: a column holding "Solo" carried five points of
/// gap on each side, which is a third as much again as the word.
pub const CELL_SPACING: f32 = 3.0;

/// How wide the picker that says whose figures a run is read for is drawn. Wide
/// enough for an ordinary handle without the column following the longest one
/// in the log about.
pub const PLAYER_PICKER_WIDTH: f32 = 130.0;

/// How much room a run's number keeps around itself inside its badge.
pub const BADGE_PADDING: f32 = 4.0;

/// The size of the fold-out arrow, matching the one in the damage tables.
pub const ARROW_SIZE: Vec2 = vec2(14.0, 14.0);

// ── The filter menus ────────────────────────────────────────────────────────

/// How wide the deaths menu is drawn. Enough for an ordinary handle beside its
/// tick box, and for the line above them to read in two or three.
pub const DEATHS_MENU_WIDTH: f32 = 230.0;

/// How tall it may grow before it scrolls: a log of a year's play holds more
/// handles than fit on screen, and a menu taller than the panel is unusable.
pub const DEATHS_MENU_HEIGHT: f32 = 260.0;

/// How wide the menu of what dealt the damage is drawn. Wider than the deaths
/// menu because what it lists is longer: a full weapon name off a real build
/// runs to "Omni-Directional Trilithium-Enhanced Phaser Beam Array", and a name
/// wrapped over two lines beside a tick box cannot be read down a column.
pub const DEALT_BY_MENU_WIDTH: f32 = 360.0;

/// The narrowest a filter picker may be squeezed to — what it needs to be
/// pointed at at all.
pub const PICKER_MIN_WIDTH: f32 = 60.0;

// ── Windows ─────────────────────────────────────────────────────────────────

/// How wide the window that reports clearing the log is drawn. Enough for the
/// longest of the lines it says without the window resizing itself under the
/// reader as one phase follows another.
pub const JOB_WINDOW_WIDTH: f32 = 260.0;

// ── Rules, rather than raw numbers ──────────────────────────────────────────

/// The slack over a run of digits that covers letters wider than one. The face
/// is proportional and no width fits every possible fifty characters, so this
/// is a judgement: enough for ordinary prose, not enough for fifty `M`s.
const NOTE_WIDTH_SLACK: f32 = 1.2;

/// Room for a whole note at once.
///
/// Measured from the font in use rather than fixed, so it holds at any UI
/// scale. Both places that show a note ask for the same width — the field under
/// the tabs where one is written, and the column in the list of fights, which
/// **reserves** it whether or not there is a note to show. A column that sized
/// itself to its contents was a sliver on a log with no notes in it, and moved
/// every other column sideways the moment one was written.
pub fn note_width(ui: &Ui) -> f32 {
    let digit = ui.fonts_mut(|fonts| fonts.glyph_width(&TextStyle::Body.resolve(ui.style()), '0'));
    digit * MAX_NOTE_CHARS as f32 * NOTE_WIDTH_SLACK
}
