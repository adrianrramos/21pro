#!/usr/bin/env bash
set -euo pipefail

if (( $# > 2 )); then
    echo "Usage: $0 [output.png] [fixture]" >&2
    exit 2
fi
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
output=${1:-target/visual/table.png}
features=dev-screenshot
fixture_env=(-u TWENTY_ONE_PRO_FIXTURE)
if (( $# == 2 )); then
    features+=,dev-fixtures
    fixture_env=("TWENTY_ONE_PRO_FIXTURE=$2")
fi
sandbox=$(mktemp -d)
trap 'rm -rf -- "$sandbox"' EXIT

# Capture into a fresh directory so an old screenshot cannot count as success.
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET "${fixture_env[@]}" \
    LIBGL_ALWAYS_SOFTWARE=1 WINIT_X11_SCALE_FACTOR=1 \
    TMPDIR="$sandbox" XDG_RUNTIME_DIR="$sandbox" TWENTY_ONE_PRO_DATA_DIR="$sandbox/profile" \
    EGUI_INSPECTION=0 EFRAME_SCREENSHOT_TO="$sandbox/table.png" \
    xvfb-run -a -s '-screen 0 1280x1024x24 -dpi 96 -nolisten tcp' \
    cargo run --locked --features "$features"

test -s "$sandbox/table.png"
mkdir -p -- "$(dirname -- "$output")"
cp -- "$sandbox/table.png" "$output"
printf 'Screenshot saved to %s\n' "$output"
