#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: $0 BUNDLE_A BUNDLE_B" >&2; exit 64; }
tuple_tree() {
  local root=$1 relative hash size content
  while IFS= read -r -d '' file; do
    relative=${file#"$root"/}
    if [[ $relative == provenance/buildconf.txt ]]; then
      content=$(sed -E 's#--prefix=[^[:space:]]+#--prefix=$OVAYRA_FFMPEG_STAGE#g; s#(-I|-L)[^[:space:]]+#\1$OVAYRA_FFMPEG_DEPS#g' "$file")
      hash=$(printf '%s' "$content" | shasum -a 256 | awk '{print $1}')
      size=$(printf '%s' "$content" | wc -c | tr -d ' ')
    else
      hash=$(shasum -a 256 "$file" | awk '{print $1}')
      size=$(wc -c < "$file" | tr -d ' ')
    fi
    printf '%s\t%s\t%s\n' "$relative" "$hash" "$size"
  done < <(find "$root" -type f -print0) | LC_ALL=C sort
}

left=$(mktemp); right=$(mktemp)
trap 'rm -f "$left" "$right"' EXIT
tuple_tree "$1" > "$left"; tuple_tree "$2" > "$right"
diff -u "$left" "$right"
