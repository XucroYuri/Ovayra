#!/usr/bin/env bash
# Extracts final macOS/Linux native artifacts and delegates the shared resource proof.
set -euo pipefail

usage() { echo 'usage: inspect-native-package.sh --kind app|dmg|appimage|deb --artifact PATH [--require-nv]' >&2; exit 2; }
kind=''
artifact=''
require_nv=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --kind) kind="$2"; shift 2 ;;
    --artifact) artifact="$2"; shift 2 ;;
    --require-nv) require_nv=true; shift ;;
    *) usage ;;
  esac
done
test -n "$kind" && test -n "$artifact" || usage
if [ "$kind" = app ]; then
  test -d "$artifact" || usage
else
  test -f "$artifact" || usage
fi

temporary="$(mktemp -d)"
mount_point=''
device=''
cleanup() {
  if [ -n "$device" ]; then hdiutil detach "$device" >/dev/null 2>&1 || true
  elif [ -n "$mount_point" ]; then hdiutil detach "$mount_point" >/dev/null 2>&1 || true; fi
  rm -rf "$temporary"
}
trap cleanup EXIT

locate_one() {
  local search_root="$1" suffix="$2" matches
  matches="$(find "$search_root" -type f -path "*/$suffix" -print)"
  test "$(printf '%s\n' "$matches" | sed '/^$/d' | wc -l | tr -d ' ')" = 1 || {
    echo "expected exactly one $suffix under $search_root" >&2; exit 1;
  }
  printf '%s\n' "$matches"
}

inspect_root() {
  local tree="$1" app_name="$2" ffmpeg_name="$3" notice root app relative hash size
  notice="$(locate_one "$tree" 'ffmpeg/NOTICE.txt')"
  root="${notice%/ffmpeg/NOTICE.txt}"
  app="$(locate_one "$tree" "$app_name")"
  args=(--root "$root" --app "$app" --ffmpeg-name "$ffmpeg_name")
  if "$require_nv"; then args+=(--require-nv); fi
  scripts/inspect-package-contents.sh "${args[@]}"
  while IFS= read -r -d '' file; do
    relative=${file#"$tree"/}
    hash=$(shasum -a 256 "$file" | awk '{print $1}')
    size=$(wc -c < "$file" | tr -d ' ')
    printf '%s\t%s\t%s\n' "$relative" "$hash" "$size"
  done < <(find "$tree" -type f -print0) | LC_ALL=C sort | shasum -a 256 | awk '{print "INSPECTION_TREE_SHA256=" $1}'
}

case "$kind" in
  app)
    test -d "$artifact" || usage
    inspect_root "$artifact" ovayra-spike ffmpeg
    ;;
  dmg)
    test -f "$artifact" || usage
    attach_plist="$temporary/attach.plist"
    hdiutil attach -plist -readonly -nobrowse "$artifact" > "$attach_plist"
    attachment="$(scripts/parse-hdiutil-plist.py "$attach_plist")"
    device="$(printf '%s' "$attachment" | python3 -c 'import json, sys; print(json.load(sys.stdin)["device"])')"
    mount_point="$(printf '%s' "$attachment" | python3 -c 'import json, sys; print(json.load(sys.stdin)["mount_point"])')"
    app="$(find "$mount_point" -maxdepth 1 -type d -name '*.app' -print)"
    test "$(printf '%s\n' "$app" | sed '/^$/d' | wc -l | tr -d ' ')" = 1 || { echo 'DMG must contain exactly one app' >&2; exit 1; }
    inspect_root "$app" ovayra-spike ffmpeg
    ;;
  appimage)
    test -f "$artifact" || usage
    artifact="$(cd "$(dirname "$artifact")" && pwd)/$(basename "$artifact")"
    (cd "$temporary" && "$artifact" --appimage-extract >/dev/null)
    inspect_root "$temporary/squashfs-root" ovayra-spike ffmpeg
    ;;
  deb)
    test -f "$artifact" || usage
    dpkg-deb -x "$artifact" "$temporary/deb"
    inspect_root "$temporary/deb" ovayra-spike ffmpeg
    ;;
  *) usage ;;
esac
