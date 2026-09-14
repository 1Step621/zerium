#!/usr/bin/env bash
set -euo pipefail

version="$({
  awk '/^\[package\]/{in_package=1; next} /^\[/{in_package=0} in_package && $1 == "version" {gsub(/"/, "", $3); print $3; exit}' Cargo.toml
})"
archive_name="zerium-${version}-linux-x86_64.tar.xz"

bundle_dir="target/package/${archive_name%.tar.xz}"
rm -rf dist "$bundle_dir"
mkdir -p dist
mkdir -p "$bundle_dir/bin" "$bundle_dir/lib"
strip -s target/release/zerium
find target/ffmpeg-sdk/lib -type f -name '*.so.*' -exec patchelf --set-rpath '$ORIGIN' {} +
cp target/release/zerium "$bundle_dir/bin/zerium"
cp -a target/ffmpeg-sdk/lib/. "$bundle_dir/lib/"
cp LICENSE "$bundle_dir/LICENSE"
tar --create --xz --file "dist/$archive_name" --directory "$(dirname "$bundle_dir")" "$(basename "$bundle_dir")"
cargo deb --no-build --output "dist/zerium_${version}-1_amd64.deb"
cargo generate-rpm -o "dist/zerium-${version}-1.x86_64.rpm"
