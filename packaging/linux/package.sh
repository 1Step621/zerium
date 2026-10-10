#!/usr/bin/env bash
set -euo pipefail

version="${ZERIUM_RELEASE_VERSION:?release version is required}"
appdir="target/package/zerium.AppDir"
rm -rf "$appdir"
mkdir -p dist "$appdir/usr/bin" "$appdir/usr/lib"
mkdir -p "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/usr/share/mime/packages"
strip -s target/release/zerium
find target/ffmpeg-sdk/lib -type f -name '*.so.*' -exec patchelf --set-rpath '$ORIGIN' {} +
cp target/release/zerium "$appdir/usr/bin/zerium"
cp -a target/ffmpeg-sdk/lib/. "$appdir/usr/lib/"
cp LICENSE "$appdir/LICENSE"
cp assets/inter/OFL.txt "$appdir/Inter-OFL.txt"
cp packaging/linux/AppRun "$appdir/AppRun"
chmod +x "$appdir/AppRun"
cp packaging/linux/zerium.desktop "$appdir/usr/share/applications/zerium.desktop"
cp packaging/linux/zerium.xml "$appdir/usr/share/mime/packages/zerium.xml"
cp assets/zerium.png "$appdir/usr/share/icons/hicolor/256x256/apps/zerium.png"
cp packaging/linux/zerium.desktop "$appdir/zerium.desktop"
cp assets/zerium.png "$appdir/zerium.png"
ln -s zerium.png "$appdir/.DirIcon"

vpk pack --packId zerium --packTitle Zerium \
  --packVersion "$version" \
  --packDir "$appdir" --mainExe zerium --runtime linux-x64 \
  --outputDir dist

# The installer uses the same desktop integration files as the AppImage.
sed "s/@VERSION@/$version/g" packaging/linux/install.sh.in > dist/install.sh
cp packaging/linux/zerium.desktop packaging/linux/zerium.xml assets/zerium.png dist/
