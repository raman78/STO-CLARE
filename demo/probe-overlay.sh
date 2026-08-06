#!/usr/bin/env bash
# Measure what the overlay actually does, rather than trusting that it does it.
#
#   ./demo/probe-overlay.sh opacity       # is it really see-through?
#   ./demo/probe-overlay.sh clickthrough  # do clicks reach past it?
#   ./demo/probe-overlay.sh toggle        # does the Overlay button switch it off?
#
# The opacity probe samples the overlay through the compositor, because a
# screenshot of the window alone shows what was painted, not what the screen
# ends up displaying. Needs a screenshot tool that can grab the whole screen
# (spectacle here); everything else works on X11/XWayland.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/release/sto-clare"
CFG=/tmp/clare-demo
LOG="${DEMO_LOG:-/tmp/games/Star Trek Online/Live/logs/GameClient/combatlog.log}"
WHAT="${1:-opacity}"

start() {  # start(opacity, overlay_shown)
  "$REPO/demo/settings.py" "$CFG" LightDark "$LOG" >/dev/null
  python3 - "$CFG" "$1" "$2" <<'PY'
import json, pathlib, sys
p = pathlib.Path(sys.argv[1]) / "STO-CLARE/STO-CLARE_Settings.json"
d = json.loads(p.read_text())
d["visuals"]["overlay_opacity"] = float(sys.argv[2])
d["general"]["overlay_shown"] = sys.argv[3] == "1"
p.write_text(json.dumps(d, indent=2))
PY
  env -u WAYLAND_DISPLAY DISPLAY="${DISPLAY:-:0}" XDG_SESSION_TYPE=x11 XDG_CONFIG_HOME="$CFG" \
    "$BIN" >/dev/null 2>&1 &
  APP=$!
  sleep 20
  W=$(xdotool search --name "STO-CLARE" | head -1)
  xdotool windowactivate "$W" 2>/dev/null || true; sleep 1
}
stop() { kill "$APP" 2>/dev/null || true; wait "$APP" 2>/dev/null || true; }

case "$WHAT" in
opacity)
  # Same patch of overlay at two settings. If the numbers match, the alpha is
  # being discarded somewhere between here and the screen.
  for value in 1.0 0.3; do
    start "$value" 1
    xdotool mousemove --window "$W" 1045 38 click 1; sleep 8   # Refresh Now, so it has rows
    O=$(xdotool search --name "CLA Overlay" | head -1)
    eval "$(xdotool getwindowgeometry --shell "$O")"
    spectacle -b -n -f -o /tmp/probe.png -d 400 >/dev/null 2>&1; sleep 1
    printf "opacity %s -> " "$value"
    magick /tmp/probe.png -crop 40x10+$((X + 150))+$((Y + 12)) +repage \
      -format "%[fx:int(255*mean.r)],%[fx:int(255*mean.g)],%[fx:int(255*mean.b)]\n" info:
    stop
  done
  ;;
clickthrough)
  start 0.85 1
  xdotool mousemove --window "$W" 1045 38 click 1; sleep 8
  O=$(xdotool search --name "CLA Overlay" | head -1)
  eval "$(xdotool getwindowgeometry --shell "$O")"
  xdotool mousemove $((X + 34)) $((Y + HEIGHT - 13)) click 1; sleep 3   # ✋ off: click-through on
  eval "$(xdotool getwindowgeometry --shell "$O")"
  before=$HEIGHT
  xdotool mousemove $((X + 14)) $((Y + HEIGHT - 13)); sleep 1           # hover ⛭
  xdotool click 1; sleep 3
  eval "$(xdotool getwindowgeometry --shell "$O")"
  echo "overlay height ${before} -> ${HEIGHT} (taller = the toolbar took the click)"
  stop
  ;;
toggle)
  start 0.85 1
  echo "overlay windows at start:        $(xdotool search --name 'CLA Overlay' | wc -l)"
  xdotool mousemove --window "$W" 631 59 click 1; sleep 4
  echo "after clicking Overlay (off):    $(xdotool search --name 'CLA Overlay' | wc -l)"
  xdotool mousemove --window "$W" 631 59 click 1; sleep 4
  echo "after clicking Overlay (on):     $(xdotool search --name 'CLA Overlay' | wc -l)"
  stop
  ;;
*)
  echo "usage: $0 [opacity|clickthrough|toggle]"; exit 2;;
esac
