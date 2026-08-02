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
  `DamageResistanceChart::sort`). egui hands out colours by the order items are
  added, so any chart that ordered its series differently gave the same player a
  different colour and a different place in the legend.
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
