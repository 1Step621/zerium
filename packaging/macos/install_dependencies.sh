#!/usr/bin/env bash
set -euo pipefail

export HOMEBREW_NO_AUTO_UPDATE=1
brew install --force-bottle dylibbundler

# The official SDK includes SoundTouch and its GStreamer plugin.
gst_version="1.28.8"
gst_root="/Library/Frameworks/GStreamer.framework/Versions/1.0"
mkdir -p target/tools
for component in gstreamer-1.0 gstreamer-1.0-devel; do
  name="$component-$gst_version-universal.pkg"
  url="https://gstreamer.freedesktop.org/data/pkg/osx/$gst_version/$name"
  curl --fail --location --retry 3 "$url" --output "target/tools/$name"
  curl --fail --location --retry 3 "$url.sha256sum" --output "target/tools/$name.sha256sum"
  (cd target/tools && shasum -a 256 --check "$name.sha256sum")
  sudo installer -pkg "target/tools/$name" -target /
done

# Cargo and packaging must use the same SDK, without Homebrew's GStreamer.
if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'GSTREAMER_ROOT=%s\nPKG_CONFIG_PATH=%s/lib/pkgconfig\nPKG_CONFIG_LIBDIR=%s/lib/pkgconfig\n' \
    "$gst_root" "$gst_root" "$gst_root" >> "$GITHUB_ENV"
  printf '%s/bin\n' "$gst_root" >> "$GITHUB_PATH"
fi
