#!/bin/sh
# Local "editable" install (the closest Rust analogue to `pipx install -e .`).
#
# Rust compiles to a native binary, so there is no true editable install.
# Instead we build a release binary and symlink `sto-clare` to it: after every
# `cargo build --release` the symlink points at the freshly built binary, so
# your latest changes run immediately — no reinstall step.
#
# It also registers the desktop / menu entry (via --install-desktop), which
# points at the same build. Re-running this script is idempotent.
set -e

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET="$PROJECT_DIR/target/release/sto-clare"
BIN_DIR="$HOME/.local/bin"
LINK="$BIN_DIR/sto-clare"

echo "==========================================="
echo "        STO-CLARE — dev install           "
echo "==========================================="
echo ""

echo "Building release binary..."
( cd "$PROJECT_DIR" && cargo build --release )

mkdir -p "$BIN_DIR"

# Replace any existing symlink/file without using force flags.
if [ -L "$LINK" ] || [ -e "$LINK" ]; then
    rm "$LINK"
fi
ln -s "$TARGET" "$LINK"
echo "Linked $LINK -> $TARGET"

# Register the menu entry / icon for this build location.
"$TARGET" --install-desktop

echo ""
echo "==========================================="
echo "Done. Run it with:  sto-clare"
echo "After code changes:  cargo build --release  (the symlink auto-updates)"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo ""
       echo "Note: $BIN_DIR is not on your PATH. Add it, e.g.:"
       echo "    export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac
echo "==========================================="
