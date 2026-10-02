#!/usr/bin/env bash
set -euo pipefail

pkg_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/zerium-pkg"
app_dir="$pkg_root/Applications/Zerium.app"

version="$({
  awk '/^\[workspace.package\]/{in_package=1; next} /^\[/{in_package=0} in_package && $1 == "version" {gsub(/"/, "", $3); print $3; exit}' Cargo.toml
})"
installer_name="zerium-${version}-macos-aarch64.pkg"

rm -rf "$pkg_root" dist
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources" dist
cp target/release/zerium "$app_dir/Contents/MacOS/zerium"
cp LICENSE "$app_dir/Contents/Resources/LICENSE"
cp assets/inter/OFL.txt "$app_dir/Contents/Resources/Inter-OFL.txt"
cp assets/zerium.icns "$app_dir/Contents/Resources/zerium.icns"
chmod +x "$app_dir/Contents/MacOS/zerium"
sed "s/__VERSION__/${version}/g" \
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
mkdir -p "$pkg_root/usr/local/bin"
install -m755 packaging/macos/zerium "$pkg_root/usr/local/bin/zerium"
pkgbuild \
  --root "$pkg_root" \
  --component-plist packaging/macos/components.plist \
  --identifier dev.zerium.Zerium \
  --version "$version" \
  --install-location / \
  --ownership recommended \
  "dist/$installer_name"
