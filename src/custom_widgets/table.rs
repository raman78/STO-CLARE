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
pub const SORT_MARKERS: [&str; 2] = [" ⏷", " ⏶"];

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
    // The whole cell, top to bottom, the way every other heading works.
    let height = ui.available_height().max(ui.spacing().interact_size.y);
    let width = ui
        .available_width()
        .max(text_width(ui, text) + sort_marker_width(ui) + ui.spacing().item_spacing.x);
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
            match marker.is_empty() {
                true => ui.add_space(sort_marker_width(ui)),
                false => {
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
    /// How far the body settled this frame, which is what the header is shifted
    /// by so the two stay level.
    offset_x: f32,
}

pub struct TableBody<'a> {
    ui: &'a mut Ui,
    row_height: f32,
    cell_spacing: f32,
    striped: bool,
    state: &'a mut State,
    current_row: usize,
    left_top: Pos2,
}

pub struct TableRow<'a> {
    ui: &'a mut Ui,
    state: &'a mut State,
    current_column: usize,
    left_top: Pos2,
    left_offset: f32,
    row_height: f32,
    cell_spacing: f32,
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
        }
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
        } = self;
        let mut state = State::load(ui, id);
        // A table with no header row has no groups; see `Table::header`.
        state.ungroup();
        let (body_rect, _) = show_body(
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
            },
            add_body,
        );
        finish_table(ui, id, state, body_rect, cell_spacing);
        body_rect
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
        } = table;

        let (body_rect, offset_x) = show_body(
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
            body_rect,
            offset_x,
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
            offset_x,
        } = self;

        show_header(
            ui,
            &mut state,
            header_rect,
            header_height,
            cell_spacing,
            offset_x,
            add_header,
        );

        let full_rect = header_rect.union(body_rect);
        finish_table(ui, id, state, full_rect, cell_spacing);
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
}

/// Draws the rows, and reports the rectangle they came to and how far the table
/// is scrolled sideways.
fn show_body(
    ui: &mut Ui,
    id: Id,
    state: &mut State,
    body: Body,
    add_body: impl FnOnce(&mut TableBody),
) -> (Rect, f32) {
    let Body {
        row_height,
        min_scroll_height,
        max_scroll_height,
        striped,
        cell_spacing,
        shrink_to_content,
    } = body;
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
            let mut body = TableBody {
                current_row: 0,
                left_top,
                row_height,
                cell_spacing,
                striped,
                state,
                ui,
            };

            add_body(&mut body);

            let rect = Rect::from_min_size(left_top, state.last_size);
            ui.allocate_rect(rect, Sense::hover());
            rect
        });

    (
        scroll_output.inner.intersect(scroll_output.inner_rect),
        scroll_output.state.offset.x,
    )
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
    offset_x: f32,
    add_header: impl FnOnce(&mut TableRow),
) {
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
        add_header,
        false,
        None,
    );
}

/// The separators between column groups, and the state the next frame reads.
fn finish_table(ui: &mut Ui, id: Id, state: State, rect: Rect, cell_spacing: f32) {
    ColumnState::draw_separators(&state.columns, ui, rect, cell_spacing);
    if state.finish(ui, id) {
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

        let mut row = TableRow {
            current_column: 0,
            state,
            ui,
            left_top,
            left_offset: 0.0,
            row_height,
            cell_spacing,
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
        self.show_cell(layout, add_column, Sense::hover(), None)
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
        self.show_cell(layout, add_column, Sense::click(), Some(checked))
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
        let response = self
            .ui
            .interact(rect, self.ui.next_auto_id(), Sense::hover());
        let mut ui = self.ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );

        let needed = add_column(&mut ui).max(ui.min_rect().width());
        // What the first column of the group has to be, once the others have
        // contributed what they are: `update` takes the largest claim on a
        // column, so this competes with the rows' own contents rather than
        // overriding them. Claiming nothing while the heading happened to fit
        // let the column shrink back to its numbers on the next frame, and the
        // width oscillated between the two.
        let others: f32 =
            span_widths[1..].iter().sum::<f32>() + 2.0 * self.cell_spacing * (columns - 1) as f32;
        self.state.columns[self.current_column].update(needed - others);
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
        add_column: impl FnOnce(&mut Ui),
        sense: Sense,
        checked: Option<bool>,
    ) -> Response {
        if self.state.columns.len() <= self.current_column {
            self.state.columns.push(Default::default());
        }

        let column = &mut self.state.columns[self.current_column];

        self.left_offset += self.cell_spacing;

        let rect = Rect::from_min_size(
            self.left_top + vec2(self.left_offset, 0.0),
            vec2(column.last_size, self.row_height),
        );
        let interact_rect = rect.expand2(vec2(self.cell_spacing, 0.0));
        let response = self
            .ui
            .interact(interact_rect, self.ui.next_auto_id(), sense);
        draw_visuals(self.ui, false, checked, &response);
        let mut ui = self
            .ui
            .new_child(UiBuilder::new().max_rect(rect).layout(layout));

        add_column(&mut ui);

        let content_rect = ui.min_rect();

        self.current_column += 1;
        self.left_offset += column.last_size + self.cell_spacing;
        column.update(content_rect.width());
        self.state.update_width(self.left_offset);
        response
    }
}

