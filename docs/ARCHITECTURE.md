# Architecture

How the program is put together: what runs on which thread, how a line of the
combat log becomes a number in a table, and where each concern lives.

This is the entry point for the technical docs; `docs/README.md` indexes them. It stays at the level of
modules and data flow; each subsystem with real depth has its own document,
linked from the relevant section.

## Purpose

`STO-CLARE` reads the combat log Star Trek Online writes, splits it
into fights, and reports what happened in each — damage, healing, hits, kills,
per-ability breakdowns, charts, a live overlay, and upload to the OSCR ladder.
It is a native desktop application; there is no server component of its own.

Entry point: `src/main.rs` → `eframe::run_native` → `app::App`.

## Layout

| module           | responsibility                                                       | depends on           |
|------------------|----------------------------------------------------------------------|----------------------|
| `analyzer`       | reads the log, builds `Combat`s and their metric trees               | nothing in the app   |
| `app`            | windows, tables, charts, settings, overlay, the analysis thread      | `analyzer`, `upload` |
| `upload`         | packaging and sending a combat to the OSCR ladder                    | `analyzer`           |
| `custom_widgets` | table, splitter, sliders — egui widgets the app needs and egui lacks | egui only            |
| `helpers`        | number formatting, small shared utilities                            | nothing              |

`analyzer` knows nothing about egui and never reads settings the UI owns; the
dependency runs one way only.

## Threads

```
  main thread (egui)                    analysis thread
  ──────────────────                    ───────────────
  App::update  ──── Instruction ──────►  AnalysisContext::run
      │            (crossbeam)                  │
      │                                    Analyzer::update
      │                                         │
      │  ◄──────── AnalysisInfo ─────────  Arc<Combat>
      │            (crossbeam)
      ▼
  MainTabs / CompareView / Overlay
```

`app::analysis_handling` owns the split. The UI never parses anything: it sends
an `Instruction` (refresh, fetch a combat, delete combats, change settings) and
receives an `AnalysisInfo`. Combats cross the boundary as `Arc<Combat>`, so
handing the same fight to the main window, the compare view and the overlay
costs a refcount.

Several handlers can subscribe (`AnalysisHandler::get_handler`); the overlay
uses its own so it can auto-refresh while the main window does not. Parsing is
incremental — `Analyzer::update` resumes where the last call stopped, so a live
refresh only reads what the log has grown by.

## From a log line to a number

```
combatlog.log
     │  Parser::parse_next            src/analyzer/parser.rs
     ▼
  Record { time, owner, source, target, ability, type, flags, value }
     │  Analyzer::process_next_record src/analyzer/mod.rs
     ├─ starts a new Combat when the gap exceeds the separation time
     ├─ interns every name                     (NameManager)
     ├─ accumulates per-NPC facts for detection (detection::CritterMeta)
     ▼
  Player::add_out_value / add_in_value
     │  routes to one of the trees, building a grouping path
     ▼
  DamageGroup / HealPool          src/analyzer/{groups,damage,heal}.rs
     │  leaves hold the raw hits/ticks, branches aggregate
     ▼
  Combat::update  → metrics, percentages, map and difficulty
     ▼
  Arc<Combat> ──► tables (MetricsTable) and charts (diagrams)
```

Key decisions along that path:

- **Names are interned.** `NameManager` maps every name to a `NameHandle`, so
  the trees compare and hash integers. Handles are only resolved to strings when
  the UI builds a row.
- **Raw values live once.** `ValuesManager` keeps a flat `Vec` of hits/ticks;
  a branch node refers to a range of it, a leaf owns its own. Charts slice that
  buffer instead of copying trees.
- **Metrics are recomputed, not incremented in place.** `Combat::update` runs
  after a batch of records and rebuilds the aggregates, which keeps an
  incremental refresh consistent with a cold read of the same log.

## The four analysis trees

Each `Player` carries damage dealt, damage taken, and three healing pools. The
healing split, and why it is three rather than two, is in
`docs/HEALING_MODEL.md`. What the log's numeric fields mean, and the one line
kind that needs look-ahead to classify, is in `docs/COMBATLOG_FORMAT.md`.

A tree node is a `DamageGroup` or `HealGroup`: a name, a metrics block, and its
children. The path a record takes into the tree is built by
`Player::build_grouping_path` from the record plus the user's grouping rules,
which is where Custom Group Rules and Source Reversal rules take effect.

## Map and difficulty

`analyzer::detection` derives `(map, difficulty)` from which curated NPCs
appeared and how much hull damage they took — the log carries no map marker at
all. Rules live in `src/analyzer/detection_rules.json`, and a file of the same
name next to the settings overrides it without a rebuild. See
`docs/DIFFICULTY_DETECTION.md` and the measurements in
`docs/DETECTION_SAMPLES.md`.

## UI

| area                        | module                   | notes                                                            |
|-----------------------------|--------------------------|------------------------------------------------------------------|
| shell, combat picker, menus | `app/mod.rs`             | owns `MainTabs`, `CompareView`, the overlay handle               |
| per-combat tabs             | `app/main_tabs`          | Summary, Damage Dealt/Taken, the three healing tabs              |
| tables                      | `app/main_tabs/tables`   | one generic `MetricsTable<T>` driven by a static column list     |
| charts                      | `app/main_tabs/diagrams` | Gauss-filtered per-second graphs and time-sliced bar charts      |
| compare                     | `app/compare`            | several combats side by side, averaged, or written to a workbook |
| spreadsheet export          | `app/export.rs`          | the workbook writer both the compare view and the tabs use       |
| which columns are shown     | `app/settings/columns.rs`| per table kind, and what the picker in the tab row writes         |
| settings                    | `app/settings`           | split into analysis settings (invalidate the parse) and the rest |
| how it looks                | `app/theme.rs`           | the themes on offer, the app's own colours, the text sizes       |
| overlay                     | `app/overlay`            | separate always-on-top window; see `docs/OVERLAY.md`             |

