#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
windows_ps1="$root/scripts/build-ffmpeg-windows.ps1"
windows_bash="$root/scripts/build-ffmpeg-windows-msys.sh"
workflow="$root/.github/workflows/phase-0-ffmpeg.yml"
for requirement in 'vswhere.exe' 'VsDevCmd.bat' 'cl.exe' 'link.exe' 'lib.exe' 'CC = '\''cl'\''' 'CXX = '\''cl'\''' 'AR = '\''lib'\''' 'LD = '\''link'\'''; do rg -F -- "$requirement" "$windows_ps1" >/dev/null; done
for requirement in '--target=x86_64-win64-vs17' 'CMAKE_C_COMPILER=cl' 'CMAKE_CXX_COMPILER=cl' '--toolchain=msvc' '--enable-dxva2' 'changes.diff' '.ovayra-target'; do
  rg -F -- "$requirement" "$windows_bash" >/dev/null
done
if rg -F -- '--host=x86_64-w64-mingw32' "$windows_bash"; then echo 'MinGW Opus target is forbidden' >&2; exit 1; fi
rg -F -- 'ffmpeg-stable' "$workflow" >/dev/null
rg -F -- 'compare-ffmpeg-reproducibility.sh target/ffmpeg-a-stage target/ffmpeg-b-stage' "$workflow" >/dev/null
