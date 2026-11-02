# Keyboard shortcuts

How the program answers a key, where the reader's own combinations are kept,
what a row left on no key means, and how one key is taken from the whole
desktop so it works while the game is in front.

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
**Its order is load-bearing** in two places: it is the order the settings tab
lists the rows in, and it decides which of two actions on one combination
answers the key — see [Two rows on one key](#two-rows-on-one-key). Reordering it
therefore changes behaviour, not just a table.

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
- **S7 — the grab checks the modifiers too, not just the key.** A press that
  reaches the grabbing client is not proof that the chord was pressed: an active
  grab reports what follows it, and a compositor forwarding keys into XWayland
  can land a bare press there. `chord_held` compares the event's modifier state
  against the shortcut's, with the lock bits taken out of both sides (the grab
  is held with them, so they say nothing). Without it the action answers the
  letter alone, which from the outside is indistinguishable from a modifier the
  program thinks is still held down. Held by
  `a_press_without_the_modifiers_is_not_the_shortcut`.
- **S8 — a cleared action is not in the table at all.** `Shortcuts::rebuild`
  drops it rather than keeping a row that matches nothing, so the combination it
  used to hold is left in the frame for whoever else wants it. "No key" costs
  one entry in a `Vec`, not a special case in `consume`, and the desktop-wide
  grab is not taken for a row with nothing in it. Held by
  `a_cleared_shortcut_answers_nothing_and_keeps_its_hands_off_the_frame` and
  `the_desktop_takes_nothing_when_the_overlay_row_is_empty`.

## What is stored

`ShortcutSettings` is its own section of the settings file (`shortcuts`), for
the reason every other section has one: `SettingsWindow::apply_setting_changes`
compares sections to decide what a change costs, and rebinding a key is no
reason to re-read the combat log.

```jsonc
// STO-CLARE_Settings.json
"shortcuts": {
  "custom": { "ToggleLadder": "Ctrl+F9", "ToggleCompare": "Off" },
  "system_wide": true
}
```

| Field         | Type                    | Meaning                                     |
|---------------|-------------------------|---------------------------------------------|
| `custom`      | map of text to text     | only the actions whose combination the reader changed |
| `system_wide` | bool, default `true`    | whether `ToggleOverlay` is also taken from the desktop |

