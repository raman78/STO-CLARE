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

Several handlers can subscribe (`AnalysisHandler::get_handler`), and each says
what it wants of a live refresh:

| flag on `HandlerContext` | what arrives on a log change | who sets it |
|--------------------------|------------------------------|-------------|
| `auto_refresh`           | `AnalysisInfo::Refreshed` — the list **and** the newest combat, which moves the view onto it | the overlay always; the main window while "Auto Refresh" is on |
| `list_refresh`           | `AnalysisInfo::CombatsListRefreshed` — the list alone | the main window, always (`App::new`) |

The watcher runs while either is set, which is why the combats list is current
whatever the setting says: that setting is about the *view* following the log,
not the list. A handler that wants both gets one message, the whole one.

Parsing is incremental — `Analyzer::update` resumes where the last call stopped,
so a live refresh only reads what the log has grown by. `Instruction::ReadOneLog`
is the exception and reads a whole file: it builds a second `Analyzer` for a log
of its own, returns the fight in it as `AnalysisInfo::OneLog`, and throws the
analyzer away. The one holding the reader's log is untouched — that is the whole
point of it (see *Runs from the ladder* below).

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
| shell, toolbar, menus       | `app/mod.rs`             | owns `MainTabs`, `CompareView`, the runs off the ladder, the overlay handle |
| the list of fights          | `app/combats_list.rs`    | the side panel: one table of combats, and the cells every list of them is drawn from |
| per-combat tabs             | `app/main_tabs`          | Summary, Damage Dealt/Taken, the three healing tabs              |
| tables                      | `app/main_tabs/tables`   | one generic `MetricsTable<T>` driven by a static column list     |
| part of a tree              | `app/damage_subset.rs`   | the figures of a set of rows, for the ticks and the type pickers |
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
  context does not use it). The compare table uses the same face for anything it
  draws in a colour — `header_lines` and `delta_text` — because those colours are
  hues rather than steps in brightness, and on the light face they land as *less*
  ink than the plain text they sit beside.

Settings changes are gated by cost: only `analysis` invalidates the `Analyzer`
and forces a re-read of the log; a `general` change just rebuilds the views,
because formatting is baked into the row strings when a table is built.

The `combat_notes` section (`app/settings/combat_notes.rs`) holds the user's own
short description per combat, written in the Summary tab and repeated wherever a
combat is listed: the combats list (whose search box reads it), and the parts of
a comparison that name a run — its chart and its column headers (see below). It
is keyed by the combat's **start time**, which `CombatSummary` carries. The start
time is the only identifier the log itself fixes — `Combat::identifier` carries
whatever the name rules or the map detection produced, so a rename would orphan
the notes. Changing
`combat_separation_time_seconds` re-cuts the log into different combats and does
orphan them; there is no key that survives that.

### Reading part of a tree — `app/damage_subset`

