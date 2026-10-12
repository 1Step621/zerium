#!/usr/bin/env bash
set -euo pipefail

export HOMEBREW_NO_AUTO_UPDATE=1
brew install --force-bottle dylibbundler pkg-config gstreamer
