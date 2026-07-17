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
  'release prove-package --target-id macos-arm64-vt' \
  'release prove-package --target-id windows-x64-mf' \
  'release prove-package --target-id linux-x64-vaapi-wayland' \
  'release prove-update --target-id "$target"' \
  'scripts/write-package-attestation.py' \
  'scripts/validate-release-producer-event.sh' \
  'head_repository.full_name == github.repository' \
  'producer tag does not match workflow run' \
  'release version does not match tag'; do
  rg -F --quiet "$required" "$workflow" || { echo "workflow missing required release hardening: $required" >&2; exit 1; }
done
rg -F --quiet 'hdiutil attach -plist -readonly -nobrowse' "$inspector"
rg -F --quiet 'attachment="$(scripts/parse-hdiutil-plist.py' "$inspector"
rg -F --quiet 'hdiutil detach "$device"' "$inspector"
test -f scripts/write-package-attestation.py
python3 scripts/write-package-attestation.py --help >/dev/null
