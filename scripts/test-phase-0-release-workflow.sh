#!/usr/bin/env bash
set -euo pipefail

workflow=.github/workflows/phase-0-release.yml
inspector=scripts/inspect-native-package.sh
for required in \
  'scripts/inspect-native-package.sh --kind app' \
  'scripts/inspect-native-package.sh --kind dmg' \
  'scripts/inspect-native-package.ps1 -Kind msi' \
  'scripts/inspect-native-package.sh --kind appimage' \
  'scripts/inspect-native-package.sh --kind deb' \
  'scripts/validate-release-producer-event.sh' \
  'head_repository.full_name == github.repository' \
  'producer tag does not match workflow run' \
  'release version does not match tag'; do
  rg -F --quiet "$required" "$workflow" || { echo "workflow missing required release hardening: $required" >&2; exit 1; }
done
rg -F --quiet 'hdiutil attach -plist -readonly -nobrowse' "$inspector"
rg -F --quiet 'parse-hdiutil-plist.py --device' "$inspector"
rg -F --quiet 'hdiutil detach "$device"' "$inspector"
