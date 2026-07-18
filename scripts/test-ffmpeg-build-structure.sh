#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
windows_ps1="$root/scripts/build-ffmpeg-windows.ps1"
windows_bash="$root/scripts/build-ffmpeg-windows-msys.sh"
workflow="$root/.github/workflows/phase-0-ffmpeg.yml"
for requirement in 'vswhere.exe' 'VsDevCmd.bat' 'cl.exe' 'link.exe' 'lib.exe' 'cmake.exe' 'ninja.exe' 'MSYS2_LOCATION' 'OVAYRA_MSVC_BIN' 'OVAYRA_MSYS_BIN' 'OVAYRA_NATIVE_CMAKE' 'OVAYRA_NATIVE_NINJA' 'Remove-Item -Path "Env:$name"'; do rg -F -- "$requirement" "$windows_ps1" >/dev/null; done
for requirement in 'PATH="/usr/bin:/ucrt64/bin:$PATH"' 'OVAYRA_MSYS_BIN' 'OVAYRA_NATIVE_CMAKE' 'OVAYRA_NATIVE_NINJA' 'make_cmd="$msys_bin/make.exe"' '"$make_cmd" -j' 'MSYS GNU make' 'OVAYRA_MSVC_BIN' 'PATH="$msvc_bin:$PATH"' 'env -u CC -u CXX -u AR -u LD ./configure' '--target=x86_64-win64-vs17' 'vpxmd.lib' 'PKG_CONFIG_LIBDIR' 'tail -n 200 ffbuild/config.log' '"$cmake_cmd" -S' 'CMAKE_MAKE_PROGRAM="$ninja_win"' 'CMAKE_C_COMPILER=cl' 'CMAKE_CXX_COMPILER=cl' '--toolchain=msvc' '--extra-cflags="-MD ' '--enable-ffnvcodec' '--enable-dxva2' 'changes.diff' '.ovayra-target'; do
  rg -F -- "$requirement" "$windows_bash" >/dev/null
done
rg -F -- '--enable-ffnvcodec' "$root/scripts/build-ffmpeg-linux.sh" >/dev/null
if rg -F -- '--host=x86_64-w64-mingw32' "$windows_bash"; then echo 'MinGW Opus target is forbidden' >&2; exit 1; fi
for requirement in 'id: msys2' 'steps.msys2.outputs.msys2-location' 'ffmpeg-stable'; do rg -F -- "$requirement" "$workflow" >/dev/null; done
rg -F -- 'compare-ffmpeg-reproducibility.sh target/ffmpeg-a-stage target/ffmpeg-b-stage' "$workflow" >/dev/null
for script in scripts/build-ffmpeg-linux.sh scripts/build-ffmpeg-macos.sh scripts/build-ffmpeg-windows-msys.sh; do
  rg -F --quiet '{ printf '\''configuration: '\''' "$script"
  rg -F --quiet '} > "$stage_root/provenance/buildconf.txt"' "$script"
done
for requirement in '"scripts/**"' '"crates/**"' 'target/ffmpeg-a-stage/**' 'target/ffmpeg-b-stage/**' 'target/ffmpeg-a-cpu-evidence.json' 'target/ffmpeg-b-cpu-evidence.json' 'if-no-files-found: error'; do
  rg -F -- "$requirement" "$workflow" >/dev/null
done
