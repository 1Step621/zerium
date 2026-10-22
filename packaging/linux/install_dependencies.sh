#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install --no-install-recommends -y \
  ca-certificates \
  curl \
  libasound2-dev \
  libdbus-1-dev \
  libfontconfig1-dev \
  libgstreamer1.0-dev \
  libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly \
  gstreamer1.0-libav \
  libvulkan-dev \
  libwayland-dev \
  libx11-dev \
  libx11-xcb-dev \
  libxcb1-dev \
  libxkbcommon-dev \
  libxkbcommon-x11-dev \
  mold \
  pkg-config \
  squashfs-tools

mkdir -p target/tools
curl --fail --location --retry 3 \
  --output target/tools/linuxdeploy.AppImage \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
chmod +x target/tools/linuxdeploy.AppImage
