#!/usr/bin/env bash
set -euo pipefail
root=$(mktemp -d); trap 'rm -rf "$root"' EXIT
mkdir -p "$root/a/provenance" "$root/b/provenance"
printf '%s\n' 'configuration: --prefix=/tmp/a/stage --extra-cflags=-I/tmp/a/deps/include' > "$root/a/provenance/buildconf.txt"
printf '%s\n' 'configuration: --prefix=/tmp/b/stage --extra-cflags=-I/tmp/b/deps/include' > "$root/b/provenance/buildconf.txt"
printf '%s\n' binary > "$root/a/ffmpeg"; printf '%s\n' binary > "$root/b/ffmpeg"
"$(dirname "$0")/compare-ffmpeg-reproducibility.sh" "$root/a" "$root/b"
printf '%s\n' changed > "$root/b/ffmpeg"
if "$(dirname "$0")/compare-ffmpeg-reproducibility.sh" "$root/a" "$root/b"; then exit 1; fi
