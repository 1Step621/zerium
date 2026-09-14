#!/usr/bin/env bash
set -euo pipefail

version="$({
  awk '/^\[package\]/{in_package=1; next} /^\[/{in_package=0} in_package && $1 == "version" {gsub(/"/, "", $3); print $3; exit}' Cargo.toml
})"
short_sha="${GITHUB_SHA:0:7}"
tag="v${version}-${short_sha}"

if gh release view "$tag" >/dev/null 2>&1; then
  gh release upload "$tag" release-assets/* --clobber
else
  gh release create "$tag" \
    --target "$GITHUB_SHA" \
    --title "Zerium $version ($short_sha)" \
    --notes "Automated release build from the release branch at commit $GITHUB_SHA." \
    release-assets/*
fi
