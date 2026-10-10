#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install --no-install-recommends -y \
  libasound2-dev \
  libdbus-1-dev \
  libfontconfig1-dev \
  libvulkan-dev \
  libwayland-dev \
  libx11-dev \
  libx11-xcb-dev \
  libxcb1-dev \
  libxkbcommon-dev \
  libxkbcommon-x11-dev \
  patchelf \
  pkg-config \
  squashfs-tools

ffmpeg_archive="${RUNNER_TEMP:-/tmp}/ffmpeg-n9.0-latest-linux64-gpl-shared-9.0.tar.xz"
ffmpeg_url="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-linux64-gpl-shared-9.0.tar.xz"
rm -rf target/ffmpeg-sdk
mkdir -p target/ffmpeg-sdk
curl --fail --location --retry 3 --output "$ffmpeg_archive" "$ffmpeg_url"
tar --extract --xz --file "$ffmpeg_archive" --strip-components=1 --directory target/ffmpeg-sdk
printf '%s\n' "FFMPEG_DIR=$GITHUB_WORKSPACE/target/ffmpeg-sdk" >> "$GITHUB_ENV"
printf '%s\n' 'RUSTFLAGS=-C link-arg=-Wl,-rpath,$ORIGIN/../lib' >> "$GITHUB_ENV"
