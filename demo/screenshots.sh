#!/usr/bin/env bash
# Take the screenshots the readme and the manual use, against the demo log.
#
#   ./demo/screenshots.sh images          # the whole set
#   ./demo/screenshots.sh /tmp/out themes # just the theme gallery
#
# Runs the program on X11 (or XWayland) and grabs its window by name, so the
# desktop is never photographed and nothing has to have focus. On a Wayland
# session the overlay takes a different code path and is not covered here.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/release/sto-clare"
OUT="${1:-$REPO/images}"
WHAT="${2:-all}"
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
  xdotool windowactivate "$W"; sleep 1
}
stop() { kill "$APP" 2>/dev/null || true; wait "$APP" 2>/dev/null || true; }
shot() { sleep 2; import -window "${2:-$W}" "$OUT/$1.png"; echo "  $1"; }
# A strip of the window rather than all of it, for the pictures that would
# otherwise be a second copy of the whole screen with one field circled.
crop() { sleep 2; import -window "$W" -crop "$2" +repage "$OUT/$1.png"; echo "  $1"; }
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
  # row, which shows nothing the manual is talking about — so the third entry
  # is opened instead, a team TFO with five players and a note of its own.
  # Check the list picture if the choice looks wrong.
  click 66 38                                      # ☰ Combats, the side panel
  dblclick 300 245                                 # the third fight in the list
  click 66 38                                      # and close the panel again
  shot summary-tab
  click 122 97; shot damage-dealt-tab
  click 66 164;  shot ability-breakdown            # the arrow, right of the tick
  click 160 130; shot damage-type-picker           # ☰ Type in the Name header
  click 160 130
  # Two abilities out of the player's figures. The smaller of the two goes
  # first: taking out the big one drops the player below the next one and the
  # table re-sorts under the pointer, after which neither coordinate names the
  # row it did. They are not put back either — nothing after this photographs
  # Damage Dealt, and every other tab keeps its own ticks.
  click 25 214; click 25 189; shot damage-row-ticks
  click 223 97; shot damage-taken-tab
  click 400 97; shot healing-tab
  click 39 97
  click 598 97; shot columns-menu                  # the Columns menu, open
  click 598 97
  crop combat-note 1280x32+0+52                    # the name and note, above the tabs
  click 66 38;  shot combats-list
  stop
fi

if [ "$WHAT" = all ] || [ "$WHAT" = settings ]; then
  echo "settings and compare:"
  start LightDark
  click 34 17;   shot settings-general
  click 110 65;  shot settings-analysis
  click 169 65;  shot settings-visuals
  click 226 65;  shot settings-upload
  click 282 65;  shot settings-debug
  click 79 607                                     # Cancel
  click 66 38                                      # the combats panel
  click 169 38                                     # Compare Combats
  # Five runs of one patrol — the set the manual's worked example is about. A
  # click anywhere on a row ticks it, so the map column will do; the comparison
  # follows the ticks and there is nothing to press when they are all in.
  for y in 295 345 370 445 495; do click 300 "$y"; done
  sleep 4; shot compare-pick
  click 66 38; sleep 6; shot compare-result        # the panel out of the way
  click 124 59; shot compare-averages              # Σ Averages, under the toolbar
  click 124 59
  # Two rows out of the Total. Safe to undo by the same coordinates: ticking
  # changes what the Total is of, never the order of the rows under it.
  click 25 207; click 25 232; shot compare-row-ticks
  click 25 207; click 25 232
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
  clickw "$L" 990 130 14                           # the magnifier on the first entry
  xdotool windowactivate "$W"; sleep 1
  click 66 38                                      # the combats panel
  dblclick 300 195 6                               # the run, pinned at the top
  click 825 97                                     # Summary, right of the panel
  shot ladder-run
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
