#!/usr/bin/env bash
set -euo pipefail

if (( $# > 1 )); then
    echo "Usage: $0 [output.png]" >&2
    exit 2
fi
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
output=${1:-target/visual/table.png}
sandbox=$(mktemp -d)
trap 'rm -rf -- "$sandbox"' EXIT

# Capture into a fresh directory so an old screenshot cannot count as success.
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET \
    LIBGL_ALWAYS_SOFTWARE=1 WINIT_X11_SCALE_FACTOR=1 \
    XDG_RUNTIME_DIR="$sandbox" TWENTY_ONE_PRO_DATA_DIR="$sandbox/profile" \
    EGUI_INSPECTION=0 EFRAME_SCREENSHOT_TO="$sandbox/table.png" \
    xvfb-run -a -s '-screen 0 1280x1024x24 -dpi 96 -nolisten tcp' \
    cargo run --locked --features dev-screenshot

test -s "$sandbox/table.png"
mkdir -p -- "$(dirname -- "$output")"
cp -- "$sandbox/table.png" "$output"
printf 'Screenshot saved to %s\n' "$output"
