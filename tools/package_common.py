"""Hashing, copying, git and licence-notice helpers shared by the tools/package-*.py packagers."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
# The Streamline runtime every packager copies.
RUNTIME = ("sl.interposer.dll", "sl.common.dll", "sl.dlss.dll", "sl.pcl.dll",
           "sl.dlss_g.dll", "sl.reflex.dll", "nvngx_dlss.dll", "nvngx_dlssg.dll")


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True, encoding="utf-8").strip()


def digest_stream(stream) -> str:
    value = hashlib.sha256()
    for data in iter(lambda: stream.read(1024 * 1024), b""):
        value.update(data)
    return value.hexdigest()


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return digest_stream(stream)


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


def release_identity(revision=None):
    """Standalone calls check cleanliness; a release-set caller supplies its checked commit."""
    if revision is None and git("status", "--porcelain"):
        raise RuntimeError("Commit the intended release source before packaging.")
    current = git("rev-parse", "HEAD")
    if revision is not None and revision != current:
        raise RuntimeError("Release source commit changed during packaging.")
    return current
