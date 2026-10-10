#!/usr/bin/env bash
set -euo pipefail

version="${ZERIUM_RELEASE_VERSION:?release version is required}"
short_sha="${GITHUB_SHA:0:7}"
tag="v${version}"

if draft="$(gh release view "$tag" --json isDraft --jq .isDraft 2>/dev/null)"; then
  if [[ "$draft" == false ]]; then
    printf 'Release %s is already published.\n' "$tag"
    exit 0
  fi
  gh release upload "$tag" release-assets/* --clobber
  gh release edit "$tag" --draft=false
else
  gh release create "$tag" \
    --target "$GITHUB_SHA" \
    --title "Zerium $version" \
    --notes "Zerium $version ($short_sha)" \
    release-assets/*
fi
