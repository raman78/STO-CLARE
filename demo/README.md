# demo — tools for pictures and for proving behaviour

Scripts kept because they were written once and would otherwise be rewritten
from scratch every time. Two jobs:

1. **Take the screenshots** the readme and the manual use, reproducibly and
   against data that is safe to publish.
2. **Measure what the program actually does** on screen, for the things a unit
   test cannot reach — whether the overlay is really see-through, whether clicks
   pass through it, whether a button does what it says.

Nothing here is part of the program, and nothing here runs in CI.

| Script                | What it is for                                                                                                                           |
|-----------------------|------------------------------------------------------------------------------------------------------------------------------------------|
| `make-demo-log.py`    | Copies the tail of a real combat log and renames every player in it. Screenshots can then show real numbers without showing real people. |
| `settings.py`         | Builds a throwaway settings folder pointing at that log, with a chosen theme. Never touches your own settings.                           |
| `screenshots.sh`      | Runs the program and grabs the pictures used by the docs: the tabs, the settings sections, compare, records, the theme gallery.          |
| `probe-overlay.sh`    | Measures the overlay: opacity through the compositor, click-through, and the Overlay button.                                             |
| `format-md-tables.py` | Lines up the columns in markdown tables. Cosmetic; the rendered output does not change.                                                  |

## Taking the screenshots again

```sh
cargo build --release
./demo/make-demo-log.py ~/'path/to/combatlog.log' /tmp/games
./demo/screenshots.sh images
```

`screenshots.sh themes` or `screenshots.sh tabs` does one part on its own.

## Why the screenshots are taken this way

The program is driven on **X11 or XWayland** and its window is grabbed by name.
That is deliberate: the window can be photographed without the desktop behind
it, and without the window needing focus — so a screenshot cannot accidentally
publish whatever else is on screen.

The anonymised log matters for the same reason. A real combat log names every
player in the instance, not only you, and those names would end up in the
repository for good.

## Why the probes exist

Some of what this program does cannot be asserted in a unit test, because the
question is "what reached the screen". The overlay is the clear case: its
opacity was being thrown away in three separate places while every value in
memory looked correct. Sampling the pixels through the compositor is what
showed it, and the same probe is what confirms a fix.

`probe-overlay.sh opacity` prints the overlay's colour at two settings. If the
two readings match, the alpha is being discarded on the way to the screen —
which is exactly the bug that existed before 2.1.0.

## Caveats

- The scripts click at fixed coordinates. Change a layout and they will click
  the wrong thing — check the pictures, do not assume.
- Waits are generous rather than clever (the log takes seconds to read).
- The Wayland overlay is a different code path and is not covered here; those
  probes exercise the ordinary-window one, which is what Windows uses.
