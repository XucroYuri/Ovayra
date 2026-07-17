#!/usr/bin/env bash
set -euo pipefail

fixtures=scripts/fixtures/hdiutil-plist
mount_point="$(scripts/parse-hdiutil-plist.py "$fixtures/one-space-name.plist")"
test "$mount_point" = '/Volumes/Ovayra Phase 0'
device="$(scripts/parse-hdiutil-plist.py --device "$fixtures/one-space-name.plist")"
test "$device" = /dev/disk4
for fixture in missing-mount-point multiple-mount-points; do
  if scripts/parse-hdiutil-plist.py "$fixtures/$fixture.plist" >/dev/null 2>&1; then
    echo "invalid hdiutil plist unexpectedly passed: $fixture" >&2
    exit 1
  fi
done
