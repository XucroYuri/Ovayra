#!/usr/bin/env bash
# This script deliberately runs in MSYS2 only as a POSIX build shell. cl/link/lib remain MSVC.
set -euo pipefail
# MSYS2 supplies the POSIX tools, while compiler selection below remains MSVC.
[[ -d /ucrt64/bin ]] && PATH="/ucrt64/bin:$PATH"
source_root= dependency_prefix= stage_root= parallelism=
while [[ $# -gt 0 ]]; do case "$1" in
  --source-root) source_root=$2; shift 2;; --dependency-prefix) dependency_prefix=$2; shift 2;;
  --stage-root) stage_root=$2; shift 2;; --parallelism) parallelism=$2; shift 2;; *) exit 64;; esac; done
[[ -n "$source_root" && -n "$dependency_prefix" && -n "$stage_root" && "$parallelism" =~ ^[1-9][0-9]*$ ]]
[[ "${SOURCE_DATE_EPOCH:-}" == 1781663615 && "${CC:-}" == cl && "${CXX:-}" == cl && "${AR:-}" == lib && "${LD:-}" == link && -n "${OVAYRA_MSVC_BIN:-}" ]] || { echo 'locked epoch or MSVC tool environment missing' >&2; exit 65; }
msvc_bin=$(cygpath -u "$OVAYRA_MSVC_BIN")
[[ -d "$msvc_bin" ]] || { echo 'MSVC binary directory is unavailable to MSYS2' >&2; exit 65; }
# MSYS2 also ships /usr/bin/link.exe. Keep the Visual Studio directory first so
# libvpx and FFmpeg invoke the MSVC linker selected by VsDevCmd.
PATH="$msvc_bin:$PATH"
hash -r
for tool in cl link lib nasm perl make cmake ninja cygpath sha256sum diff; do command -v "$tool" >/dev/null || { echo "required Windows build tool missing: $tool" >&2; exit 65; }; done
for tool in cl link lib; do
  resolved=$(command -v "$tool")
  [[ "$resolved" == "$msvc_bin/$tool" || "$resolved" == "$msvc_bin/$tool.exe" ]] || { echo "MSYS2 resolved a non-MSVC $tool executable" >&2; exit 65; }
done
source_root=$(cygpath -u "$source_root"); dependency_prefix=$(cygpath -u "$dependency_prefix"); stage_root=$(cygpath -u "$stage_root")
target_id=windows-x64-mf
marker="$stage_root/.ovayra-target"
[[ ! -e "$stage_root" || ( -f "$marker" && "$(<"$marker")" == "$target_id" ) ]] || { echo 'refusing cross-target stage overwrite' >&2; exit 65; }
mkdir -p "$dependency_prefix" "$stage_root"/{provenance,LICENSES,sbom}
# Capture only intentional patch delta before configure or generated build files can pollute it.
diff -ruN "$source_root/ffmpeg.pristine" "$source_root/ffmpeg" > "$stage_root/provenance/changes.diff" || [[ $? -eq 1 ]]
cd "$source_root/libvpx"
./configure --target=x86_64-win64-vs17 --prefix="$(cygpath -m "$dependency_prefix")" --disable-examples --disable-tools --enable-vp9-highbitdepth
make -j"$parallelism"; make install
cmake -S "$source_root/opus" -B "$source_root/opus-msvc" -G Ninja -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl -DCMAKE_INSTALL_PREFIX="$(cygpath -m "$dependency_prefix")"
cmake --build "$source_root/opus-msvc" --parallel "$parallelism"; cmake --install "$source_root/opus-msvc"
test -f "$dependency_prefix/include/opus/opus.h"
cd "$source_root/nv-codec-headers"; make PREFIX="$(cygpath -m "$dependency_prefix")" install
test -f "$dependency_prefix/include/ffnvcodec/nvEncodeAPI.h"
cd "$source_root/ffmpeg"
prefix_win=$(cygpath -m "$dependency_prefix")
configure=(--prefix="$(cygpath -m "$stage_root")" --toolchain=msvc --target-os=win32 --arch=x86_64 --disable-autodetect --disable-debug --disable-doc --disable-ffplay --disable-network --enable-ffmpeg --enable-ffprobe --enable-libopus --enable-libvpx --enable-version3 --disable-gpl --disable-nonfree --enable-d3d11va --enable-dxva2 --enable-mediafoundation --enable-nvenc --enable-nvdec --extra-cflags="-I$prefix_win/include" --extra-ldflags="-LIBPATH:$prefix_win/lib" --extra-libs="opus.lib vpx.lib")
./configure "${configure[@]}"; make -j"$parallelism"; fate_targets=$(make fate-list | grep -E '^fate-(lavf-matroska|vp9|opus)' | head -n 3 || true); [[ -n "$fate_targets" ]] || exit 66; set -- $fate_targets; make "$@"; make install
test -f "$stage_root/bin/ffmpeg.exe" && test -f "$stage_root/bin/ffprobe.exe"
{ printf 'configuration: '; printf '%q ' "${configure[@]}"; printf '\n'; } > "$stage_root/provenance/buildconf.txt"
cp "$source_root"/{ffmpeg-8.1.2.tar.xz,ffmpeg-8.1.2.tar.xz.asc,libvpx-source.tar.zst,opus-source.tar.zst,nv-codec-headers-source.tar.zst,ffmpeg-signature-attestation.json} "$stage_root/provenance/"
cp "$(dirname "$0")/../packaging/ffmpeg.lock" "$stage_root/provenance/ffmpeg.lock"
cp "$source_root/ffmpeg/COPYING.LGPLv2.1" "$stage_root/LICENSES/FFmpeg-LGPL-2.1-or-later.txt"; cp "$source_root/libvpx/LICENSE" "$stage_root/LICENSES/libvpx-BSD-3-Clause.txt"; cp "$source_root/opus/COPYING" "$stage_root/LICENSES/Opus-BSD-3-Clause.txt"; cp "$source_root/nv-codec-headers/LICENSE" "$stage_root/LICENSES/nv-codec-headers-MIT.txt"; cp "$(dirname "$0")/../packaging/NOTICE.txt" "$stage_root/NOTICE.txt"
ff=$(sha256sum "$stage_root/provenance/ffmpeg-8.1.2.tar.xz" | awk '{print $1}'); vx=$(sha256sum "$stage_root/provenance/libvpx-source.tar.zst" | awk '{print $1}'); op=$(sha256sum "$stage_root/provenance/opus-source.tar.zst" | awk '{print $1}')
printf '{"bomFormat":"CycloneDX","specVersion":"1.5","components":[{"name":"FFmpeg","version":"8.1.2","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"LGPL-2.1-or-later"}}]},{"name":"libvpx","version":"1.16.0","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]},{"name":"opus","version":"1.6.1","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]}]}' "$ff" "$vx" "$op" > "$stage_root/sbom/ffmpeg.cdx.json"
printf '%s\n' "$target_id" > "$marker"
(cd "$stage_root" && find . -type f ! -path './provenance/SHA256SUMS' -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sed 's#  \./#  #' > provenance/SHA256SUMS)
