#!/usr/bin/env bash
# Take the screenshots the readme and the manual use, against the demo log.
#
#   ./demo/screenshots.sh images          # the whole set
#   ./demo/screenshots.sh /tmp/out themes # just the theme gallery
#   ./demo/screenshots.sh images settings settings-debug,settings-shortcuts
#                                         # that section, but only those files
#
# Runs the program on X11 (or XWayland) and grabs its window by name, so the
# desktop is never photographed and nothing has to have focus. On a Wayland
# session the overlay takes a different code path and is not covered here.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/release/sto-clare"
OUT="${1:-$REPO/images}"
WHAT="${2:-all}"
# Comma-separated picture names to write; empty means all of them.
ONLY="${3:-}"
CFG=/tmp/clare-demo
LOG="${DEMO_LOG:-/tmp/games/Star Trek Online/Live/logs/GameClient/combatlog.log}"

command -v xdotool >/dev/null || { echo "needs xdotool"; exit 1; }
command -v import   >/dev/null || { echo "needs ImageMagick"; exit 1; }
[ -x "$BIN" ] || { echo "build it first: cargo build --release"; exit 1; }
[ -f "$LOG" ] || { echo "no demo log at $LOG — see demo/make-demo-log.py"; exit 1; }
mkdir -p "$OUT"

start() {  # start(theme) -> sets $W to the window id
  "$REPO/demo/settings.py" "$CFG" "$1" "$LOG" >/dev/null
  env -u WAYLAND_DISPLAY DISPLAY="${DISPLAY:-:0}" XDG_SESSION_TYPE=x11 XDG_CONFIG_HOME="$CFG" \
    "$BIN" >/dev/null 2>&1 &
  APP=$!
  sleep 20                                   # the log is large; let it be read
  W=$(xdotool search --name "STO-CLARE" | head -1)
  # Raising the window needs a window manager to ask. There is none on a bare
  # X server — which is exactly where this is worth running, since a throwaway
  # display costs nobody their desktop for the length of a run — and the clicks
  # below do not need it: they are aimed at the window, and it is the only one
  # mapped there.
  xdotool windowactivate "$W" 2>/dev/null || true
  sleep 1
}
stop() { kill "$APP" 2>/dev/null || true; wait "$APP" 2>/dev/null || true; }

# Whether this picture is one of the ones asked for. With no list given, every
# picture in the section is taken, as before.
#
# The *clicks* between shots always run whatever the list says: they are how the
# program is walked to the state the next picture is of, so skipping them would
# photograph the wrong screen. What the list saves is the file — and therefore
# the review: `import` rewrites a PNG even when nothing in it changed, so a full
# section leaves a dozen files to look at and revert to find the two that
# actually moved.
wanted() { [ -z "$ONLY" ] || [[ ",$ONLY," == *",$1,"* ]]; }

shot() {
  sleep 2
  wanted "$1" || { echo "  $1 (skipped)"; return 0; }
  import -window "${2:-$W}" "$OUT/$1.png"
  echo "  $1"
}
# A strip of the window rather than all of it, for the pictures that would
# otherwise be a second copy of the whole screen with one field circled.
crop() {
  sleep 2
  wanted "$1" || { echo "  $1 (skipped)"; return 0; }
  import -window "$W" -crop "$2" +repage "$OUT/$1.png"
  echo "  $1"
}
# The Ladder is a window of its own (a viewport), so it is grabbed by name.
ladder_win() { xdotool search --name "^Ladder$" | tail -1; }
clickw() { xdotool mousemove --window "$1" "$2" "$3" click 1; sleep "${4:-1}"; }
click() { xdotool mousemove --window "$W" "$1" "$2" click 1; sleep 1; }
# Opening a fight from the list takes two clicks, as it does for the reader.
dblclick() {
  xdotool mousemove --window "$W" "$1" "$2"
  xdotool click --repeat 2 --delay 120 1
  sleep "${3:-3}"
}

