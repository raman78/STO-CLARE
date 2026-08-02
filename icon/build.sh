#!/bin/sh
# Regenerate the shipped icon files from icon/icon.svg, the single source.
#
#   icon.png — 512x512 RGBA, the window and desktop-entry icon
#   icon.ico — multi-size, used by the Windows build and installer
#
# Needs inkscape (SVG rendering) and python3 with Pillow (ICO packing).
set -e
DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SVG="$DIR/icon.svg"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

inkscape --export-type=png --export-filename="$DIR/icon.png" -w 512 -h 512 "$SVG" >/dev/null

for size in 16 24 32 48 64 128 256; do
    inkscape --export-type=png --export-filename="$TMP/$size.png" \
        -w "$size" -h "$size" "$SVG" >/dev/null
done

python3 - "$TMP" "$DIR/icon.ico" <<'PY'
import sys
from PIL import Image
tmp, out = sys.argv[1], sys.argv[2]
sizes = [16, 24, 32, 48, 64, 128, 256]
images = [Image.open(f"{tmp}/{s}.png").convert("RGBA") for s in sizes]
images[-1].save(out, format="ICO", sizes=[(s, s) for s in sizes], append_images=images[:-1])
PY

echo "wrote $DIR/icon.png and $DIR/icon.ico"
