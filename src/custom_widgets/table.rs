use eframe::{egui::*, emath::GuiRounding};

/// Which column a table is ordered by, and which way round.
///
/// Lives here rather than in each table because every table in the program is
/// meant to behave the same way: click a heading to order by it, click again to
/// turn it round, and see at a glance which one is doing the ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortState<C> {
    /// The column, once one has been picked.
    pub column: Option<C>,
    /// Whether the order runs the way the column reads it — largest first for
    /// most, smallest first for the few where small is good.
    pub natural: bool,
}

impl<C> Default for SortState<C> {
    fn default() -> Self {
        // Nothing picked yet, and the first pick reads the column the way the
        // column itself reads it.
        Self {
            column: None,
            natural: true,
        }
    }
}

impl<C: PartialEq + Copy> SortState<C> {
    /// Take a click on `column`: pick it, or turn the order round if it is
    /// already the one in charge.
    pub fn clicked(&mut self, column: C) {
        self.natural = self.column != Some(column) || !self.natural;
        self.column = Some(column);
    }

    /// Whether `column` is the one the rows are ordered by.
    pub fn is_sorted_by(&self, column: C) -> bool {
        self.column == Some(column)
    }

    /// The mark to draw beside a heading: which way the order runs, or nothing
    /// on a column that is not doing the ordering.
    pub fn marker(&self, column: C) -> &'static str {
        if !self.is_sorted_by(column) {
            return "";
        }
        if self.natural {
            SORT_MARKERS[0]
        } else {
            SORT_MARKERS[1]
        }
    }
}

/// Every mark a heading can end up carrying — the order running the column's
/// way, and the other way round.
pub const SORT_MARKERS: [&str; 2] = ["⏷", "⏶"];

/// The room the sort mark needs, whichever of the two it turns out to be.
///
/// A heading keeps this room whether or not it is the one ordering the rows, so
/// that clicking it does not widen its column — a long heading used to push the
/// numbers under it sideways the moment it took charge of the order.
pub fn sort_marker_width(ui: &Ui) -> f32 {
    SORT_MARKERS
        .iter()
        .map(|marker| text_width(ui, marker))
        .fold(0.0, f32::max)
}

/// How wide a piece of text is in the body style, unwrapped.
pub fn text_width(ui: &Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_string(),
            TextStyle::Body.resolve(ui.style()),
            Color32::PLACEHOLDER,
        )
        .size()
        .x
}

/// Draw a piece of text that is allowed to be cut short, and say how much room
/// it was short of.
///
/// For the cells of a frozen column, which can be narrowed to fit the view. A
/// label left to run past its column would be drawn over what is beside it;
/// truncated, it fits — but then it reports the width it was cut *to*, and a
/// column measured from that could never widen again when the room came back.
/// The shortfall is what [`TableRow::measured_cell`] has to be told.
///
/// A name that did not fit is not a name the reader has to do without: what was
/// cut off is on the tooltip, so pointing at the row still answers what it is.
pub fn show_truncated(ui: &mut Ui, text: impl Into<WidgetText>) -> f32 {
    let text: WidgetText = text.into();
    let whole = text.text().to_string();
    let wanted = text_width(ui, &whole);
    let response = ui.add(Label::new(text).truncate());
    let cut = (wanted - response.rect.width()).max(0.0);
    if cut > 0.0 {
        response.on_hover_text(whole);
    }
    cut
}

/// How wide the widest row of a tree of names would be with every branch of it
/// open: the indent that row sits at, plus what it is called.
///
/// A tree column measured from the rows on screen is a column that grows the
/// moment a branch is opened, and takes every column right of it sideways with
/// it. Measured over the whole tree, open or not, it starts as wide as it will
/// ever need to be and then holds still.
///
/// What this leaves out is the room the open/close arrow and the gap after it
/// take. That much is the same on every row, and the row being drawn measures it
/// for real — see [`TableRow::measured_cell`] — so nothing here has to guess at
/// what a button comes to under the current theme.
///
/// Once per tree, not once per frame: a name is laid out to be measured, and a
/// tree is thousands of rows.
pub fn widest_name<T>(
    ui: &Ui,
    nodes: &[T],
    depth: f32,
    indent: f32,
    name: fn(&T) -> &str,
    children: fn(&T) -> &[T],
) -> f32 {
    nodes
        .iter()
        .map(|node| {
            (depth * indent + text_width(ui, name(node))).max(widest_name(
                ui,
                children(node),
                depth + 1.0,
                indent,
                name,
                children,
            ))
        })
        .fold(0.0, f32::max)
}

/// The heading of a column that carries buttons beside its name — the Name
/// column, with the eye and the type picker next to the word.
///
/// The whole cell orders the rows, like every other heading: it fills while it
/// is the one in charge, rims under the pointer and takes the sort mark at its
/// right-hand edge. The buttons are drawn on top of it and are widgets in their
/// own right, so a click on one of them is that button's, not the heading's.
pub fn show_sortable_header_cell(
    ui: &mut Ui,
    picked: bool,
    marker: &str,
    text: &str,
    buttons: impl FnOnce(&mut Ui),
) -> Response {
    let wanted = text_width(ui, text) + sort_marker_width(ui) + ui.spacing().item_spacing.x;
    show_header_cell(ui, wanted, true, picked, marker, text, buttons)
}

/// The same, at a width the caller has worked out.
///
/// For a table whose columns are sized by what a column *can* hold rather than
/// by its heading: a column of four-letter words should not carry the room for
/// a mark only one heading at a time ever draws.
pub fn show_sortable_header_cell_sized(
    ui: &mut Ui,
    wanted: f32,
    picked: bool,
    marker: &str,
    text: &str,
    buttons: impl FnOnce(&mut Ui),
) -> Response {
    show_header_cell(ui, wanted, false, picked, marker, text, buttons)
}

/// `keep_marker_room` holds the mark's width beside a heading that is not
/// carrying one, so whatever is drawn after it — the eye and the type picker on
/// the damage tables' Name column — does not move when the ordering changes
/// hands. A heading with nothing after it has nothing to hold still, and the
/// room would only make its column wider.
#[allow(clippy::too_many_arguments)]
fn show_header_cell(
    ui: &mut Ui,
    wanted: f32,
    keep_marker_room: bool,
    picked: bool,
    marker: &str,
    text: &str,
    buttons: impl FnOnce(&mut Ui),
) -> Response {
    // The whole cell, top to bottom, the way every other heading works.
    let height = ui.available_height().max(ui.spacing().interact_size.y);
    let width = ui.available_width().max(wanted);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    draw_cell_visuals(ui, picked, &response);
    // Drawn after the strip, so the pointer finds them first: egui gives a click
    // to the widget on top, and these are meant to be pressed on their own.
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal_centered(|ui| {
            ui.label(text);
            // Beside the word rather than at the column's edge, which on this
            // column is where the buttons are: a mark drawn there sat on top of
            // one. The room is kept either way, so nothing shifts when the
            // column takes charge of the order.
            match (marker.is_empty(), keep_marker_room) {
                (true, true) => ui.add_space(sort_marker_width(ui)),
                (true, false) => (),
                (false, _) => {
                    ui.label(marker);
                }
            }
            buttons(ui);
        });
    });
    response
}

/// Draw the sort mark against the right-hand edge of a heading, where the
/// numbers under it end, rather than trailing the words — the mark is then
/// looked for in one place down the row of headings instead of wherever a name
/// happens to finish.
pub fn show_sort_marker(ui: &mut Ui, rect: Rect, marker: &str) {
    if marker.is_empty() {
        return;
    }
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            ui.label(marker);
        },
    );
}

/// The most of the view a frozen strip is allowed to take before the widest
/// column in it is narrowed to fit.
///
/// A frozen column never leaves the screen, so an over-wide one is a column of
/// names with no room left for the figures they are about — which is the whole
/// reason the table is being read.
const FROZEN_MAX_SHARE: f32 = 0.4;

/// How narrow a frozen column may be made, whatever that share works out to. A
/// name cut to nothing tells the reader less than no name at all.
const FROZEN_MIN_WIDTH: f32 = 60.0;

