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
tar --create --use-compress-program='xz -T0 -6' \
  --file "dist/$archive_name" \
  --directory "$(dirname "$bundle_dir")" "$(basename "$bundle_dir")"

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
