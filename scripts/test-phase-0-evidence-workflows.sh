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
rg -F --quiet 'media generate-hardware-fixture' .github/workflows/phase-0-device.yml
if rg -F --quiet 'libx264' .github/workflows/phase-0-device.yml; then
  echo 'protected device workflow must not require libx264' >&2
  exit 1
fi

gemini_matrix_targets() {
  awk '
    /^  gemini:$/ { in_gemini = 1; next }
    in_gemini && /^  [[:alnum:]_-]+:$/ { exit }
    in_gemini { print }
  ' | sed -n 's/^[[:space:]]*- { target_id: \([^,}]*\).*/\1/p' | sort | paste -sd, -
}

gemini_targets="$(gemini_matrix_targets < .github/workflows/phase-0-device.yml)"
expected_gemini_targets='linux-x64-nvidia,linux-x64-vaapi-wayland,linux-x64-vaapi-x11,macos-arm64-vt,windows-x64-mf,windows-x64-nvidia'
if [ "$gemini_targets" != "$expected_gemini_targets" ]; then
  echo "Gemini matrix targets drifted: $gemini_targets" >&2
  exit 1
fi

# Simulate the historical three-target Gemini matrix without changing the core job. A global
# target search would still see all six core entries and incorrectly pass this regression check.
historical_gemini_targets="$(awk '
  /^  gemini:$/ { in_gemini = 1 }
  in_gemini && /target_id: (windows-x64-nvidia|linux-x64-vaapi-x11|linux-x64-nvidia)/ { next }
  { print }
' .github/workflows/phase-0-device.yml | gemini_matrix_targets)"
expected_historical_gemini_targets='linux-x64-vaapi-wayland,macos-arm64-vt,windows-x64-mf'
if [ "$historical_gemini_targets" != "$expected_historical_gemini_targets" ]; then
  echo 'Gemini matrix check is not scoped to jobs.gemini.strategy.matrix.include' >&2
  exit 1
fi
