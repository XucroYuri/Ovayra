#!/usr/bin/env bash
set -euo pipefail

fixtures=scripts/fixtures/hdiutil-plist
attachment="$(scripts/parse-hdiutil-plist.py "$fixtures/one-space-name.plist")"
test "$attachment" = '{"device":"/dev/disk4s1","mount_point":"/Volumes/Ovayra Phase 0"}'
for fixture in root-array duplicate-mounted-entities split-device-mount unexpected-field-type; do
  if scripts/parse-hdiutil-plist.py "$fixtures/$fixture.plist" >/dev/null 2>&1; then
    echo "invalid hdiutil plist unexpectedly passed: $fixture" >&2
    exit 1
  fi
done
