# Technical documentation

Notes for people who read or change the code. User-facing material lives at the
top level: `README.md` for the overview and installing, `MANUAL.md` for the
guide to using the program.

Start with **`ARCHITECTURE.md`** — it maps the modules, the threads and the
path a log line takes to become a number in a table, and links onward.

| Document                  | Scope                                                                                                                            |
|---------------------------|----------------------------------------------------------------------------------------------------------------------------------|
| `ARCHITECTURE.md`         | Modules, threads, data flow, UI conventions. The entry point.                                                                    |
| `COMBATLOG_FORMAT.md`     | What the log's numeric fields mean per line kind, the sources for it, the resistance formulas, and what the log cannot tell you. |
| `HEALING_MODEL.md`        | The three disjoint healing pools, how a record is routed into one, and the two grouping orders.                                  |
| `DIFFICULTY_DETECTION.md` | How a combat's map and difficulty are derived, and why name rules alone cannot do it.                                            |
| `DETECTION_SAMPLES.md`    | The measurements the difficulty tiers were built from.                                                                           |
| `OVERLAY.md`              | The always-on-top overlay, including the Wayland layer-shell path.                                                               |
| `LADDER_UPLOAD.md`        | Uploading a combat to the OSCR ladder, and why one is rejected.                                                                  |
| `DISTRIBUTION.md`         | Packaging, installers and releases.                                                                                              |

`legacy_combat_name_rules.json` is not a document: it is the archived set of
hand-written naming rules that detection replaced, kept as a safety net and as
a list of maps still missing an anchor. `DIFFICULTY_DETECTION.md` explains it.