Three conventions worth knowing before changing a table or a chart:

- **A toggle is a button, not a `selectable_label`.** `custom_widgets::toggle`
  offers `Ui::steady_toggle` / `Ui::steady_toggle_value`, and the tab strips,
  chart pickers and toolbar toggles use them. egui's own pair draws no frame
  while resting and works the button's inner margin out as `button_padding -
  the frame's stroke width`, which only comes out even when the resting state
  has no stroke — egui's themes have none, this app's do (`glassify`). The
  widget was therefore two points narrower resting than hovered, and pointing
  at one nudged the rest of the row along. The frame is now drawn in every
  state; `egui_s_own_selectable_label_is_what_moves` holds the diagnosis, and
  fails if a future egui fixes it. List rows inside a `ComboBox` or a context
  menu are left as they were: they are not a row of buttons, and nothing sits
  beside them to be pushed.

- **A hidden column is a setting, not a rebuild.** `ColumnVisibility` (in the
  settings, keyed by `TableKind` — summary, damage, heal) records what the user
  **hid**, so a metric added later is on screen rather than missing from a list
  written before it existed. `MetricsTable::show` and `SummaryTable::show` take
  a `shown` predicate and gather the visible columns per frame, so the picker
  in the tab row takes effect at once. The two damage tabs share a kind and the
  three healing tabs share another, because they are the same table. That
  button is rimmed in the theme's `hyperlink_color` (`main_tabs::accent_rim`)
  to tell it from the tabs beside it — colour only, since a wider stroke would
  come off the button's inner margin and bring the size shift back.

- **Columns are data.** A table is a `&'static [ColumnDescriptor<T>]`; a column
  carries its label, its sort function and its render function. A metric that
  splits into hull and shield uses `shield_hull_col!`, which adds the two extra
  cells that the split-columns setting shows. Each half carries its own `sort`,
  so All, Hull and Shield order by their own figure; ordering all three by the
  total made two of the three headings a lie.

- **One heading control, drawn once.** `metrics_table::show_sortable_header` is
  what every heading in `MetricsTable` and in `SummaryTable` is made of, so the
  two cannot drift apart. A heading is its column's whole cell — every line of
  it, the metric name included where that column carries one — painted with
  `table::draw_cell_visuals`, the fill-when-picked, rim-under-the-pointer look a
  pickable cell has rather than a button frame. What must *not* happen is one
  control over a whole split group: All, Hull and Shield are three columns and
  three cells, so pointing at one says which of the three is about to order the
  rows. The sort mark sits against the right-hand edge of the cell, on the line
  the label is on.

  A column whose header carries buttons — Name, with the eye and the type picker
  — uses `table::show_sortable_header_cell` instead: the cell still orders the
  rows, and the buttons are drawn on top of it, so egui hands a click on one of
  them to that button and everything else to the heading. There the mark follows
  the word rather than sitting at the edge, which is where the buttons are.

  Which heading is in charge lives in `SortState<ColumnKey>` (`custom_widgets::
  table`): `ColumnKey` is the metric plus which half, `natural` is which way the
  order runs, and `marker` returns one of `SORT_MARKERS` (`⏷`/`⏶`). A second
  click on the same heading reverses the rows rather than sorting them again — a
  column knows one order (largest first, or smallest where small is the good
  end), and the other way round is that one turned over. Rebuilding a table
  carries the state across (`MetricsTable::take_state_from`,
  `SummaryTable::take_state_from`) and re-applies it, so ticking a row off or
  opening another combat does not undo the order the reader chose. Neither takes
  the state of a table that has picked no column — that is the empty table a tab
  is born with, and taking it left the first real table sorted with no heading
  saying so.

- **A heading keeps room for a mark it has not got.** `show_sort_marker` draws
  the mark against the right-hand edge of the heading — where the numbers under
  it end, so it is looked for in one place down the row rather than wherever a
  name happens to finish — and `heading_width` reserves `sort_marker_width` in
  every heading whether or not it is carrying one. Laying the mark out with the
  label made a column widen the moment it took charge of the order, shifting
  every column right of it. `MetricsTable`, `SummaryTable` and the comparison's
  headers all measure this way.

- **A chart is dragged sideways, never up and down.** Every chart scales its y
  axis to the data (`auto_bounds`, `include_y`), so moving it vertically only
  slides the lines out of a frame that was already the right size. `Plot` takes
  `PAN_SIDEWAYS_ONLY` (`diagrams::common`) for both `allow_scroll` and
  `allow_drag`; the wheel is a vertical gesture, so it now does nothing on a
  chart rather than scrolling the picture away.

- **Nothing in a `PopupButton` may ask for all the width on offer.** The window
  is sized to what it holds, and `ui.separator()` takes whatever it is given —
  which in an auto-sized window is the width of the screen. The damage-type
  pickers opened as a banner across the window until their separators became
  `add_space`.

- **Charts are anchored to the combat, not to the series.** Every data set spans
  the whole fight, so a player who only started healing a minute in still draws
  from the start and several series share bucket boundaries.