/// How thick the rule along the edge of the frozen strip is, against the one
/// point every other rule between columns is drawn at.
const FREEZE_RULE_WIDTH: f32 = 2.0;

/// How much of the theme's accent that rule carries.
///
/// The accent — `hyperlink_color` — is the one colour every one of the themes
/// declares as bright enough for its own background, which is why the rims
/// elsewhere in the program are taken from it as well; a fixed grey would suit
/// two themes and vanish into the rest. Faded, because the edge of the strip is
/// something to notice, not something that has been picked.
const FREEZE_RULE_ACCENT: f32 = 0.65;

pub struct Table<'a> {
    ui: &'a mut Ui,
    id: Id,
    min_scroll_height: f32,
    max_scroll_height: f32,
    cell_spacing: f32,
    striped: bool,
    /// Whether the table takes only the width its columns come to. See
    /// [`Table::shrink_to_content`].
    shrink_to_content: bool,
    /// Where the row's name ends and its figures begin, and whether the
    /// columns before it stay put. See [`Table::divided_after`] and
    /// [`Table::frozen_columns`].
    divide: Divide,
}

pub struct TableWithHeader<'a> {
    table: Table<'a>,
    state: State,
    /// The space kept for the header row, which is drawn after the body.
    header_rect: Rect,
    header_height: f32,
}

/// A table whose rows are drawn and whose header is still to come.
pub struct HeaderSlot<'a> {
    ui: &'a mut Ui,
    id: Id,
    state: State,
    header_rect: Rect,
    header_height: f32,
    cell_spacing: f32,
    body_rect: Rect,
    /// The columns that stay put, and how far the rest have been dragged.
    divide: Divide,
    /// Where the columns begin on screen — see [`BodyOutput::columns_left`].
    columns_left: f32,
    /// How wide the view is — see [`BodyOutput::view_width`].
    view_width: f32,
}

pub struct TableBody<'a> {
    ui: &'a mut Ui,
    row_height: f32,
    cell_spacing: f32,
    striped: bool,
    state: &'a mut State,
    current_row: usize,
    left_top: Pos2,
    divide: Divide,
}

pub struct TableRow<'a> {
    ui: &'a mut Ui,
    state: &'a mut State,
    current_column: usize,
    left_top: Pos2,
    left_offset: f32,
    row_height: f32,
    cell_spacing: f32,
    divide: Divide,
    /// Where the frozen strip ends on screen, when there is one. The columns
    /// that scroll are clipped to the right of it, so they pass *under* the
    /// frozen ones.
    clip_left: Option<f32>,
    /// Whether this row is the header, whose claims are the floor a frozen
    /// column may not be narrowed below — a heading carries the controls the
    /// column is worked by, and those cannot be cut off.
    is_header: bool,
}

/// Where a row stops naming itself and starts measuring itself.
///
/// Two things follow from it, and a table can ask for either the first alone or
/// both:
///
/// - **The rule.** One heavier line stands at the divide and nothing else may
///   draw a boundary against it (`is_divide`, `TableRow::opens_after_divide`).
/// - **The strip.** The columns left of the divide can be held on screen while
///   the rest scroll under them (`frozen`). A frozen cell is laid out where it
///   always was — in the table's own coordinates, after the columns before it —
///   and then pushed back to the right by exactly what the body has been
///   scrolled, which lands it at the left edge of the view whatever the reader
///   has dragged. The columns that scroll are clipped to the right of the strip,
///   so nothing of theirs is ever drawn on top of a frozen cell and nothing of
///   theirs can be clicked through one: egui intersects a widget's interaction
///   rectangle with the clip rectangle (`Ui::interact`), so clipping takes the
///   pointer with it.
#[derive(Debug, Default, Clone, Copy)]
struct Divide {
    /// How many columns at the left name the row rather than measure it. Zero
    /// for a table that has asked for neither the rule nor the strip, which is
    /// most of them.
    columns: usize,
    /// Whether those columns stay on screen while the rest scroll under them.
    frozen: bool,
    /// How far a frozen cell is pushed right — the distance the body has been
    /// scrolled sideways. Zero while the columns are not frozen, since then
    /// they travel with everything else.
    shift: f32,
}

impl Divide {
    /// Whether the column at `index` is one of the ones that stay put.
    fn holds(&self, index: usize) -> bool {
        self.frozen && index < self.columns
    }

    /// Whether the rule that follows the column at `index` is the divide.
    fn is_divide(&self, index: usize) -> bool {
        self.columns > 0 && index + 1 == self.columns
    }

    /// How wide the strip of frozen columns comes to, spacing included.
    fn strip_width(&self, columns: &[ColumnState], cell_spacing: f32) -> f32 {
        columns
            .iter()
            .take(self.columns)
            .map(|column| column.last_size + 2.0 * cell_spacing)
            .sum()
    }

    /// Where the strip ends on screen, given where the table's columns begin.
    fn strip_right(&self, columns: &[ColumnState], cell_spacing: f32, columns_left: f32) -> f32 {
        columns_left + self.shift + self.strip_width(columns, cell_spacing)
    }

    /// Where the columns that scroll are cut off — `None` when nothing is
    /// frozen, and so nothing for them to pass under.
    fn clip_at(
        &self,
        columns: &[ColumnState],
        cell_spacing: f32,
        columns_left: f32,
    ) -> Option<f32> {
        (self.frozen && self.columns > 0)
            .then(|| self.strip_right(columns, cell_spacing, columns_left))
    }
}

#[derive(Debug, Default, Clone)]
struct State {
    columns: Vec<ColumnState>,
    size: Vec2,
    last_size: Vec2,
}

#[derive(Debug, Default, Clone)]
struct ColumnState {
    size: f32,
    /// The widest claim the header row made on this column, which is as narrow
    /// as a frozen column may be made: the heading carries the controls the
    /// column is worked by (the eye, the type picker), and a heading cut off is
    /// a table that cannot be used.
    floor: f32,
    last_size: f32,
    /// Whether this column and the next are under one heading, in which case no
    /// rule is drawn between them: a value and its difference are two columns so
    /// that both line up, but they read as one.
    merged_with_next: bool,
}

#[allow(dead_code)]
impl<'a> Table<'a> {
    pub fn new(ui: &'a mut Ui) -> Self {
        let id = ui.id().with(module_path!());
        Self {
            ui,
            id,
            min_scroll_height: 0.0,
            max_scroll_height: f32::INFINITY,
            cell_spacing: 5.0,
            striped: true,
            shrink_to_content: false,
            divide: Divide::default(),
        }
    }

    /// Keep the first `count` columns on screen while the rest scroll sideways
    /// under them, the way a spreadsheet freezes the panes left of a split.
    ///
    /// For a table wide enough to be dragged: the row a figure belongs to is
    /// named in the first column, and once that column has scrolled away a
    /// screen of numbers says nothing about what any of them is of.
    ///
    /// The strip is held to [`FROZEN_MAX_SHARE`] of the view — a column of long
    /// ability names would otherwise leave no room for the figures — by
    /// narrowing the widest column in it, but never past what its heading needs
    /// nor below [`FROZEN_MIN_WIDTH`].
    pub fn frozen_columns(mut self, count: usize) -> Self {
        self.divide = Divide {
            columns: count,
            frozen: true,
            shift: 0.0,
        };
        self
    }

    /// Draw the divide after `count` columns without holding them on screen.
    ///
    /// For a table wide enough to want the boundary marked but not wide enough
    /// to be dragged far — the summary is one row per player, and freezing the
    /// player's name would only cost the figures beside it room in a small
    /// window.
    pub fn divided_after(mut self, count: usize) -> Self {
        self.divide = Divide {
            columns: count,
            frozen: false,
            shift: 0.0,
        };
        self
    }

