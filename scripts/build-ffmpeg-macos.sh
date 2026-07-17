#!/usr/bin/env bash
set -euo pipefail

source_root= dependency_prefix= stage_root= parallelism=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --source-root) source_root=$2; shift 2 ;;
    --dependency-prefix) dependency_prefix=$2; shift 2 ;;
    --stage-root) stage_root=$2; shift 2 ;;
    --parallelism) parallelism=$2; shift 2 ;;
    *) echo "usage: $0 --source-root DIR --dependency-prefix DIR --stage-root DIR --parallelism N" >&2; exit 64 ;;
  esac
done
[[ -n "$source_root" && -n "$dependency_prefix" && -n "$stage_root" && -n "$parallelism" ]]
target_triple=aarch64-apple-darwin
[[ -n "${SOURCE_DATE_EPOCH:-}" ]] || { echo 'SOURCE_DATE_EPOCH must be set from FFmpeg n8.1.2' >&2; exit 64; }
marker="$stage_root/.ovayra-target"
if [[ -e "$stage_root" && (! -f "$marker" || "$(<"$marker")" != "$target_triple") ]]; then echo "refusing cross-target stage overwrite" >&2; exit 65; fi
mkdir -p "$stage_root" "$stage_root/provenance" "$stage_root/LICENSES" "$stage_root/sbom"
printf '%s\n' "$target_triple" > "$marker"
export SOURCE_DATE_EPOCH CFLAGS="${CFLAGS:-} -fdebug-prefix-map=$source_root=/usr/src/ovayra" LDFLAGS="${LDFLAGS:-}"
cd "$source_root/libvpx"; ./configure --prefix="$dependency_prefix" --disable-examples --disable-tools --enable-vp9-highbitdepth; make -j"$parallelism"; make test; make install
cd "$source_root/opus"; ./configure --prefix="$dependency_prefix" --disable-doc; make -j"$parallelism"; make check; make install
cd "$source_root/ffmpeg"
configure=(--prefix="$stage_root" --disable-autodetect --disable-debug --disable-doc --disable-ffplay --disable-network --enable-ffmpeg --enable-ffprobe --enable-libopus --enable-libvpx --enable-version3 --disable-gpl --disable-nonfree --enable-videotoolbox --enable-audiotoolbox --extra-cflags="-I$dependency_prefix/include" --extra-ldflags="-L$dependency_prefix/lib")
printf 'configuration: '; printf '%q ' "${configure[@]}"; printf '\n' > "$stage_root/provenance/buildconf.txt"
PKG_CONFIG_PATH="$dependency_prefix/lib/pkgconfig" ./configure "${configure[@]}"; make -j"$parallelism"
fate_targets=$(make fate-list | grep -E '^fate-(lavf-matroska|vp9|opus)' | head -n 3 || true); [[ -n "$fate_targets" ]] || { echo 'required FATE smoke targets unavailable' >&2; exit 66; }; set -- $fate_targets; make "$@"; make install
cp "$source_root/ffmpeg-8.1.2.tar.xz" "$source_root/ffmpeg-8.1.2.tar.xz.asc" "$stage_root/provenance/"
cp "$source_root/libvpx-source.tar.zst" "$source_root/opus-source.tar.zst" "$stage_root/provenance/"
diff -ruN "$source_root/ffmpeg.pristine" "$source_root/ffmpeg" > "$stage_root/provenance/changes.diff" || [[ $? -eq 1 ]]
cp "$source_root/ffmpeg/COPYING.LGPLv2.1" "$stage_root/LICENSES/FFmpeg-LGPL-2.1-or-later.txt"; cp "$source_root/libvpx/LICENSE" "$stage_root/LICENSES/libvpx-BSD-3-Clause.txt"; cp "$source_root/opus/COPYING" "$stage_root/LICENSES/Opus-BSD-3-Clause.txt"
cp "$(dirname "$0")/../packaging/NOTICE.txt" "$stage_root/NOTICE.txt"
cp "$(dirname "$0")/../packaging/ffmpeg.lock" "$stage_root/provenance/ffmpeg.lock"; cp "$source_root/ffmpeg-signature-attestation.json" "$stage_root/provenance/"
ffmpeg_hash=$(shasum -a 256 "$stage_root/provenance/ffmpeg-8.1.2.tar.xz" | awk '{print $1}'); libvpx_hash=$(shasum -a 256 "$stage_root/provenance/libvpx-source.tar.zst" | awk '{print $1}'); opus_hash=$(shasum -a 256 "$stage_root/provenance/opus-source.tar.zst" | awk '{print $1}')
printf '{"bomFormat":"CycloneDX","specVersion":"1.5","components":[{"name":"FFmpeg","version":"8.1.2","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"LGPL-2.1-or-later"}}]},{"name":"libvpx","version":"1.16.0","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]},{"name":"opus","version":"1.6.1","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]}]}' "$ffmpeg_hash" "$libvpx_hash" "$opus_hash" > "$stage_root/sbom/ffmpeg.cdx.json"
(cd "$stage_root" && find bin provenance LICENSES NOTICE.txt sbom -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 shasum -a 256 > provenance/SHA256SUMS)
