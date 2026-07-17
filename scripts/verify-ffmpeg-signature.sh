#!/usr/bin/env bash
# Verify an FFmpeg release signature using only the supplied pinned key artifact.
set -euo pipefail

# setup-msys2 installs native tools below this prefix; include it when invoked by pwsh.
[[ -d /ucrt64/bin ]] && PATH="/ucrt64/bin:$PATH"

die() { printf '%s\n' "verify-ffmpeg-signature: $*" >&2; exit 65; }

parse_status() {
  local status=$1 expected=$2 count=0 line signer primary
  while IFS= read -r line; do
    case "$line" in
      '[GNUPG:] VALIDSIG '*)
        set -- $line
        signer=${3:-}; primary=${12:-}
        [[ "$signer" =~ ^[A-F0-9]{40}$ && "$primary" =~ ^[A-F0-9]{40}$ ]] || die "malformed VALIDSIG record"
        [[ "$primary" == "$expected" && "$signer" == "$expected" ]] || die "VALIDSIG fingerprint does not match pinned release key"
        count=$((count + 1))
        ;;
    esac
  done < "$status"
  [[ $count -eq 1 ]] || die "expected exactly one VALIDSIG record, found $count"
}

sha256() {
  local value
  if command -v sha256sum >/dev/null; then
    value=$(sha256sum "$1" | awk '{print $1}')
  elif command -v shasum >/dev/null; then
    value=$(shasum -a 256 "$1" | awk '{print $1}')
  else
    die "sha256sum or shasum is required"
  fi
  [[ "$value" =~ ^[a-f0-9]{64}$ ]] || die "SHA-256 tool returned an invalid digest"
  printf '%s\n' "$value"
}

if [[ ${1:-} == "--parse-status" ]]; then
  [[ $# -eq 4 && ${3:-} == "--fingerprint" ]] || die "usage: --parse-status STATUS --fingerprint FINGERPRINT"
  parse_status "$2" "$4"
  exit 0
fi

key= tarball= signature= fingerprint= attestation=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --key) key=${2:-}; shift 2 ;;
    --tar) tarball=${2:-}; shift 2 ;;
    --signature) signature=${2:-}; shift 2 ;;
    --fingerprint) fingerprint=${2:-}; shift 2 ;;
    --attestation) attestation=${2:-}; shift 2 ;;
    *) die "unknown argument $1" ;;
  esac
done
[[ -n "$key" && -n "$tarball" && -n "$signature" && -n "$fingerprint" && -n "$attestation" ]] || die "missing required argument"
[[ "$fingerprint" =~ ^[A-F0-9]{40}$ ]] || die "fingerprint must be an uppercase 40-hex primary fingerprint"
[[ -s "$key" && -s "$tarball" && -s "$signature" ]] || die "key, tarball, and signature must be nonempty regular inputs"
command -v gpg >/dev/null || die "gpg is required"

home=$(mktemp -d "${TMPDIR:-/tmp}/ovayra-ffmpeg-gpg.XXXXXX")
status=$(mktemp "${TMPDIR:-/tmp}/ovayra-ffmpeg-status.XXXXXX")
cleanup() { rm -rf "$home" "$status"; }
trap cleanup EXIT
chmod 700 "$home"
GNUPGHOME="$home" gpg --batch --no-options --import "$key" >/dev/null 2>&1 || die "cannot import pinned key artifact"
GNUPGHOME="$home" gpg --batch --no-options --status-fd=1 --verify "$signature" "$tarball" >"$status" 2>/dev/null || die "detached signature verification failed"
parse_status "$status" "$fingerprint"
hash=$(sha256 "$tarball")
printf '{"schema_version":1,"verified":true,"signer_fingerprint":"%s","primary_fingerprint":"%s","sha256":"%s"}\n' "$fingerprint" "$fingerprint" "$hash" > "$attestation"