    /// Take no more width than the columns come to.
    ///
    /// The default is the other way round — a table fills the panel it is in, so
    /// its scroll bar sits at the edge of that panel rather than tucked against
    /// the last column. In a window sized to what it holds that is backwards:
    /// the table would ask for everything on offer, and the window would open as
    /// wide as the screen around three columns of figures.
    pub fn shrink_to_content(mut self) -> Self {
        self.shrink_to_content = true;
        self
    }

    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = id.into();
        self
    }

    pub fn min_scroll_height(mut self, min_scroll_height: f32) -> Self {
        self.min_scroll_height = min_scroll_height;
        self
    }

    pub fn max_scroll_height(mut self, max_scroll_height: f32) -> Self {
        self.max_scroll_height = max_scroll_height;
        self
    }

    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    pub fn cell_spacing(mut self, cell_spacing: f32) -> Self {
        self.cell_spacing = cell_spacing;
        self
    }

    /// Keep the room a header row needs. The row itself is drawn last, by
    /// [`HeaderSlot::header_row`] — see [`TableWithHeader::body`] for why.
    ///
    /// The room is as wide as the columns were last frame, never as wide as the
    /// space on offer: the overlay sizes its window to what the table asks for,
    /// so a header that took everything available grew the window, which offered
    /// more, and the window ran away across the screen.
    pub fn header(self, header_height: f32) -> TableWithHeader<'a> {
        let mut state = State::load(self.ui, self.id);
        // Which columns are under one heading is said again by the header row
        // this frame, or not at all — a table that stops grouping must not keep
        // the last frame's groups.
        state.ungroup();
        // Narrower of the two: never wider than the columns, so the overlay's
        // window cannot grow itself, and never wider than the view, so the
        // scroll area is not pushed out past the right-hand edge with its bar.
        let width = state.last_size.x.min(self.ui.available_width());
        let (header_rect, _) = self
            .ui
            .allocate_exact_size(vec2(width, header_height), Sense::hover());

        TableWithHeader {
            table: self,
            state,
            header_rect,
            header_height,
        }
    }

    pub fn body(self, row_height: f32, add_body: impl FnOnce(&mut TableBody)) -> Rect {
        let Self {
            ui,
            id,
            min_scroll_height,
            max_scroll_height,
            striped,
            cell_spacing,
            shrink_to_content,
            divide,
        } = self;
        let mut state = State::load(ui, id);
        // A table with no header row has no groups; see `Table::header`.
        state.ungroup();
        let body = show_body(
            ui,
            id,
            &mut state,
            Body {
                row_height,
                min_scroll_height,
                max_scroll_height,
                striped,
                cell_spacing,
                shrink_to_content,
                divide,
            },
            add_body,
        );
        finish_table(
            ui,
            id,
            state,
            body.rect,
            body.columns_left,
            cell_spacing,
            body.divide,
            body.view_width,
        );
        body.rect
    }
}

impl<'a> TableWithHeader<'a> {
    /// The rows. The header goes on afterwards, through
    /// [`HeaderSlot::header_row`].
    ///
    /// That order is what keeps the header level with the columns under it. The
    /// table scrolls both ways in one area, so the vertical bar sits at the
    /// right-hand edge of the view and the horizontal bar along its bottom, the
    /// way a browser does it — but the header cannot be inside that area or it
    /// would scroll off the top. It is drawn afterwards instead, shifted by the
    /// offset the body has just settled on, so it follows sideways within the
    /// same frame. Drawn first it could only ever be given the previous frame's
    /// offset and would lag behind the columns while the table was dragged.
    pub fn body(self, row_height: f32, add_body: impl FnOnce(&mut TableBody)) -> HeaderSlot<'a> {
        let Self {
            table,
            mut state,
            header_rect,
            header_height,
        } = self;
        let Table {
            ui,
            id,
            min_scroll_height,
            max_scroll_height,
            striped,
            cell_spacing,
            shrink_to_content,
            divide,
        } = table;

        let body = show_body(
            ui,
            id,
            &mut state,
            Body {
                row_height,
                min_scroll_height,
                max_scroll_height,
                striped,
                cell_spacing,
                shrink_to_content,
                divide,
            },
            add_body,
        );

        HeaderSlot {
            ui,
            id,
            state,
            header_rect,
            header_height,
            cell_spacing,
            body_rect: body.rect,
            divide: body.divide,
            columns_left: body.columns_left,
            view_width: body.view_width,
        }
    }
}

impl<'a> HeaderSlot<'a> {
    /// Draws the header in the room kept for it, and finishes the table.
    pub fn header_row(self, add_header: impl FnOnce(&mut TableRow)) -> Rect {
        let Self {
            ui,
            id,
            mut state,
            header_rect,
            header_height,
            cell_spacing,
            body_rect,
            divide,
            columns_left,
            view_width,
        } = self;

        show_header(
            ui,
            &mut state,
            header_rect,
            header_height,
            cell_spacing,
            divide,
            add_header,
        );

        let full_rect = header_rect.union(body_rect);
        finish_table(
            ui,
            id,
            state,
            full_rect,
            columns_left,
            cell_spacing,
            divide,
            view_width,
        );
        full_rect
    }
}

/// What a body needs to draw itself, kept together so the two `body` methods
/// hand over the same thing.
struct Body {
    row_height: f32,
    min_scroll_height: f32,
    max_scroll_height: f32,
    striped: bool,
    cell_spacing: f32,
    shrink_to_content: bool,
    divide: Divide,
}

/// What a drawn body reports back to whoever has to line something up with it.
struct BodyOutput {
    /// The part of the rows that is on screen.
    rect: Rect,
    /// The columns that stayed put, and how far the rest were dragged.
    divide: Divide,
    /// Where the first column begins on screen. Left of the view — off the
    /// screen, even — once the table is scrolled sideways, which is exactly what
    /// makes it the right thing to place the rules between the columns against:
    /// the view's own left edge stays put while the columns move past it.
    columns_left: f32,
    /// How wide the view is, measured before the scroll area took it: what the
    /// frozen strip's share is worked out from, and what says whether the table
    /// has to scroll at all.
    view_width: f32,
}

/// Draws the rows, and reports where they landed.
fn show_body(
    ui: &mut Ui,
    id: Id,
    state: &mut State,
    body: Body,
    add_body: impl FnOnce(&mut TableBody),
) -> BodyOutput {
    let Body {
        row_height,
        min_scroll_height,
        max_scroll_height,
        striped,
        cell_spacing,
        shrink_to_content,
        divide,
    } = body;
    // Where the view begins and how wide it is, taken before the scroll area
    // claims the space: the frozen strip is measured against the view, and how
    // far the body has been dragged is the distance between this edge and where
    // the columns end up being laid out.
    let view = ui.available_rect_before_wrap();
    let scroll_output = ScrollArea::both()
        .id_salt(id.with("__table_scroll"))
        // Full width, whatever the columns come to: the scroll bar belongs at
        // the edge of the space the table was given, not tucked against the last
        // column with a stretch of empty panel beside it. A table that sizes a
        // window around itself asks for the other behaviour
        // (`Table::shrink_to_content`).
        .auto_shrink([shrink_to_content, true])
        .min_scrolled_height(min_scroll_height)
        .max_height(max_scroll_height)
        .show(ui, |ui| {
            let left_top = ui.cursor().left_top();
            // What the body was actually laid out with, rather than the
            // scroll area's stored offset: that one has this frame's wheel
            // already added to it, and a frozen column pushed by a distance the
            // rows were not moved by would drift away from them for as long as
            // the wheel was turning. Nothing to push where nothing is frozen.
            let divide = Divide {
                shift: if divide.frozen {
                    (view.left() - left_top.x).max(0.0)
                } else {
                    0.0
                },
                ..divide
            };
            let mut body = TableBody {
                current_row: 0,
                left_top,
                row_height,
                cell_spacing,
                striped,
                state,
                ui,
                divide,
            };

            add_body(&mut body);

            let rect = Rect::from_min_size(left_top, state.last_size);
            ui.allocate_rect(rect, Sense::hover());
            (rect, divide)
        });

    let (rect, divide) = scroll_output.inner;
    BodyOutput {
        rect: rect.intersect(scroll_output.inner_rect),
        divide,
        // Taken before the clip to the view: the columns start where the rows
        // were actually laid out, which is what they are drawn against.
        columns_left: rect.left(),
        view_width: view.width(),
    }
}

