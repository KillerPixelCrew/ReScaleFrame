#!/usr/bin/env python3
"""Package the AC7 Release build, overlay and required DLSS, FSR and XeSS runtimes."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import time
import urllib.error
import urllib.request
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ("sl.interposer.dll", "sl.common.dll", "sl.dlss.dll", "sl.pcl.dll", "nvngx_dlss.dll")


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True, encoding="utf-8").strip()


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def copy(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def dependency_notices(stage: Path) -> None:
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT, encoding="utf-8"))
    index = ["# Third-party notices", "", "Dependency licenses apply to their respective components.", ""]
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if not package["source"]:
            continue
        root = Path(package["manifest_path"]).parent
        name = f'{package["name"]}-{package["version"]}'
        destination = stage / "ReScaleFrame/licenses/rust" / name
        files = sorted(p for p in root.rglob("*") if p.is_file() and
                       re.search(r"license|licence|copying|copyright|notice|ofl\.txt|unlicense", p.name, re.I))
        for path in files:
            copy(path, destination / path.relative_to(root))
        provenance = "crate distribution"
        if not files:
            # Some workspace crates omit their repository-root license from the crate archive.
            # Fetch it at the exact source revision recorded by Cargo, never a moving branch.
            vcs = json.loads((root / ".cargo_vcs_info.json").read_text(encoding="utf-8"))
            sha = vcs["git"]["sha1"]
            match = re.match(r"(https://github.com/[\w.-]+/[\w.-]+)(?:/|$)", package.get("repository") or "")
            if not match or not re.fullmatch(r"[0-9a-f]{40}", sha):
                raise RuntimeError(f"No verifiable license source for {name}")
            repository = match[1].removesuffix(".git")
            base = repository.replace("https://github.com/", "https://raw.githubusercontent.com/") + f"/{sha}/"
            for filename in ("LICENSE-MIT", "LICENSE-MIT.txt", "LICENSE", "LICENSE.txt", "LICENSE.md"):
                try:
                    with urllib.request.urlopen(base + filename, timeout=30) as response:
                        content = response.read()
                except urllib.error.HTTPError as error:
                    if error.code == 404:
                        continue
                    raise
                destination.mkdir(parents=True, exist_ok=True)
                (destination / filename).write_bytes(content)
                provenance = base + filename
                files = [destination / filename]
                break
            if not files:
                raise RuntimeError(f"Missing license text for {name}")
        index += [f'## {name}', f'License: {package.get("license") or "see included text"}',
                  f"Source: {provenance}", ""]
    (stage / "ReScaleFrame/licenses/RUST-DEPENDENCIES.md").write_text("\n".join(index), encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--streamline-root", required=True, type=Path)
    parser.add_argument("--fidelityfx-root", type=Path, default=ROOT / "vendor/fidelityfx")
    parser.add_argument("--xess-root", type=Path, default=ROOT / "vendor/xess")
    parser.add_argument("--output", type=Path, default=ROOT / "build/releases")
    parser.add_argument("--expected-proxy-sha256", help="Require the game-tested proxy bytes")
    parser.add_argument("--expected-plugin-sha256", help="Require the game-tested AC7 plugin bytes")
    parser.add_argument("--expected-overlay-sha256", help="Require the tested overlay bytes")
    args = parser.parse_args()
    if git("status", "--porcelain"):
        raise RuntimeError("Commit the intended release source before packaging.")
    version = (ROOT / "VERSION").read_text().strip()
    revision = git("rev-parse", "HEAD")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    stage = output / f"stage-ac7-{uuid.uuid4().hex[:8]}"
    stage.mkdir()
    native = ROOT / "build/windows-x64/bin/Release/dinput8.dll"
    plugin = ROOT / "build/windows-x64/bin/Release/ReScaleFrame.Game.AC7.dll"
    overlay = ROOT / "target/release/rescaleframe_overlay.dll"
    for source, expected in ((native, args.expected_proxy_sha256),
                             (plugin, args.expected_plugin_sha256),
                             (overlay, args.expected_overlay_sha256)):
        if expected and digest(source).lower() != expected.lower():
            raise RuntimeError(f"{source.name} differs from the validated build.")
    # The FSR4 compatibility hook recognizes this exact signed SDK binary.
    fsr = args.fidelityfx_root / "Kits/FidelityFX/signedbin/amd_fidelityfx_upscaler_dx12.dll"
    xess = args.xess_root / "bin/libxess.dll"
    for source, expected in (
        (fsr, "d0dcccc74a43c44ba435b7a369b456e0970d8a4464e4bd683119b374f2c9fb46"),
        (xess, "251659dd84a3e84de67c886a4186e01f3eca49b00641906fe38bb6b807e5d5b7"),
    ):
        if digest(source) != expected:
            raise RuntimeError(f"{source.name} differs from the validated SR runtime.")
    for source, relative in (
        (native, "dinput8.dll"), (plugin, "ReScaleFrame.Game.AC7.dll"),
        (overlay, "rescaleframe_overlay.dll"),
        (ROOT / "loader/ReScaleFrame.ini.sample", "ReScaleFrame.ini"),
        (ROOT / "docs/releases/ac7-install.md", "ReScaleFrame/README.md"),
        (ROOT / "docs/releases/ac7-third-party.md", "ReScaleFrame/licenses/README.md"),
        (ROOT / "LICENSE", "ReScaleFrame/licenses/ReScaleFrame-GPL-3.0.txt"),
        (ROOT / "sdk/game/LICENSE", "ReScaleFrame/licenses/GameSDK-MIT.txt"),
        (ROOT / "build/windows-x64/_deps/minhook-src/LICENSE.txt", "ReScaleFrame/licenses/MinHook.txt"),
        (args.streamline_root / "license.txt", "ReScaleFrame/licenses/Streamline.txt"),
        (args.streamline_root / "3rd-party-licenses.md", "ReScaleFrame/licenses/Streamline-third-party.md"),
        (args.streamline_root / "bin/x64/nvngx_dlss.license.txt", "ReScaleFrame/streamline/nvngx_dlss.license.txt"),
        (args.streamline_root / "bin/x64/reflex.license.txt", "ReScaleFrame/licenses/NVIDIA-Reflex.txt"),
        (fsr, "ReScaleFrame/fidelityfx/amd_fidelityfx_upscaler_dx12.dll"),
        (args.fidelityfx_root / "Kits/FidelityFX/docs/license.md", "ReScaleFrame/fidelityfx/LICENSE.md"),
        (args.fidelityfx_root / "3rdpartynotice.md", "ReScaleFrame/fidelityfx/3rdpartynotice.md"),
        (xess, "ReScaleFrame/xess/libxess.dll"),
        (args.xess_root / "LICENSE.txt", "ReScaleFrame/xess/LICENSE.txt"),
        (args.xess_root / "third-party-programs.txt", "ReScaleFrame/xess/third-party-programs.txt"),
    ):
        copy(source, stage / relative)
    for name in RUNTIME:
        copy(args.streamline_root / "bin/x64" / name, stage / "ReScaleFrame/streamline" / name)
    header = (ROOT / "vendor/renderdoc/renderdoc_app.h").read_text(encoding="utf-8")
    (stage / "ReScaleFrame/licenses/RenderDoc-header.txt").write_text(header[:header.index("#pragma once")], encoding="utf-8")
    dependency_notices(stage)
    config = (stage / "ReScaleFrame.ini").read_text()
    if re.search(r"^RSF_BRIEFING_CAPTURE_PREFIX=|^RSF_DUMP_DIR=|[A-Z]:\\", config, re.M):
        raise RuntimeError("A developer capture setting or local path entered the package.")
    manifest = {
        "product": "ReScaleFrame for Ace Combat 7", "version": version, "source_commit": revision,
        "platform": "Windows x64", "streamline": "2.14.1", "dlss_runtime": "310.9.1.0",
        "fidelityfx_sdk": "2.3.0", "fsr_providers": ["2.3.4", "3.1.5", "4.1.1"],
        "xess_sdk": "3.0.2", "xess_sr_runtime": "2.0.2",
        "files": {p.relative_to(stage).as_posix(): digest(p) for p in sorted(stage.rglob("*")) if p.is_file()},
    }
    (stage / "ReScaleFrame/manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    archive = output / f"ReScaleFrame-{version}-AC7-Windows-x64.zip"
    stamp = time.gmtime(int(git("show", "-s", "--format=%ct", "HEAD")))[:6]
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
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
    source_archive = output / f"ReScaleFrame-{version}-source.zip"
    subprocess.run(["git", "archive", "--format=zip", f"--prefix=ReScaleFrame-{version}/",
                    "-o", str(source_archive), revision], cwd=ROOT, check=True)
    sums = output / "SHA256SUMS.txt"
    sums.write_text("".join(f"{digest(p)}  {p.name}\n" for p in (archive, source_archive)), encoding="ascii")
    print(json.dumps({"archive": str(archive), "source": str(source_archive),
                      "checksums": str(sums), "stage": str(stage), "revision": revision}, indent=2))


if __name__ == "__main__":
    main()
