#!/usr/bin/env bash
# Validates the exact regular-file resource contract after a native package is extracted.
set -euo pipefail

root=''
app=''
ffmpeg_name=''
require_nv=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --root) root="$2"; shift 2 ;;
    --app) app="$2"; shift 2 ;;
    --ffmpeg-name) ffmpeg_name="$2"; shift 2 ;;
    --require-nv) require_nv=true; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

test -n "$root" && test -d "$root" || { echo 'resource root is required' >&2; exit 2; }
test -n "$app" || { echo 'application executable is required' >&2; exit 2; }
test -n "$ffmpeg_name" || { echo 'ffmpeg executable name is required' >&2; exit 2; }

require_regular() {
  local relative="$1" path="$root/$relative"
  test -f "$path" && test ! -L "$path" && test -s "$path" || {
    echo "missing, empty, or non-regular required resource: $relative" >&2
    exit 1
  }
}

test -f "$app" && test ! -L "$app" && test -s "$app" || {
  echo "application executable is missing, empty, or non-regular: $app" >&2
  exit 1
}

ffprobe_name="ffprobe${ffmpeg_name#ffmpeg}"
for relative in \
  NOTICE.txt \
  "ffmpeg/bin/$ffmpeg_name" "ffmpeg/bin/$ffprobe_name" "ffmpeg/NOTICE.txt" \
  ffmpeg/LICENSES/FFmpeg-LGPL-2.1-or-later.txt \
  ffmpeg/LICENSES/libvpx-BSD-3-Clause.txt \
  ffmpeg/LICENSES/Opus-BSD-3-Clause.txt \
  ffmpeg/provenance/ffmpeg.lock \
  ffmpeg/provenance/ffmpeg-8.1.2.tar.xz \
  ffmpeg/provenance/ffmpeg-8.1.2.tar.xz.asc \
  ffmpeg/provenance/ffmpeg-signature-attestation.json \
  ffmpeg/provenance/libvpx-source.tar.zst \
  ffmpeg/provenance/opus-source.tar.zst \
  ffmpeg/provenance/buildconf.txt \
  ffmpeg/provenance/changes.diff \
  ffmpeg/provenance/SHA256SUMS \
  ffmpeg/sbom/ffmpeg.cdx.json; do
  require_regular "$relative"
done

if "$require_nv"; then
  require_regular ffmpeg/LICENSES/nv-codec-headers-MIT.txt
  require_regular ffmpeg/provenance/nv-codec-headers-source.tar.zst
fi