/// Draws the header row in the space reserved for it, shifted by how far the
/// body is scrolled sideways and clipped to that space, so a column heading
/// stops at the edge of the view rather than running over the panel beside it.
fn show_header(
    ui: &mut Ui,
    state: &mut State,
    header_rect: Rect,
    header_height: f32,
    cell_spacing: f32,
    divide: Divide,
    add_header: impl FnOnce(&mut TableRow),
) {
    let offset_x = divide.shift;
    let left_top = header_rect.left_top() - vec2(offset_x, 0.0);
    let mut header_ui = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(
        left_top,
        vec2(state.last_size.x.max(header_rect.width()), header_height),
    )));
    // Clipped to its own band vertically, but to the view horizontally: the
    // reserved rectangle is only as wide as the columns, and on a table's first
    // frame it has no width at all.
    let band = Rect::from_x_y_ranges(ui.clip_rect().x_range(), header_rect.y_range());
    header_ui.set_clip_rect(band.intersect(ui.clip_rect()));
    TableRow::show(
        &mut header_ui,
        state,
        0,
        left_top,
        header_height,
        cell_spacing,
        divide,
        true,
        add_header,
        false,
        None,
    );
}

/// The separators between column groups, and the state the next frame reads.
#[allow(clippy::too_many_arguments)]
fn finish_table(
    ui: &mut Ui,
    id: Id,
    state: State,
    rect: Rect,
    columns_left: f32,
    cell_spacing: f32,
    divide: Divide,
    view_width: f32,
) {
    ColumnState::draw_separators(&state.columns, ui, rect, columns_left, cell_spacing, divide);
    // Only a frozen column is held to a share of the view. One that scrolls away
    // with the rest is not keeping anything off the screen, so there would be
    // nothing to buy by cutting a name short in it.
    let capped = if divide.frozen { divide.columns } else { 0 };
    if state.finish(ui, id, cell_spacing, capped, view_width) {
        ui.ctx().request_repaint();
    }
}

impl<'a> TableBody<'a> {
    pub fn row(&mut self, add_cells: impl FnOnce(&mut TableRow)) -> Response {
        let response = TableRow::show(
            self.ui,
            self.state,
            self.current_row,
            self.left_top,
            self.row_height,
            self.cell_spacing,
            self.divide,
            false,
            add_cells,
            self.striped && self.current_row.is_multiple_of(2),
            None,
        );

        self.current_row += 1;

        response
    }

    pub fn selectable_row(
        &mut self,
        checked: bool,
        add_cells: impl FnOnce(&mut TableRow),
    ) -> Response {
        let response = TableRow::show(
            self.ui,
            self.state,
            self.current_row,
            self.left_top,
            self.row_height,
            self.cell_spacing,
            self.divide,
            false,
            add_cells,
            self.striped && self.current_row.is_multiple_of(2),
            Some(checked),
        );

        self.current_row += 1;

        response
    }
}

impl<'a> TableRow<'a> {
    // Drawing context threaded through; a struct of the same fields would
    // only move the list somewhere else.
    #[allow(clippy::too_many_arguments)]
    fn show(
        ui: &mut Ui,
        state: &mut State,
        row_index: usize,
        table_left_top: Pos2,
        row_height: f32,
        cell_spacing: f32,
        divide: Divide,
        is_header: bool,
        add_cells: impl FnOnce(&mut TableRow),
        is_stripe: bool,
        checked: Option<bool>,
    ) -> Response {
        let left_top = pos2(
            table_left_top.x,
            table_left_top.y + row_index as f32 * row_height,
        );
        let rect = Rect::from_min_size(left_top, vec2(state.last_size.x, row_height));
        let sense = if checked.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, ui.id().with(row_index), sense);

        draw_visuals(ui, is_stripe, checked, &response);

        let clip_left = divide.clip_at(&state.columns, cell_spacing, left_top.x);
        let mut row = TableRow {
            current_column: 0,
            state,
            ui,
            left_top,
            left_offset: 0.0,
            row_height,
            cell_spacing,
            divide,
            clip_left,
            is_header,
        };
        add_cells(&mut row);
        state.update_height(row_index + 1, row_height);

        response
    }

    pub fn cell(&mut self, add_column: impl FnOnce(&mut Ui)) -> Response {
        self.cell_with_layout(Layout::left_to_right(Align::Center), add_column)
    }

    pub fn cell_with_layout(
        &mut self,
        layout: Layout,
        add_column: impl FnOnce(&mut Ui),
    ) -> Response {
        self.show_cell(
            layout,
            |ui| {
                add_column(ui);
                0.0
            },
            Sense::hover(),
            None,
        )
    }

    /// Whether the next cell would be the first one past the divide.
    ///
    /// For a caller that opens a group of columns with a rule of its own: the
    /// divide is already a rule, and a heavier one, so another beside it is one
    /// line too many. Asked at the same point of the same sequence of cells in
    /// the header and in every row, so the two cannot answer differently and
    /// put the columns out of step.
    pub fn opens_after_divide(&self) -> bool {
        self.divide.columns > 0 && self.current_column == self.divide.columns
    }

    /// A cell that says how wide it would like its column to be, rather than
    /// leaving it to be measured afterwards.
    ///
    /// For content that cannot be measured after the fact: a name drawn with an
    /// ellipsis reports the width it was *cut to*, not the width it wanted, so a
    /// column narrowed once — which is what happens to a frozen column in a
    /// small window — could never widen again when the room came back. The
    /// larger of the two is what the column takes, so a caller that has nothing
    /// to add returns zero.
    pub fn measured_cell(&mut self, add_column: impl FnOnce(&mut Ui) -> f32) -> Response {
        self.show_cell(
            Layout::left_to_right(Align::Center),
            add_column,
            Sense::hover(),
            None,
        )
    }

    pub fn selectable_cell(&mut self, checked: bool, add_column: impl FnOnce(&mut Ui)) -> Response {
        self.selectable_cell_with_layout(checked, Layout::left_to_right(Align::Center), add_column)
    }

    pub fn selectable_cell_with_layout(
        &mut self,
        checked: bool,
        layout: Layout,
        add_column: impl FnOnce(&mut Ui),
    ) -> Response {
        self.show_cell(
            layout,
            |ui| {
                add_column(ui);
                0.0
            },
            Sense::click(),
            Some(checked),
        )
    }

    /// One cell standing over several columns, for a heading that belongs to a
    /// group of them — a value and the difference beside it are two columns so
    /// that both line up, but they are one metric of one combat and take one
    /// heading.
    ///
    /// The group's own columns keep their widths: a heading is not what a column
    /// of numbers should be sized by. Only when the heading is wider than the
    /// group is the surplus handed to the first column, so nothing is cut off.
    /// `add_column` returns how wide its contents need to be. Measured by the
    /// caller rather than read off the `Ui` afterwards: a heading is laid out
    /// without wrapping, and text drawn past the rectangle it was given does not
    /// reach `min_rect` — the columns then never grew and neighbouring headings
    /// ran into each other.
    pub fn spanning_cell(
        &mut self,
        columns: usize,
        add_column: impl FnOnce(&mut Ui) -> f32,
    ) -> Response {
        let columns = columns.max(1);
        while self.state.columns.len() < self.current_column + columns {
            self.state.columns.push(Default::default());
        }

        let span_widths: Vec<f32> = self.state.columns
            [self.current_column..self.current_column + columns]
            .iter()
            .map(|column| column.last_size)
            .collect();
        let width: f32 =
            span_widths.iter().sum::<f32>() + 2.0 * self.cell_spacing * (columns - 1) as f32;

        self.left_offset += self.cell_spacing;
        let rect = Rect::from_min_size(
            self.left_top + vec2(self.left_offset, 0.0),
            vec2(width, self.row_height),
        );
        // Cut off at the frozen strip like any other scrolling cell; a heading
        // that stands over a group is never one of the frozen columns.
        let restore_clip = self.clip_left.map(|clip_left| {
            let whole = self.ui.clip_rect();
            let mut cut = whole;
            cut.min.x = cut.min.x.max(clip_left);
            self.ui.set_clip_rect(cut);
            whole
        });
        let response = self
            .ui
            .interact(rect, self.ui.next_auto_id(), Sense::hover());
        let mut ui = self.ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );

        let needed = add_column(&mut ui).max(ui.min_rect().width());
        if let Some(whole) = restore_clip {
            self.ui.set_clip_rect(whole);
        }
        // What the first column of the group has to be, once the others have
        // contributed what they are: `update` takes the largest claim on a
        // column, so this competes with the rows' own contents rather than
        // overriding them. Claiming nothing while the heading happened to fit
        // let the column shrink back to its numbers on the next frame, and the
        // width oscillated between the two.
        let others: f32 =
            span_widths[1..].iter().sum::<f32>() + 2.0 * self.cell_spacing * (columns - 1) as f32;
        self.state.columns[self.current_column].update(needed - others, self.is_header);
        // No rules inside the group: it is one heading over one metric of one
        // combat, whatever it took two columns to line up.
        for column in self.current_column..self.current_column + columns - 1 {
            self.state.columns[column].merged_with_next = true;
        }

        self.current_column += columns;
        self.left_offset += width + self.cell_spacing;
        self.state.update_width(self.left_offset);
        response
    }

    fn show_cell(
        &mut self,
        layout: Layout,
        add_column: impl FnOnce(&mut Ui) -> f32,
        sense: Sense,
        checked: Option<bool>,
    ) -> Response {
        if self.state.columns.len() <= self.current_column {
            self.state.columns.push(Default::default());
        }

        let index = self.current_column;
        let column = &mut self.state.columns[index];
        // A frozen cell is laid out where it belongs and then pushed back by
        // what the body has been dragged, which lands it at the left edge of the
        // view. A scrolling cell keeps to the table's own coordinates and is cut
        // off at the strip, so it passes under the frozen columns rather than
        // over them — and cannot be clicked through them either, since egui
        // narrows a widget's interaction rectangle to the clip rectangle.
        let frozen = self.divide.holds(index);
        let shift = if frozen { self.divide.shift } else { 0.0 };

        self.left_offset += self.cell_spacing;

        let rect = Rect::from_min_size(
            self.left_top + vec2(self.left_offset + shift, 0.0),
            vec2(column.last_size, self.row_height),
        );
        let interact_rect = rect.expand2(vec2(self.cell_spacing, 0.0));
        let restore_clip = self.clip_left.filter(|_| !frozen).map(|clip_left| {
            let whole = self.ui.clip_rect();
            let mut cut = whole;
            cut.min.x = cut.min.x.max(clip_left);
            self.ui.set_clip_rect(cut);
            whole
        });
        let response = self
            .ui
            .interact(interact_rect, self.ui.next_auto_id(), sense);
        draw_visuals(self.ui, false, checked, &response);
        let mut ui = self
            .ui
            .new_child(UiBuilder::new().max_rect(rect).layout(layout));

        let wanted = add_column(&mut ui);

        let content_rect = ui.min_rect();
        if let Some(whole) = restore_clip {
            self.ui.set_clip_rect(whole);
        }

        self.current_column += 1;
        self.left_offset += column.last_size + self.cell_spacing;
        column.update(content_rect.width().max(wanted), self.is_header);
        self.state.update_width(self.left_offset);
        response
    }
}

