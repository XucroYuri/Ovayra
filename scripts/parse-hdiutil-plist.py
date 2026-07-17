#!/usr/bin/env python3
"""Emit the one mounted hdiutil partition as a compact JSON document."""

from __future__ import annotations

import json
import plistlib
import re
import sys
from pathlib import Path

MAX_PLIST_BYTES = 256 * 1024
MAX_SYSTEM_ENTITIES = 64
DEVICE = re.compile(r"/dev/disk\d+(?:s\d+)?\Z")


def reject(message: str) -> None:
    raise SystemExit(message)


def safe_text(value: str, field: str) -> str:
    if "\n" in value or "\r" in value or "\x00" in value:
        reject(f"hdiutil {field} contains a control newline or NUL")
    return value


def mounted_attachment(document: object) -> tuple[str, str]:
    if not isinstance(document, dict):
        reject("hdiutil plist root is not a dictionary")
    entities = document.get("system-entities")
    if not isinstance(entities, list) or not 1 <= len(entities) <= MAX_SYSTEM_ENTITIES:
        reject("hdiutil plist has an invalid system-entities array")

    mounted: list[tuple[str, str]] = []
    mount_fields = 0
    for entity in entities:
        if not isinstance(entity, dict):
            reject("hdiutil system entity is not a dictionary")
        device = entity.get("dev-entry")
        mount_point = entity.get("mount-point")
        if device is not None and not isinstance(device, str):
            reject("hdiutil dev-entry has an unexpected type")
        if mount_point is not None and not isinstance(mount_point, str):
            reject("hdiutil mount-point has an unexpected type")
        if mount_point is not None:
            mount_fields += 1
        if device is None or mount_point is None:
            continue
        device = safe_text(device, "dev-entry")
        mount_point = safe_text(mount_point, "mount-point")
        if not DEVICE.fullmatch(device):
            reject("hdiutil device is outside the allowed /dev/disk* form")
        if not mount_point.startswith("/Volumes/") or mount_point == "/Volumes/":
            reject("hdiutil mount-point is outside /Volumes")
        mounted.append((device, mount_point))

    if mount_fields != 1 or len(mounted) != 1:
        reject("hdiutil plist must contain exactly one complete mounted partition entity")
    return mounted[0]


def main() -> int:
    if len(sys.argv) != 2:
        reject("usage: parse-hdiutil-plist.py ATTACH-PLIST")
    raw = Path(sys.argv[1]).read_bytes()
    if len(raw) > MAX_PLIST_BYTES:
        reject("hdiutil plist is too large")
    device, mount_point = mounted_attachment(plistlib.loads(raw))
    print(json.dumps({"device": device, "mount_point": mount_point}, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
