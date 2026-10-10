#!/usr/bin/env bash
set -euo pipefail

app_dir="target/package/Zerium.app"

version="${ZERIUM_RELEASE_VERSION:?release version is required}"

rm -rf "$app_dir"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources" dist
cp target/release/zerium "$app_dir/Contents/MacOS/zerium"
cp LICENSE "$app_dir/Contents/Resources/LICENSE"
cp assets/inter/OFL.txt "$app_dir/Contents/Resources/Inter-OFL.txt"
cp assets/zerium.icns "$app_dir/Contents/Resources/zerium.icns"
chmod +x "$app_dir/Contents/MacOS/zerium"
bundle_version="${version%%[-+]*}"
sed "s/__VERSION__/${bundle_version}/g" \
  packaging/macos/Info.plist > "$app_dir/Contents/Info.plist"
dylibbundler \
  -od \
  -b \
  -ns \
  -x "$app_dir/Contents/MacOS/zerium" \
  -d "$app_dir/Contents/Frameworks" \
  -p "@executable_path/../Frameworks/" \
  -s "$(brew --prefix)/lib" \
  -s "$(brew --prefix)/opt/ffmpeg/lib"
vpk pack --packId zerium --packTitle Zerium --packVersion "$version" \
  --packDir "$app_dir" --mainExe zerium --runtime osx-arm64 \
  --outputDir dist
