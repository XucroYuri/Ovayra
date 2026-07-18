#!/usr/bin/env bash
# This script deliberately runs in MSYS2 only as a POSIX build shell. cl/link/lib remain MSVC.
set -euo pipefail
# MSYS2 supplies the POSIX tools, while compiler selection below remains MSVC.
# Keep /usr/bin first so recursive GNU make understands MSYS paths such as /d/.
[[ -d /ucrt64/bin ]] && PATH="/usr/bin:/ucrt64/bin:$PATH"
source_root= dependency_prefix= stage_root= parallelism=
while [[ $# -gt 0 ]]; do case "$1" in
  --source-root) source_root=$2; shift 2;; --dependency-prefix) dependency_prefix=$2; shift 2;;
  --stage-root) stage_root=$2; shift 2;; --parallelism) parallelism=$2; shift 2;; *) exit 64;; esac; done
[[ -n "$source_root" && -n "$dependency_prefix" && -n "$stage_root" && "$parallelism" =~ ^[1-9][0-9]*$ ]]
[[ "${SOURCE_DATE_EPOCH:-}" == 1781663615 && -z "${CC:-}${CXX:-}${AR:-}${LD:-}" && -n "${OVAYRA_MSVC_BIN:-}" && -n "${OVAYRA_MSYS_BIN:-}" && -n "${OVAYRA_NATIVE_CMAKE:-}" && -n "${OVAYRA_NATIVE_NINJA:-}" ]] || { echo 'locked epoch or Windows tool environment missing' >&2; exit 65; }
msvc_bin=$(cygpath -u "$OVAYRA_MSVC_BIN")
[[ -d "$msvc_bin" ]] || { echo 'MSVC binary directory is unavailable to MSYS2' >&2; exit 65; }
# MSYS2 also ships /usr/bin/link.exe. Keep the Visual Studio directory first so
# libvpx and FFmpeg invoke the MSVC linker selected by VsDevCmd.
PATH="$msvc_bin:$PATH"
hash -r
for tool in cl link lib nasm perl cygpath sha256sum diff; do command -v "$tool" >/dev/null || { echo "required Windows build tool missing: $tool" >&2; exit 65; }; done
msys_bin=$(cygpath -u "$OVAYRA_MSYS_BIN")
make_cmd="$msys_bin/make.exe"
[[ -x "$make_cmd" ]] || { echo 'MSYS GNU make must drive Visual Studio project generation' >&2; exit 65; }
cmake_cmd=$(cygpath -u "$OVAYRA_NATIVE_CMAKE")
ninja_win=$(cygpath -m "$OVAYRA_NATIVE_NINJA")
[[ -x "$cmake_cmd" ]] || { echo 'native Windows CMake is unavailable to MSYS2' >&2; exit 65; }
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
# libvpx's Visual Studio target is an external-build generator. Leaving CC set
# makes its GNU-style probe pass `-o` flags directly to cl.exe before the
# generated MSBuild project exists.
env -u CC -u CXX -u AR -u LD ./configure --target=x86_64-win64-vs17 --prefix="$(cygpath -m "$dependency_prefix")" --disable-examples --disable-tools --enable-vp9-highbitdepth
"$make_cmd" -j"$parallelism"; "$make_cmd" install
test -f "$dependency_prefix/lib/x64/vpxmd.lib"
cp "$dependency_prefix/lib/x64/vpxmd.lib" "$dependency_prefix/lib/vpx.lib"
opus_source_win=$(cygpath -m "$source_root/opus"); opus_build_win=$(cygpath -m "$source_root/opus-msvc")
"$cmake_cmd" -S "$opus_source_win" -B "$opus_build_win" -G Ninja -DCMAKE_MAKE_PROGRAM="$ninja_win" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl -DCMAKE_INSTALL_PREFIX="$(cygpath -m "$dependency_prefix")"
"$cmake_cmd" --build "$opus_build_win" --parallel "$parallelism"; "$cmake_cmd" --install "$opus_build_win"
test -f "$dependency_prefix/include/opus/opus.h"
cd "$source_root/nv-codec-headers"; "$make_cmd" PREFIX="$(cygpath -m "$dependency_prefix")" install
test -f "$dependency_prefix/include/ffnvcodec/nvEncodeAPI.h"
cd "$source_root/ffmpeg"
prefix_win=$(cygpath -m "$dependency_prefix")
# MSVC accepts '-' as the option prefix. Use that spelling so MSYS2 does not
# rewrite /Brepro as a filesystem path before the compiler toolchain sees it.
configure=(--prefix="$(cygpath -m "$stage_root")" --toolchain=msvc --target-os=win32 --arch=x86_64 --ar="lib.exe -Brepro" --disable-autodetect --disable-debug --disable-doc --disable-ffplay --disable-network --enable-ffmpeg --enable-ffprobe --enable-libopus --enable-libvpx --enable-version3 --disable-gpl --disable-nonfree --enable-d3d11va --enable-dxva2 --enable-mediafoundation --enable-ffnvcodec --enable-nvenc --enable-nvdec --extra-cflags="-MD -Brepro -I$prefix_win/include" --extra-ldflags="-Brepro -LIBPATH:$prefix_win/lib" --extra-libs="opus.lib vpx.lib")
export PKG_CONFIG_PATH="$dependency_prefix/lib/pkgconfig"
export PKG_CONFIG_LIBDIR="$PKG_CONFIG_PATH"
if ! ./configure "${configure[@]}"; then
  tail -n 200 ffbuild/config.log >&2 || true
  exit 1
