#!/usr/bin/env bash
set -euo pipefail

version="${ZERIUM_RELEASE_VERSION:?release version is required}"
appdir="target/package/zerium.AppDir"
rm -rf "$appdir"
mkdir -p dist "$appdir/usr/lib/gstreamer-1.0" "$appdir/usr/libexec/gstreamer-1.0" "$appdir/usr/share/mime/packages"
cp "$(pkg-config --variable=pluginsdir gstreamer-1.0)"/*.so "$appdir/usr/lib/gstreamer-1.0/"
cp "$(pkg-config --variable=libexecdir gstreamer-1.0)/gstreamer-1.0/gst-plugin-scanner" \
  "$appdir/usr/libexec/gstreamer-1.0/"
cp LICENSE "$appdir/LICENSE"
cp assets/inter/OFL.txt "$appdir/Inter-OFL.txt"
cp packaging/linux/zerium.xml "$appdir/usr/share/mime/packages/zerium.xml"

gst_libdir="$(pkg-config --variable=libdir gstreamer-1.0)"
# These GStreamer plugin dependencies are not present on every desktop.
target/tools/linuxdeploy.AppImage --appimage-extract-and-run \
  --appdir "$appdir" \
  --executable target/release/zerium \
  --library "$gst_libdir/libfribidi.so.0" \
  --library "$gst_libdir/libjack.so.0" \
  --library "$gst_libdir/libpipewire-0.3.so.0" \
  --library "$gst_libdir/libusb-1.0.so.0" \
  --deploy-deps-only "$appdir/usr/lib/gstreamer-1.0" \
  --deploy-deps-only "$appdir/usr/libexec/gstreamer-1.0/gst-plugin-scanner" \
  --desktop-file packaging/linux/zerium.desktop \
  --icon-file assets/zerium.png \
  --custom-apprun packaging/linux/AppRun

vpk pack --packId zerium --packTitle Zerium \
  --packVersion "$version" \
  --packDir "$appdir" --mainExe zerium --runtime linux-x64 \
  --outputDir dist

# The installer uses the same desktop integration files as the AppImage.
sed "s/@VERSION@/$version/g" packaging/linux/install.sh.in > dist/install.sh
cp packaging/linux/zerium.desktop packaging/linux/zerium.xml assets/zerium.png dist/
