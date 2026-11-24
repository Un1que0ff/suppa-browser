#!/bin/sh
# Installs Suppa Browser for your Linux user (no root needed):
#   ~/.local/share/suppa-browser/   the program and its scripts folder
#   ~/.local/bin/suppa              command to start it
#   menu entry + icon               (Suppa Browser in your app menu)
# Run it from the unpacked download folder:   sh install.sh
# Remove it again with:                       sh install.sh --uninstall
set -e
here=$(cd "$(dirname "$0")" && pwd)
dest="${XDG_DATA_HOME:-$HOME/.local/share}/suppa-browser"
bin="$HOME/.local/bin"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

if [ "$1" = "--uninstall" ]; then
  rm -rf "$dest" "$bin/suppa" "$apps/suppa.desktop"
  command -v update-desktop-database >/dev/null && update-desktop-database "$apps" 2>/dev/null || true
  echo "Suppa Browser removed. Your settings and tabs stay in ~/.local/share/SuppaBrowser (delete that folder to remove them too)."
  exit 0
fi

if [ ! -x "$here/suppa" ]; then
  echo "suppa not found next to install.sh - unpack the whole download first."
  exit 1
fi
mkdir -p "$dest" "$bin"
cp "$here/suppa" "$dest/suppa"
# Interface files are updated; your own filter rules and user scripts are kept.
mkdir -p "$dest/scripts"
for f in "$here"/scripts/*; do
  name=$(basename "$f")
  case "$name" in
    filters|userscripts) mkdir -p "$dest/scripts/$name"; cp -n "$f"/* "$dest/scripts/$name/" 2>/dev/null || true ;;
    *) cp -r "$f" "$dest/scripts/" ;;
  esac
done
ln -sf "$dest/suppa" "$bin/suppa"
"$dest/suppa" --install-desktop
echo "Suppa Browser is installed. Start it from your app menu or with: suppa"
case ":$PATH:" in *":$bin:"*) ;; *) echo "(Add $bin to your PATH to start it by typing 'suppa'.)" ;; esac

# The web engine (WebKitGTK) comes from your system.
if ! ldd "$dest/suppa" | grep -q "not found"; then exit 0; fi
echo
echo "Missing system libraries. Install WebKitGTK 4.1, for example:"
echo "  Ubuntu / Debian:  sudo apt install libwebkit2gtk-4.1-0"
echo "  Fedora:           sudo dnf install webkit2gtk4.1"
echo "  Arch:             sudo pacman -S webkit2gtk-4.1"
