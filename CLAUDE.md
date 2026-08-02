# STO-CLARE — Claude Code Context

## Project overview

**STO-CLARE** (Combat Log Analyzer ReMastered) is a desktop app that parses and
analyzes Star Trek Online combat logs (DPS, healing, hits, kills, per-combat
breakdowns, live overlay, OSCR upload). It is a native GUI application built
with **egui/eframe**.

The project began as a fork of **AnotherNathan/STO_CombatLogAnalyzer** and was
renamed to STO-CLARE in 2.0.0, when it went its own way. The attribution stays:
both licence files carry the original copyright, and the README says plainly
what it is a fork of. Binary/package name: `sto-clare`.

- **`origin`** → `raman78/STO-CLARE` (ours — push here)
- **`upstream`** → `AnotherNathan/STO_CombatLogAnalyzer` (the original)

Two pull requests are still open upstream (#6 overlay, #7 combats/logs). Unless
a change is meant for one of them, it does not need to be upstream-shaped.

**Stack:** Rust (edition 2024), eframe/egui `0.34` with the `wgpu` renderer,
egui_plot `0.35`, `log` + `simplelog`, serde/serde_json.
**Entry point:** `src/main.rs` → `eframe::run_native` → `app::App`.

Cross-platform target: **Linux and Windows** must both work. Be mindful of
platform differences (e.g. Wayland is stricter than X11/Windows about window
geometry and surface configuration).

---

## Language rules

**All code must be in English** — comments, log messages, docstrings, variable
names, string literals visible in logs. No Polish in source files. When editing
existing code that contains Polish log messages or comments, translate them to
English.

(Conversation with the maintainer may be in Polish; the code and repo artefacts
stay English.)

---

## Rules

1. First think through the problem, read the codebase for relevant files.
2. Before you make any major changes, check in with me and I will verify the plan.
3. Please every step of the way just give me a high level explanation of what changes you made.
4. Make every task and code change you do as simple as possible yet not naive. We want to avoid making any massive or complex changes. Every change should impact as little code as possible. Everything is about simplicity.
5. Maintain a documentation file that describes how the architecture of the app works inside and out.
6. Maintain documentation files in the project. Recognize which are technical and which are more human-readable (manual, program description, readme).
7. Never speculate about code you have not opened. If the user references a specific file, you MUST read the file before answering. Make sure to investigate and read relevant files BEFORE answering questions about the codebase. Never make any claims about code before investigating unless you are certain of the correct answer — give grounded and hallucination-free answers.
8. Never use workarounds. Especially never change existing code just to fix your freshly made problem. Only recent changes are supposed to be fixed. If situation requires fixing existing code it requires user one-time approval.
9. NEVER EVER USE force flags (`-Force` in PowerShell, `-f`/`--force` in git/rm/etc.) in terminal commands. It is strictly forbidden! If there is no other way you NEED to ask the user to run the command in terminal themselves providing justification.

---

## Branch workflow

- Do work on a dedicated feature branch (e.g. `fix/overlay-wayland-crash`),
  never directly on `main`.
- `main` is the default branch; `develop` collects work between releases.
- A change meant for the original project is opened as a PR against
  `AnotherNathan/STO_CombatLogAnalyzer` and must not depend on anything
  STO-CLARE renamed.
- Commit or push only when the user asks.

---

## Building & running

```
cargo build            # debug build
cargo run              # build + launch the GUI
cargo build --release  # release build
```

Settings, the log file and any rule overrides live in the per-user config
directory — `~/.config/STO-CLARE` on Linux, `%APPDATA%\STO-CLARE` on Windows.
**Every name written there is declared in `src/helpers/paths.rs`** and nowhere
else; that module also carries settings over from the pre-2.0 directory on the
first start. A settings file next to the executable is still read as a
last-resort fallback (that is where versions before 1.6 kept it).

---

## Testing

Tests are **inline `#[test]` modules** next to the code they cover (e.g.
`src/analyzer/parser.rs`, `src/analyzer/mod.rs`,
`src/helpers/number_formatting.rs`). There is no separate `tests/` dir yet.

- When modifying or adding logic under `src/`, add or update a corresponding
  `#[cfg(test)]` test in the same module.
- Keep tests focused — one behaviour per test; prefer many small tests.
- Run the suite: `cargo test`.

Prefer `cargo check` / `cargo clippy` for fast feedback while iterating.

---

## Logging

Uses the `log` crate through `simplelog` (`src/app/logging.rs`). Logging is
**opt-in** via the Debug settings (`settings.debug.enable_log`) and, when
enabled, writes to **both** stderr and `STO-CLARE.log` in the config directory.

```rust
log::info!("message");
log::warn!("...");
log::error!("...");
```

Do not print user-facing diagnostics with bare `println!` in library code — use
the `log` macros so they honor the configured level and file mirror. (`main.rs`
intentionally also `println!`s panics as a last-resort console fallback.)

---

## Documentation

Recognize two distinct audiences and keep their docs separate (Rules 5 & 6):

- **Technical docs** — for people who read or change the code (architecture,
  data flow, decisions). Use precise identifiers, ASCII diagrams, tables, small
  code snippets. Written in English. Home them under a `docs/` folder with
  descriptive `UPPER_SNAKE_CASE.md` names (e.g. `docs/OVERLAY.md`,
  `docs/ARCHITECTURE.md`) as they get written. Use the **`techdoc`** skill.
- **User-facing docs** — for regular STO players (README, guides, manual). Plain
  language, no internals. Use the **`userdoc`** skill.
- `README.md` — human-readable overview / manual entry point.
- `CHANGELOG.md` — release notes. Format: a `# unreleased` section at the top,
  then `## vX.Y.Z` sections with `### Major Changes` / `### Other Changes` /
  `### Fixes` subsections.

**Changelog wording:** every bullet must read as a user-visible outcome a
non-programmer STO player understands — no file paths, type/function names,
symbols, or internal terminology (those belong in commit messages). Describe
what changed for the player, not the mechanism. Consolidate related fixes into
one symptom-level bullet.

## Versioning & releases

- Version lives in `Cargo.toml` (`version = ...`), bumped manually alongside the
  CHANGELOG. Release tags are `vX.Y.Z`.
- This is a **fork**: we do not cut the project's official releases — those are
  the upstream maintainer's. Our releases (if any, on our fork) and, more
  importantly, our fixes flow **upstream via pull requests**. Confirm the
  maintainer's actual process before publishing anything user-facing.
