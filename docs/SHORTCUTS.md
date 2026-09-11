# Keyboard shortcuts

How the program answers a key, where the reader's own combinations are kept,
and how one key is taken from the whole desktop so it works while the game is
in front.

Owned by `src/app/shortcuts/mod.rs` (the table and the settings) and
`src/app/shortcuts/global.rs` (the desktop-wide key). The Settings tab that
edits them is `src/app/settings/shortcuts.rs`.

## Purpose

Five things a reader does over and over get a key: the overlay, the Ladder
window, the Settings window, the custom grouping rules inside it, and a
comparison. The overlay's key is the odd one out — it is pressed *while
playing*, when the program's window is not in front and therefore hears no
keyboard at all — so it is also taken from the desktop itself.

Escape and Tab are deliberately outside this subsystem. They are structural,
not preferences: Escape belongs to whichever dialog is open
(`custom_widgets::dialog::escape_closes`) and Tab is stripped from the frame
before egui can walk the focus with it (`App::raw_input_hook`). Both are
described in `docs/ARCHITECTURE.md`, *The keyboard*.

## The shipped table

| Action           | Default | What it does                                       |
|------------------|---------|----------------------------------------------------|
| `ToggleOverlay`  | `Alt+O` | shows or hides the overlay; the only one that can be taken desktop-wide |
| `ToggleLadder`   | `Alt+L` | opens or closes the Ladder window                  |
| `OpenSettings`   | `Alt+S` | opens Settings on the tab it was last left on      |
| `OpenGrouping`   | `Alt+G` | opens Settings on the Custom Grouping rules        |
| `ToggleCompare`  | `Alt+C` | opens or leaves a comparison of the ticked fights  |

`ShortcutAction::ALL` is the list; `default_combination` holds the keys above.
Each arm of `App::act_on` makes the same call as the button it stands for
(`Overlay::toggle`, `Records::toggle_from_keyboard`, `SettingsWindow::open`,
`SettingsWindow::open_grouping`, `App::toggle_compare`), so a key and a click
cannot drift apart.

Two of them are **not** toggles, matching the buttons they mirror: the Settings
window is left by Ok or Cancel, so pressing `Alt+S` again while it is open does
nothing, and `Alt+G` only moves to the rules. A key that discarded a
half-written rule would be a trap.

`OpenGrouping` sets **both** the tab and the section: the Analysis tab holds
four rule sets of its own and opens on Combat Names, so stopping at the tab
lands the reader on a different rule set from the one the key names
(`SettingsWindow::open_grouping`, `AnalysisTab::show_custom_grouping`).

## Where a press comes from

```
  a key pressed while STO-CLARE is in front
        │  egui event stream
        ▼
  Shortcuts::triggered ──► consume() takes the event out of the frame
        ▲                        │
        │                        ├──► ShortcutAction
  GlobalHotkey::presses          │
        ▲  crossbeam channel     ▼
        │                   App::act_on
  cla-global-hotkey thread (X11 grab)
        ▲
        │  a key pressed anywhere on the desktop, game included
```

`App::ui` asks once per frame, **after the window has been drawn**. Anything on
screen that wanted the key has taken it by then — a dialog, or the row in
Settings → Shortcuts that is recording a new combination — so a key being bound
never also runs what it is bound to.

## Invariants

- **S1 — a key is taken, not read.** `consume` removes the matching
  `Event::Key` from the frame, the same discipline Tab and Escape follow, so no
  second reader answers the same press.
- **S2 — a shortcut means the keys it names and no others.** Matching is
  `Modifiers::matches_exact`, not egui's `consume_key`, which matches logically
  and would let `Alt+Shift+L` answer a shortcut bound to `Alt+L`. Held by
  `extra_modifiers_are_a_different_shortcut`.
