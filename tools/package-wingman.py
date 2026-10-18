#!/usr/bin/env python3
"""Package the exact-build Project Wingman observer, without GPU providers."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import time
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True, encoding="utf-8").strip()


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "build/releases/wingman")
    args = parser.parse_args()
    if git("status", "--porcelain"):
        raise RuntimeError("Commit the intended release source before packaging.")
    version = (ROOT / "VERSION").read_text().strip()
    revision = git("rev-parse", "HEAD")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    stage = output / f"stage-wingman-{uuid.uuid4().hex[:8]}"
    stage.mkdir()
    for source, target in (
        ("build/windows-x64/wingman/bin/Release/xinput1_3.dll", "xinput1_3.dll"),
        ("build/windows-x64/bin/Release/ReScaleFrame.Runtime.dll", "ReScaleFrame/ReScaleFrame.Runtime.dll"),
        ("build/windows-x64/bin/Release/ReScaleFrame.Game.ProjectWingman.dll", "ReScaleFrame/ReScaleFrame.Game.ProjectWingman.dll"),
        ("docs/releases/wingman-observer-install.md", "ReScaleFrame/README.md"),
        ("LICENSE", "ReScaleFrame/licenses/ReScaleFrame-GPL-3.0.txt"),
        ("sdk/game/LICENSE", "ReScaleFrame/licenses/GameSDK-MIT.txt"),
        ("build/windows-x64/_deps/minhook-src/LICENSE.txt", "ReScaleFrame/licenses/MinHook.txt"),
        ("runtime/backends/dlss/third_party/mfg_unlocker/LICENSE", "ReScaleFrame/licenses/MFG-Unlocker-MIT.txt"),
    ):
        destination = stage / target
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / source, destination)
    (stage / "ReScaleFrame.ini").write_text("[ProjectWingman]\nObserveNative=1\n", encoding="utf-8")
    engine = json.loads((ROOT / "games/project-wingman/engine.json").read_text(encoding="utf-8"))
    manifest = {
        "product": "ReScaleFrame for Project Wingman: observer preview",
        "version": version, "source_commit": revision, "platform": "Windows x64",
        "game": engine["game"], "capabilities": {"temporal_graph_observer": True,
            "super_resolution": False, "frame_generation": False, "latency": False, "vr_processing": False},
        "fresh_game_tested": False,
        "files": {p.relative_to(stage).as_posix(): digest(p) for p in sorted(stage.rglob("*")) if p.is_file()},
    }
    (stage / "ReScaleFrame/manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    archive = output / f"ReScaleFrame-{version}-ProjectWingman-Observer-Windows-x64.zip"
    stamp = time.gmtime(int(git("show", "-s", "--format=%ct", "HEAD")))[:6]
    with zipfile.ZipFile(archive, "w") as bundle:
        for path in sorted(stage.rglob("*")):
            if path.is_file():
                entry = zipfile.ZipInfo(path.relative_to(stage).as_posix(), stamp)
                entry.compress_type = zipfile.ZIP_DEFLATED
                entry.external_attr = 0o100644 << 16
                bundle.writestr(entry, path.read_bytes(), compresslevel=9)
    with zipfile.ZipFile(archive) as bundle:
        if bundle.testzip() is not None:
            raise RuntimeError("Archive CRC verification failed.")
        for name, expected in manifest["files"].items():
            if hashlib.sha256(bundle.read(name)).hexdigest() != expected:
                raise RuntimeError(f"Archive hash mismatch: {name}")
    source = output / f"ReScaleFrame-{version}-ProjectWingman-source.zip"
    subprocess.run(["git", "archive", "--format=zip", f"--prefix=ReScaleFrame-{version}/",
                    "-o", str(source), revision], cwd=ROOT, check=True)
    sums = output / "SHA256SUMS.txt"
    sums.write_text("".join(f"{digest(p)}  {p.name}\n" for p in (archive, source)), encoding="ascii")
    print(json.dumps({"archive": str(archive), "source": str(source), "checksums": str(sums),
                      "stage": str(stage), "revision": revision}, indent=2))


if __name__ == "__main__":
    main()
