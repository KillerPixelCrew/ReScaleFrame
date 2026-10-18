#!/usr/bin/env python3
"""Collect the same pinned Rust notices as the AC7 packager, for a Unity stage."""
from pathlib import Path
import runpy
import sys

if __name__ == "__main__":
    root = Path(__file__).resolve().parents[1]
    collect = runpy.run_path(str(root / "tools/package-ac7.py"), run_name="package_ac7")["dependency_notices"]
    collect(Path(sys.argv[1]).resolve())
