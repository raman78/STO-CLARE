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

| module | responsibility | depends on |
|---|---|---|
| `analyzer` | reads the log, builds `Combat`s and their metric trees | nothing in the app |
| `app` | windows, tables, charts, settings, overlay, the analysis thread | `analyzer`, `upload` |
| `upload` | packaging and sending a combat to the OSCR ladder | `analyzer` |
| `custom_widgets` | table, splitter, sliders — egui widgets the app needs and egui lacks | egui only |
| `helpers` | number formatting, small shared utilities | nothing |

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

| area | module | notes |
|---|---|---|
| shell, combat picker, menus | `app/mod.rs` | owns `MainTabs`, `CompareView`, the overlay handle |
| per-combat tabs | `app/main_tabs` | Summary, Damage Dealt/Taken, the three healing tabs |
| tables | `app/main_tabs/tables` | one generic `MetricsTable<T>` driven by a static column list |
| charts | `app/main_tabs/diagrams` | Gauss-filtered per-second graphs and time-sliced bar charts |
| compare | `app/compare` | several combats side by side with coloured deltas |
| settings | `app/settings` | split into analysis settings (invalidate the parse) and the rest |
| how it looks | `app/theme.rs` | the themes on offer, the app's own colours, the text sizes |
| overlay | `app/overlay` | separate always-on-top window; see `docs/OVERLAY.md` |

Two conventions worth knowing before changing a table or a chart:

- **Columns are data.** A table is a `&'static [ColumnDescriptor<T>]`; a column
  carries its label, its sort function and its render function. A metric that
  splits into hull and shield uses `shield_hull_col!`, which adds the two extra
  cells that the split-columns setting shows.
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
box reads it) and the compare legend. It is keyed by the combat's **start time**,
which the refresh messages carry alongside the list (`start_times`, aligned with
`combats`) because those views hold parallel arrays rather than whole combats.
The start time is the only identifier the log itself fixes — `Combat::identifier` carries whatever the name rules or
the map detection produced, so a rename would orphan the notes. Changing
`combat_separation_time_seconds` re-cuts the log into different combats and does
orphan them; there is no key that survives that.

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

| what | where | note |
|---|---|---|
| the themes on offer | `THEMES` | one entry per theme: the `Theme` variant, its label, its `Visuals`, its `Palette`. The settings tab lists the registry, so adding a theme is a variant plus an entry — both in this file |
| widget colours | `Visuals` per entry | egui's own: backgrounds, strokes, selection |
| the material | `glassify` + `Glass` | the shape every theme shares: corner radii, the rim on each widget state, the window and popup shadows |
| the app's colours | `Palette` | what egui does not know about: the compare deltas, the warning mark, the status/upload marks, and the chart series |
| text sizes | `TEXT_SIZES` | spelled out rather than inherited from egui, so the sizes are one table |

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

## Related documents

| document | scope |
|---|---|
| `docs/COMBATLOG_FORMAT.md` | what the log's fields mean, and their sources |
| `docs/HEALING_MODEL.md` | the three healing pools and the two grouping orders |
| `docs/DIFFICULTY_DETECTION.md` | how map and difficulty are derived |
| `docs/DETECTION_SAMPLES.md` | the measurements behind the difficulty tiers |
| `docs/OVERLAY.md` | the always-on-top overlay, including the Wayland path |
| `docs/LADDER_UPLOAD.md` | uploading a combat to the OSCR ladder |
| `docs/DISTRIBUTION.md` | packaging and releases |
