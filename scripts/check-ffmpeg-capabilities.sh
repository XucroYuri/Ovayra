#!/usr/bin/env bash
set -euo pipefail
target_id= ffmpeg=
while [[ $# -gt 0 ]]; do case "$1" in --target-id) target_id=$2; shift 2;; --ffmpeg) ffmpeg=$2; shift 2;; *) exit 64;; esac; done
[[ -n "$target_id" && -n "$ffmpeg" ]]
require() { local name=$1 inventory=$2; grep -Fqx -- "$name" <<<"$inventory" >/dev/null || { echo "missing required capability: $name" >&2; exit 1; }; }
hwaccels=$("$ffmpeg" -hide_banner -hwaccels 2>&1 | awk 'seen { sub(/^[[:space:]]*/, ""); print } /Hardware acceleration methods:/ { seen = 1 }')
decoders=$("$ffmpeg" -hide_banner -decoders 2>&1 | awk '$1 ~ /^[A-Z.][A-Z.][A-Z.][A-Z.][A-Z.][A-Z.]$/ {print $2}')
encoders=$("$ffmpeg" -hide_banner -encoders 2>&1 | awk '$1 ~ /^[A-Z.][A-Z.][A-Z.][A-Z.][A-Z.][A-Z.]$/ {print $2}')
filters=$("$ffmpeg" -hide_banner -filters 2>&1 | awk '$1 ~ /^[A-Z.][A-Z.][A-Z.]$/ {print $2}')
codec_inventory=$(printf '%s\n%s\n' "$decoders" "$encoders")
require vp9 "$decoders"; require libvpx-vp9 "$encoders"; require libopus "$encoders"
case "$target_id" in
  macos-arm64-vt)
    require videotoolbox "$hwaccels"; require h264_videotoolbox "$encoders"; require hevc_videotoolbox "$encoders"; require aac_at "$encoders" ;;
  windows-x64-mf)
    require d3d11va "$hwaccels"; require dxva2 "$hwaccels"; require h264_mf "$codec_inventory"; require cuda "$hwaccels"; require h264_nvenc "$encoders"; require hevc_nvenc "$encoders"; require h264_cuvid "$decoders"; require hevc_cuvid "$decoders" ;;
  linux-x64-vaapi-wayland)
    require vaapi "$hwaccels"; require scale_vaapi "$filters"; require cuda "$hwaccels"; require h264_nvenc "$encoders"; require hevc_nvenc "$encoders"; require h264_cuvid "$decoders"; require hevc_cuvid "$decoders" ;;
  *) echo "unsupported target: $target_id" >&2; exit 64 ;;
esac