- **Every chart orders its series the same way** — by `PreparedDataSet::
  total_value`, largest first (`ValuesChart::sort`, `ValuePerSecondGraph::sort`,
  `DamageResistanceChart::sort`). Series colours are handed out by that order
  (`theme::series_color`), so any chart that ordered its series differently gave
  the same player a different colour and a different place in the legend.
- **The per-second charts are a kernel density estimate, so the kernel has to
  integrate to one.** It is cut at `KERNEL_CUTOFF_SIGMAS` (4 σ) and divided by
  the mass inside that cut, which makes the line's height independent of the
  smoothing setting. The line still dips where the kernel hangs over the start
  or the end of the fight — that is inherent to smoothing a finite record, and
  it shows up at smoothing widths comparable to the length of the fight.
- **Bold text needs its own font.** egui's `RichText::strong()` only picks a
  brighter colour, and the fonts epaint bundles have no bold face. `app/fonts`
  embeds `assets/fonts/Ubuntu-Bold.ttf` — the matching weight of the Ubuntu-Light
  epaint uses — as the family `FontFamily::Name("Ubuntu-Bold")` and binds it on
  the main context in `App::new`; `main_tabs::common::bold_text` is how widgets
  ask for it. epaint panics on a family that is not bound, so any further egui
  context that wants bold text has to call `fonts::install` too (the overlay
  context does not use it).

Settings changes are gated by cost: only `analysis` invalidates the `Analyzer`
and forces a re-read of the log; a `general` change just rebuilds the views,
because formatting is baked into the row strings when a table is built.

The `combat_notes` section (`app/settings/combat_notes.rs`) holds the user's own
short description per combat, written in the Summary tab and repeated wherever a
combat is listed: the main window's dropdown, the compare picker (whose search
box reads it), and all three parts of a comparison — its legend, its chart and
its column headers (see below). It is keyed by the combat's **start time**,
which the refresh messages carry alongside the list (`start_times`, aligned with
`combats`) because those views hold parallel arrays rather than whole combats.
The start time is the only identifier the log itself fixes — `Combat::identifier` carries whatever the name rules or
the map detection produced, so a rename would orphan the notes. Changing
`combat_separation_time_seconds` re-cuts the log into different combats and does
orphan them; there is no key that survives that.

### Comparing several combats — `app/compare`

`CompareView` (`app/compare/mod.rs`) is a picker plus a table. The picker works
on the parallel arrays a refresh message carries (`combats`, `difficulties`,
`base_names`, `environments`, `start_times`), so it never holds a combat; the
selection is a `Vec<usize>` of indices into them. Pressing **Compare selected**
sends those indices as `Instruction::GetCombats`, and the answer
(`AnalysisInfo::Combats`) builds a `Comparison` (`compare_table.rs`).

Three filters narrow the picker, and all three also decide what **Select all**
adds — the button takes exactly the list on screen, adding to the selection
rather than replacing it, so a selection can be built from one filtered list
after another.

A **ticked combat is never filtered out** (`CompareView::visible_combats`
returns it with `matches = false`, and the row carries a `⚠` in
`Palette::warn`). It is going into the comparison either way, so hiding it —
narrowing the level to Elite over a selection that holds an Advanced run —
would leave a combat being compared that cannot be seen or unticked.

| filter      | type           | shared with the main window | notes                        |
|-------------|----------------|-----------------------------|------------------------------|
| search box  | `String`       | no                          | matches identifier **and** the user's note |
| type/level/map | `CombatFilter` | yes (`app/combat_filter.rs`) | each menu offers only what the other two leave reachable |
| date window | `DateRange`    | no                          | `app/compare/date_range.rs`  |

`DateRange` is two `%Y-%m-%d %H:%M` fields, either of which may be empty (no
bound at that end) or half-typed (bounds nothing, drawn in `Palette::worse`
until it parses). Two decisions are worth keeping:

- **The upper bound covers its whole minute.** The fields are typed to the
  minute; a combat that started at `20:07:45` is inside a window ending at
  `20:07`, or the run whose time the user typed would be the one dropped.
- **The presets count back from the newest combat in the list, not from the
  wall clock.** `chrono` is built here without its `clock` feature, so there is
  no local `now()` to count from — and the times in a log are the game's, so a
  log copied from another machine would answer "the last 24 hours" with an
  empty list.

#### What a selected combat costs

Nothing caps the selection (`MAX_COMBATS` is gone). What gives way instead is
stated in the picker by `selection_hint`, because both limits are gradual:

| past | what happens | why |
|------|--------------|-----|
| 8 combats  | chart line colours start over | `theme::series_color` cycles a palette of eight |
| 50 combats | a word about build time and width | a column per combat per metric |

`AnalysisContext::get_combats` deep-clones each `Combat` (its `HitsManager`
included), and `build_series` copies the hits of every tree node it builds —
`Values::Branch` is a range into the manager, but `SeriesData` holds a
`Vec<Hit>`, so a combat's hits are copied once per tree level.

Measured on a real log (18 combats, release build): `Hit` is 40 bytes, the
heaviest run — a 7½-minute Infected Space Elite — carries 24 673 hits, and one
such combat in a comparison costs **≈ 3.4 MB** (1.0 MB of clone, 2.4 MB of
series copies). All 18 at once came to 26 MB. Holding `Values<Hit>` in
`SeriesData` and resolving it against the slot's manager at chart time would
remove the 2.4 MB share, but at that scale it buys little and the charts have no
automated coverage to catch a mistake with; the copies stay.

