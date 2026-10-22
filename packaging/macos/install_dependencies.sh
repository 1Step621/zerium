#!/usr/bin/env bash
set -euo pipefail

# Python virtual environments belong to the runner, outside the Rust cache.
tools_dir="${RUNNER_TEMP:?RUNNER_TEMP is required}/zerium-macos-tools"
mkdir -p "$tools_dir"
python3 -m venv "$tools_dir/delocate"
"$tools_dir/delocate/bin/python" -m pip install --disable-pip-version-check 'delocate==0.13.0'

# The official SDK includes SoundTouch and its GStreamer plugin.
gst_version="1.28.8"
gst_root="/Library/Frameworks/GStreamer.framework/Versions/1.0"
for component in gstreamer-1.0 gstreamer-1.0-devel; do
  name="$component-$gst_version-universal.pkg"
  url="https://gstreamer.freedesktop.org/data/pkg/osx/$gst_version/$name"
  curl --fail --location --retry 3 "$url" --output "$tools_dir/$name"
  curl --fail --location --retry 3 "$url.sha256sum" --output "$tools_dir/$name.sha256sum"
  (cd "$tools_dir" && shasum -a 256 --check "$name.sha256sum")
  sudo installer -pkg "$tools_dir/$name" -target /
done

# Cargo and packaging must use the same SDK, without Homebrew's GStreamer.
if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'GSTREAMER_ROOT=%s\nPKG_CONFIG_PATH=%s/lib/pkgconfig\nPKG_CONFIG_LIBDIR=%s/lib/pkgconfig\n' \
    "$gst_root" "$gst_root" "$gst_root" >> "$GITHUB_ENV"
  printf '%s/bin\n' "$gst_root" >> "$GITHUB_PATH"
fi
