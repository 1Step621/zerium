#!/usr/bin/env bash
set -euo pipefail

app_dir="target/package/Zerium.app"
plugin_dir="$app_dir/Contents/PlugIns/gstreamer"
gst_root="${GSTREAMER_ROOT:-/Library/Frameworks/GStreamer.framework/Versions/1.0}"
gst_lib_dir="target/package/gstreamer-libs"

version="${ZERIUM_RELEASE_VERSION:?release version is required}"

rm -rf "$app_dir" "$gst_lib_dir"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources" \
  "$app_dir/Contents/Frameworks" "$gst_lib_dir" dist
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
# dylibbundler ignores dependencies whose resolved paths contain .framework.
# Stage the SDK libraries outside the framework before collecting dependencies.
cp "$gst_root/lib/"*.dylib "$gst_lib_dir/"
cp "$gst_root/libexec/gstreamer-1.0/gst-plugin-scanner" \
  "$app_dir/Contents/MacOS/gst-plugin-scanner"
gst_binaries=(-x "$app_dir/Contents/MacOS/gst-plugin-scanner")
for plugin in "$plugin_dir/"*.dylib; do
  gst_binaries+=(-x "$plugin")
done
# dylibbundler ad-hoc signs the relocated dependencies and all -x binaries.
dylibbundler \
  -b \
  -x "$app_dir/Contents/MacOS/zerium" \
  "${gst_binaries[@]}" \
  -d "$app_dir/Contents/Frameworks" \
  -p "@executable_path/../Frameworks/" \
  -s "$PWD/$gst_lib_dir/"

# Velopack signs the final bundle after adding UpdateMac and metadata.
vpk pack --packId zerium --packTitle Zerium --packVersion "$version" \
  --packDir "$app_dir" --mainExe zerium --runtime osx-arm64 \
  --signAppIdentity - --signDisableDeep \
  --outputDir dist