#### What a big comparison does to the layout

The compare view is drawn straight into the central panel; nothing around it
scrolls vertically. The table scrolls sideways on its own
(`ScrollArea::horizontal`), so more columns only ever means more scrolling —
but the legend is a row per combat, and a row measures 21 points under the
default style, so an uncapped legend filled a 720-point window at 34 combats
and left the table and the chart drawn past the bottom edge with no way to
reach them. `legend_height` caps it at `LEGEND_ROWS` (6) and lets it scroll
inside itself; below that it still shrinks to what it holds.

The one thing left uneven is the table's `Name` column, which scrolls away with
everything else — the averages toggle is the answer to a table too wide to
read, not a frozen first column.

**A heading can stand over a group of columns.** `TableRow::spanning_cell` draws
one cell across several, which is what a comparison's headings use: a run's
value and its difference against the reference are two columns so that both line
up, but they are one metric of one run and take one heading. The group's columns
keep the widths their numbers need; only a heading wider than the group hands
the surplus to the group's first column — and it claims that width on *every*
frame, not only when it does not fit. Claiming nothing while it happened to fit
let the column shrink back to its numbers on the next frame, and the width
oscillated (`a_spanning_heading_widens_the_group_under_it` pins it). The width a
heading needs is returned by the closure rather than read off `min_rect`: a
heading is laid out without wrapping, and text drawn past its rectangle never
reaches `min_rect`.

No rule is drawn between columns of a group (`ColumnState::merged_with_next`,
cleared by `State::ungroup` at the start of every frame so a table that stops
grouping does not keep the last frame's groups). A rule through the middle of a
heading is what makes one heading read as two, which is what the split looked
like even once the heading spanned both columns.

**A column index is not a slot index.** `Comparison::slots` is every run the
comparison was built from, in the order they were picked; `numbers` maps a
*column* to the slot it holds, and `columns_in_play` puts the reference first.
Anything read per run — the note, the number, the colour, `CompareNode::dps` —
has to be indexed by the right one of the two. They were the same number while
columns were slots in order, so mixing them cost nothing; making the reference
lead broke that, and a column was labelled with the first run's note while
`⚖ vs rest` measured a combat nobody had picked. `note_of_column` and
`number_of` take a column and do the mapping; `CompareNode::impact` takes a
column, since a node holds its figures per column.

**The comparison's headers are the main window's headers.** A column header is
three lines — the metric, the combat's number, the note — and only the lines
naming the combat take the click (`show_header_cell`), drawn with the same
`draw_cell_visuals` fill and the same `show_sort_marker` placement as
`MetricsTable`. The metric's name spans its whole group of columns, so lighting
it up said nothing about which column was about to order the rows. The header's
height comes from `ui.text_style_height` rather than a hand-counted constant: at
17 points a line, the note line had its bottom half cut off by the first row.

**A table scrolls itself, both ways, and draws its header last.**
`custom_widgets::table` used to scroll only vertically and be wrapped in a
`ScrollArea::horizontal` by every caller. That put the vertical bar in the wrong
place: with the horizontal direction disabled and auto-shrink off, egui sizes
the inner rect as `inner.max(content_size.x)`, so the bar was pinned to the
right-hand edge of the *table* — hundreds of points off screen on a wide
comparison — and with the solid (non-floating) bar style the cross range came
out inverted and nothing was drawn at all.

`Table` now uses one `ScrollArea::both`, so both bars sit at the edges of the
view. The header cannot live inside that area or it would scroll off the top, so
the call order is inverted:

```rust
Table::new(ui)
    .header(height)              // keeps the room
    .body(ROW_HEIGHT, |t| { … }) // draws the rows, settles the offset
    .header_row(|r| { … });      // draws the header into the kept room
```

The room `header` keeps is the narrower of the columns' own width and the width
of the view — never the space on offer, and never wider than what is on screen. The overlay sizes its window to what its table asks for,
so a header that took the available width grew the window, which then offered
more — the overlay ran away across the screen. Reserving the columns' full width
instead pushed the scroll area past the right-hand edge on a wide comparison,
and took the vertical bar with it; hence the narrower of the two. `HeaderSlot::header_row` shifts
the header by the offset the body settled on *this* frame, clips it vertically
to that band and horizontally to the view. Drawing the header first —
the obvious order — could only ever use the previous frame's offset, and the
headings would lag behind their columns while the table was dragged. The two
closures also cannot be alive at once: both borrow the table's own state, which
is the other reason `header` takes no closure.

**A list of rows is drawn like a table's rows.** `table::list_row` stripes every
other row and picks out the one under the pointer, and it is what the lists of
runs use — the one a comparison is picked from and the legend inside one. They
were rows of widgets on a flat background, where a dozen runs of the same map on
the same evening differ only by the time at the end of the line. The background
is reserved with `Shape::Noop` before the contents are drawn and filled in
afterwards, because a row is only as tall as what went into it.

**A scrolling list must not be wider than its pane.** Every vertical
`ScrollArea` in the program carries `auto_shrink([false, true])` so its bar sits
at the edge of the pane rather than against the longest line in it. That only
holds while the content fits: with `direction_enabled[0] == false` and
`auto_shrink[0] == false`, egui sizes the inner rect as
`inner.max(content_size.x)` (`scroll_area.rs`), so one over-long line widens the
whole area and carries the vertical bar off the right-hand edge with it. That is
what hid the legend's bar — the line explaining the reference run. Any label
that can outgrow its pane is `Label::truncate()`d.

#### Picking what the Total is of

The table's first column is a tick per row, and the Total row is added up from
the ticked ones only. `Comparison::excluded` holds the names of the rows that
are out; `refresh_total` rebuilds the Total row — its cells, its averages, its
chart series and its name — whenever that set changes.

Only the rows directly under Total carry a tick. A branch's hits are the whole
branch's (`Values::Branch` is a range over the slot's `HitsManager`), so a row
goes in with everything under it; a tick deeper in the tree would mean re-adding
every branch above it on every click, for a granularity the damage tree already
expresses as a row of its own.