Two views let a reader take rows out: the main window's tables (the player's own
row is of the rows ticked under it) and a comparison (its Total is, see
[Picking what the Total is of](#picking-what-the-total-is-of)). Both ask the same
question — what would these figures be if only these rows counted — and neither
can answer it by arithmetic on the columns: a percentage does not add, and
resistance, crit rate and accuracy are ratios of hit counts. The one place that
answers it is `app/damage_subset.rs`. It pools the hits of the rows that are
left and puts them back through the pass `DamageGroup::recalculate_metrics`
makes over a branch, so what comes out is what the analyzer would have said
about a group holding exactly those rows.

```
group.sub_groups()
   │  drop the rows that are out       (subset_hits, player_damage_without)
   ▼
Vec<Hit>
   │  DamageMetrics::calc_and_apply_delta            (subset_group)
   │  DamageMetrics::recalculate_time_based_metrics(metrics_duration(..))
   ▼
DamageGroup { hits: Hits::Leaf(..), .. }
```

| function                          | answers                                                                         |
|-----------------------------------|---------------------------------------------------------------------------------|
| `subset_hits`                     | the kept rows' hits, pooled, or `None` when none of them are there at all        |
| `subset_group`                    | a `DamageGroup` standing for a pool of hits, every metric recalculated from them |
| `player_damage_without`           | one player's outgoing damage without the named rows                             |
| `player_heal_without`             | the same for one heal pool, from `HealTick`s                                     |
| `damage_of_types` (`TypeFilter`)  | the tree rebuilt from what was dealt in the picked damage types                  |
| `metrics_duration`                | the seconds a rebuilt metric is divided by                                       |

What holds across both callers:

- **The result is recalculated, never subtracted.** The one thing that is *not*
  recomputed is `damage_percentage`: it stays a share of the combat's whole
  outgoing damage (`Combat::total_damage_out`), because the column means the
  same filtered or not — how much of the fight this is.
- **An absent combat is `None`; a player who ticked everything off is zero.**
  `subset_hits` and `damage_of_types` answer `None` when the rows are not there
  at all, and the comparison leaves that column empty and off the chart — a zero
  would read as a run that did nothing rather than one that flew something else.
  `player_damage_without` and `player_heal_without` always hand back a group,
  zeroed when every row is out: that player is still on screen with every tick
  box they had, and one click brings the figures back.
- **A rebuilt group keeps every row under it**, ticked and unticked alike: its
  `sub_groups` are the whole of the original's. They are what the ticks are
  *of*: dropping the unticked ones would take their tick boxes off the screen and
  leave the row above without the count that tells all/some/none apart.
- **Each pool is divided by the duration the analyzer divides it by** —
  `Player::combat_time` for outgoing damage, `Player::active_time` for healing,
  both through `metrics_duration`. The charts' `combat_duration_seconds` is a
  different question and would state a DPS nothing else in the program agrees
  with.
- **Percentages down a rebuilt tree are set by hand** (`set_child_percentages`):
  the analyzer's own pass is `pub(super)`, and a rebuilt group has nobody to have
  set them. The top row's share is of the whole fight, each row below of its
  parent, which is what the analyzer states.
- **A picked type is applied before the ticks**, since the type decides what the
  rows even are: `DamageTab::kept_damage` filters by type first and reads the
  ticks over what is left. (The comparison reaches the same end by two different
  routes — see [Which rows are on screen](#which-rows-are-on-screen).)

**The main window's side of it.** `RowTicks` (`main_tabs/tables/metrics_table.rs`)
is the tick column plus the two buttons in the `Name` header, and the tabs own
the state it edits:

| state                | owner                          | what it is                                                     |
|----------------------|--------------------------------|-----------------------------------------------------------------|
| `excluded`           | `DamageTab`, `HealTab`         | per player, the rows that are out — one tick tree each          |
| `hide_unticked`      | `DamageTab`, `HealTab`         | the `👁`: unticked rows off the screen, out of the figures either way |
| `types`, `all_types` | `DamageTab`                    | the `☰ Type` picker; `HealTab` passes `all_types: &[]`, healing having no damage type |

| decision | why |
|----------|-----|
| the ticks are keyed by `NameHandle`, not by the name | a grouping rule can give a group the name of an ability it collects, and then two different rows read the same; ticking one would take both. The comparison keys by name instead — it aligns its runs by name, so a handle from one combat means nothing in the next |
| one tick tree per player | two players who both flew a Phaser Beam Array can have it ticked off for one and in for the other |
| ticks only at two levels | `RowTicks::show_cell`: the player's row carries the tri-state tick that stands for all of theirs, the rows directly under it carry their own, deeper there is none. A branch's hits are the whole branch's, so a row goes in with what it is part of |
| the table and the chart are built from one function | `DamageTab::rebuild` feeds `kept_damage` to both, so a chart cannot end up drawing rows the table has taken out. `MetricsTable::take_state_from` carries the open tree and the sort order across that rebuild |
| `MetricsTable::new_base` takes a `Cow<G>` | the unfiltered case — nothing ticked off, no type picked — borrows the analyzer's own group and copies nothing |

Open question: `excluded` is keyed by handles from the open combat, and
`MainTabs::update` does not clear it when another combat arrives, so ticks made
on one combat land on whatever rows happen to hold those handles in the next.
`types` does not have the problem (it holds type names). Nobody has decided
whether the ticks should be dropped on a combat change or translated through the
names.

### The numbers that were chosen — `app/tuning.rs`

Sizes, limits and the room the window leaves for things live in one module, with
the reason each has the value it has. A number that falls out of the code — an
index, a divisor, a count — stays where it is used; a number settled by
measuring the program or by taste goes here, so a maintainer has one file to
open rather than a grep to run.

| what | value | changing it |
|------|-------|-------------|
| `ROW_HEIGHT`, `HEADER_HEIGHT` | 25.0 | the combats table's rows |
| `PANEL_MIN_WIDTH` | 260.0 | how far the panel can be dragged in before the table is a column of ellipses |
| `PANEL_AUTO_WIDTH` | 1200.0 | how wide it will size *itself* to fit its table before it stops and scrolls |
| `CELL_SPACING` | 3.0 | the gap either side of a cell in that table |
| `PLAYER_PICKER_WIDTH` | 130.0 | the "whose figures" picker in a comparison |
| `BADGE_PADDING`, `ARROW_SIZE` | 4.0, 14×14 | a run's number badge, the fold-out arrow |
| `DEATHS_MENU_WIDTH` / `_HEIGHT` | 230.0 / 260.0 | the deaths checklist popup |
| `PICKER_MIN_WIDTH` | 60.0 | the floor `fitting` squeezes a filter picker to |
| `JOB_WINDOW_WIDTH` | 260.0 | the window that reports clearing the log |
| `MAX_NOTE_CHARS` | 50 | re-exported, not defined here: it truncates what is **stored**, so it lives with the store that enforces it (`settings::CombatNotes::set`) |

`note_width(ui)` is the one *rule* in there rather than a raw number:
`glyph_width('0') × MAX_NOTE_CHARS × NOTE_WIDTH_SLACK`, measured from the font in
use so it holds at any UI scale. Both places that show a note ask for it — the
field under the tabs where one is written, and `CombatColumn::Note`, which
**reserves** it whether or not there is a note to show. Sized to its rows the
column was a sliver on a log nobody had written in, and the first note anybody
wrote moved every column beside it.

Measured cost: **440 points** of panel width at the default scale
(`print_column_widths`, the last figure in each row it prints). That is most of
the headroom `PANEL_AUTO_WIDTH` had: with a run from the ladder in the list —
which adds the column its `✕` sits in — and a comparison being picked, the
panel reaches the cap and its table scrolls sideways. Anything that has to be
photographed beside the panel no longer fits at 1280×720, which is why
`demo/screenshots.sh` folds the panel away before shooting `ladder-run`.

The slack is a judgement, not a guarantee: the face is proportional, so fifty
`M`s are wider than the room reserved and would still push the column out. Fifty
characters of prose fit, which is what a note is.

### Saying what the thread is doing — `app/job.rs`

`is_busy` (an `AtomicBool`) is enough for a refresh: it takes as long as it
takes and cannot be interrupted, and the toolbar hourglass is the whole of what
there is to say. Clearing the log is not that. It reads every kept fight out of
the file, replaces the file and reads the whole thing back, and until that
finishes **the window is showing a list of fights that no longer exist**.

`JobStatus` is an `Arc` of atomics shared with the drawing thread, read once per
frame (`AnalysisHandler::job_progress`) rather than sent down the info channel —
the worker only reaches that channel between instructions, so progress would
arrive in one burst at the end.

| `Phase` | counted | cancellable | why |
|---------|---------|-------------|-----|
| `CopyingKept` | yes, `done`/`total` fights | **yes** | nothing has been written; giving up leaves the log as it was |
| `RewritingLog` | no | no | `rewrite_file` replaces the file in one step (temp → `sync_data` → `rename`); there is no half way to stop at |
| `ReadingLogAgain` | no | no | this is what puts the new list on screen; stopping it leaves the window on a log that no longer exists |

`total == 0` is what makes the window draw a spinner instead of a bar it would
have to make up. The cancel flag is checked by the worker
(`JobStatus::cancelled`), not by the button: a press outside `CopyingKept` is
kept but not acted on.

**The status is cleared by the run loop**, beside `set_is_busy(false)`, not by
`keep_combats`. Both of that function's aborts — a fight that cannot be read out
of the log, a cancelled deletion — return early, and clearing it where the
phases are set would leave the window holding a bar for work nobody is doing.

Two things follow from a deletion being in flight:

| what | why |
|------|-----|
| the combats list is drawn disabled (`CombatsListView::locked`) | a fight is asked for **by its place in the list**, and the list on screen is the one from before the rewrite. A second delete, or a double-click, would hand the analyzer positions that mean something else by the time it reads them |
| the window keeps repainting (`request_repaint_after`) | nothing is sent from the thread during a phase, so the progress would otherwise stand still at whatever the last click drew |

The rest of the window is left alone deliberately: reading the fight already on
screen is safe, so this is a plain `Window`, not a `Modal` with a backdrop.

Covered by tests in `app/analysis_handling.rs` that run a real deletion against
a synthetic log in a scratch directory: a rewrite that keeps a subset, deleting
**every** fight (the empty-log case), an index the list no longer holds, and a
cancel — each asserting the log's bytes and that the context still reads it
afterwards.

### The list of fights — `app/combats_list.rs`

One table of combats, drawn in a side panel and used for everything that picks a
fight. There is no second list anywhere: the compare view carried a picker of its
own until 2.5, and two lists of the same fights disagreed about which of them was
filtered how.

What travels from the analysis thread is `CombatSummary`
(`analyzer/combat_summary.rs`), one value per fight — name, identifier, map,
content type, environment, difficulty, solo, start, duration and the players with
their DPS and how often each was killed — sent as `Arc<[CombatSummary]>`. It
replaced six `Vec`s indexed alongside each other, where one list left behind read
the wrong entry for every combat after it.

`PlayerSummary::deaths` is the same count `Combat::total_deaths` adds up over
everyone (`player.damage_in.kills`), kept per player because the list filters by
it.

**A fight is named by `start`.** Indices are only ever used to ask the analyzer
for a combat; anything the list has to match — a tick, a fold-out, the row a
comparison's slot belongs to — uses the start time, because the list is live (a
fight with no damage in it is dropped as it grows) and because a run off the
ladder is not in the analyzer's list at all.

| what | held in | keyed by |
|------|---------|----------|
| ticked for deletion | `CombatsPanel::to_delete` (`FxHashSet`) | start |
| ticked for a comparison | `CombatsPanel::to_compare` (`Vec`, in tick order) | start |
| folded-out player lists | `CombatsPanel::unfolded` | start |
| the run each comparison slot is of | `ComparisonSlot::start` | start |

`to_compare` is a `Vec` and not a set because its order *is* the numbering: the
badge a row carries is its position in it, and the comparison is built in that
order, so its columns and colours follow the order the reader ticked.

The panel has three modes on one mechanism — browse, `Clearing`, `Comparing` —
and which one is on is **asked of the window every frame** (`CombatsListView::comparing`)
rather than remembered: copied at the click, folding the list away left the panel
browsing while the window was still comparing.

Decisions worth keeping:

| decision | why |
|----------|-----|
| ticks are pruned to what is on screen | acting on a fight a filter is hiding is acting out of sight; the count in the strip would lie about it. The runs off the ladder count as on screen — they are rows of this list, and the filters do not reach them |
| columns of short words have a fixed width (`CombatColumn::widest`) | so the same column is the same width in every list, and a column of four-letter words does not take as much room as the map name beside it |
| the panel is exactly as wide as its table | `fitting_width` measures the table (`table::table_content_width`) and the panel is pinned to it until the reader drags the edge, after which their width is remembered and only a drag changes it — read back every frame, an empty table (a log still being read) overwrote it |
| the fold-out arrow is always drawn, invisible where there is nothing to fold | `Ui::add_visible` keeps the geometry identical; room measured out beside it instead was room of a different size, and the column drew two points wider or narrower depending on which rows were in it |
| a run's number is a badge, not coloured text | several series colours vanish into the blue of a picked row; `theme::badge_colors` fills a patch with the run's colour and picks black or white for the number, taking the fill a shade further where neither reaches 3:1 |

#### The filter menus — `app/combat_filter.rs`

`CombatFilter` is the row of pickers above the table. It is asked about one
`CombatEntry` at a time — a borrowed view of a `CombatSummary`, built through
`From`, so nothing is copied per frame and the two cannot drift apart.
`CombatsPanel::matches` asks it per combat; `CombatsPanel::show_contents` hands
it the whole list as `&[CombatEntry]` so each menu can work out what to offer.

| part | what it asks | empty value |
|------|--------------|-------------|
| `solo` | one player in the log — the ladder's test, `Combat::is_solo` | `None` |
| `environment` | the detected map's curated environment (Space, Ground, …) | `None` |
| `difficulty` | `DifficultyFilter`; its `Unknown` catches a map whose tier did not resolve, which would otherwise be invisible under every setting | `Any` |
| `map` | `CombatSummary::base_name` — with the `[TFO]` prefix, since that is what naming rules are written against | `None` |
| `deaths_of` + `deaths` | a fact about the *players* in the fight, not about the fight: which handles, and which of the two questions they are being asked | empty set |

Four of them narrow by what a fight **was**; the deaths menu narrows by how it
**went**, and is the only one whose answer depends on more than the combat's own
columns.

Every menu offers only what the others leave reachable — `options(combats,
dimension)` re-runs the filter with that one dimension cleared — and
`drop_impossible_choices` gives up a choice the rest have made unreachable. The
invariant: **no combination reachable through the menus leaves the list empty**,
with one deliberate exception below.

A picker with nothing to offer is **drawn disabled, not hidden**
(`Ui::add_enabled_ui`, with a `disabled_hover` saying why): the size picker
where every fight on screen was fought the same way, the deaths menu where
nobody answers its question. They used to be left out, and a picker that comes
and goes moves every picker beside it — the row is read by where things are.
The cost is that the combo box then sits in a child `Ui` whose id egui chooses,
so `show_deaths` writes its popup's id into `Ui::data` under
`deaths_popup_key(id)` for anything outside that needs to find it.

#### The deaths menu

A checklist of handles rather than a picker, because the question is often about
more than one player — a team's clean runs — and a tick box says "and this one
too" where a drop-down says "instead of that one".

`deaths_of` is a `BTreeSet<String>` of handles; `deaths` is a `DeathsFilter`
saying which way they read. A handle answers for a fight when it is **present
in it** with a death count the direction wants (`DeathsFilter::matches`); how
several handles add up is the direction's too (`DeathsFilter::matches_all`):

| `DeathsFilter` | keeps | set by |
|----------------|-------|--------|
| `Without` (default) | fights **every** ticked handle is in with `deaths == 0` | browsing, and while a comparison is being picked |
| `With` | fights **any** ticked handle is in with `deaths > 0` | `PanelMode::Clearing` |

The quantifier turns round with the direction, and that is the point of it: the
two are complements. Ticking a group, `Without` is "the runs that went perfectly
for all of them" and `With` is "the runs somebody has something to say about",
so between them they split the fights those players were all in. `matches_all`
holds both, and special-cases the empty set — `all` over nothing is true and
`any` over nothing is false, so a menu with no ticks in it would otherwise hide
the whole log the moment the panel started clearing.

The direction is not a control of its own: `CombatsPanel::show_contents` sets it
from the mode each frame and bumps `filter_generation` when it changes. Clearing
the log is a list of fights **to delete**, and the fights worth deleting are the
ones somebody died in — the same ticks, asked the other way round, so the list
is always showing what the mode acts on.

Four decisions sit behind the rest of it:

| decision | why |
|----------|-----|
| `Without` `and`s the ticks | "no deaths of @me and @friend" is one question about one run they both had a clean pass at. `or` there would answer a question nobody asks |
| `With` `or`s them | one death by one of them is a run that did not go well, and that is what is being cleared out. It also means this direction **cannot** come back empty: every offered handle has fights behind it, and the union keeps them |
| a fight the handle was not in does **not** pass, either direction | the menu asks how *their* runs went; a run without them is not an answer. Under `With` it also stops a fight being lined up for deletion because a ticked player was absent from it |
| the direction follows the mode instead of being a third state | a checkbox for "invert" is a second thing to notice; the box's own label and the line above the list already say which question is being asked — including with nothing ticked (`Any deaths` / `All fights`), which is the moment the change is easiest to miss |

Turning the menu round re-reads who it can offer: `options` collects the handles
that answer the *current* direction, so a player who never died is not offered
while clearing, and `drop_impossible_choices` gives up their tick rather than
leaving it holding an empty list.

Entering `Clearing` ticks **nothing**. It used to seed `to_delete` with every
fight on screen but the newest; a pre-ticked list makes unticking-to-safety the
work, on a button whose next press rewrites the log, and turning the deaths menu
round would have seeded it from a list that no longer means the same thing.
`Select all` covers the bulk case from the safe end.

`Without` is the one filter here that **may leave the list empty** — the
exception to the invariant above. `and`ed ticks the reader made deliberately are not
something to undo behind their back, so a set of handles with no clean run
between them narrows to nothing and the footer count (`ListCounts::text`) says
so until one is unticked. `drop_impossible_choices` still drops a handle that
survives nothing at all under the *other* menus, which is the case the cascade
exists for (a refresh, a deleted fight) rather than a choice the reader made.

The rows are ordered by how many fights on screen each handle answers for
(`by_fights_matched`), which floats the log's owner to the top without the
filter being told whose log it is. The popup is a `ComboBox` with
`PopupCloseBehavior::CloseOnClickOutside` — a tick is not a choice made and done
with — under a wrapped `deaths_prompt` line saying what the ticks do, since a
column of handles does not say it and the box is too narrow to. The label is
wrapped explicitly because a popup lays its contents out with wrapping off
(`TextWrapMode::Extend`), which would widen the whole menu to one long line.

The text in the search box lives in `Ui::data`, not in the filter: it narrows
the *menu*, and a filter that changed as it was typed in would have the table
re-measured (`filter_generation`) on every keystroke.

### Comparing several combats — `app/compare`

`CompareView` (`app/compare/mod.rs`) is the table and nothing else: the fights it
is of are ticked in the combats list, and every change of those ticks rebuilds
it. `ListAction::Compare` carries the ticked start times; `App::compare_fights`
splits them into runs it holds already and fights the analyzer has to be asked
for (`Instruction::GetCombats`), remembers the order in `App::pending_compare`,
and `App::build_comparison` puts the two together in that order once the answer
arrives.

Rebuilding rather than dropping a slot is deliberate: numbering from one each
time is what keeps unticking one fight and ticking another from walking the
numbers — and the colours with them — up and up.

What the list needs back from a comparison is `CompareView::slots()`:
`ComparisonSlot { start, player }`, one per run. The number and the colour are
the list's own (see above); only *whose figures are being read* is the
comparison's to say, and `Comparison::set_player` takes a start time and a
handle to change it.

### Runs from the ladder — `App::ladder_runs`

The magnifier in the Ladder window (`upload/records.rs`) fetches a run into a
scratch file and hands the path to `App::open_ladder_run`, which asks for it with
`Instruction::ReadOneLog`. The answer is kept as a `LadderRun { path, combat,
summary }` and shown as a row at the top of the list, in `Palette::busy`, with a
`✕` that drops it. Several can be open at once.

Until 2.5 a run *replaced* the analyzer's log for as long as it was on screen.
Everything that had to be carried across that switch — the reader's own fights,
captured beforehand; a third log composed from the run and the fights it was
compared against; a mode to be in and a way out — meant the same fight had three
different indices depending on which of the three logs was being counted, and
every bug in the feature was one of those three confused for another. Holding the
run beside the log removed the class.

| what a run is not | why |
|-------------------|-----|
| uploadable | it is already on the ladder, and it is somebody else's fight (`App::showing_ladder_run` gates the Upload button) |
| cut out of the reader's log when saved | it *is* a log of one fight — `App::save_shown_combat` copies the file it was fetched into |
| filtered by the list's menus | it is not a fight out of the log those menus narrow; it leads the list whatever the headings sort by |

What the magnifier does is split between the press and the answer, and the two
halves are deliberate:

| when | what | why |
|------|------|-----|
| at the press (`App::open_ladder_run`) | `CombatsPanel::open` | reading the log takes a moment, and the panel opening is what answers the press. With the list folded away, adding a run to it changed nothing anybody could see |
| when the fight arrives (`AnalysisInfo::OneLog`) | `suggest_filter`, then `open_combat` | the run goes on screen because that is what the button says it does — it was pressed on a row of its own, and a row added to a list for the reader to find is a step the label already promised away. The filter is pointed at the reader's own runs of the same map and level, which is what a fetched run is nearly always fetched for |

Pressing it on several runs leaves the last one on screen and the rest a
double-click away in the list.

One thing the ladder window must be told: which runs are already open
(`Records::show(.., already_open)`), so the magnifier on one of them is drawn
spent and takes no click. The click used to land even on the greyed label, and a
second press threw away the fetch already running.

#### What a selected combat costs

Nothing caps the selection: there is no maximum, and none of the two limits
below is enforced. What gives way instead is stated in the list's footer by
`compare::selection_hint` (`MANY_COMBATS`, `COLORS_RUN_OUT_AT`), because both
limits are gradual:

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
(`ScrollArea::horizontal`), so more columns only ever means more scrolling. Two
panes share the height — the table and the chart — with one draggable boundary.

There used to be a third: a legend, one row per combat, which at 34 runs filled a
720-point window and pushed the table and the chart past the bottom edge. It is
gone; which runs a comparison is of, in what colour, under what number and read
for which player is said in the combats list, on the rows they were ticked on.

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

**A scrolling list must not be wider than its pane.** Every vertical
`ScrollArea` in the program carries `auto_shrink([false, true])` so its bar sits
at the edge of the pane rather than against the longest line in it. That only
holds while the content fits: with `direction_enabled[0] == false` and
`auto_shrink[0] == false`, egui sizes the inner rect as
`inner.max(content_size.x)` (`scroll_area.rs`), so one over-long line widens the
whole area and carries the vertical bar off the right-hand edge with it. That is
what hid the bar of the comparison's own list of runs, back when it had one. Any
label
that can outgrow its pane is `Label::truncate()`d — except in the combats list,
where nothing wraps or truncates (`TextWrapMode::Extend`): a cell that wraps
asks for less width than its text needs, and the panel sizes itself to its
table, so a long map name would have folded and the panel settled around the
fold.

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

Per slot, the rows named in `Comparison::excluded` are dropped and the rest
pooled and recalculated by `app/damage_subset` — `subset_hits`, `subset_group`,
`metrics_duration(&player.combat_time)`, described in
[Reading part of a tree](#reading-part-of-a-tree--appdamage_subset) along with
the invariants that hold there. The synthetic group it hands back is fed to the
same `build_row` and `build_series` the tree is built with, so a filtered Total
carries deltas, averages and a chart series like any other row, and nothing
downstream knows it was filtered.

| decision | why |
|----------|-----|
| with nothing excluded the row is rebuilt from `player.damage_out` instead | float addition is not associative, and an unfiltered Total should be the analyzer's own figure to the last digit rather than the pieces added back in another order |
| the ticks are held by row name, not by node id | a column change or another player rebuilds the tree and hands out fresh ids; the reader's selection has no reason to survive one and not the other |
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
| `Δ Spread` in the toolbar | `show_differences`, `difference_measure`, `share_threshold` / `dps_threshold` | only the rows the combats disagree over |

The type picker is **not** a view filter: picking a type rebuilds the tree from
a copy of the player's damage holding only what was dealt in it
(`damage_subset::damage_of_types`, kept per slot in `Comparison::of_type` and
reached through the local `of_type`). So `Polaron Beam Array` under `Cold` shows
the 60k its Frostbite proc did, not the 4.3M the beams did around it, and every
column of that row — DPS, resistance, criticals — is of the proc, having been
recomputed from its hits. Which level of the tree that rebuild happens at, and
why the percentages have to be set by hand afterwards, is in
[Reading part of a tree](#reading-part-of-a-tree--appdamage_subset).

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

Every combat counts once — never weighted by how long a run was or how much it
dealt, so the column states what the numbers above it average out to.

What a run the row was never flown in counts as depends on the metric, decided
by `CompareMetric::adds_up`:

| Kind | Metrics | Divisor | Why |
|---|---|---|---|
| adds up | DPS, Total Damage, Damage %, Hits, Hits/s, Base DPS | every run of the comparison | in one run the rows come to the row above them, so their averages have to as well |
| ratio | Resistance %, Critical %, Flanking %, Accuracy %, Average Hit, Max One-Hit | the runs that have the row | a run in which an ability never fired has no rate to average in, and a zero would report a collapse that never happened |

The divisor for the first kind is `runs_in_play` — the columns whose player has
a damage tree at all, *not* the column count, since a run the player is absent
from has no figure on the Total either. It is computed once per rebuild and
passed down `build_level` unchanged, so every depth divides by the same number;
recounting per level would make a branch's children sum to something other than
the branch. `Comparison::refresh_total` counts it off the whole trees rather
than off the ticked subsets, for the same reason.

This is a correctness property and not a matter of taste. Measured on a
three-run comparison of one map: with the ratio rule applied to DPS as well, the
ability rows came to 679'617 against a Total of 661'229 — the whole 18'388 gap
being one ability flown in a single run, divided by that run alone. The
regression test is `the_rows_average_out_to_the_row_above_them`.

`AverageCell` carries both counts (`count`, `runs`) plus `over_all_runs`, so a
diluted figure can say so instead of reading as a bad run: the table draws a weak
`count/runs` beside any partial row, `average_tooltip` states what the missing
runs counted as and what the row did in the runs it was in, and the spreadsheet
export — which has no tooltip to hover — leads the averaged metrics with an
`In combats` column (`CompareNode::present_in`).

Averaged columns belong to every combat at once, so in that mode the note line
and the ΔDPS breakdown columns are suppressed (`Comparison::show_table`): both
are about one combat measured against the reference.

The chart follows the toggle too (`average_series`): one line instead of one per
combat. Every combat's hits are pooled onto a single axis — a hit already
carries its offset from the start of its own combat — and every figure is scaled
by `1/runs`, the same divisor the metrics that add up use, so a branch's line and
the Total's line stay on one scale; charting a row against its own runs while the
Total was charted against all of them drew two lines that do not compare. The
charts are linear in the point values (a smoothed sum for the per-second lines, a
bucketed sum for the bars), so the pooled-and-scaled series **is** the mean of
the individual lines at every point, not an approximation. `average_label` says
which runs the line is of ("average over 3 combats, flown in 1"), since a line
drawn at a third of its height cannot show whether the ability is rare or weak.
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
| column header     | metric name / `#<slot>` / note, on three lines | `header_lines` → `LayoutJob` |
| the combats list  | a badge with the run's number, in its colour   | `combats_list::show_number_badge` |

The header is a `LayoutJob` rather than a string because its parts differ in
colour: the **number and the note** are drawn in the
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
the header height follows (`header_height`: two lines of the body font, three
with notes). The table reserves that height before it draws anything, so it is
measured from the font in use rather than assumed — a line taller than the
reserve had its bottom half cut off by the row below, which is asserted in a
test.

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
