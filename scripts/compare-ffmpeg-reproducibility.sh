#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: $0 BUNDLE_A BUNDLE_B" >&2; exit 64; }
posix_find=/usr/bin/find
[[ -x /usr/bin/find.exe ]] && posix_find=/usr/bin/find.exe
[[ -x "$posix_find" ]] || posix_find=$(command -v find)

sha256_file() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

tuple_tree() {
  local root=$1 relative hash size
  while IFS= read -r -d '' file; do
    relative=${file#"$root"/}
    hash=$(sha256_file "$file")
    size=$(wc -c < "$file" | tr -d ' ')
    printf '%s\t%s\t%s\n' "$relative" "$hash" "$size"
  done < <("$posix_find" "$root" -type f -print0) | LC_ALL=C sort
}

left=$(mktemp); right=$(mktemp)
trap 'rm -f "$left" "$right"' EXIT
tuple_tree "$1" > "$left"; tuple_tree "$2" > "$right"
diff -u "$left" "$right"
