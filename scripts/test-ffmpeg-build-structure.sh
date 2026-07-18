#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
rg -F -- 'packaging/ffmpeg.lock text eol=lf' "$root/.gitattributes" >/dev/null
windows_ps1="$root/scripts/build-ffmpeg-windows.ps1"
windows_bash="$root/scripts/build-ffmpeg-windows-msys.sh"
workflow="$root/.github/workflows/phase-0-ffmpeg.yml"
for requirement in 'vswhere.exe' 'VsDevCmd.bat' 'cl.exe' 'link.exe' 'lib.exe' 'cmake.exe' 'ninja.exe' 'MSYS2_LOCATION' 'OVAYRA_MSVC_BIN' 'OVAYRA_MSYS_BIN' 'OVAYRA_NATIVE_CMAKE' 'OVAYRA_NATIVE_NINJA' 'Remove-Item -Path "Env:$name"'; do rg -F -- "$requirement" "$windows_ps1" >/dev/null; done
for requirement in 'PATH="/usr/bin:/ucrt64/bin:$PATH"' 'OVAYRA_MSYS_BIN' 'OVAYRA_NATIVE_CMAKE' 'OVAYRA_NATIVE_NINJA' 'make_cmd="$msys_bin/make.exe"' '"$make_cmd" -j' 'MSYS GNU make' 'OVAYRA_MSVC_BIN' 'PATH="$msvc_bin:$PATH"' 'env -u CC -u CXX -u AR -u LD ./configure' '--target=x86_64-win64-vs17' 'vpxmd.lib' 'PKG_CONFIG_LIBDIR' 'tail -n 200 ffbuild/config.log' '"$cmake_cmd" -S' 'CMAKE_MAKE_PROGRAM="$ninja_win"' 'CMAKE_C_COMPILER=cl' 'CMAKE_CXX_COMPILER=cl' '--toolchain=msvc' '--ar="lib.exe -Brepro"' '--extra-ldflags="-Brepro ' '--extra-cflags="-MD -Brepro ' '--enable-ffnvcodec' '--enable-dxva2' 'changes.diff' '.ovayra-target'; do
  rg -F -- "$requirement" "$windows_bash" >/dev/null
done
rg -F -- '--enable-ffnvcodec' "$root/scripts/build-ffmpeg-linux.sh" >/dev/null
nv_license="$root/packaging/licenses/nv-codec-headers-MIT.txt"
test -s "$nv_license"
for script in "$root/scripts/build-ffmpeg-linux.sh" "$windows_bash"; do
  rg -F -- 'packaging/licenses/nv-codec-headers-MIT.txt' "$script" >/dev/null
  if rg -F -- 'nv-codec-headers/LICENSE' "$script"; then echo 'nv-codec-headers has no standalone LICENSE file at the pinned tag' >&2; exit 1; fi
done
if rg -F -- '--host=x86_64-w64-mingw32' "$windows_bash"; then echo 'MinGW Opus target is forbidden' >&2; exit 1; fi
for requirement in 'id: msys2' 'steps.msys2.outputs.msys2-location' 'ffmpeg-stable' '$ErrorActionPreference = '\''Stop'\'''; do rg -F -- "$requirement" "$workflow" >/dev/null; done
rg -F -- 'libnuma-dev cmake' "$workflow" >/dev/null
rg -F -- 'brew install nasm pkg-config zstd cmake' "$workflow" >/dev/null
rg -F -- 'compare-ffmpeg-reproducibility.sh target/ffmpeg-a-stage target/ffmpeg-b-stage' "$workflow" >/dev/null
for script in scripts/build-ffmpeg-linux.sh scripts/build-ffmpeg-macos.sh; do
  for requirement in 'script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)' 'repo_root=$(cd "$script_dir/.." && pwd)' 'cmake_cmd=$(command -v cmake || true)' 'opus-build' 'OPUS_BUILD_TESTING=OFF' 'OPUS_BUILD_PROGRAMS=OFF' 'CMAKE_POSITION_INDEPENDENT_CODE=ON' '"$cmake_cmd" --build' '"$cmake_cmd" --install' '--pkg-config-flags=--static' 'PKG_CONFIG_LIBDIR="$pkg_config_dir' 'tail -n 200 ffbuild/config.log'; do
    rg -F --quiet -- "$requirement" "$script"
  done
  if rg -F -- 'make test' "$script" || rg -F -- 'cd "$source_root/opus"; ./configure' "$script" || rg -F -- '$(dirname "$0")/../packaging' "$script"; then
    echo 'POSIX dependency builds must use bounded runtime validation, CMake Opus sources, and stable repository paths' >&2
    exit 1
  fi
done
rg -F --quiet -- 'system_pkg_config_dirs=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig' scripts/build-ffmpeg-linux.sh
rg -F --quiet -- 'PKG_CONFIG_LIBDIR="$pkg_config_dir:$system_pkg_config_dirs"' scripts/build-ffmpeg-linux.sh
rg -F --quiet -- 'export SOURCE_DATE_EPOCH ZERO_AR_DATE=1' scripts/build-ffmpeg-macos.sh
for script in scripts/build-ffmpeg-linux.sh scripts/build-ffmpeg-macos.sh scripts/build-ffmpeg-windows-msys.sh; do
  rg -F --quiet '{ printf '\''configuration: '\''' "$script"
  rg -F --quiet '} > "$stage_root/provenance/buildconf.txt"' "$script"
  rg -F --quiet "sed 's# [* ]\\./#  #'" "$script"
  for fate_target in fate-lavf-mkv fate-filter-testsrc2-yuv420p fate-filter-aloop; do
    rg -F --quiet "$fate_target" "$script"
  done
done
for requirement in '"scripts/**"' '"crates/**"' 'target/ffmpeg-a-stage/**' 'target/ffmpeg-b-stage/**' 'target/ffmpeg-a-cpu-evidence.json' 'target/ffmpeg-b-cpu-evidence.json' 'if-no-files-found: error'; do
  rg -F -- "$requirement" "$workflow" >/dev/null
done
