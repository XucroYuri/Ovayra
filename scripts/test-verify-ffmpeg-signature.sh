#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
script="$root/scripts/verify-ffmpeg-signature.sh"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/ovayra-ffmpeg-status-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
fingerprint=FCF986EA15E6E293A5644F10B4322F04D67658D8
valid="[GNUPG:] VALIDSIG $fingerprint 20260717 0 0 4 0 1 10 00 $fingerprint"
printf '%s\n' "$valid" > "$tmp/valid"
bash "$script" --parse-status "$tmp/valid" --fingerprint "$fingerprint"
for case_name in wrong multiple missing; do
  case "$case_name" in
    wrong) printf '%s\n' "${valid/$fingerprint/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA}" > "$tmp/$case_name" ;;
    multiple) printf '%s\n%s\n' "$valid" "$valid" > "$tmp/$case_name" ;;
    missing) printf '%s\n' '[GNUPG:] GOODSIG harmless display text' > "$tmp/$case_name" ;;
  esac
  if bash "$script" --parse-status "$tmp/$case_name" --fingerprint "$fingerprint"; then
    echo "expected $case_name VALIDSIG fixture to fail" >&2; exit 1
  fi
done
