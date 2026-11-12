#!/usr/bin/env python3
"""Package the RDR2 ASI, shared runtime, overlay and required DLSS, FSR and XeSS runtimes."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import time
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]
# Reuse the AC7 packager's hashing, copying and Rust notice collection rather than forking them.
_spec = importlib.util.spec_from_file_location("package_ac7", ROOT / "tools/package-ac7.py")
ac7 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ac7)
git, digest, copy = ac7.git, ac7.digest, ac7.copy

# Portable defaults. Overlay changes are written back to this file; diagnostics stay off.
CONFIG = """[RDR2]
Enable=1
ForceD3D12=1
Census=0
TraceD3D12=0
SR=1
FG=0
StatusOverlay=1
"""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--streamline-root", required=True, type=Path)
    parser.add_argument("--fidelityfx-root", type=Path, default=ROOT / "vendor/fidelityfx")
    parser.add_argument("--xess-root", type=Path, default=ROOT / "vendor/xess")
    parser.add_argument("--output", type=Path, default=ROOT / "build/releases/rdr2")
    args = parser.parse_args()
    if git("status", "--porcelain"):
        raise RuntimeError("Commit the intended release source before packaging.")
    version = (ROOT / "VERSION").read_text().strip()
    revision = git("rev-parse", "HEAD")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    stage = output / f"stage-rdr2-{uuid.uuid4().hex[:8]}"
    stage.mkdir()
    fsr = args.fidelityfx_root / "Kits/FidelityFX/signedbin/amd_fidelityfx_upscaler_dx12.dll"
    xess = args.xess_root / "bin/libxess.dll"
    # Same validated SR runtimes as AC7; the FSR4 compatibility hook recognizes this exact binary.
    for source, expected in (
        (fsr, "aad4fccccb29084a655f9a4f9f613bd22c8c03fb4a0f8840148cc6543b4ccace"),
        (xess, "251659dd84a3e84de67c886a4186e01f3eca49b00641906fe38bb6b807e5d5b7"),
    ):
        if digest(source) != expected:
            raise RuntimeError(f"{source.name} differs from the validated SR runtime.")
    release = ROOT / "build/windows-x64/bin/Release"
    for source, relative in (
        (ROOT / "build/windows-x64/rdr2/bin/Release/ReScaleFrame.RDR2.asi", "ReScaleFrame.RDR2.asi"),
        (release / "ReScaleFrame.Runtime.dll", "ReScaleFrame/ReScaleFrame.Runtime.dll"),
        (release / "ReScaleFrame.Game.RDR2.dll", "ReScaleFrame/ReScaleFrame.Game.RDR2.dll"),
        (ROOT / "target/release/rescaleframe_overlay.dll", "ReScaleFrame/rescaleframe_overlay.dll"),
        (ROOT / "docs/releases/rdr2-install.md", "ReScaleFrame/README.md"),
        (ROOT / "LICENSE", "ReScaleFrame/licenses/ReScaleFrame-GPL-3.0.txt"),
        (ROOT / "sdk/game/LICENSE", "ReScaleFrame/licenses/GameSDK-MIT.txt"),
        (ROOT / "runtime/backends/dlss/third_party/mfg_unlocker/LICENSE", "ReScaleFrame/licenses/MFG-Unlocker-MIT.txt"),
        (ROOT / "build/windows-x64/_deps/minhook-src/LICENSE.txt", "ReScaleFrame/licenses/MinHook.txt"),
        (args.streamline_root / "license.txt", "ReScaleFrame/licenses/Streamline.txt"),
        (args.streamline_root / "3rd-party-licenses.md", "ReScaleFrame/licenses/Streamline-third-party.md"),
        (args.streamline_root / "bin/x64/nvngx_dlss.license.txt", "ReScaleFrame/streamline/nvngx_dlss.license.txt"),
        (args.streamline_root / "bin/x64/reflex.license.txt", "ReScaleFrame/licenses/NVIDIA-Reflex.txt"),
        (fsr, "ReScaleFrame/fidelityfx/amd_fidelityfx_upscaler_dx12.dll"),
        (args.fidelityfx_root / "Kits/FidelityFX/signedbin/amd_fidelityfx_loader_dx12.dll",
         "ReScaleFrame/fidelityfx/amd_fidelityfx_loader_dx12.dll"),
        (args.fidelityfx_root / "Kits/FidelityFX/signedbin/amd_fidelityfx_framegeneration_dx12.dll",
         "ReScaleFrame/fidelityfx/amd_fidelityfx_framegeneration_dx12.dll"),
        (args.fidelityfx_root / "Kits/FidelityFX/docs/license.md", "ReScaleFrame/fidelityfx/LICENSE.md"),
        (args.fidelityfx_root / "3rdpartynotice.md", "ReScaleFrame/fidelityfx/3rdpartynotice.md"),
        (xess, "ReScaleFrame/xess/libxess.dll"),
        (args.xess_root / "bin/libxess_fg.dll", "ReScaleFrame/xess/libxess_fg.dll"),
        (args.xess_root / "bin/libxell.dll", "ReScaleFrame/xess/libxell.dll"),
        (args.xess_root / "LICENSE.txt", "ReScaleFrame/xess/LICENSE.txt"),
        (args.xess_root / "third-party-programs.txt", "ReScaleFrame/xess/third-party-programs.txt"),
    ):
        copy(source, stage / relative)
    for name in ac7.RUNTIME:
        copy(args.streamline_root / "bin/x64" / name, stage / "ReScaleFrame/streamline" / name)
    (stage / "ReScaleFrame.ini").write_text(CONFIG, encoding="utf-8")
    (stage / "ReScaleFrame/licenses/README.md").write_text(
        "# Package notices\n\nFirst-party code is GPL-3.0-only; the public Game SDK is MIT.\n"
        "Source identity is recorded in ../manifest.json and the corresponding-source archive.\n\n"
        "Streamline/DLSS/DLSS-G/Reflex, FidelityFX SR/FG and XeSS SR/FG/XeLL runtimes retain their\n"
        "own licenses. Their terms and third-party notices are included beside the runtimes\n"
        "or in this directory. Vendor binaries are unmodified. Rust dependency notices and\n"
        "MinHook are also included. ScriptHookRDR2 and the ASI loader are not redistributed.\n",
        encoding="utf-8")
    ac7.dependency_notices(stage)
    engine = json.loads((ROOT / "games/rdr2/engine.json").read_text(encoding="utf-8"))
    manifest = {
        "product": "ReScaleFrame for Red Dead Redemption 2", "version": version, "source_commit": revision,
        "platform": "Windows x64", "game": engine["game"], "streamline": "2.14.1",
        "fidelityfx_sdk": "2.4.0", "xess_sdk": "3.0.2",
        "carrier": "ASI loaded by the game's ASI loader",
        "super_resolution": ["DLSS", "FSR3", "FSR4", "XeSS"],
        "frame_generation": ["DLSS-G", "FSR3", "FSR4 when exposed by the device SDK", "XeSS FG"],
        "fresh_rdr2_game_tested": False,
        "files": {p.relative_to(stage).as_posix(): digest(p) for p in sorted(stage.rglob("*")) if p.is_file()},
    }
    (stage / "ReScaleFrame/manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    archive = output / f"ReScaleFrame-{version}-RDR2-Windows-x64.zip"
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
    source = output / f"ReScaleFrame-{version}-RDR2-source.zip"
    subprocess.run(["git", "archive", "--format=zip", f"--prefix=ReScaleFrame-{version}/",
                    "-o", str(source), revision], cwd=ROOT, check=True)
    sums = output / "SHA256SUMS.txt"
    sums.write_text("".join(f"{digest(p)}  {p.name}\n" for p in (archive, source)), encoding="ascii")
    print(json.dumps({"archive": str(archive), "source": str(source), "checksums": str(sums),
                      "stage": str(stage), "revision": revision, "entries": len(manifest["files"])}, indent=2))


if __name__ == "__main__":
    main()
