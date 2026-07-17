#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: $0 BUNDLE_A BUNDLE_B" >&2; exit 64; }
tuple_tree() {
  local root=$1 relative hash size
  while IFS= read -r -d '' file; do
    relative=${file#"$root"/}
    hash=$(shasum -a 256 "$file" | awk '{print $1}')
    size=$(wc -c < "$file" | tr -d ' ')
    printf '%s\t%s\t%s\n' "$relative" "$hash" "$size"
  done < <(find "$root" -type f -print0) | LC_ALL=C sort
}

left=$(mktemp); right=$(mktemp)
trap 'rm -f "$left" "$right"' EXIT
tuple_tree "$1" > "$left"; tuple_tree "$2" > "$right"
diff -u "$left" "$right"
