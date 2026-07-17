#!/usr/bin/env bash
set -euo pipefail

target_id= ffmpeg=
while [[ $# -gt 0 ]]; do case "$1" in --target-id) target_id=$2; shift 2;; --ffmpeg) ffmpeg=$2; shift 2;; *) echo "usage: $0 --target-id ID --ffmpeg PATH" >&2; exit 64;; esac; done
[[ -n "$target_id" && -n "$ffmpeg" ]]
require() { local name=$1; shift; "$@" | grep -Fqx -- "$name" >/dev/null || { echo "missing required capability: $name" >&2; exit 1; }; }
hwaccels=$("$ffmpeg" -hide_banner -hwaccels | sed '1,/Hardware acceleration methods:/d' | sed 's/^[[:space:]]*//')
decoders=$("$ffmpeg" -hide_banner -decoders | awk '/^ V/ {print $2}')
encoders=$("$ffmpeg" -hide_banner -encoders | awk '/^ V|^ A/ {print $2}')
filters=$("$ffmpeg" -hide_banner -filters | awk '/^[ .TSC]{3}/ {print $2}')
require vp9 echo "$decoders"; require libvpx-vp9 echo "$encoders"; require libopus echo "$encoders"
case "$target_id" in
  macos-arm64-vt) require videotoolbox echo "$hwaccels" ;;
  windows-x64-mf) require d3d11va echo "$hwaccels"; require h264_mf echo "$encoders" ;;
  windows-x64-nvidia|linux-x64-nvidia) require cuda echo "$hwaccels"; require h264_nvenc echo "$encoders" ;;
  linux-x64-vaapi-wayland|linux-x64-vaapi-x11) require vaapi echo "$hwaccels"; require scale_vaapi echo "$filters" ;;
  *) echo "unsupported target: $target_id" >&2; exit 64 ;;
esac
