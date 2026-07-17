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

make_command_path() {
  local bin=$1 hash_tool=$2 tool
  mkdir -p "$bin"
  for tool in mktemp chmod rm awk; do ln -s "$(command -v "$tool")" "$bin/$tool"; done
  cat > "$bin/gpg" <<EOF
#!/bin/sh
case "\$*" in *--verify*) printf '%s\\n' '$valid' ;; esac
EOF
  cat > "$bin/$hash_tool" <<'EOF'
#!/bin/sh
printf '%064d  %s\n' 0 "$1"
EOF
  chmod +x "$bin/gpg" "$bin/$hash_tool"
}

for hash_tool in sha256sum shasum; do
  bin="$tmp/$hash_tool-bin"; make_command_path "$bin" "$hash_tool"
  key="$tmp/$hash_tool-key"; tarball="$tmp/$hash_tool-tar"; signature="$tmp/$hash_tool-asc"; attestation="$tmp/$hash_tool-attestation"
  printf 'key' > "$key"; printf 'tar' > "$tarball"; printf 'asc' > "$signature"
  PATH="$bin" "$(command -v bash)" "$script" --key "$key" --tar "$tarball" --signature "$signature" --fingerprint "$fingerprint" --attestation "$attestation"
  grep -Fq '"sha256":"0000000000000000000000000000000000000000000000000000000000000000"' "$attestation"
done
