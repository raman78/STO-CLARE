#!/bin/sh
# Bootstrap installer for STO-CLARE (Linux).
#
# Downloads the latest prebuilt binary from GitHub Releases, installs it under
# ~/.local/opt/sto-clare, symlinks `sto-clare` onto your PATH, and registers the
# applications-menu entry. Re-running upgrades in place (no duplicate menu
# entries). For a from-source dev install instead, use scripts/dev-install.sh.
set -e

REPO="raman78/STO-CLARE"
OPT_DIR="$HOME/.local/opt/sto-clare"
BIN_DIR="$HOME/.local/bin"
LINK="$BIN_DIR/sto-clare"
BIN_NAME="sto-clare"

echo "==========================================="
echo "        STO-CLARE installer (Linux)         "
echo "==========================================="
echo ""

for tool in curl tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Error: '$tool' is required but not installed." >&2
        exit 1
    fi
done

echo "Looking up the latest release..."
# Use /releases (not /releases/latest): the latter excludes pre-releases and
# 404s when only pre-releases exist. The list is newest-first, so the first
# matching asset belongs to the newest release.
API="https://api.github.com/repos/$REPO/releases"
ASSET_URL=$(curl -fsSL "$API" \
    | grep -o '"browser_download_url": *"[^"]*linux-x86_64\.tar\.gz"' \
    | head -1 \
    | sed -E 's/.*"(https[^"]+)"/\1/')

if [ -z "$ASSET_URL" ]; then
    echo "Error: could not find a linux-x86_64 asset in the latest release." >&2
    echo "See https://github.com/$REPO/releases" >&2
    exit 1
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
echo "Downloading $ASSET_URL"
curl -fsSL "$ASSET_URL" -o "$TMP/sto-clare.tar.gz"
mkdir -p "$TMP/extract"
tar -xzf "$TMP/sto-clare.tar.gz" -C "$TMP/extract"

if [ ! -f "$TMP/extract/$BIN_NAME" ]; then
    echo "Error: unexpected archive layout." >&2
    exit 1
fi

echo "Installing to $OPT_DIR"
if [ -d "$OPT_DIR" ]; then
    rm -rf "$OPT_DIR"
fi
mkdir -p "$OPT_DIR"
cp "$TMP/extract/$BIN_NAME" "$OPT_DIR/"
[ -f "$TMP/extract/icon.png" ] && cp "$TMP/extract/icon.png" "$OPT_DIR/"
chmod +x "$OPT_DIR/$BIN_NAME"

mkdir -p "$BIN_DIR"
if [ -L "$LINK" ] || [ -e "$LINK" ]; then
    rm "$LINK"
fi
ln -s "$OPT_DIR/$BIN_NAME" "$LINK"

"$OPT_DIR/$BIN_NAME" --install-desktop

echo ""
echo "==========================================="
echo "Installation complete! Run it with:  sto-clare"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo ""
       echo "Note: $BIN_DIR is not on your PATH. Add it, e.g.:"
       echo "    export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac
echo "==========================================="
