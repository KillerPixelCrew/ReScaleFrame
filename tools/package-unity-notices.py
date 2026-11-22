#!/usr/bin/env python3
"""Collect the same pinned Rust notices as the AC7 packager, for a Unity stage."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from package_common import dependency_notices  # noqa: E402

if __name__ == "__main__":
    dependency_notices(Path(sys.argv[1]).resolve())
