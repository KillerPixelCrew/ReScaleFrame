#!/usr/bin/env python3
"""Assemble every shipping integration from one clean RSF commit, without publishing."""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
GAMES = ("AC7", "RDR2", "Anomaly", "DragNWash")


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True, encoding="utf-8").strip()


def digest_stream(stream):
    value = hashlib.sha256()
    for data in iter(lambda: stream.read(1024 * 1024), b""):
        value.update(data)
    return value.hexdigest()


def digest(path):
    with path.open("rb") as stream:
        return digest_stream(stream)


def require_clean():
    if git("status", "--porcelain", "--untracked-files=all"):
        raise RuntimeError("Commit the intended release source before packaging (dirty tree).")
    return git("rev-parse", "HEAD")


def require_unchanged(revision):
    if git("rev-parse", "HEAD") != revision:
        raise RuntimeError("Source commit changed during release assembly.")
    for args in (("diff", "--quiet", revision, "--"), ("diff", "--cached", "--quiet", revision, "--")):
        if subprocess.run(["git", *args], cwd=ROOT).returncode:
            raise RuntimeError("Tracked source changed during release assembly.")
    if git("ls-files", "--others", "--exclude-standard"):
        raise RuntimeError("Untracked source appeared during release assembly.")


def require_main_merged(revision):
    result = subprocess.run(["git", "merge-base", "--is-ancestor", "origin/main", revision],
                            cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if result.returncode:
        raise RuntimeError("Fetch origin and merge origin/main before preparing a release.")


def versions():
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise RuntimeError("VERSION is not a safe RSF release version.")
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    if cargo["workspace"]["package"]["version"] != version:
        raise RuntimeError("VERSION and the Cargo workspace version differ.")
    props = (ROOT / "games/minecraft/gradle.properties").read_text(encoding="utf-8")
    match = re.search(r"^mod_version\s*=\s*([0-9A-Za-z.+-]+)\s*$", props, re.M)
    if not match:
        raise RuntimeError("Missing safe Minecraft mod_version.")
    return version, match[1]


def asset_names(version, mod_version):
    return [f"ReScaleFrame-{version}-{game}-Windows-x64.zip" for game in GAMES] + [
        f"rescaleframecraft-{mod_version}.jar", f"ReScaleFrame-{version}-source.zip"]


def required_outputs(mod_version):
    native = ROOT / "build/windows-x64/bin/Release"
    return [native / name for name in (
        "ReScaleFrame.Runtime.dll", "ReScaleFrame.Game.AC7.dll", "ReScaleFrame.Game.RDR2.dll",
        "ReScaleFrame.Game.Anomaly.dll", "ReScaleFrame.Game.UnityMono.dll", "ReScaleFrame.Unity.Managed.dll",
        "ReScaleFrame.Loader.dll", "ReScaleFrame.Minecraft.dll", "rescaleframe_overlay.dll")] + [
        ROOT / "build/windows-x64/ac7/bin/Release/dinput8.dll",
        ROOT / "build/windows-x64/thin/bin/Release/dinput8.dll",
        ROOT / "build/windows-x64/rdr2/bin/Release/ReScaleFrame.RDR2.asi",
        ROOT / "target/release/rescaleframe_overlay.dll",
        ROOT / f"games/minecraft/build/libs/rescaleframecraft-{mod_version}.jar"]


def check_outputs(mod_version, commit_time, built=False):
    paths = required_outputs(mod_version)
    missing = [path.relative_to(ROOT).as_posix() for path in paths if not path.is_file()]
    older = [path.relative_to(ROOT).as_posix() for path in paths if path.is_file() and path.stat().st_mtime < commit_time]
    if missing or (older and not built):
        raise RuntimeError("Release outputs are not fresh; run --build.\n" +
                           "\n".join([*("Missing: " + name for name in missing),
                                      *("Older than commit: " + name for name in older)]))
    native_overlay = ROOT / "build/windows-x64/bin/Release/rescaleframe_overlay.dll"
    if digest(native_overlay) != digest(ROOT / "target/release/rescaleframe_overlay.dll"):
        raise RuntimeError("Unity's staged overlay differs from the Release Rust overlay; rebuild/stage it.")
    return older


def command(args, cwd=None):
    subprocess.run([str(arg) for arg in args], cwd=cwd or ROOT, check=True, stdout=sys.stderr)


def build_outputs(args):
    # This is an explicit opt-in build, never part of a packaging-only or dry run.
    command(["cargo", "build", "--release", "--locked"])
    command(["cmake", "--build", "build/windows-x64", "--config", "Release"])
    unity = [args.powershell, "-NoProfile", "-File", ROOT / "eng/build-unity-sr.ps1",
             "-UnityManagedDirectory", args.unity_managed_directory]
    cache = (ROOT / "build/windows-x64/CMakeCache.txt").read_text(encoding="utf-8")
    if "Visual Studio 18" in cache:
        unity.append("-VS2026")
    command(unity)
    shutil.copyfile(ROOT / "target/release/rescaleframe_overlay.dll",
                    ROOT / "build/windows-x64/bin/Release/rescaleframe_overlay.dll")
    command([args.powershell, "-NoProfile", "-File", ROOT / "games/minecraft/eng/stage-native.ps1"])
    command([ROOT / "games/minecraft/gradlew.bat", "build", "--no-daemon", "--max-workers=1",
             "-Dorg.gradle.jvmargs=-Xmx768m"], cwd=ROOT / "games/minecraft")


def load_packager(game):
    filename = {"AC7": "ac7", "RDR2": "rdr2", "Anomaly": "anomaly"}[game]
    spec = importlib.util.spec_from_file_location("release_package_" + filename, ROOT / f"tools/package-{filename}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def run_packager(game, output, args, revision, version):
    output.mkdir()
    expected = output / f"ReScaleFrame-{version}-{game}-Windows-x64.zip"
    if game == "DragNWash":
        command([args.powershell, "-NoProfile", "-File", ROOT / "eng/package-unity-sr.ps1",
                 "-OutputDirectory", output, "-Python", sys.executable, "-StreamlineRoot", args.streamline_root])
    else:
        with contextlib.redirect_stdout(sys.stderr):
            result = load_packager(game).main(["--streamline-root", str(args.streamline_root),
                                               "--output", str(output)],
                                              release_revision=revision, game_only=True)
        if Path(result["archive"]) != expected or result["revision"] != revision:
            raise RuntimeError("Integration packager returned a different artifact or commit: " + game)
    verify_game(expected, revision, version, game)
    return expected


def safe_name(name):
    path = PurePosixPath(name)
    if not name or "\\" in name or ":" in name or path.is_absolute() or any(
            part in ("", ".", "..") for part in name.split("/")):
        raise RuntimeError("Unsafe archive name: " + name)
    return path


def verify_zip(path):
    with zipfile.ZipFile(path) as bundle:
        if len(bundle.namelist()) != len(set(bundle.namelist())) or bundle.testzip() is not None:
            raise RuntimeError("Archive duplicate entry or CRC failure: " + path.name)
        for entry in bundle.infolist():
            safe_name(entry.filename.rstrip("/") if entry.is_dir() else entry.filename)


def verify_game(path, revision, version, game):
    verify_zip(path)
    with zipfile.ZipFile(path) as bundle:
        if game == "DragNWash":
            manifest = json.loads(bundle.read("package.json"))
            if (manifest["source_baseline"] != revision or manifest["version"] != version or
                    manifest["source_has_uncommitted_changes"]):
                raise RuntimeError("Unity package source identity differs from the release commit.")
        else:
            manifest = json.loads(bundle.read("ReScaleFrame/manifest.json"))
            if manifest["source_commit"] != revision or manifest["version"] != version:
                raise RuntimeError("Game package source identity differs from the release commit: " + game)


def minecraft_inputs(args):
    inputs = {"ReScaleFrame.Minecraft.dll": ROOT / "build/windows-x64/bin/Release/ReScaleFrame.Minecraft.dll"}
    inputs.update({"streamline/" + name: args.streamline_root / "bin/x64" / name for name in (
        "sl.interposer.dll", "sl.common.dll", "sl.dlss.dll", "sl.dlss_g.dll", "sl.reflex.dll", "sl.pcl.dll",
        "nvngx_dlss.dll", "nvngx_dlssg.dll")})
    inputs.update({"fidelityfx/" + name: ROOT / "vendor/fidelityfx/Kits/FidelityFX/signedbin" / name for name in (
        "amd_fidelityfx_loader_dx12.dll", "amd_fidelityfx_upscaler_dx12.dll", "amd_fidelityfx_framegeneration_dx12.dll")})
    inputs.update({"xess/" + name: ROOT / "vendor/xess/bin" / name for name in ("libxess.dll", "libxess_fg.dll", "libxell.dll")})
    return inputs


def verify_minecraft(path, args, mod_version):
    verify_zip(path)
    prefix = "native/windows-x64/"
    with zipfile.ZipFile(path) as bundle:
        metadata = tomllib.loads(bundle.read("META-INF/neoforge.mods.toml").decode("utf-8"))
        mods = [mod for mod in metadata.get("mods", []) if mod.get("modId") == "rescaleframecraft"]
        if len(mods) != 1 or mods[0].get("version") != mod_version:
            raise RuntimeError("Minecraft JAR metadata differs from mod_version.")
        entries = set()
        for line in bundle.read(prefix + "files.sha256").decode("utf-8-sig").splitlines():
            match = re.fullmatch(r"([0-9a-fA-F]{64})  (.+)", line)
            if not match:
                raise RuntimeError("Malformed Minecraft native payload manifest.")
            expected, name = match.groups()
            safe_name(name)
            if name in entries:
                raise RuntimeError("Duplicate Minecraft native payload entry.")
            entries.add(name)
            with bundle.open(prefix + name) as stream:
                if digest_stream(stream) != expected.lower():
                    raise RuntimeError("Minecraft native payload hash differs: " + name)
        actual = {name[len(prefix):] for name in bundle.namelist() if name.startswith(prefix) and not name.endswith("/")}
        if actual != entries | {"files.sha256"} or "ReScaleFrame.Minecraft.dll" not in entries:
            raise RuntimeError("Minecraft native payload does not match its manifest.")
        for name, source in minecraft_inputs(args).items():
            with bundle.open(prefix + name) as stream:
                if digest_stream(stream) != digest(source):
                    raise RuntimeError("Minecraft embeds stale/different release input: " + name)


def source_archive(output, revision, version, commit_time):
    command(["git", "archive", "--format=zip", f"--prefix=ReScaleFrame-{version}/", "-o", output, revision])
    stamp = time.gmtime(commit_time)[:6]
    additions = load_packager("Anomaly").source_additions()
    with zipfile.ZipFile(output, "a") as bundle:
        existing = set(bundle.namelist())
        for name, path in sorted(additions.items()):
            safe_name(name)
            name = f"ReScaleFrame-{version}/" + name
            if name in existing:
                raise RuntimeError("Source addition duplicates committed source: " + name)
            entry = zipfile.ZipInfo(name, stamp)
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = 0o100644 << 16
            bundle.writestr(entry, path.read_bytes(), compresslevel=9)
    verify_zip(output)


def release_notes(version, revision, mod_version):
    template = (ROOT / "docs/releases/release-notes-template.md").read_text(encoding="utf-8")
    return template.format(version=version, commit=revision, minecraft_version=mod_version)


def record(path):
    return {"name": path.name, "size": path.stat().st_size, "sha256": digest(path)}


def assemble(args):
    revision = require_clean()  # Before building, reading release inputs or creating any output.
    require_main_merged(revision)
    version, mod_version = versions()
    commit_time = int(git("show", "-s", "--format=%ct", revision))
    output = (args.output or ROOT / "build/releases" / version).resolve()
    if output.exists():
        raise RuntimeError("Release destination already exists: " + str(output))
    # The original Unity packager retains its independent clean-tree guard.
    if output.is_relative_to(ROOT):
        ignored = subprocess.run(["git", "check-ignore", "--quiet", str(output.relative_to(ROOT))], cwd=ROOT)
        if ignored.returncode:
            raise RuntimeError("Release output inside the repository must be gitignored.")
    if args.build and args.dry_run:
        raise RuntimeError("--dry-run cannot execute --build.")
    if args.build:
        build_outputs(args)
    reused = check_outputs(mod_version, commit_time, built=args.build)
    inputs = {path: digest(path) for path in set(required_outputs(mod_version)) | set(minecraft_inputs(args).values())}
    jar = ROOT / f"games/minecraft/build/libs/rescaleframecraft-{mod_version}.jar"
    verify_minecraft(jar, args, mod_version)
    # Check source additions even in a dry run, without emitting an archive.
    load_packager("Anomaly").source_additions()
    names = asset_names(version, mod_version)
    summary = {"version": version, "commit": revision, "tag": "v" + version,
               "minecraft_version": mod_version, "output": str(output), "dry_run": args.dry_run,
               "incremental_build_reused_outputs": reused}
    if args.dry_run:
        require_unchanged(revision)
        summary["assets"] = [{"name": name, "size": None, "sha256": None} for name in names]
        summary["checksums"] = "SHA256SUMS.txt"
        summary["notes"] = "release-notes.md"
        return summary
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".rsf-release-", dir=output.parent) as temporary:
        private = Path(temporary)
        final = private / "release"
        final.mkdir()
        for index, game in enumerate(GAMES):
            archive = run_packager(game, private / game, args, revision, version)
            if archive.name != names[index]:
                raise RuntimeError("Unexpected integration asset name: " + archive.name)
            verify_game(archive, revision, version, game)
            shutil.copyfile(archive, final / archive.name)
        shutil.copyfile(jar, final / jar.name)
        source_archive(final / names[-1], revision, version, commit_time)
        (final / "release-notes.md").write_text(release_notes(version, revision, mod_version), encoding="utf-8", newline="\n")
        assets = [record(final / name) for name in names]
        sums = final / "SHA256SUMS.txt"
        sums.write_text("".join(f'{asset["sha256"]}  {asset["name"]}\n' for asset in assets), encoding="ascii", newline="\n")
        # Detect changed build inputs, including the JAR, while child packagers were running.
        verify_minecraft(final / jar.name, args, mod_version)
        for path, expected in inputs.items():
            if digest(path) != expected:
                raise RuntimeError("Build input changed during release assembly: " + str(path))
        require_unchanged(revision)
        if output.exists():
            raise RuntimeError("Release destination appeared during assembly: " + str(output))
        final.rename(output)  # Complete set only; never replace an existing release.
        summary.update(assets=assets, checksums=record(output / sums.name), notes="release-notes.md")
    return summary


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--streamline-root", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--build", action="store_true", help="explicitly build all shipping outputs before packaging")
    parser.add_argument("--dry-run", action="store_true", help="check source/freshness and list planned assets; write nothing")
    parser.add_argument("--powershell", default="pwsh")
    parser.add_argument("--unity-managed-directory", type=Path, default=ROOT / ".local/unity-references/6000.3.14f1")
    args = parser.parse_args(argv)
    try:
        summary = assemble(args)
    except (RuntimeError, OSError, ValueError, KeyError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        print("Release packaging refused: " + str(error), file=sys.stderr)
        return 1
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
