#!/usr/bin/env bash
set -euo pipefail
source_root= dependency_prefix= stage_root= parallelism=
while [[ $# -gt 0 ]]; do case "$1" in --source-root) source_root=$2; shift 2;; --dependency-prefix) dependency_prefix=$2; shift 2;; --stage-root) stage_root=$2; shift 2;; --parallelism) parallelism=$2; shift 2;; *) exit 64;; esac; done
[[ -n "$source_root" && -n "$dependency_prefix" && -n "$stage_root" && "$parallelism" =~ ^[1-9][0-9]*$ ]]
target_id=linux-x64-vaapi-wayland
[[ "${SOURCE_DATE_EPOCH:-}" == 1781663615 ]] || { echo 'SOURCE_DATE_EPOCH must equal locked FFmpeg n8.1.2 value' >&2; exit 64; }
marker="$stage_root/.ovayra-target"; [[ ! -e "$stage_root" || ( -f "$marker" && "$(<"$marker")" == "$target_id" ) ]] || exit 65
mkdir -p "$stage_root"/{provenance,LICENSES,sbom}; export SOURCE_DATE_EPOCH CFLAGS="${CFLAGS:-} -fdebug-prefix-map=$source_root=/usr/src/ovayra"
diff -ruN "$source_root/ffmpeg.pristine" "$source_root/ffmpeg" > "$stage_root/provenance/changes.diff" || [[ $? -eq 1 ]]
cd "$source_root/libvpx"; ./configure --prefix="$dependency_prefix" --disable-examples --disable-tools --enable-vp9-highbitdepth; make -j"$parallelism"; make test; make install
cd "$source_root/opus"; ./configure --prefix="$dependency_prefix" --disable-doc; make -j"$parallelism"; make check; make install
cd "$source_root/nv-codec-headers"; make PREFIX="$dependency_prefix" install
cd "$source_root/ffmpeg"; configure=(--prefix="$stage_root" --disable-autodetect --disable-debug --disable-doc --disable-ffplay --disable-network --enable-ffmpeg --enable-ffprobe --enable-libopus --enable-libvpx --enable-version3 --disable-gpl --disable-nonfree --enable-vaapi --enable-libdrm --enable-ffnvcodec --enable-nvenc --enable-nvdec --extra-cflags="-I$dependency_prefix/include" --extra-ldflags="-L$dependency_prefix/lib"); ./configure "${configure[@]}"; make -j"$parallelism"
fate_smoke_targets=(fate-lavf-mkv fate-filter-testsrc2-yuv420p fate-filter-aloop)
available_fate_targets=$(make fate-list | tr -d '\r')
for fate_target in "${fate_smoke_targets[@]}"; do
  grep -Fqx -- "$fate_target" <<< "$available_fate_targets" || { echo "required FATE smoke target unavailable: $fate_target" >&2; exit 66; }
done
make "${fate_smoke_targets[@]}"; make install
{ printf 'configuration: '; printf '%q ' "${configure[@]}"; printf '\n'; } > "$stage_root/provenance/buildconf.txt"
cp "$source_root"/{ffmpeg-8.1.2.tar.xz,ffmpeg-8.1.2.tar.xz.asc,libvpx-source.tar.zst,opus-source.tar.zst,nv-codec-headers-source.tar.zst,ffmpeg-signature-attestation.json} "$stage_root/provenance/"; cp "$(dirname "$0")/../packaging/ffmpeg.lock" "$stage_root/provenance/ffmpeg.lock"
cp "$source_root/ffmpeg/COPYING.LGPLv2.1" "$stage_root/LICENSES/FFmpeg-LGPL-2.1-or-later.txt"; cp "$source_root/libvpx/LICENSE" "$stage_root/LICENSES/libvpx-BSD-3-Clause.txt"; cp "$source_root/opus/COPYING" "$stage_root/LICENSES/Opus-BSD-3-Clause.txt"; cp "$source_root/nv-codec-headers/LICENSE" "$stage_root/LICENSES/nv-codec-headers-MIT.txt"; cp "$(dirname "$0")/../packaging/NOTICE.txt" "$stage_root/NOTICE.txt"
ff=$(sha256sum "$stage_root/provenance/ffmpeg-8.1.2.tar.xz" | awk '{print $1}'); vx=$(sha256sum "$stage_root/provenance/libvpx-source.tar.zst" | awk '{print $1}'); op=$(sha256sum "$stage_root/provenance/opus-source.tar.zst" | awk '{print $1}')
printf '{"bomFormat":"CycloneDX","specVersion":"1.5","components":[{"name":"FFmpeg","version":"8.1.2","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"LGPL-2.1-or-later"}}]},{"name":"libvpx","version":"1.16.0","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]},{"name":"opus","version":"1.6.1","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]}]}' "$ff" "$vx" "$op" > "$stage_root/sbom/ffmpeg.cdx.json"
printf '%s\n' "$target_id" > "$marker"; (cd "$stage_root" && find . -type f ! -path './provenance/SHA256SUMS' -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sed 's#  \./#  #' > provenance/SHA256SUMS)
