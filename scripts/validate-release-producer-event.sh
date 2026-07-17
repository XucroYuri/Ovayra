#!/usr/bin/env bash
# Validates only a trusted, successful Task 10 push run for one immutable tag/SHA pair.
set -euo pipefail

event=''
repository=''
sha=''
tag=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --event) event="$2"; shift 2 ;;
    --repository) repository="$2"; shift 2 ;;
    --sha) sha="$2"; shift 2 ;;
    --tag) tag="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

test -f "$event" && test -n "$repository" && test -n "$sha" && test -n "$tag" || {
  echo 'event, repository, SHA, and tag are required' >&2; exit 2;
}
case "$tag" in phase-0-v*) ;; *) echo 'producer tag is outside the Phase 0 tag policy' >&2; exit 1;; esac
jq -e \
  --arg repository "$repository" \
  --arg sha "$sha" \
  --arg tag "$tag" \
  '.name == "phase-0-ffmpeg" and .event == "push" and .conclusion == "success" and .head_repository.full_name == $repository and .head_sha == $sha and .head_branch == $tag' \
  "$event" >/dev/null || {
  echo 'producer run is not the exact trusted successful Phase 0 tag build' >&2
  exit 1
}