impl ColumnState {
    fn update(&mut self, cell_width: f32, is_header: bool) {
        self.size = self.size.max(cell_width);
        if is_header {
            self.floor = self.floor.max(cell_width);
        }
    }

    /// Settle on the width the column is drawn at next frame, which is what it
    /// asked for unless a frozen column had to give some of it back.
    fn finish(&mut self, drawn: f32) -> bool {
        let repaint_required = (self.last_size - drawn).abs() > 0.5;
        self.last_size = drawn;
        self.size = 0.0;
        self.floor = 0.0;
        repaint_required
    }

    /// A rule down each boundary between columns, drawn from where the columns
    /// begin (`columns_left`) rather than from the left edge of `rect`, which is
    /// the view. The two are the same until the table is scrolled sideways, and
    /// while it is scrolled the view stays put — rules measured from it stood
    /// still on screen while the columns slid out from under them.
    ///
    /// Clipped to the table for the same reason: a rule now moves off either
    /// edge of the view, and outside `rect` it would be drawn over whatever the
    /// table sits next to.
    /// A frozen column's rule travels with it — which is to say it stays put —
    /// and the rules of the columns that scroll are cut off at the strip along
    /// with the columns themselves, or they would slide out over the frozen
    /// names.
    fn draw_separators(
        columns: &[Self],
        ui: &mut Ui,
        rect: Rect,
        columns_left: f32,
        cell_spacing: f32,
        divide: Divide,
    ) {
        if columns.is_empty() {
            return;
        }

        let whole = rect.intersect(ui.clip_rect());
        let painter = ui.painter().with_clip_rect(whole);
        let mut past_the_strip = whole;
        if let Some(clip_left) = divide.clip_at(columns, cell_spacing, columns_left) {
            past_the_strip.min.x = past_the_strip.min.x.max(clip_left);
        }
        let scrolled_painter = ui.painter().with_clip_rect(past_the_strip);
        let mut left_offset = 0.0;
        for (index, column) in columns.iter().enumerate().take(columns.len() - 1) {
            left_offset += column.last_size + 2.0 * cell_spacing;
            // Inside a group there is no rule: the heading spans it, and a rule
            // through the middle of a heading is what makes it read as two.
            if column.merged_with_next {
                continue;
            }
            let shift = if divide.holds(index) {
                divide.shift
            } else {
                0.0
            };
            // The divide is drawn whole even where the columns before it
            // scroll: half of a two-point rule falls on the far side of where
            // the scrolling columns are cut off.
            let painter = if divide.holds(index) || divide.is_divide(index) {
                &painter
            } else {
                &scrolled_painter
            };
            // The divide is not a rule between two columns but the line a row
            // stops naming itself at and starts measuring itself — and, where
            // the columns before it are frozen, the seam the table folds along.
            // Drawn like every other rule it read as one more line in a row of
            // them, and there was nothing on screen to say why the figures
            // stopped travelling there.
            let stroke = if divide.is_divide(index) {
                Stroke::new(
                    FREEZE_RULE_WIDTH,
                    ui.visuals()
                        .hyperlink_color
                        .gamma_multiply(FREEZE_RULE_ACCENT),
                )
            } else {
                ui.visuals().noninteractive().bg_stroke
            };
            let start = pos2(columns_left + shift + left_offset, rect.top())
                .round_to_pixels(ui.pixels_per_point());
            let end = pos2(start.x, rect.bottom()).round_to_pixels(ui.pixels_per_point());
            painter.line_segment([start, end], stroke);
        }
    }
}

/// How wide the columns of the table under `id` came to when it was last drawn.
///
/// For a caller that sizes something *around* a table — the combats panel makes
/// itself wide enough to hold its list rather than making the reader drag it
/// there. `None` before the table's first frame, when there is nothing measured
/// to report.
/// The id a table drawn straight into `ui` keeps its columns under.
///
/// So a test can ask a real table what its columns came to without knowing how
/// the id is put together — and without a caller having to name its table just
/// to be measurable.
#[cfg(test)]
pub fn table_id(ui: &Ui) -> Id {
    ui.id().with(module_path!())
}

#[cfg(test)]
pub fn table_column_widths(ui: &Ui, id: impl Into<Id>) -> Vec<f32> {
    let state: Option<State> = ui.data_mut(|d| d.get_temp(id.into()));
    state
        .map(|state| state.columns.iter().map(|c| c.last_size).collect())
        .unwrap_or_default()
}

pub fn table_content_width(ui: &Ui, id: impl Into<Id>) -> Option<f32> {
    let state: State = ui.data_mut(|d| d.get_temp(id.into()))?;
    (state.last_size.x > 0.0).then_some(state.last_size.x)
}

impl State {
    /// Forget which columns were drawn under one heading last frame.
    fn ungroup(&mut self) {
        for column in self.columns.iter_mut() {
            column.merged_with_next = false;
        }
    }

    fn load(ui: &Ui, id: Id) -> Self {
        ui.data_mut(|d| d.get_temp(id)).unwrap_or_default()
    }

