#!/usr/bin/env bash
set -euo pipefail

app_dir="target/package/Zerium.app"
plugin_dir="$app_dir/Contents/PlugIns/gstreamer"
gst_root="${GSTREAMER_ROOT:-/Library/Frameworks/GStreamer.framework/Versions/1.0}"

version="${ZERIUM_RELEASE_VERSION:?release version is required}"

rm -rf "$app_dir"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources" \
  "$app_dir/Contents/Frameworks" dist
cp target/release/zerium "$app_dir/Contents/MacOS/zerium"
cp LICENSE "$app_dir/Contents/Resources/LICENSE"
cp assets/inter/OFL.txt "$app_dir/Contents/Resources/Inter-OFL.txt"
cp assets/zerium.icns "$app_dir/Contents/Resources/zerium.icns"
chmod +x "$app_dir/Contents/MacOS/zerium"
bundle_version="${version%%[-+]*}"
sed "s/__VERSION__/${bundle_version}/g" \
  packaging/macos/Info.plist > "$app_dir/Contents/Info.plist"
# Avoid dots in nested code directory names: codesign treats them as bundles.
mkdir -p "$plugin_dir"
cp "$gst_root/lib/gstreamer-1.0/"*.dylib "$plugin_dir/"
cp "$gst_root/libexec/gstreamer-1.0/gst-plugin-scanner" \
  "$app_dir/Contents/MacOS/gst-plugin-scanner"

# Relocate all dependencies, including the SDK's framework-contained dylibs.
# delocate uses @loader_path references instead of collapsing distinct rpaths
# into duplicate LC_RPATH commands, and ad-hoc signs modified binaries.
target/tools/delocate/bin/delocate-path \
  --lib-path Contents/Frameworks \
  --executable-path "$PWD/$app_dir/Contents/MacOS" \
  "$app_dir"

# Velopack signs the final bundle after adding UpdateMac and metadata.
vpk pack --packId zerium --packTitle Zerium --packVersion "$version" \
  --packDir "$app_dir" --mainExe zerium --runtime osx-arm64 \
  --signAppIdentity - --signDisableDeep \
  --outputDir dist
