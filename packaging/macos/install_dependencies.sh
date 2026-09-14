#!/usr/bin/env bash
set -euo pipefail

export HOMEBREW_NO_AUTO_UPDATE=1
brew install --force-bottle dylibbundler ffmpeg pkg-config
ffmpeg_version="$(brew list --versions ffmpeg | awk '{print $2}')"
[[ "$ffmpeg_version" == 9.* ]] || {
  echo "expected FFmpeg 9, got ${ffmpeg_version:-unavailable}" >&2
  exit 1
}
printf '%s\n' "FFMPEG_DIR=$(brew --prefix ffmpeg)" >> "$GITHUB_ENV"