An action's value is one of two things: a combination, or the word `Off`, which
is the action the reader **cleared** — see [A shortcut on no
key](#a-shortcut-on-no-key).

Both spellings are pinned by tests — `the_written_shape_is_what_the_document_says`
and `a_cleared_shortcut_is_written_the_way_the_document_says` — which fail if a
field or the `Off` spelling is renamed. An entry that cannot be read is ignored,
so a rename would otherwise read as "the reader never set that" in every file
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
  from a later version. It is not registered — there is no function here to put
  it on — but it is written back untouched, so running an older build for an
  evening is not a way to lose it. The tab says that the file holds such
  entries, without naming them: the name is an identifier from the code, and the
  only question the reader has is whether the key is gone.
  `ShortcutSettings::reset_all` is the one thing that removes them, because that
  is the reader asking to be rid of what they set.

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

### A shortcut on no key

An action can be left on nothing at all: `ShortcutSettings::clear` writes
`CLEARED` — the word `Off` — and `combination` answers `Ok(None)` for it.
`Combination` is therefore always a real combination, and "no key" is the
`Option` around it, so nothing downstream can mistake one for the other.

A third spelling is needed because the other two are already spoken for:

| What the file holds | What it means |
|---|---|
| no entry for the action | the shipped combination |
| `"Ctrl+F9"` | that combination |
| `"Off"` | the reader cleared it; the action answers no key |

Absence cannot stand in for "cleared" — it already means "never changed", so a
cleared row would come back on its default at the next start. The empty string
cannot either: unreadable text already means "the file is damaged here, the
default is in force", which would bring the row back *with a warning beside it*.
`Off` is not the name of any key `egui::Key::from_name` knows, so it cannot
collide with a combination; `a_cleared_shortcut_is_written_the_way_the_document_says`
holds both halves of that.

Two consequences worth stating, because both are places a cleared row could
quietly behave like a bound one:

- **A cleared action is never on a key.** `on_the_same_key` compares against
  `Some(combination)`, so two cleared actions are not counted as sharing one and
  the combination a cleared action used to hold is nobody's.
- **Clearing is a change, so `is_custom` is true for it** — which is what makes
  `Reset` appear on a cleared row and gives the reader the way back.

An older build reading `Off` treats it as an unreadable entry: the action comes
back on its shipped combination, with the warning the tab shows for any value it
cannot parse. Nothing is lost, but the row is no longer clear.

## The Settings tab

Settings → **Shortcuts**, drawn by `ShortcutsTab::show`. A button above the
table, then one row per action, then whatever went wrong:

```
  [ Reset all shortcuts ]

  [x] Global   Overlay           [ Alt+O             ]  !          [Clear]
               Ladder            [ Alt+L             ]  !  [Reset] [Clear]
               Settings          [ Ctrl+Alt+Shift+F5 ]     [Reset] [Clear]
               Custom grouping   [ Alt+G             ]             [Clear]
               Compare combats   [                   ]     [Reset]

  ! Overlay: Could not be taken: already held by another program.
  ! Ladder: the settings file says ...; Alt+L is in force.
```

Six columns, each its own, so `Reset` and `Clear` stand in a line down the table
however few rows have them. A change takes effect on **Ok**, like every other
setting.

The tab carries no explanatory sentence of its own. What a global shortcut is
belongs in the manual; the word *Global* is on the tick, and the combination it
applies to is in the same row.

### The row

| Column | What it is |
|---|---|
| tick | `Global` — only the Overlay row; see [The desktop-wide key](#the-desktop-wide-key) |
| name | `ShortcutAction::label`, with `hint` under the pointer |
| combination | a button of fixed width; pressing it records the next key |
| sign | `⚠` when this row is the subject of a line below, with the same text under the pointer |
| `Reset` | only where `is_custom` — back to the shipped combination |
| `Clear` | only where there is a combination — leaves the action on no key |

Recording, rather than a text field, is the whole input method: a field would
accept `Alt+Nonsense` and the reader would learn it was nonsense by the
shortcut never working. Escape gives up the recording, which is why the tab asks
for the key before the Ok/Cancel row does.

`Clear` is the only way out of a shortcut that gets in the way of something
else, which otherwise could only ever be moved to another key. A cleared row is
an **empty button**, not a word standing in for one: the field is the thing that
is empty, and it is still what the reader presses to put a key back into it.

Both fixed widths exist for the same reason — a control must not move out from
under the pointer:

| Width | From | Why |
|---|---|---|
| the combination button | `combination_width`: the widest combination the table currently holds **and the `LISTENING` label**, floored at `NARROWEST_COMBINATION` | a column sized by the longest row moves every other row's buttons when one is cleared or rebound; the floor is what keeps an all-cleared table clickable |
| the sign column | `WARNING_SIGN_WIDTH` | allocated whether the row has a sign or not, so a warning appearing does not push the buttons sideways |

`LISTENING` — what a row that is recording says — has to be counted in that
width, and is the one that is easy to forget. `Ui::add_sized` sets the space a
widget is *given*, not a limit it is held to: a label longer than the column was
measured for widens the column, so the row's `Reset` and `Clear` slide sideways
the moment a row starts listening. That is the instant the reader's hand is
already moving towards one of them.

One thing the tab refuses, naming what is wrong rather than ignoring the press:
a key with no modifier, or one the grab cannot name — see [What a combination
may be](#what-a-combination-may-be). Everything else is recorded.

### Two rows on one key

A combination another row already holds is **taken, not turned down**, and both
rows then show it with a warning sign.

Only one of them can ever run: `Shortcuts::triggered` walks the table in
`ShortcutAction::ALL` order and `consume` removes the event from the frame, so
the first holder answers and the rest find nothing. That was true before this
was allowed, which is what made the old refusal wrong rather than merely strict
— it told the reader about a state the table could not then show them, and left
the row it was refused in looking as though nothing had been pressed.

The warning is said **once**, in the row that answers, and named the way every
other row's warning is:

```
! Alt+O is on Overlay and Ladder. Only Overlay answers it: a key runs the
  first thing that holds it and the rest never see the press.
```

Both rows raise the sign for it and both repeat that sentence under the pointer.
The point of the warning is the *pair*, so a line per row would state one fact
twice; but a row marked with no way to find out why would be worse, hence the
tooltip in the row that has no line of its own.

`ShortcutSettings::on_the_same_key` is the list, in the order they are tried,
and `only_the_first_of_two_actions_on_one_key_runs` holds the runtime to the
order the sentence claims.

### Where a warning goes

In the row: a sign, saying *this row is the one the sentence below is about*.
Under the table: the sentence. The sign carries the same text under the pointer,
which saves looking down the page when two rows are marked.

| Warning | Raised by | Line under the table | Sign in the row |
|---|---|---|---|
| the file holds a value this row cannot use | `ShortcutSettings::combination` returning `Err` | yes | yes |
| the desktop-wide key was asked for and not given | `GlobalState::is_problem` | yes | yes, in the Overlay row |
| two rows on one key | `ShortcutSettings::on_the_same_key` | once, in the row that answers | in **both** rows |
| the combination just pressed was turned down | `ShortcutsTab::record` | yes | no |
| the file holds shortcuts for a newer version | `holds_shortcuts_for_a_newer_version` | yes | no |

The last two have no sign on purpose. A refusal belongs to the key the reader
pressed a second ago rather than to a row, and turning it into a sign would read
as "nothing happened" with the reason hidden behind a hover. An entry for an
action this build does not have has no row to put a sign in.

A row's warnings are counted from where that row started rather than taken off
the end of the list — the sign would otherwise be raised in a row with nothing
to say, for whatever the row above it said.

### Reset all shortcuts

`ShortcutsTab::confirm_reset_all` asks before `ShortcutSettings::reset_all` puts
every row back to the shipped table. A window rather than a second click on the
same button, and the button that does it names the thing it does — the shape
`CombatsList::show_delete_confirmation` already uses for deleting fights.

It sits inside the tab rather than beside Ok and Cancel, which are shared by
every tab: a `Load Default` down there would mean the log path, the grouping
rules, the notes and the columns as well, which is not what a reader clicking it
from this page would expect.

This is also the one place the entries written by a newer version go (see
[What is stored](#what-is-stored)); everywhere else they are left exactly as
they are. Opening the question stops any row that was recording, so the Escape
that backs out of the question is not eaten by the row first.

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

`GlobalState` carries what actually became of the key, which is not the same as
what the tick box was set to:

| State | Shown in the tab | Means |
|---|---|---|
| `Off` | nothing — the box is unticked, or the Overlay row is empty and the box is greyed | not asked for |
| `Held` | nothing — the box is ticked | the grab is up |
| `Refused` | `⚠` in the Overlay row, "Could not be taken: …" below | another program holds that key, or it is not on the layout |
| `Unsupported` | `⚠` in the Overlay row, "Not available here: …" below | no X server to take it from, or no backend on this platform |

**Only the refusals are drawn**, in the theme's warning colour. The other two
are already on screen as the state of the box, and a line restating it is one
more thing to read on a page whose point is a table of keys; a key that was
asked for and *not* given is the one case the box itself gets wrong. Every
state still reaches the log, where the sequence matters.

**A cleared Overlay row takes nothing and greys the tick.** There is no key to
ask for, so `rebuild` binds `None` and the state is `Off`. The tick keeps the
value the reader gave it rather than being turned off with the row: it is the
standing answer to "and globally?", and clearing it behind their back would hand
them a window-only shortcut on the day they set a combination again. Greyed
rather than hidden, with `Set a combination first: there is no key here to take.`
under the pointer — a control that vanishes tells the reader nothing about why.

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
| the shortcut works in the window but not outside it | the Global tick is off, or the grab was refused | the tick in the Overlay row, and the warning line under the table |
| a key runs the wrong one of two things | two rows hold it; the first in the table answers | the signs in both rows, and the line naming the pair |
| a key does nothing at all, and its row is empty | the row was cleared | press the empty button and record one, or `Reset` |
| the Global tick cannot be clicked | the Overlay row is empty, so there is nothing to take | the combination button in that row |
| `global shortcut: … was lost, taking it again` in the log | the X connection dropped | [When the grab dies](#when-the-grab-dies) |

## Testing

`cargo test` covers the table, the settings shape and the tab: what a key does,
that it is consumed, that extra modifiers are a different shortcut, that a
repeat does not run twice, that only changes are written, that an unreadable or
unknown entry costs nothing else.

A cleared shortcut has four of its own, one per place it could quietly behave
like a bound one: that it answers nothing and leaves the press in the frame,
that it survives the settings file instead of coming back on its default, that
the desktop is asked for nothing, and that the key it used to hold is free for
another row (`a_cleared_action_is_not_in_the_way_of_its_old_key`, in the tab's
own tests). Each was confirmed to fail with the `CLEARED` branch of
`ShortcutSettings::combination` removed.

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

**The Windows backend has never been run.** It cannot be built on a Linux
machine here — a C dependency of the uploader will not cross-compile — so it is
*type-checked* against the `x86_64-pc-windows-msvc` target from a throwaway
crate that `#[path]`-includes these two files rather than copying them. That
catches a wrong API name, a missing feature flag or a changed signature, and
nothing else: the first time `RegisterHotKey` actually runs is on a machine
with Windows on it.

What that leaves unverified, and what to look at first when it is run:

| Question | Where the answer shows |
|---|---|
| does `WM_HOTKEY` reach the thread's own queue with a null window? | the shortcut works at all; otherwise the log's `taken from the desktop` line appears and nothing ever fires |
| does the combination survive a full-screen game? | press it with the game in front |
| is it refused when another program holds it? | the Settings tab says "Could not be taken" rather than nothing |

The in-window half needs none of this: it is egui's event stream on every
platform, with no platform code between the key and the action.

## Related documents

| Document | Scope |
|---|---|
| `docs/ARCHITECTURE.md` | *The keyboard* — Escape and Tab, and where this subsystem sits |
| `docs/OVERLAY.md` | the overlay the desktop-wide key shows and hides |
