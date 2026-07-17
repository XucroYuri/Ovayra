#!/usr/bin/env python3
"""Write the strict, redacted package attestation consumed by `prove-package`."""

import argparse
import hashlib
import json
from pathlib import Path


def digest(path: Path) -> tuple[str, int]:
    if not path.is_file() or path.is_symlink():
        raise SystemExit("attestation input must be a regular file")
    data = path.read_bytes()
    return hashlib.sha256(data).hexdigest(), len(data)


def artifact(value: str) -> dict[str, object]:
    try:
        format_name, filename = value.split("=", 1)
    except ValueError as error:
        raise SystemExit("artifact must be FORMAT=PATH") from error
    if not format_name or not filename:
        raise SystemExit("artifact must be FORMAT=PATH")
    sha256, length = digest(Path(filename))
    return {"format": format_name, "sha256": sha256, "length": length}


parser = argparse.ArgumentParser()
parser.add_argument("--target", required=True)
parser.add_argument("--platform-verification", required=True)
parser.add_argument("--source-lock", required=True, type=Path)
parser.add_argument("--inspection-log", required=True, type=Path)
parser.add_argument("--notarization", choices=["accepted"])
parser.add_argument("--artifact", action="append", required=True)
parser.add_argument("--output", required=True, type=Path)
args = parser.parse_args()

source_lock_sha256, _ = digest(args.source_lock)
inspection_sha256, _ = digest(args.inspection_log)
artifacts = sorted((artifact(value) for value in args.artifact), key=lambda value: value["format"])
if len({value["format"] for value in artifacts}) != len(artifacts):
    raise SystemExit("duplicate artifact format")
payload = {
    "schema_version": 1,
    "target": args.target,
    "source_lock_sha256": source_lock_sha256,
    "inspection_sha256": inspection_sha256,
    "platform_verification": args.platform_verification,
    "notarization": args.notarization,
    "artifacts": artifacts,
}
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")
