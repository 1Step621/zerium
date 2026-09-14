#!/usr/bin/env bash
set -euo pipefail

version="$({
  awk '/^\[package\]/{in_package=1; next} /^\[/{in_package=0} in_package && $1 == "version" {gsub(/"/, "", $3); print $3; exit}' Cargo.toml
})"
archive_name="zerium-${version}-linux-x86_64.tar.xz"
appimage_name="zerium-${version}-linux-x86_64.AppImage"

bundle_dir="target/package/${archive_name%.tar.xz}"
appdir="target/package/zerium.AppDir"
rm -rf dist "$bundle_dir" "$appdir"
mkdir -p dist
mkdir -p "$bundle_dir/bin" "$bundle_dir/lib"
mkdir -p "$bundle_dir/share/applications" "$bundle_dir/share/icons/hicolor/256x256/apps"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib"
mkdir -p "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps"
strip -s target/release/zerium
find target/ffmpeg-sdk/lib -type f -name '*.so.*' -exec patchelf --set-rpath '$ORIGIN' {} +
cp target/release/zerium "$bundle_dir/bin/zerium"
cp -a target/ffmpeg-sdk/lib/. "$bundle_dir/lib/"
cp LICENSE "$bundle_dir/LICENSE"
cp packaging/linux/zerium.desktop "$bundle_dir/share/applications/zerium.desktop"
cp assets/zerium.png "$bundle_dir/share/icons/hicolor/256x256/apps/zerium.png"

cp target/release/zerium "$appdir/usr/bin/zerium"
cp -a target/ffmpeg-sdk/lib/. "$appdir/usr/lib/"
cp LICENSE "$appdir/LICENSE"
cp packaging/linux/AppRun "$appdir/AppRun"
chmod +x "$appdir/AppRun"
cp packaging/linux/zerium.desktop "$appdir/usr/share/applications/zerium.desktop"
cp assets/zerium.png "$appdir/usr/share/icons/hicolor/256x256/apps/zerium.png"
cp packaging/linux/zerium.desktop "$appdir/zerium.desktop"
cp assets/zerium.png "$appdir/zerium.png"
ln -s zerium.png "$appdir/.DirIcon"
tar --create --use-compress-program='xz -T0 -6' \
  --file "dist/$archive_name" \
  --directory "$(dirname "$bundle_dir")" "$(basename "$bundle_dir")"

appimagetool="${APPIMAGETOOL:-${RUNNER_TEMP:-/tmp}/appimagetool}"
if [[ ! -x "$appimagetool" ]]; then
  echo "appimagetool is missing or not executable: $appimagetool" >&2
  exit 1
fi
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$appimagetool" --no-appstream "$appdir" "dist/$appimage_name"

deb_log="${RUNNER_TEMP:-/tmp}/zerium-cargo-deb.log"
rpm_log="${RUNNER_TEMP:-/tmp}/zerium-cargo-rpm.log"
cargo deb --no-build --output "dist/zerium_${version}-1_amd64.deb" >"$deb_log" 2>&1 &
deb_pid=$!
cargo generate-rpm --payload-compress gzip -o "dist/zerium-${version}-1.x86_64.rpm" >"$rpm_log" 2>&1 &
rpm_pid=$!

deb_status=0
rpm_status=0
wait "$deb_pid" || deb_status=$?
wait "$rpm_pid" || rpm_status=$?
if ((deb_status != 0)); then
  cat "$deb_log" >&2
  exit "$deb_status"
fi
if ((rpm_status != 0)); then
  cat "$rpm_log" >&2
  exit "$rpm_status"
fi