Per slot, with rows excluded:

```
player.damage_out.sub_groups()
   │  drop the rows named in Comparison::excluded        (subset_hits)
   ▼
Vec<Hit>
   │  DamageMetrics::calc_and_apply_delta                (subset_group)
   │  DamageMetrics::recalculate_time_based_metrics(player.combat_time)
   ▼
DamageGroup { hits: Hits::Leaf(..), .. }  ──►  build_row, build_series
```

The synthetic group is fed to the same `build_row` and `build_series` the tree
is built with, so a filtered Total carries deltas, averages and a chart series
like any other row, and nothing downstream knows it was filtered.

| decision | why |
|----------|-----|
| the metrics are recalculated from the hits, not summed from the columns | a percentage cannot be added up: the resistance, crit rate and accuracy of a subset are only defined against that subset's hits. `DamageGroup::recalculate_metrics` does exactly this for a branch, so a filtered Total is what the analyzer would give a group holding those rows |
| with nothing excluded the row is rebuilt from `player.damage_out` instead | float addition is not associative, and an unfiltered Total should be the analyzer's own figure to the last digit rather than the pieces added back in another order |
| `metrics_duration` reads `Player::combat_time` | `damage_out` is measured against the time in combat (`Player::recalculate_metrics`). `combat_duration_seconds`, which the charts use, is `active_time`; dividing by it would state a DPS no other part of the program agrees with |
| `damage_percentage` stays a share of `Combat::total_damage_out` | the column means the same filtered or not — how much of the fight this is |
| the ticks are held by row name, not by node id | a column change or another player rebuilds the tree and hands out fresh ids; the reader's selection has no reason to survive one and not the other |
| a combat that has none of the ticked rows leaves an empty cell | `subset_hits` answers `None` rather than an empty pool. That is what an absent row is everywhere else in the table: no cell, out of the averages, off the chart. A zero would draw a flat line along the bottom and pull every average down for a run that simply flew something else |
| a tick is not also a chart selection | the tick sits inside a `selectable_row`, so it clicks the row as well. `CompareNode::show` drops any click whose pointer landed in the tick cell (`show_tick` returns its `Rect`) — not only one that changed a tick, since aiming at a box and missing it by two points would otherwise chart that row |
| the row is renamed `Total (k of n rows)` while filtered | the one way this can mislead is a filtered figure read as the run's own DPS — on screen and in the exported sheet alike |

The `👁` toggle in the `Name` header (`Comparison::hide_unticked`, `is_hidden`)
only takes the unticked rows off the screen; they are out of the Total either
way. Neither it nor the ticks are persisted: `CompareSettings` holds the
columns, the breakdown and the averages, and a fresh `Comparison` starts with
every row ticked.

#### Which rows are on screen

Three filters decide it, `and`ed in `is_hidden`, and all three apply only to the
rows directly under Total — the level the ticks are at, and the level a reader
compares builds at.

| filter | state | what it asks |
|--------|-------|--------------|
| `👁` in the `Name` header | `hide_unticked` | only the rows the Total is added up from |
| `☰ Type` in the `Name` header | `types` | only the rows that dealt one of these damage types |
| `Δ Differences` in the toolbar | `show_differences`, `difference_measure`, `share_threshold` / `dps_threshold` | only the rows the combats disagree over |

The type picker is **not** a view filter: picking a type rebuilds the tree from
a copy of the player's damage holding only what was dealt in it (`of_type`,
`TypeFilter::keep`, kept per slot in `Comparison::of_type`). A group of one
picked type is kept whole; a group of several is rebuilt from the sub-groups
that survive, since a hit carries no damage type of its own — the group it lands
in does, and that is the only level the log separates them at. So `Polaron Beam
Array` under `Cold` shows the 60k its Frostbite proc did, not the 4.3M the beams
did around it, and every column of that row — DPS, resistance, criticals — is of
the proc, having been recomputed from its hits.

Percentages are set by hand on the rebuilt tree (`set_child_percentages`): the
analyzer's own pass is `pub(super)`, and a rebuilt group has nobody to have set
them. The top row's is of the whole fight, each row below of its parent, which
is what the analyzer states.

`CompareNode::damage_types` is the union of `DamageGroup::damage_types` over the
slots. The analyzer already keeps `Shield` out of a group that has a real type
(`add_damage_type_non_pool`) — the log carries no energy type for a hit on
shields, only for one on hull — so a row's types are its weapon types, and a
group holding several weapons carries all of theirs. The picker offers only what
the comparison actually holds (`all_damage_types`).

A difference is `spread`: the row's largest value across the combats less its
smallest, **counting a combat that does not have the row as zero**. That zero is
the point — a row flown in two runs out of five is the difference being looked
for, and treating the absence as missing data would rank it as agreement. What
"value" means is the reader's choice, and neither answer is right on its own:

