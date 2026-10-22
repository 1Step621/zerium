#!/usr/bin/env bash
set -euo pipefail

tools_dir="${RUNNER_TEMP:?RUNNER_TEMP is required}/zerium-macos-tools"
app_dir="target/package/Zerium.app"
stage_dir="target/package/macos-staging"
plugin_dir="$stage_dir/Contents/PlugIns/gstreamer"
gst_root="${GSTREAMER_ROOT:-/Library/Frameworks/GStreamer.framework/Versions/1.0}"

version="${ZERIUM_RELEASE_VERSION:?release version is required}"

rm -rf "$app_dir" "$stage_dir"
# Use a staging name different from zerium, without .app or Info.plist.
# codesign treats a recognized bundle's main executable as the whole app, even
# when given the executable's path. Relocate nested code before that seal.
mkdir -p "$stage_dir/Contents/MacOS" "$stage_dir/Contents/Resources" \
  "$stage_dir/Contents/Frameworks" dist
cp target/release/zerium "$stage_dir/Contents/MacOS/zerium"
cp LICENSE "$stage_dir/Contents/Resources/LICENSE"
cp assets/inter/OFL.txt "$stage_dir/Contents/Resources/Inter-OFL.txt"
cp assets/zerium.icns "$stage_dir/Contents/Resources/zerium.icns"
chmod +x "$stage_dir/Contents/MacOS/zerium"
# Avoid dots in nested code directory names: codesign treats them as bundles.
mkdir -p "$plugin_dir"
cp "$gst_root/lib/gstreamer-1.0/"*.dylib "$plugin_dir/"
cp "$gst_root/libexec/gstreamer-1.0/gst-plugin-scanner" \
  "$stage_dir/Contents/MacOS/gst-plugin-scanner"

# Relocate all dependencies, including the SDK's framework-contained dylibs.
# delocate uses @loader_path references instead of collapsing distinct rpaths
# into duplicate LC_RPATH commands, and ad-hoc signs modified binaries.
# Copied plugins/scanner can no longer resolve their SDK-relative rpaths.
# Supply the SDK search path only while collecting and relocating dependencies.
# Dependencies use @loader_path after relocation; keep existing LC_RPATH entries
# to avoid delocate's per-dependency RPATH cleanup and repeated signing.
DYLD_LIBRARY_PATH="$gst_root/lib" \
"$tools_dir/delocate/bin/delocate-path" \
  --no-sanitize-rpaths \
  --lib-path Contents/Frameworks \
  --executable-path "$PWD/$stage_dir/Contents/MacOS" \
  "$stage_dir"

mv "$stage_dir" "$app_dir"
bundle_version="${version%%[-+]*}"
sed "s/__VERSION__/${bundle_version}/g" \
  packaging/macos/Info.plist > "$app_dir/Contents/Info.plist"

# Velopack recursively signs nested code and the final bundle after adding
# UpdateMac and metadata.
vpk pack --packId zerium --packTitle Zerium --packVersion "$version" \
  --packDir "$app_dir" --mainExe zerium --runtime osx-arm64 \
  --signAppIdentity - \
  --outputDir dist
