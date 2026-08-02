#!/bin/sh
# Build the checkout this script lives in, then run it — the "try the newest
# code" launcher. scripts/dev-install.sh installs it as its own menu entry.
#
# The entry opens a terminal (Terminal=true), so the build is visible while it
# runs. It ends one of two ways: the app starts detached and the terminal
# closes, or the build fails and the window stays with the error in it.
set -e

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BIN="$PROJECT_DIR/target/release/sto-clare"

# Wait for the reader before closing, so a message is not lost when the
# terminal disappears with it.
pause_then_exit() {
    echo ""
    printf 'Press Enter to close... '
    read -r _
    exit "$1"
}

# A desktop session does not always carry ~/.cargo/bin on PATH — it is set up by
# the shell profile, which a menu entry never runs.
if ! command -v cargo >/dev/null 2>&1 && [ -r "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found. Install the Rust toolchain from https://www.rust-lang.org/"
    pause_then_exit 1
fi

echo "==========================================="
echo "        STO-CLARE - building the           "
echo "        current source, then running       "
echo "==========================================="
echo ""
echo "$PROJECT_DIR"
echo ""

if ! (cd "$PROJECT_DIR" && cargo build --release); then
    echo ""
    echo "Build failed - the app was not started."
    pause_then_exit 1
fi

echo ""
echo "Starting $BIN"

# Detach, so this terminal can close while the app keeps running. setsid comes
# with util-linux and is there on any desktop Linux; nohup is the fallback.
if command -v setsid >/dev/null 2>&1; then
    setsid "$BIN" >/dev/null 2>&1 &
else
    nohup "$BIN" >/dev/null 2>&1 &
fi