| measure | value per slot | why |
|---------|----------------|-----|
| `Share` | `damage_percentage.all` — the row's share of that combat's own damage, so the threshold is in percentage points | a shorter or weaker run does not read as a different build in every row at once |
| `Dps` | `dps.all` | what the row was actually worth, whatever share of the run it came to |

Each carries its own threshold, from zero up: a number that means something in
percentage points means nothing in DPS, and the bottom of either scale has to
mean "hide nothing". The figure itself is shown in a `Spread` column, so the
threshold is visible rather than an invisible rule the table obeys.

Two decisions worth keeping:

- **The order does not change.** Rows stay sorted by the reference combat's DPS,
  as they are with the toggle off, so a row known from the full table is where it
  was. The alternative — largest difference first — makes the toggle a different
  table rather than the same one with rows taken out.
- **A row missing from some combats says so** (`missing_from`, "(in 2 of 5)").
  "Flown in two runs out of five" and "flown in all five, unevenly" are different
  findings, and the numbers in the columns do not tell them apart at a glance.

Measured on five runs of one map: at a 3 pp threshold the tree drops from 25 rows
to 9, and at 18.5 pp to the two the runs really differ over — the antiproton
beams of one build and the phaser group of the other.

**The Total is added up from the rows that are ticked *and* on screen**
(`rows_left_out_of_the_total`). A figure the reader has filtered away is not
part of what they are reading, so narrowing to one damage type gives the Total
of that type, and raising the difference threshold gives the Total of what is
left. The two filters therefore reach the Total by different routes: the type
through the tree it is built from, the differences through which rows are
summed.

#### What one combat did differently

Both of these put their column **beside the name**, before the metrics. They
were at the far end, behind every metric of every combat, which on a wide
comparison is a screen or two of sideways scrolling: the reader saw the rows
reordered and no reason for it, which is exactly the report that came back.

`⚖ vs rest` puts a column beside the name saying what each row added to
one combat, or cost it, against the other combats in the comparison
(`CompareNode::impact`, `Comparison::impact_slot`). The rows under Total are
then ordered by how much they weighed either way (`sort_rows`) — the ranking is
the point of the column, unlike the differences toggle, which is a filter and
deliberately leaves the order alone.

The reference is the **mean** of the other combats, and that is not a matter of
taste:

```
impact(row) = dps(row, this combat) - mean(dps(row, other combats))
Σ impact(row) = impact(Total)      exactly, because a mean is additive
```

A median is steadier against one odd run but sums to nothing, and a column whose
figures do not add up to the one above them invites arithmetic that is wrong.
What a median is good for is said separately, in the tooltip:
`CompareNode::typicality` reports `(value − median) / MAD` over the other
combats — the median absolute deviation standing in for a standard deviation,
since a comparison holds a handful of runs and one odd one drags a mean and an
SD far enough to hide everything else. It is `None` when the others agree
exactly, or when there are fewer than two of them to disagree.

A combat without the row counts as zero on both measures: not having flown
something is exactly the difference being looked for.

#### The damage-type summary

`🎯 By type` opens a window of its own (`Comparison::show_type_summary`) holding
one row per damage type and one column per combat, each cell the share of that
combat the type came to. A window rather than another panel: the table and the
chart already divide the screen, and this is read once and put away.

`damage_by_type` walks the tree and **descends until a row has a single type**,
putting that whole row's damage under it (`collect_types`). Descending is what
makes the figure whole: the log gives no energy type for a hit on *shields*,
only for one on hull, so counting log lines by type loses a fifth to a quarter
of the damage to a `Shield` bucket. A row knows what weapon it is, and its
shield damage goes with it.

| case | where it lands | why |
|------|----------------|-----|
| one type | that type | the ordinary row |
| several types, sub-rows below | split among its sub-rows | a group of several weapons is told apart by what is under it |
| several types, nothing below | `mixed` | a proc that deals two types at once and has no rows to split |
| no type at all | `untyped` | the shares are meant to add up to the run, so nothing is quietly dropped |

Each type carries the rows it was built from (`TypeRow::parts`, largest first),
which the window unfolds under it — the question after "these two runs differ on
phaser" is always "on which phaser". A row nested under a group is named with
its path (`Beams › Phaser Array`), since a bare leaf name is not always enough
to place it.

Rows are sorted by `spread` — largest disagreement between the combats first,
since a type every run leaned on equally is the one thing this window has
nothing to say about — and by name where two are equally far apart, the rows
coming out of an `FxHashMap`.

#### Averages

`build_row` returns both shapes of a row in one pass: a `SlotCell` per combat,
and a `Vec<Option<AverageCell>>` — one entry per configured column, averaged
across the slots. The average is always built (a division per column), so
`CompareSettings::show_averages` is a redraw and not a rebuild.

The mean is plain: every combat counts once, percentages included, so the column
states what the columns above it average out to rather than a differently
weighted figure that no column shows. A row absent from a combat is left out
instead of counted as zero — an ability flown in two runs out of twelve would
otherwise read as a collapse. `AverageCell` therefore carries `count`, `min` and
`max`, and `average_tooltip` says which of the two cases a number is.

Averaged columns belong to every combat at once, so in that mode the note line
and the ΔDPS breakdown columns are suppressed (`Comparison::show_table`): both
are about one combat measured against the reference.

