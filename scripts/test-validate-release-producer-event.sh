#!/usr/bin/env bash
set -euo pipefail

fixtures=scripts/fixtures/release-producer-events
scripts/validate-release-producer-event.sh --event "$fixtures/valid-push-tag.json" --repository ovayra/ovayra --sha abc123 --tag phase-0-v0.0.1
for fixture in pr branch wrong-repo wrong-workflow wrong-sha wrong-tag; do
  if scripts/validate-release-producer-event.sh --event "$fixtures/$fixture.json" --repository ovayra/ovayra --sha abc123 --tag phase-0-v0.0.1 >/dev/null 2>&1; then
    echo "invalid producer fixture unexpectedly passed: $fixture" >&2
    exit 1
  fi
done