    fn store(self, ui: &Ui, id: Id) {
        ui.data_mut(|d| d.insert_temp(id, self));
    }

    fn update_width(&mut self, row_width: f32) {
        self.size.x = self.size.x.max(row_width);
    }

    fn update_height(&mut self, rows: usize, row_height: f32) {
        self.size.y = self.size.y.max(rows as f32 * row_height);
    }

    fn finish(
        mut self,
        ui: &Ui,
        id: Id,
        cell_spacing: f32,
        frozen: usize,
        view_width: f32,
    ) -> bool {
        let size_change = (self.size - self.last_size).abs();
        let mut repaint_required = size_change.x > 0.5 || size_change.y > 0.5;
        self.last_size = self.size;
        self.size = Vec2::ZERO;

        while self.columns.last().map(|s| s.size == 0.0).unwrap_or(false) {
            self.columns.pop();
        }

        let drawn = frozen_widths(&self.columns, frozen, view_width, cell_spacing);
        for (column_size, drawn) in self.columns.iter_mut().zip(drawn) {
            repaint_required |= column_size.finish(drawn);
        }

        self.store(ui, id);

        repaint_required
    }
}

/// How wide each column is drawn: what it asked for, unless the frozen ones
/// together take more than [`FROZEN_MAX_SHARE`] of the view, in which case the
/// widest of them gives up the excess.
///
/// The widest one and not a share each: a frozen strip is a tick box and a name,
/// and it is the name that is long. What it may not give up is what its heading
/// needs — the eye and the type picker sit there, and a heading cut off is a
/// table that cannot be worked — nor may it fall under [`FROZEN_MIN_WIDTH`], so
/// a very small window ends up with a strip over its share rather than with a
/// column of nothing. The claim a column makes is untouched, so the width comes
/// straight back when the window is opened out again.
///
/// Nothing is narrowed while the whole table fits the view: there is nothing to
/// scroll, so the frozen columns are not holding anything off the screen and a
/// name cut short would be cut for nothing. That question is asked of what the
/// columns *claim*, never of what they were drawn at — a table judged by its
/// drawn width could be narrowed until it fitted, then found to fit and let out
/// again, and the widths would swing between the two every frame.
fn frozen_widths(
    columns: &[ColumnState],
    frozen: usize,
    view_width: f32,
    cell_spacing: f32,
) -> Vec<f32> {
    let mut widths: Vec<f32> = columns.iter().map(|column| column.size).collect();
    let frozen = frozen.min(widths.len());
    if frozen == 0 {
        return widths;
    }
    let width = |width: &f32| width + 2.0 * cell_spacing;
    if widths.iter().map(width).sum::<f32>() <= view_width {
        return widths;
    }
    let strip: f32 = widths[..frozen].iter().map(width).sum();
    let excess = strip - view_width * FROZEN_MAX_SHARE;
    if excess <= 0.0 {
        return widths;
    }
    let Some(widest) = (0..frozen).max_by(|a, b| widths[*a].total_cmp(&widths[*b])) else {
        return widths;
    };
    let floor = columns[widest]
        .floor
        .max(FROZEN_MIN_WIDTH)
        .min(widths[widest]);
    widths[widest] = (widths[widest] - excess).max(floor);
    widths
}

/// The look a table cell takes when it can be picked: filled while it is the
/// one picked, and rimmed under the pointer. Exposed so a caller can give the
/// same look to part of a cell — a heading whose second line is the thing being
/// clicked, say — instead of drawing a button there and having two kinds of
/// heading in one table.
pub fn draw_cell_visuals(ui: &mut Ui, checked: bool, response: &Response) {
    draw_visuals(ui, false, Some(checked), response);
}