The chart follows the toggle too (`average_series`): one line instead of one per
combat. Every combat's hits are pooled onto a single axis — a hit already
carries its offset from the start of its own combat — and every figure is scaled
by `1/n`. The charts are linear in the point values (a smoothed sum for the
per-second lines, a bucketed sum for the bars), so the pooled-and-scaled series
**is** the mean of the individual lines at every point, not an approximation.
The window is the longest of the pooled combats, so no hit falls outside it; the
tail is then an average of fewer runs than the head, which is unavoidable when
runs differ in length.

Two consequences worth knowing:

- `PreparedHitValue::hits_count` is `f64`, not `u64`. A hit out of a combat
  contributes a whole 1.0, but a hit inside an average contributes `1/n` —
  without that, the hits-per-second and hits-count charts would draw the runs
  added up rather than averaged.
- The averaged series is built from `PreparedHit` points through
  `PreparedDataSet::base_new` rather than from `Hit`s through
  `PreparedDamageDataSet::new`, because the count only exists on the prepared
  point. `average_series` therefore repeats that constructor's `ValueFlags::
  IMMUNE` filter.

#### The workbook export

`app/export.rs` owns the layout and takes a `&[Sheet]`, one worksheet each. The
compare view hands it one sheet; the main window hands it six —
`main_tabs::export::all_sheets` builds one per tab, named after the tab
(`MainTab::name`, which a test holds to Excel's rules for a sheet name). Both
fill a `Sheet` from the analyzer's groups rather than from the built tables,
since a table cell holds text and a spreadsheet wants a number.
`Comparison::export_sheet` builds the compare view's data. The sheet is the table on screen minus the deltas — a spreadsheet can
subtract two of its own columns, and a delta arrives as text nothing can
compute with.

| decision | why |
|----------|-----|
| `MetricCell::value` carries the raw `f64` beside the formatted text | a workbook wants a number it can add up, not `"1.2M"` |
| every row of the tree is written, `open` or not, ticked or not | the file is the comparison; a spreadsheet has its own way of hiding rows |
| a filtered Total goes in under its `Total (k of n rows)` name | the file states what its top row is of, since the ticks that made it are not in there |
| a missing value writes no cell at all | a zero would average and chart as a real number |
| the name column is indented with spaces | survives a copy into anything else, unlike a cell indent |
| `Column::decimals` mirrors `CompareMetric::precision` | the file rounds the way the table does |

#### One combat, one name and one colour across a comparison

`Comparison` (`app/compare/compare_table.rs`) keeps the notes of its slots in
`notes`, refreshed from the settings every frame — the chart bakes its series
names in when it is built, so a note written while a comparison is up has to be
noticed and the chart rebuilt for it.

| where             | what it shows                                  | built by                    |
|-------------------|------------------------------------------------|-----------------------------|
| chart series name | `"<slot> — <note>"`, or the slot number alone  | `chart_label`               |
| column header     | metric name / `#<slot>` / note, on three lines | `header_text` → `LayoutJob` |
| legend above      | `"<slot>: <identifier> — <note>"`              | `legend_text` → `LayoutJob` |

Both the header and the legend are a `LayoutJob` rather than a string because
their parts differ in colour: the **number and the note** are drawn in the
colour of that combat's line on the chart, while what stands between them is
not — the metric name belongs to the whole group of columns, and the identifier
is long enough that a whole row of it in a chart colour reads as a warning.
A combat's colour is `theme::series_color(slot)` — its own, and fixed. The
charts otherwise colour a series by where it sorted (by total, largest first),
which is what the ability rows of one fight want and the wrong thing for
combats: the row ticks under Total change every combat's total, so a colour
taken from that order moved on every click, and moved at random once several
totals were equal at zero. `Comparison::rebuild_diagram` therefore pins each
series with `PreparedDataSet::with_color`, and `slot_colors` states the same
colour for the table without asking the chart. A slot the charted row is absent
from still gets `None`, and its number and note stay in the ordinary text
colour.

The note line is only added when some combat in the comparison carries one, and
the header height follows (`header_height`): the table reserves the height
before it draws, so `HEADER_LINE_HEIGHT` has to cover a row of the body font —
which is asserted in a test rather than assumed.

### Why eframe/winit, and not SDL3

Parked on 2026-08-02, after it was built far enough to judge. The branch
`experiment/sdl3` carries a working replacement of eframe/winit with a
hand-rolled SDL3 + egui-wgpu driver (`src/platform.rs`, ~360 lines): winit
leaves the dependency graph entirely and the app runs on Linux/Wayland, with
cursors, maximised-state persistence, the window icon and the Windows/macOS
paths still to do.

It is not merged, and should not be without a reason that is missing today. It
trades a maintained window layer for one we own, and until the Windows path
follows it means **two** window stacks side by side. The one real pain — an
overlay that stays above a full-screen game on Wayland — is already solved by
the layer-shell backend (`docs/OVERLAY.md`), which does not touch this choice.
The branch stays as a record; revisit only if winit blocks something users ask
for.

### One place for the look — `app/theme.rs`

Everything about how the app looks is declared in that one module and reaches
the screen through `theme::apply`, which the settings window calls at startup
and whenever the choice changes.

| what                | where                | note                                                                                                                                                                                     |
|---------------------|----------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| the themes on offer | `THEMES`             | one entry per theme: the `Theme` variant, its label, its `Visuals`, its `Palette`. The settings tab lists the registry, so adding a theme is a variant plus an entry — both in this file |
| widget colours      | `Visuals` per entry  | egui's own: backgrounds, strokes, selection                                                                                                                                              |
| the material        | `glassify` + `Glass` | the shape every theme shares: corner radii, the rim on each widget state, the window and popup shadows                                                                                   |
| the app's colours   | `Palette`            | what egui does not know about: the compare deltas, the warning mark, the status/upload marks, and the chart series                                                                       |
| text sizes          | `TEXT_SIZES`         | spelled out rather than inherited from egui, so the sizes are one table                                                                                                                  |

