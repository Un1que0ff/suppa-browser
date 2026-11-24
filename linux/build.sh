#!/bin/sh
# Builds Suppa Browser for Linux: installs what is missing (Rust, WebKitGTK
# development files), then builds dist/linux/ with suppa, scripts/ and install.sh.
# Run from anywhere:   sh linux/build.sh
set -e
cd "$(dirname "$0")"
root=$(cd .. && pwd)

if ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
  echo "[1/3] Installing the WebKitGTK and GTK development files (asks for your password)..."
  if command -v apt-get >/dev/null; then
    sudo apt-get install -y build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev curl
  elif command -v dnf >/dev/null; then
    sudo dnf install -y gcc pkgconf-pkg-config gtk3-devel webkit2gtk4.1-devel libsoup3-devel curl
  elif command -v pacman >/dev/null; then
    sudo pacman -S --needed --noconfirm base-devel pkgconf gtk3 webkit2gtk-4.1 libsoup3 curl
  else
    echo "Install the WebKitGTK 4.1 and GTK 3 development packages of your distribution, then run this again."
    exit 1
  fi
fi
echo "[1/3] WebKitGTK OK"

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if ! command -v cargo >/dev/null; then
  echo "[2/3] Installing Rust..."
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  . "$HOME/.cargo/env"
fi
echo "[2/3] Rust OK"

echo "[3/3] Building (release)..."
# Your home folder and the project folder are written as "~" and "." inside
# the program instead of their real paths.
CARGO_ENCODED_RUSTFLAGS=$(printf '%s\037%s' "--remap-path-prefix=$HOME=~" "--remap-path-prefix=$root=.")
export CARGO_ENCODED_RUSTFLAGS
cargo build --release
target="${CARGO_TARGET_DIR:-$(pwd)/target}"
out="$root/dist/linux"
mkdir -p "$out"
cp "$target/release/suppa" "$out/suppa"
cp -r "$root/scripts" "$out/"
cp install.sh "$out/install.sh"
echo
echo "Done: $out/suppa ($(du -h "$out/suppa" | cut -f1))"
echo "Install it for your user with:  sh $out/install.sh"
