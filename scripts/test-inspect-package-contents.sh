#!/usr/bin/env bash
set -euo pipefail

root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT

make_tree() {
  local base="$1" binary="$2" require_nv="$3"
  mkdir -p "$base/ffmpeg/bin" "$base/ffmpeg/LICENSES" "$base/ffmpeg/provenance" "$base/ffmpeg/sbom"
  for file in \
    "NOTICE.txt" "ffmpeg/NOTICE.txt" "ffmpeg/bin/$binary" "ffmpeg/bin/ffprobe${binary#ffmpeg}" \
    "ffmpeg/LICENSES/FFmpeg-LGPL-2.1-or-later.txt" "ffmpeg/LICENSES/libvpx-BSD-3-Clause.txt" "ffmpeg/LICENSES/Opus-BSD-3-Clause.txt" \
    "ffmpeg/provenance/ffmpeg.lock" "ffmpeg/provenance/ffmpeg-8.1.2.tar.xz" "ffmpeg/provenance/ffmpeg-8.1.2.tar.xz.asc" \
    "ffmpeg/provenance/ffmpeg-signature-attestation.json" "ffmpeg/provenance/libvpx-source.tar.zst" "ffmpeg/provenance/opus-source.tar.zst" \
    "ffmpeg/provenance/buildconf.txt" "ffmpeg/provenance/changes.diff" "ffmpeg/provenance/SHA256SUMS" "ffmpeg/sbom/ffmpeg.cdx.json"; do
    printf 'fixture\n' > "$base/$file"
  done
  if [ "$require_nv" = true ]; then
    printf 'fixture\n' > "$base/ffmpeg/LICENSES/nv-codec-headers-MIT.txt"
    printf 'fixture\n' > "$base/ffmpeg/provenance/nv-codec-headers-source.tar.zst"
  fi
}

positive="$root/positive"
make_tree "$positive" ffmpeg true
printf 'binary\n' > "$positive/ovayra-spike"
scripts/inspect-package-contents.sh --root "$positive" --app "$positive/ovayra-spike" --ffmpeg-name ffmpeg --require-nv

missing="$root/missing"
make_tree "$missing" ffmpeg false
printf 'binary\n' > "$missing/ovayra-spike"
rm "$missing/ffmpeg/provenance/ffmpeg.lock"
if scripts/inspect-package-contents.sh --root "$missing" --app "$missing/ovayra-spike" --ffmpeg-name ffmpeg >/dev/null 2>&1; then
  echo 'missing required provenance unexpectedly passed' >&2
  exit 1
fi

symlinked="$root/symlinked"
make_tree "$symlinked" ffmpeg true
printf 'binary\n' > "$symlinked/ovayra-spike"
rm "$symlinked/ffmpeg/sbom/ffmpeg.cdx.json"
ln -s ../provenance/ffmpeg.lock "$symlinked/ffmpeg/sbom/ffmpeg.cdx.json"
if scripts/inspect-package-contents.sh --root "$symlinked" --app "$symlinked/ovayra-spike" --ffmpeg-name ffmpeg --require-nv >/dev/null 2>&1; then
  echo 'symlinked required resource unexpectedly passed' >&2
  exit 1
fi