Colour and material are kept apart on purpose. Every entry's `Visuals` function
ends in `glassify`, so the app is made of one material throughout and the themes
differ only in colour — but `glassify` changes **shape only**. It never touches
a fill (`bg_fill`, `weak_bg_fill`, `faint_bg_color`), because those are what set
a drop-down, a text box or a table row apart from the page behind them;
replacing them with translucent panes looks like glass and reads like fog. The
accent it paints a pressed rim with is the theme's own `hyperlink_color`, the
one colour every theme already declares as bright enough for its background.
`a_field_stands_out_from_the_page_in_every_theme` holds that line: a resting
field has to differ from `panel_fill` by at least 15 of perceived brightness.

A corollary for the radius: a checkbox is about 14 points across and shares
`WidgetVisuals::corner_radius` with buttons, so the widget radius stays at 4 —
rounder turns every checkbox into a radio button.

Two things follow from `Theme` being stored in the settings file by variant
name: a variant may be **added but never renamed**, and both of egui's
light/dark slots get the same style — the app follows its own setting, not the
desktop's preference.

Which theme is active is a process-wide value (`ACTIVE`), so `theme::palette()`
works from any call site, including the overlay's separate egui context.

The series palette is eight hues validated as a set — lightness band, chroma
floor, and separation between neighbouring hues under normal vision and under
protanopia, deuteranopia and tritanopia — with a step for a dark surface and a
step for a light one. Past eight the order starts again: how many series a chart
holds is the user's choice, and every chart names its series in the legend and
on hover, so colour is never the only thing telling two apart.

### The colour-blind series

That validation covers **neighbouring** hues, which is what a chart of two or
three series draws. Across the whole eight the ordinary palette does collapse
for a dichromat — its blue against its violet is ΔE 2 under protanopia, its aqua
against its magenta ΔE 5 — and a chart of eight abilities draws all of them.

`Palette::color_blind_series` is a second set for that case, switched on by
`visuals.color_blind_series` in the settings and reaching the charts through
`theme::set_color_blind_series` → the `COLOR_BLIND` flag → `theme::series()`.
The flag sits beside `ACTIVE` and for the same reason: the overlay draws from
these colours in its own egui context.

|                          | ordinary                 | colour-blind              |
|--------------------------|--------------------------|---------------------------|
| worst pair, protanopia   | ΔE 2 (dark) / 16 (light) | ΔE 17 (dark) / 21 (light) |
| worst pair, deuteranopia | ΔE 5 (dark) / 14 (light) | ΔE 16 (dark) / 19 (light) |

Both sets are Okabe–Ito, the published colour-blind-safe eight, with the two
changes our surfaces force: on a dark plate its blue is lightened (2.0:1 against
the "Light Dark" chart plate at its published value) and its black becomes a
light neutral; on a white plate its yellow is replaced by a dark red rather than
darkened, because darkening walks it into the orange — for a dichromat those two
differ only in lightness.

The floors are asserted in `theme.rs`'s tests, which carry a Viénot/Brettel
dichromat simulation and a CIELAB distance, so retuning either palette cannot
quietly undo this. One test states the *relation* — the colour-blind set must be
further apart than the ordinary one — rather than a second absolute number.

Only the series move. `improve`/`worse` and the status marks keep their green
and red: a delta carries its `+`/`-` sign and a mark its word, so colour is not
what carries the meaning there.

## Log files on disk

STO under Proton rotates its combat log. On Linux `app/log_consolidation` merges
completed files into a single `combatlog.log` in the background so the overlay
and the combats list see one continuous history; the file currently being read
is never touched. Combats carry their byte range in the log (`Combat::log_pos`),
which is what Save Combat and combat deletion slice with — so anything that
touches line reading has to keep those ranges exact.

## Where things are written

Settings and the log file go to the per-user config directory
(`~/.config/STO-CLARE` on Linux, `%APPDATA%` on Windows), with the
old next-to-the-executable location read as a fallback. See
`app/settings/app_settings.rs` and `app/logging.rs`.

Logging is opt-in (Debug → **Enable Log**) and mirrors to stderr at `Info` and to
`STO-CLARE.log` at the chosen level. `log::set_logger` only takes effect once per
process, so `app/logging.rs` installs one router at startup and the settings
apply path swaps what it forwards to — the switch and the level therefore take
effect on **OK**, not at the next start, and switched off it closes the file and
drops `log::max_level()` to `Off` so every call site skips its formatting.

## Related documents

| document                       | scope                                                 |
|--------------------------------|-------------------------------------------------------|
| `docs/COMBATLOG_FORMAT.md`     | what the log's fields mean, and their sources         |
| `docs/HEALING_MODEL.md`        | the three healing pools and the two grouping orders   |
| `docs/DIFFICULTY_DETECTION.md` | how map and difficulty are derived                    |
| `docs/DETECTION_SAMPLES.md`    | the measurements behind the difficulty tiers          |
| `docs/OVERLAY.md`              | the always-on-top overlay, including the Wayland path |
| `docs/LADDER_UPLOAD.md`        | uploading a combat to the OSCR ladder                 |
| `docs/DISTRIBUTION.md`         | packaging and releases                                |