fi
"$make_cmd" -j"$parallelism"
fate_smoke_targets=(fate-lavf-mkv fate-filter-testsrc2-yuv420p fate-filter-aloop)
available_fate_targets=$("$make_cmd" fate-list | tr -d '\r')
for fate_target in "${fate_smoke_targets[@]}"; do
  grep -Fqx -- "$fate_target" <<< "$available_fate_targets" || { echo "required FATE smoke target unavailable: $fate_target" >&2; exit 66; }
done
"$make_cmd" "${fate_smoke_targets[@]}"; "$make_cmd" install
test -f "$stage_root/bin/ffmpeg.exe" && test -f "$stage_root/bin/ffprobe.exe"
{ printf 'configuration: '; printf '%q ' "${configure[@]}"; printf '\n'; } > "$stage_root/provenance/buildconf.txt"
cp "$source_root"/{ffmpeg-8.1.2.tar.xz,ffmpeg-8.1.2.tar.xz.asc,libvpx-source.tar.zst,opus-source.tar.zst,nv-codec-headers-source.tar.zst,ffmpeg-signature-attestation.json} "$stage_root/provenance/"
cp "$(dirname "$0")/../packaging/ffmpeg.lock" "$stage_root/provenance/ffmpeg.lock"
cp "$source_root/ffmpeg/COPYING.LGPLv2.1" "$stage_root/LICENSES/FFmpeg-LGPL-2.1-or-later.txt"; cp "$source_root/libvpx/LICENSE" "$stage_root/LICENSES/libvpx-BSD-3-Clause.txt"; cp "$source_root/opus/COPYING" "$stage_root/LICENSES/Opus-BSD-3-Clause.txt"; cp "$(dirname "$0")/../packaging/licenses/nv-codec-headers-MIT.txt" "$stage_root/LICENSES/nv-codec-headers-MIT.txt"; cp "$(dirname "$0")/../packaging/NOTICE.txt" "$stage_root/NOTICE.txt"
ff=$(sha256sum "$stage_root/provenance/ffmpeg-8.1.2.tar.xz" | awk '{print $1}'); vx=$(sha256sum "$stage_root/provenance/libvpx-source.tar.zst" | awk '{print $1}'); op=$(sha256sum "$stage_root/provenance/opus-source.tar.zst" | awk '{print $1}')
printf '{"bomFormat":"CycloneDX","specVersion":"1.5","components":[{"name":"FFmpeg","version":"8.1.2","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"LGPL-2.1-or-later"}}]},{"name":"libvpx","version":"1.16.0","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]},{"name":"opus","version":"1.6.1","hashes":[{"alg":"SHA-256","content":"%s"}],"licenses":[{"license":{"id":"BSD-3-Clause"}}]}]}' "$ff" "$vx" "$op" > "$stage_root/sbom/ffmpeg.cdx.json"
printf '%s\n' "$target_id" > "$marker"
(cd "$stage_root" && find . -type f ! -path './provenance/SHA256SUMS' -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sed 's# [* ]\./#  #' > provenance/SHA256SUMS)
