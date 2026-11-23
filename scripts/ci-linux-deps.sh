#!/usr/bin/env bash
# The libraries and the clang the Linux build needs on an ubuntu-22.04 runner,
# shared by the Linux release build and the cache-warming build (#358) so the
# two compile the same way and share one cache. Exports the compiler choice
# through GITHUB_ENV and GITHUB_PATH.
set -euo pipefail
sudo apt-get update
# nasm: OpenH264's x86 assembly (videos; plain C without it).
# libva-dev and nvidia-cuda-dev: headers for the hardware video
# codecs of calls (VAAPI, NVENC/NVDEC), dlopened at run time.
sudo apt-get install -y --no-install-recommends \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev libx11-xcb-dev \
  libx11-dev libxext-dev libxfixes-dev libxdamage-dev libxrandr-dev libxcomposite-dev libxtst-dev \
  libfontconfig-dev libfreetype-dev libvulkan-dev libgl-dev libdrm-dev libgbm-dev \
  libssl-dev libasound2-dev libpulse-dev libpipewire-0.3-dev libdbus-1-dev \
  pkg-config cmake zip python3 nasm nvidia-cuda-dev libva-dev
# libwebrtc (calls) is built against Chromium's libc++ and needs a
# recent clang, as in the dev shell.
wget -qO llvm.sh https://apt.llvm.org/llvm.sh
sudo bash llvm.sh 21
echo "CC_x86_64_unknown_linux_gnu=clang-21" >> "$GITHUB_ENV"
echo "CXX_x86_64_unknown_linux_gnu=clang++-21" >> "$GITHUB_ENV"
# webrtc-sys runs a plain `clang++`: make that this clang.
echo "/usr/lib/llvm-21/bin" >> "$GITHUB_PATH"