fn draw_visuals(ui: &mut Ui, is_stripe: bool, checked: Option<bool>, response: &Response) {
    match checked {
        Some(true) => {
            ui.painter().rect_filled(
                response.rect,
                0.0,
                ui.style().interact_selectable(response, true).bg_fill,
            );
        }
        Some(false) if response.hovered() => {
            ui.painter().rect_filled(
                response.rect,
                0.0,
                ui.style().interact_selectable(response, false).bg_fill,
            );
        }
        _ if is_stripe => {
            ui.painter()
                .rect_filled(response.rect, 0.0, ui.visuals().faint_bg_color);
        }
        _ => (),
    }

    if let Some(checked) = checked
        && response.hovered()
    {
        ui.painter().rect_stroke(
            response.rect,
            0.0,
            ui.style().interact_selectable(response, checked).bg_stroke,
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A heading standing over a group of columns has to be able to widen that
    /// group, or it runs into the heading beside it. The width it asks for is
    /// what it returns, since text laid out without wrapping never reaches
    /// `min_rect`.
    #[test]
    fn a_spanning_heading_widens_the_group_under_it() {
        let ctx = Context::default();
        let mut width = 0.0;
        // Column widths are settled from the frame before, so this needs a few.
        for _ in 0..4 {
            let _ = ctx.run_ui(Default::default(), |ui| {
                width = Table::new(ui)
                    .id("spanning test")
                    .header(20.0)
                    .body(20.0, |t| {
                        t.row(|r| {
                            r.cell(|ui| {
                                ui.label("1");
                            });
                            r.cell(|ui| {
                                ui.label("2");
                            });
                        });
                    })
                    .header_row(|r| {
                        r.spanning_cell(2, |_| 300.0);
                    })
                    .width();
            });
        }
        assert!(
            width >= 300.0,
            "the group came to {width}, narrower than the heading over it"
        );
    }

    /// No rule is drawn inside a group: the heading spans it, and a line through
    /// the middle of a heading is exactly what makes one heading read as two.
    #[test]
    fn a_group_under_one_heading_has_no_rule_through_it() {
        let ctx = Context::default();
        let mut merged = Vec::new();
        for _ in 0..3 {
            let _ = ctx.run_ui(Default::default(), |ui| {
                Table::new(ui)
                    .id("merged test")
                    .header(20.0)
                    .body(20.0, |t| {
                        t.row(|r| {
                            for value in ["1", "2", "3"] {
                                r.cell(|ui| {
                                    ui.label(value);
                                });
                            }
                        });
                    })
                    .header_row(|r| {
                        r.spanning_cell(2, |_| 40.0);
                        r.cell(|_| {});
                    });
                merged = State::load(ui, Id::new("merged test"))
                    .columns
                    .iter()
                    .map(|column| column.merged_with_next)
                    .collect();
            });
        }
        assert_eq!(
            vec![true, false, false],
            merged,
            "only the two columns under one heading are joined"
        );
    }

    /// Every vertical rule the frame drew, by where it stands on screen.
    fn drawn_rules(shapes: &[epaint::ClippedShape]) -> Vec<f32> {
        fn walk(shape: &Shape, found: &mut Vec<f32>) {
            match shape {
                Shape::LineSegment { points, .. } if points[0].x == points[1].x => {
                    found.push(points[0].x)
                }
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
                _ => (),
            }
        }

        let mut found = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut found);
        }
        found
    }

    /// One frame of a table too wide for the window it is in, reporting where
    /// the rules between its columns were drawn and under which id its scroll
    /// area keeps how far it has been dragged.
    fn draw_wide_table(ctx: &Context) -> (Vec<f32>, Id) {
        let id = Id::new("scrolled rules");
        let mut scroll_id = Id::NULL;
        let output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(120.0, 200.0))),
                ..Default::default()
            },
            |ui| {
                scroll_id = ui.make_persistent_id(Id::new(id.with("__table_scroll")));
                Table::new(ui)
                    .id(id)
                    .header(20.0)
                    .body(20.0, |t| {
                        t.row(|r| {
                            for value in ["wwwwwwwwww", "wwwwwwwwww", "wwwwwwwwww"] {
                                r.cell(|ui| {
                                    ui.label(value);
                                });
                            }
                        });
                    })
                    .header_row(|r| {
                        for name in ["A", "B", "C"] {
                            r.cell(|ui| {
                                ui.label(name);
                            });
                        }
                    });
            },
        );
        (drawn_rules(&output.shapes), scroll_id)
    }

    /// A rule between two columns belongs to the columns, not to the window: drag
    /// the table sideways and the rules have to travel with the figures they
    /// stand between. Measured from the left edge of the view — which does not
    /// move — they stood still on screen while the table slid underneath them.
    #[test]
    fn the_rules_between_columns_travel_with_the_columns() {
        let ctx = Context::default();
        // Column widths are settled from the frame before, so this needs a few.
        let mut before = Vec::new();
        let mut scroll_id = Id::NULL;
        for _ in 0..4 {
            (before, scroll_id) = draw_wide_table(&ctx);
        }
        assert!(!before.is_empty(), "the table drew no rules to begin with");

        let dragged = 20.0;
        let mut scroll = scroll_area::State::load(&ctx, scroll_id).expect("the table scrolls");
        scroll.offset.x = dragged;
        scroll.store(&ctx, scroll_id);

        let (after, _) = draw_wide_table(&ctx);
        assert_eq!(
            dragged,
            scroll_area::State::load(&ctx, scroll_id).unwrap().offset.x,
            "the table did not stay dragged, so this proves nothing"
        );
        assert_eq!(before.len(), after.len(), "a rule went missing");
        for (before, after) in before.iter().zip(after.iter()) {
            assert!(
                (before - after - dragged).abs() < 0.5,
                "a rule at {before} moved to {after}, not to {}",
                before - dragged
            );
        }
    }

    /// What one frame of a table with a frozen first column reports: where the
    /// frozen cell landed, where the cell beside it landed, how far the latter
    /// senses the pointer, and the id its scroll area keeps its offset under.
    struct FrozenFrame {
        frozen: Rect,
        scrolled: Rect,
        scrolled_interact: Rect,
        scroll_id: Id,
        /// Every vertical rule the frame drew, by where it stands and what it
        /// was drawn with.
        rules: Vec<(f32, Stroke)>,
    }

    /// The same walk as [`drawn_rules`], keeping the stroke as well: the rule
    /// along the edge of the frozen strip is meant to be told from the rules
    /// between ordinary columns without reading the code.
    fn drawn_rule_strokes(shapes: &[epaint::ClippedShape]) -> Vec<(f32, Stroke)> {
        fn walk(shape: &Shape, found: &mut Vec<(f32, Stroke)>) {
            match shape {
                Shape::LineSegment { points, stroke } if points[0].x == points[1].x => {
                    found.push((points[0].x, *stroke));
                }
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
                _ => (),
            }
        }

        let mut found = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut found);
        }
        found
    }

    fn draw_frozen_table(ctx: &Context, view_width: f32) -> FrozenFrame {
        draw_divided_table(ctx, view_width, true)
    }

    fn draw_divided_table(ctx: &Context, view_width: f32, frozen: bool) -> FrozenFrame {
        let id = Id::new("frozen columns");
        let mut frame = FrozenFrame {
            frozen: Rect::NOTHING,
            scrolled: Rect::NOTHING,
            scrolled_interact: Rect::NOTHING,
            scroll_id: Id::NULL,
            rules: Vec::new(),
        };
        let output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(view_width, 200.0))),
                ..Default::default()
            },
            |ui| {
                frame.scroll_id = ui.make_persistent_id(Id::new(id.with("__table_scroll")));
                let table = Table::new(ui).id(id);
                let table = if frozen {
                    table.frozen_columns(1)
                } else {
                    table.divided_after(1)
                };
                table
                    .header(20.0)
                    .body(20.0, |t| {
                        t.row(|r| {
                            frame.frozen = r
                                .cell(|ui| {
                                    ui.label("nnnnnnnnnn");
                                })
                                .rect;
                            let scrolled = r.cell(|ui| {
                                ui.label("wwwwwwwwww");
                            });
                            frame.scrolled = scrolled.rect;
                            frame.scrolled_interact = scrolled.interact_rect;
                            r.cell(|ui| {
                                ui.label("wwwwwwwwww");
                            });
                        });
                    })
                    .header_row(|r| {
                        for name in ["A", "B", "C"] {
                            r.cell(|ui| {
                                ui.label(name);
                            });
                        }
                    });
            },
        );
        frame.rules = drawn_rule_strokes(&output.shapes);
        frame
    }

    /// Drag such a table sideways and its scroll area keeps the offset, so the
    /// next frame is drawn scrolled.
    fn drag_sideways(ctx: &Context, scroll_id: Id, by: f32) {
        let mut scroll = scroll_area::State::load(ctx, scroll_id).expect("the table scrolls");
        scroll.offset.x = by;
        scroll.store(ctx, scroll_id);
    }

    /// The point of a frozen column: the figures slide past it and it does not
    /// move. Laid out in the table's own coordinates it would travel with
    /// everything else, so it is pushed back by exactly what the body was
    /// dragged.
    #[test]
    fn a_frozen_column_stays_put_while_the_rest_scrolls() {
        let ctx = Context::default();
        // Column widths are settled from the frame before, so this needs a few.
        let mut before = draw_frozen_table(&ctx, 120.0);
        for _ in 0..3 {
            before = draw_frozen_table(&ctx, 120.0);
        }

        let dragged = 30.0;
        drag_sideways(&ctx, before.scroll_id, dragged);
        let after = draw_frozen_table(&ctx, 120.0);
        assert_eq!(
            dragged,
            scroll_area::State::load(&ctx, after.scroll_id)
                .unwrap()
                .offset
                .x,
            "the table did not stay dragged, so this proves nothing"
        );

        assert!(
            (before.frozen.left() - after.frozen.left()).abs() < 0.5,
            "the frozen column moved from {} to {}",
            before.frozen.left(),
            after.frozen.left()
        );
        assert!(
            (before.scrolled.left() - dragged - after.scrolled.left()).abs() < 0.5,
            "the column beside it did not travel: {} to {}, not to {}",
            before.scrolled.left(),
            after.scrolled.left(),
            before.scrolled.left() - dragged
        );
    }

    /// A scrolled column passes *under* the frozen one. It is cut off at the
    /// strip, and because egui narrows a widget's interaction rectangle to its
    /// clip rectangle, that also stops it taking clicks meant for the frozen
    /// cell drawn on top of where it would be.
    #[test]
    fn a_scrolled_column_does_not_reach_under_the_frozen_one() {
        let ctx = Context::default();
        let mut frame = draw_frozen_table(&ctx, 120.0);
        for _ in 0..3 {
            frame = draw_frozen_table(&ctx, 120.0);
        }
        drag_sideways(&ctx, frame.scroll_id, 30.0);
        let frame = draw_frozen_table(&ctx, 120.0);

        assert!(
            frame.scrolled.left() < frame.frozen.right(),
            "nothing was dragged under the frozen column, so this proves nothing"
        );
        assert!(
            frame.scrolled_interact.left() >= frame.frozen.right() - 0.5,
            "the scrolled column senses the pointer from {}, left of the frozen column's edge at {}",
            frame.scrolled_interact.left(),
            frame.frozen.right()
        );
    }

    /// The edge of the strip is the seam the table folds along, not one more
    /// rule in a row of them: what is left of it stays and what is right of it
    /// disappears underneath. It is drawn thicker and in the theme's accent so
    /// there is something on screen saying why the figures stop travelling
    /// there.
    #[test]
    fn the_edge_of_the_frozen_strip_is_drawn_to_be_seen() {
        let ctx = Context::default();
        let mut frame = draw_frozen_table(&ctx, 120.0);
        for _ in 0..3 {
            frame = draw_frozen_table(&ctx, 120.0);
        }

        let (ordinary, edge): (Vec<_>, Vec<_>) = frame
            .rules
            .iter()
            .partition(|(_, stroke)| stroke.width < FREEZE_RULE_WIDTH);
        assert!(
            !ordinary.is_empty(),
            "there is no ordinary rule to stand out from"
        );
        let [(at, stroke)] = edge[..] else {
            panic!("expected one rule along the strip, got {}", edge.len());
        };
        assert!(
            (at - frame.frozen.right()).abs() < 0.6,
            "the thick rule stands at {at}, not at the strip's edge at {}",
            frame.frozen.right()
        );
        assert!(
            ordinary
                .iter()
                .all(|(_, ordinary)| ordinary.color != stroke.color),
            "the edge is the same colour as the rules between the columns"
        );
    }

    /// A table can ask for the divide without the strip. The summary is one row
    /// per player: the boundary is worth marking, but a name held on screen
    /// there would only cost the figures beside it room in a small window. So
    /// the rule stands and the column travels like any other.
    #[test]
    fn a_table_can_have_the_divide_without_the_strip() {
        let ctx = Context::default();
        let mut before = draw_divided_table(&ctx, 120.0, false);
        for _ in 0..3 {
            before = draw_divided_table(&ctx, 120.0, false);
        }
        let dragged = 30.0;
        drag_sideways(&ctx, before.scroll_id, dragged);
        let after = draw_divided_table(&ctx, 120.0, false);

        assert!(
            (before.frozen.left() - dragged - after.frozen.left()).abs() < 0.5,
            "the first column stayed put at {} instead of travelling to {}",
            after.frozen.left(),
            before.frozen.left() - dragged
        );
        let heavy = after
            .rules
            .iter()
            .filter(|(_, stroke)| stroke.width >= FREEZE_RULE_WIDTH)
            .count();
        assert_eq!(
            1, heavy,
            "the divide is drawn, once, and heavier than the rest"
        );
    }

    /// A tree of names, standing in for the damage tree a table is drawn from.
    struct Node {
        name: &'static str,
        children: Vec<Node>,
    }

    fn node(name: &'static str, children: Vec<Node>) -> Node {
        Node { name, children }
    }

    /// The widest row of a tree is the widest row of the *whole* tree, not of
    /// the part of it that happens to be open — that is the point of measuring
    /// it at all — and how far in a row sits counts towards it.
    #[test]
    fn a_tree_is_measured_by_its_deepest_row_whether_it_is_open_or_not() {
        let ctx = Context::default();
        let (mut shallow, mut deep, mut indented) = (0.0, 0.0, 0.0);
        let _ = ctx.run_ui(Default::default(), |ui| {
            let name: fn(&Node) -> &str = |node| node.name;
            let children: fn(&Node) -> &[Node] = |node| node.children.as_slice();
            let tree = vec![node("Beam", vec![node("Phaser Beam Array", vec![])])];
            shallow = widest_name(ui, &tree[..1], 0.0, 0.0, name, children);
            deep = text_width(ui, "Phaser Beam Array");
            indented = widest_name(ui, &tree, 0.0, 50.0, name, children);
        });

        assert!(
            (shallow - deep).abs() < 0.5,
            "the closed branch was not measured: {shallow} against {deep}"
        );
        assert!(
            (indented - deep - 50.0).abs() < 0.5,
            "a row one level in needs its indent as well: {indented}"
        );
    }

    fn column(size: f32, floor: f32) -> ColumnState {
        ColumnState {
            size,
            floor,
            ..Default::default()
        }
    }

    /// A frozen strip that would take most of the view is narrowed to its
    /// share: it never scrolls away, so a column of long names would leave no
    /// room at all for the figures it is read against.
    #[test]
    fn a_frozen_strip_is_held_to_its_share_of_the_view() {
        // 530 points of columns in a 400-point view, so the table scrolls and
        // the strip may take 160 of it.
        let columns = [column(20.0, 0.0), column(400.0, 0.0), column(80.0, 0.0)];
        let widths = frozen_widths(&columns, 2, 400.0, 5.0);

        assert_eq!(20.0, widths[0], "the narrow one is left alone");
        assert!(
            widths[1] < 400.0 && widths[1] >= FROZEN_MIN_WIDTH,
            "the widest frozen column gave up the excess: {}",
            widths[1]
        );
        let strip = widths[0] + widths[1] + 4.0 * 5.0;
        assert!(strip <= 160.5, "the strip still came to {strip}");
        assert_eq!(80.0, widths[2], "and the scrolling columns are untouched");
    }

    /// What a frozen column may not give up is the room its own heading needs:
    /// the eye and the type picker sit there, and a heading cut off is a table
    /// that cannot be worked. Better an over-wide strip than an unusable one.
    #[test]
    fn a_frozen_column_is_never_narrowed_past_its_heading() {
        let columns = [column(400.0, 180.0)];
        let widths = frozen_widths(&columns, 1, 100.0, 5.0);
        assert_eq!(180.0, widths[0]);
        const {
            assert!(
                180.0 > 100.0 * FROZEN_MAX_SHARE,
                "which is more than the share, and that is the point"
            )
        };
    }

    /// Narrowing a column must not cost it the memory of what it wanted, or the
    /// room would never come back when the window was opened out again.
    #[test]
    fn a_narrowed_frozen_column_widens_again_when_the_view_does() {
        // Wide enough to scroll either way round, so what changes between the
        // two is the room, not whether the table has to be dragged at all.
        let columns = [column(400.0, 0.0), column(900.0, 0.0)];
        assert!(frozen_widths(&columns, 1, 100.0, 5.0)[0] < 400.0);
        assert_eq!(
            400.0,
            frozen_widths(&columns, 1, 1200.0, 5.0)[0],
            "the claim is untouched, so the width comes straight back"
        );
    }

    /// A table that fits its view has nothing to scroll, so its frozen columns
    /// are holding nothing off the screen — narrowing a name there would cut it
    /// short for no reason at all.
    #[test]
    fn nothing_is_narrowed_while_the_whole_table_fits() {
        let columns = [column(20.0, 0.0), column(400.0, 0.0)];
        let widths = frozen_widths(&columns, 2, 600.0, 5.0);
        assert_eq!(vec![20.0, 400.0], widths);
        let strip = 20.0 + 400.0 + 4.0 * 5.0;
        assert!(
            strip > 600.0 * FROZEN_MAX_SHARE,
            "the strip is over its share at {strip}, and is left alone anyway"
        );
    }

    /// A table that froze nothing is laid out exactly as it always was.
    #[test]
    fn a_table_with_no_frozen_columns_is_left_alone() {
        let columns = [column(400.0, 0.0), column(400.0, 0.0)];
        assert_eq!(
            vec![400.0, 400.0],
            frozen_widths(&columns, 0, 10.0, 5.0),
            "no column is frozen, so no cap applies"
        );
    }

    /// Whatever mark a heading ends up carrying is one of the marks the room is
    /// kept for. Changing one and not the other would leave a heading either
    /// short of room or holding room for a mark it can never draw.
    #[test]
    fn a_heading_only_ever_carries_a_mark_it_has_room_for() {
        let mut sort = SortState::default();
        sort.clicked("DPS");
        assert!(SORT_MARKERS.contains(&sort.marker("DPS")));

        sort.clicked("DPS");
        assert!(SORT_MARKERS.contains(&sort.marker("DPS")));

        assert_eq!("", sort.marker("Hits"), "and nothing on the other columns");
    }

    /// Clicking a heading picks that column; clicking it again turns the order
    /// round; clicking another starts that one the way it reads.
    #[test]
    fn a_heading_picks_a_column_and_then_turns_it_round() {
        let mut sort = SortState::default();
        assert_eq!(None, sort.column, "nothing is picked to begin with");

        sort.clicked("DPS");
        assert!(sort.is_sorted_by("DPS"));
        assert!(sort.natural, "the first click reads the column its own way");

        sort.clicked("DPS");
        assert!(!sort.natural, "the same heading again turns it round");

        sort.clicked("Hits");
        assert!(sort.is_sorted_by("Hits"));
        assert!(!sort.is_sorted_by("DPS"), "only one column orders the rows");
        assert!(sort.natural, "a different heading starts over");
    }

    /// The mark says which column is doing the ordering, and which way — and
    /// says nothing at all about the others.
    #[test]
    fn only_the_ordering_column_carries_a_mark() {
        let mut sort = SortState::default();
        sort.clicked("DPS");
        assert_eq!("", sort.marker("Hits"));
        let natural = sort.marker("DPS");
        sort.clicked("DPS");
        let reversed = sort.marker("DPS");
        assert!(!natural.is_empty() && !reversed.is_empty());
        assert_ne!(natural, reversed, "the two directions look different");
    }
}