impl ColumnState {
    fn update(&mut self, cell_width: f32) {
        self.size = self.size.max(cell_width);
    }

    fn finish(&mut self) -> bool {
        let repaint_required = (self.last_size - self.size).abs() > 0.5;
        self.last_size = self.size;
        self.size = 0.0;
        repaint_required
    }

    fn draw_separators(columns: &[Self], ui: &mut Ui, rect: Rect, cell_spacing: f32) {
        if columns.is_empty() {
            return;
        }

        let left_top = rect.left_top();
        let mut left_offset = 0.0;
        for column in columns.iter().take(columns.len() - 1) {
            left_offset += column.last_size + 2.0 * cell_spacing;
            // Inside a group there is no rule: the heading spans it, and a rule
            // through the middle of a heading is what makes it read as two.
            if column.merged_with_next {
                continue;
            }
            let start = (left_top + vec2(left_offset, 0.0)).round_to_pixels(ui.pixels_per_point());
            let end = (start + vec2(0.0, rect.height())).round_to_pixels(ui.pixels_per_point());
            ui.painter()
                .line_segment([start, end], ui.visuals().noninteractive().bg_stroke);
        }
    }
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

    fn finish(mut self, ui: &Ui, id: Id) -> bool {
        let size_change = (self.size - self.last_size).abs();
        let mut repaint_required = size_change.x > 0.5 || size_change.y > 0.5;
        self.last_size = self.size;
        self.size = Vec2::ZERO;

        while self.columns.last().map(|s| s.size == 0.0).unwrap_or(false) {
            self.columns.pop();
        }

        for column_size in self.columns.iter_mut() {
            repaint_required |= column_size.finish();
        }

        self.store(ui, id);

        repaint_required
    }
}

/// The look a table cell takes when it can be picked: filled while it is the
/// one picked, and rimmed under the pointer. Exposed so a caller can give the
/// same look to part of a cell — a heading whose second line is the thing being
/// clicked, say — instead of drawing a button there and having two kinds of
/// heading in one table.
pub fn draw_cell_visuals(ui: &mut Ui, checked: bool, response: &Response) {
    draw_visuals(ui, false, Some(checked), response);
}

/// How much darker a list row goes under the pointer. Enough to read at a
/// glance against both a plain row and a striped one, on a dark theme where the
/// two are close together.
const ROW_HIGHLIGHT: Color32 = Color32::from_black_alpha(60);

/// A row of a list, drawn the way a table row is: every other one on a faint
/// fill, and the one under the pointer picked out.
///
/// Lists elsewhere in the program are rows of widgets with nothing behind them,
/// which on a list of a dozen combats makes it easy to read one line's tick
/// against another line's name. The background is reserved before the contents
/// are drawn and filled in afterwards, since a row is only as tall as whatever
/// went into it.
pub fn list_row<R>(
    ui: &mut Ui,
    is_stripe: bool,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let width = ui.available_width();
    let background = ui.painter().add(Shape::Noop);
    let highlight = ui.painter().add(Shape::Noop);
    let inner = ui.scope_builder(UiBuilder::new().sense(Sense::hover()), |ui| {
        ui.set_min_width(width);
        ui.horizontal(|ui| add_contents(ui)).inner
    });

    // The full width of the list, not only what the widgets came to: a highlight
    // that stopped at the end of the text would say the row ended there.
    let rect = Rect::from_min_size(
        inner.response.rect.min,
        vec2(width, inner.response.rect.height()),
    );
    if is_stripe {
        ui.painter().set(
            background,
            epaint::RectShape::filled(rect, 0.0, ui.visuals().faint_bg_color),
        );
    }

    // `contains_pointer` rather than `hovered`: a row is mostly tick box and
    // text, and those are the widgets on top, so the row itself counted as
    // hovered only in the gaps between them.
    //
    // Drawn as a shade over whatever the row's background is, rather than as a
    // colour of its own: a theme's hover fill is meant for a small control
    // against a panel, and across a whole row the bright one glared while the
    // dark one barely showed. A shade is the same step on every theme, and it
    // reads as the row being pointed at whichever way round the theme runs.
    if inner.response.contains_pointer() {
        ui.painter().set(
            highlight,
            epaint::RectShape::filled(rect, 0.0, ROW_HIGHLIGHT),
        );
    }

    InnerResponse::new(inner.inner, inner.response)
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
