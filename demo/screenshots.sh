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
# The Ladder is a window of its own (a viewport), so it is grabbed by name.
ladder_win() { xdotool search --name "^Ladder$" | tail -1; }
clickw() { xdotool mousemove --window "$1" "$2" "$3" click 1; sleep "${4:-1}"; }
click() { xdotool mousemove --window "$W" "$1" "$2" click 1; sleep 1; }

if [ "$WHAT" = all ] || [ "$WHAT" = tabs ]; then
  echo "main tabs:"
  start LightDark
  shot summary-tab
  click 122 101; shot damage-dealt-tab
  click 27 160;  shot ability-breakdown          # open the player's row
  click 220 101; shot damage-taken-tab
  click 393 101; shot healing-tab
  click 39 101
  click 598 101; shot columns-menu                 # the Columns menu, open
  click 598 101
  click 480 38;  shot combats-list
  stop
fi

if [ "$WHAT" = all ] || [ "$WHAT" = settings ]; then
  echo "settings, compare, records:"
  start LightDark
  click 34 17;   shot settings-general
  click 110 49;  shot settings-analysis
  click 166 49;  shot settings-visuals
  click 221 49;  shot settings-upload
  click 274 49;  shot settings-debug
  click 79 688                                   # Cancel
  click 193 17; sleep 1; shot compare-pick
  # Tick the first two runs, then Compare selected — which sits on a line of its
  # own above the list, under the Selected/Select all row.
  click 15 133; click 15 154; click 65 103; sleep 2; shot compare-result
  click 124 68; shot compare-averages              # Σ Averages, on the same row
  click 124 68
  click 193 17
  click 95 17; sleep 9; shot records              # give the ladder time to load
  stop
fi

if [ "$WHAT" = all ] || [ "$WHAT" = ladder ]; then
  echo "the ladder:"
  start LightDark
  click 95 17; sleep 8                             # open the Ladder window
  L=$(ladder_win)
  shot ladder-window "$L"
  clickw "$L" 694 130 12                           # the magnifier on the first entry
  shot ladder-run                                  # the run, in the main window
  click 431 17                                     # Compare Combats
  click 15 133                                     # one of my own fights
  shot ladder-compare-pick
  click 295 103; sleep 12                          # Compare selected
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
