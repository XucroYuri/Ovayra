#!/usr/bin/env bash
set -euo pipefail

for workflow in .github/workflows/phase-0-ci.yml .github/workflows/phase-0-device.yml; do
  if rg -F 'release verify-ffmpeg' "$workflow" | rg -F 'docs/phase-0/evidence' >/dev/null; then
    echo "generic FFmpeg evidence must not enter the gate directory" >&2
    exit 1
  fi
done

rg -F --quiet 'release verify-ffmpeg-pair' .github/workflows/phase-0-ffmpeg.yml
rg -F --quiet 'ffmpeg-repro-' .github/workflows/phase-0-ffmpeg.yml
rg -F --quiet 'release prove-package' .github/workflows/phase-0-release.yml
rg -F --quiet 'release prove-update' .github/workflows/phase-0-release.yml
rg -F --quiet 'gemini-3.1-flash-lite' .github/workflows/phase-0-device.yml
rg -F --quiet 'platform checkpoint --evidence' .github/workflows/phase-0-device.yml
rg -F --quiet 'synthetic-h264-aac.mp4' .github/workflows/phase-0-device.yml
for target in windows-x64-nvidia linux-x64-vaapi-x11 linux-x64-nvidia; do
  rg -F --quiet "target_id: $target" .github/workflows/phase-0-device.yml
done