if [ "$WHAT" = all ] || [ "$WHAT" = tabs ]; then
  echo "main tabs:"
  start LightDark
  # Every picture in this section is of one run. The newest fight in the demo
  # log is whatever the log ends on — often a short solo scrap with a single
  # row, which shows nothing the manual is talking about — so a team fight is
  # opened instead, one with several players and a note of its own. Which row
  # that is depends on the log the demo was cut from: `combats-list.png` is the
  # picture to check, and the Size column says Team. Right now it is the first.
  click 66 38                                      # ☰ Combats, the side panel
  dblclick 300 195                                 # the one team fight in the list
  click 66 38                                      # and close the panel again
  shot summary-tab
  click 122 119; shot damage-dealt-tab
  click 67 186;  shot ability-breakdown            # the arrow, right of the tick
  click 160 151; shot damage-type-picker           # ☰ Type in the Name header
  click 160 151
  # Two abilities out of the player's figures. The smaller of the two goes
  # first: taking out the big one drops the player below the next one and the
  # table re-sorts under the pointer, after which neither coordinate names the
  # row it did. They are not put back either — nothing after this photographs
  # Damage Dealt, and every other tab keeps its own ticks.
  click 24 236; click 24 211; shot damage-row-ticks
  click 223 119; shot damage-taken-tab
  click 400 119; shot healing-tab
  click 39 119
  click 598 119; shot columns-menu                  # the Columns menu, open
  click 598 119
  crop combat-note 1280x46+0+56                    # the name and note, above the tabs
  click 66 38;  shot combats-list
  stop
fi

if [ "$WHAT" = all ] || [ "$WHAT" = settings ]; then
  echo "settings and compare:"
  start LightDark
  click 34 17;   shot settings-general
  # The Settings window opens centred in the main window (2.9.0), which is
  # where these coordinates come from — measured off a grab of that window, so
  # they are its own pixels. Anything that moves the window moves all of them
  # together.
  click 348 105; shot settings-analysis
  click 406 105; shot settings-visuals
  click 463 105; shot settings-upload
  click 527 105; shot settings-shortcuts
  click 590 105; shot settings-debug
  click 316 647                                    # Cancel
  click 66 38                                      # the combats panel
  click 169 38                                     # Compare Combats
  # Five runs of the same map at the same level — the set the manual's worked
  # example is about, and the only kind where a spread filter says anything:
  # rows every run used in the same measure are what it drops. A click anywhere
  # on a row ticks it, so the map column will do; the comparison follows the
  # ticks and there is nothing to press when they are all in. Check the rows
  # against `compare-pick.png` if the demo log is recut — these are the Elite
  # Infected Conduit runs in it.
  for y in 220 245 270 320 395; do click 300 "$y"; done
  sleep 4; shot compare-pick
  click 66 38; sleep 6; shot compare-result        # the panel out of the way
  click 124 59; shot compare-averages              # Σ Averages, under the toolbar
  click 124 59
  # Two rows out of the Total. Safe to undo by the same coordinates: ticking
  # changes what the Total is of, never the order of the rows under it.
  # The tick box is only about 16 px tall inside a 25 px row, so these have to
  # be the row centres — 207/232 fell in the gaps between boxes, which selects
  # the row instead and leaves the Total untouched.
  click 24 192; click 24 217; shot compare-row-ticks
  click 24 192; click 24 217
  click 197 59; sleep 2; shot compare-differences  # Δ Spread
  click 197 59
  stop
fi

if [ "$WHAT" = all ] || [ "$WHAT" = ladder ]; then
  echo "the ladder:"
  start LightDark
  click 95 17; sleep 12                            # open the Ladder window
  L=$(ladder_win)
  shot ladder-window "$L"
  # One press does the lot now: it fetches the run, brings the combats list out
  # to receive it and opens it in the main window. So there is no panel to open
  # and no row to double-click here any more — both were in this script until
  # the button started doing what its label always said.
  clickw "$L" 1002 130 14                          # the magnifier on the first entry
  xdotool windowactivate "$W"; sleep 6
  # The panel is folded away before the run is photographed. It used to be
  # shot beside the list — Summary was clicked at 825,97, "right of the panel" —
  # but the Note column now reserves room for a whole note, so the panel is
  # wide enough that nothing useful is left of the window beside it. The list
  # is what `ladder-compare-pick` shows; this one shows the run.
  click 66 38                                      # the panel out of the way
  click 39 119                                     # Summary
  shot ladder-run
  click 66 38                                      # the panel back, for the ticks
  click 169 38                                     # Compare Combats
  click 300 195                                    # tick the run
  click 300 220; sleep 5                           # and one of my own
  shot ladder-compare-pick
  click 66 38; sleep 8                             # the panel out of the way
  shot ladder-compare
  stop
fi

if [ "$WHAT" = all ] || [ "$WHAT" = themes ]; then
  echo "theme gallery:"
  for theme in Dark LightDark Light Nebula FrostLight; do
    start "$theme"
    case $theme in
      LightDark)  name=theme-light-dark;;
      FrostLight) name=theme-frost-light;;
      *)          name=theme-$(echo "$theme" | tr '[:upper:]' '[:lower:]');;
    esac
    shot "$name"
    stop
  done
fi

echo "done -> $OUT"