- **S3 — a repeat is the same press.** egui recomputes the `repeat` flag from
  the keys it saw held on the previous pass (`InputState::begin_pass`), and a
  repeat is dropped but still consumed. Without this a held key toggles the
  overlay thirty times a second. Held by
  `a_repeat_of_a_held_key_does_not_run_it_again`; the grab has its own answer to
  the same problem (see [Auto-repeat](#auto-repeat)).
- **S4 — the in-window half of the desktop-wide action is skipped while the
  grab holds the key.** A passive grab takes the key from whoever has focus,
  this program included, so the window never sees it; asking anyway would toggle
  twice on the day a compositor delivers it to both.
- **S5 — only one action can be taken desktop-wide.** Every key held against
  the desktop is a key no other program can use, the game included.
  `ShortcutAction::can_be_system_wide` answers for `ToggleOverlay` alone.
- **S6 — a desktop-wide press wakes the window.** egui draws when something
  happens to it, and a key pressed while the game is in front is exactly the
  case where nothing has: without `Context::request_repaint` from the hotkey
  thread the press would sit in the channel until some other event woke the
  program. `GlobalHotkey` holds the main window's context for this, the way
  `AnalysisHandler` does.

## What is stored

`ShortcutSettings` is its own section of the settings file (`shortcuts`), for
the reason every other section has one: `SettingsWindow::apply_setting_changes`
compares sections to decide what a change costs, and rebinding a key is no
reason to re-read the combat log.

```jsonc
// STO-CLARE_Settings.json
"shortcuts": {
  "custom": { "ToggleLadder": "Ctrl+F9" },
  "system_wide": true
}
```

| Field         | Type                    | Meaning                                     |
|---------------|-------------------------|---------------------------------------------|
| `custom`      | map of text to text     | only the actions whose combination the reader changed |
| `system_wide` | bool, default `true`    | whether `ToggleOverlay` is also taken from the desktop |

That example is pinned by `the_written_shape_is_what_the_document_says`, which
fails if either field is renamed — an unknown key is ignored on reading, so a
rename would otherwise read as "the reader never set that" in every file
already written.

Three properties of that shape, each of which exists to stop a silent loss:

- **Only changes are written.** An action left alone is absent from the file, so
  a shortcut added in a later version arrives with its own default rather than
  missing from a file written before it existed — the rule `ColumnVisibility`
  follows for hidden columns. `ShortcutSettings::set` removes the entry again
  when the combination equals the default.
- **Both sides are text.** A combination that cannot be parsed costs that one
  shortcut — the default stands, and the tab says so under the row — instead of
  failing the settings file, which `Settings::read_at` would treat as damaged
  and set aside, putting every other setting back to its default. See
  `docs/ARCHITECTURE.md`, *A config file that cannot be read*.
- **An unknown action is kept.** A name this build does not have is a shortcut
  from a later version; it is written back untouched and the tab counts it.
  Running an older build for an evening is then not a way to lose it.

Action names (`ShortcutAction::key`) may be **added but never changed** — a
changed name reads as "the reader never set that". The same rule governs
`Theme`. Held by `every_action_is_named_the_same_way_in_both_directions`.

### What a combination may be

`Combination::is_supported` refuses anything but a modifier plus a letter, a
digit or a function key. Those are the keys whose identity does not move
between layouts, which is what the desktop-wide grab has to name them by; a
bare key is refused because that is what a reader types into the note field and
the rule patterns. The text form is fixed at `Ctrl+Alt+Shift+KEY` order so one
combination is one string — two spellings would read as a change nobody made.

## The Settings tab

Settings → **Shortcuts**. One row per action: the current combination is a
button, pressing it records the next key pressed, and Escape gives up the
recording (which is why the tab asks for the key before the Ok/Cancel row does).
A `Reset` button appears only where the reader changed something.

Recording, rather than a text field, is the whole input method: a field would
accept `Alt+Nonsense` and the reader would learn it was nonsense by the
shortcut never working.

Three things the tab refuses, each naming what is wrong rather than ignoring
the press:

| Refusal                                      | Reason                                     |
|----------------------------------------------|--------------------------------------------|
| a key with no modifier, or an unsupported key | see [What a combination may be](#what-a-combination-may-be) |
| a combination another action already answers  | two on one key means only the first ever runs; the message names the other action |
| nothing — the press is recorded               | —                                          |

The system-wide checkbox is at the foot of the tab, under the state line
described next. A change there takes effect on **Ok**, like every other setting.

## The desktop-wide key

### Why an X11 grab

Measured on KWin/Wayland rather than argued, because the two candidates behave
differently in exactly the way that matters:

| Route | Result |
|---|---|
| Passive X11 grab (`XGrabKey`) | Works. With Star Trek Online in front the key reached the grab; it also reached it with a native Wayland window in front, so it is global in practice and not merely global among X clients. Needs an X server — under Wayland that is XWayland, which a Proton game brings with it. |
| `org.freedesktop.portal.GlobalShortcuts` | Works, and costs more. Requires a D-Bus client in the tree; refuses a process that has not claimed an app id through `org.freedesktop.host.portal.Registry` (`CreateSession` answers `NotAllowed: An app id is required`); and **the program cannot change its own key once bound** — re-binding the same shortcut id with a different trigger is accepted and ignored, leaving the reader to the desktop's own settings. |

The last row is what settles it: the shortcut is meant to be editable in this
program's Settings window. The portal is where to go if a session ever turns up
with no XWayland in it. On Windows the question does not arise —
`RegisterHotKey` is the platform's answer and needs nothing else.

| Platform | Mechanism | Where |
|---|---|---|
| Linux | passive `XGrabKey` on the root window, on its own thread | `global::backend::take`, `grab`, `run` |
| Windows | `RegisterHotKey` with `MOD_NOREPEAT` plus a message loop on its own thread | `global::backend::take` |
| anything else | refused, with a reason the tab states | `global::backend::take` |

### What the reader sees

`GlobalState` is what the tab reports, and it is the difference between what was
*asked for* and what is actually held:

| State | Line in the tab | Means |
|---|---|---|
| `Off` | "Not taken — the shortcut only works while this window is in front." | the box is unticked |
| `Held` | "`Alt+O` is taken from the whole desktop…" | the grab is up |
| `Refused` | "Could not be taken: …" | another program holds that key, or it is not on the layout |
| `Unsupported` | "Not available here: …" | no X server to take it from, or no backend on this platform |

The last two are drawn in the theme's warning colour. A key the reader asked
for and did not get must not look like one that works.

### Auto-repeat

A held key repeats, and a toggle answering thirty times a second is a flicker
rather than a command. The two backends answer it differently because the
platforms do:

- **Windows**: `MOD_NOREPEAT` is part of the registration; the hotkey fires once
  until the key is let go.
- **Linux**: the thread asks XKB for detectable auto-repeat
  (`PerClientFlag::DETECTABLE_AUTO_REPEAT`), which stops the server faking a
  release between repeats, so "the key is still down" is the whole test. Where
  XKB is not there, a repeat arrives as a release and a press in the same
  instant and the time between presses stands in (`REPEAT_GUARD`, 250 ms).

### Lock keys

Caps Lock and Num Lock are modifiers as far as a grab is concerned, so the
Linux backend takes every combination of the two (`LOCK_MASKS`). A key taken
without them is a key that stops working the moment either is on. A grab that
only half succeeds is given back rather than left: the shortcut would otherwise
answer with Caps Lock on and not otherwise.

### When the grab dies

The thread ends by itself if the X connection goes — the server restarting under
a Wayland session takes XWayland with it. Nothing else would ever look, so the
key would stop working while the Settings window went on saying it was taken.

`GlobalHotkey::presses` notices the closed channel, takes the key again, and
counts the attempts: past `MAX_RETAKES` (3) it stops and says so, the same shape
as the overlay's `layer_restart`. Closing Settings with Ok is the clean retry.

### Failure modes

| Symptom | Cause | Where to look |
|---|---|---|
| the key does nothing while the game is in front, and the tab says `Held` | the game is not an X client, or a compositor keeps the key | `run`'s event loop; the log line `global shortcut: … taken from the desktop` |
| the tab says "already held by another program" | another program's grab or global shortcut has that key | `refusal`; on KDE, System Settings → Shortcuts |
| the tab says "there is no X server to take a key from" | no `DISPLAY`, or no XWayland in this session | `grab`'s connect step |
| the shortcut works in the window but not outside it | the box is unticked, or the grab was refused | the state line at the foot of the tab |
| `global shortcut: … was lost, taking it again` in the log | the X connection dropped | [When the grab dies](#when-the-grab-dies) |

## Testing

`cargo test` covers the table, the settings shape and the tab: what a key does,
that it is consumed, that extra modifiers are a different shortcut, that a
repeat does not run twice, that only changes are written, that an unreadable or
unknown entry costs nothing else.

The Linux grab has one `#[ignore]`d test, `a_grab_catches_the_key`, that takes a
real key, presses it with `xdotool`, and asserts the press arrives and the key
is given back. It changes whatever desktop it runs on, so it is meant for a
throwaway one:

```
Xvfb :99 &
DISPLAY=:99 cargo test a_grab_catches_the_key -- --ignored
```

The tests build `Shortcuts` with `system_wide: false` on purpose
(`window_only`): taking a key is a change to the desktop the suite is running
on.

The Windows backend cannot be built on a Linux machine here — a C dependency of
the uploader will not cross-compile — so it is type-checked against the
`x86_64-pc-windows-msvc` target from a throwaway crate that `#[path]`-includes
these two files rather than copying them.

## Related documents

| Document | Scope |
|---|---|
| `docs/ARCHITECTURE.md` | *The keyboard* — Escape and Tab, and where this subsystem sits |
| `docs/OVERLAY.md` | the overlay the desktop-wide key shows and hides |
