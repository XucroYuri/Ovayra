#!/usr/bin/env python3
"""Print one safe hdiutil attach mount-point from a bounded plist document."""

from __future__ import annotations

import plistlib
import sys
from pathlib import Path


def exactly_one(document: list[object], key: str) -> str:
    values = [
        item[key]
        for item in document
        if isinstance(item, dict) and isinstance(item.get(key), str)
    ]
    if len(values) != 1:
        raise SystemExit(f"hdiutil plist must contain exactly one {key}")
    return values[0]


def main() -> int:
    args = sys.argv[1:]
    field = "mount-point"
    if len(args) == 2 and args[0] == "--device":
        field = "dev-entry"
        args = args[1:]
    if len(args) != 1:
        raise SystemExit("usage: parse-hdiutil-plist.py [--device] ATTACH-PLIST")
    raw = Path(args[0]).read_bytes()
    if len(raw) > 256 * 1024:
        raise SystemExit("hdiutil plist is too large")
    document = plistlib.loads(raw)
    if not isinstance(document, list):
        raise SystemExit("hdiutil plist root is not an array")
    value = exactly_one(document, field)
    if field == "mount-point":
        if not value.startswith("/Volumes/") or value == "/Volumes/":
            raise SystemExit("hdiutil mount-point is outside /Volumes")
    elif not value.startswith("/dev/"):
        raise SystemExit("hdiutil device is outside /dev")
    print(value)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
