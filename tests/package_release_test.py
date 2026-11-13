"""Release orchestration tests use a private Git fixture and stub integration packagers."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("package_release", ROOT / "tools/package-release.py")
package = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = package
SPEC.loader.exec_module(package)


class ReleasePackageTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "repo"
        self.root.mkdir()
        self.root_patch = mock.patch.object(package, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        self.write(".gitignore", "build/\ntarget/\nvendor/\nsdk/\ngames/minecraft/build/\n")
        self.write("VERSION", "2.3.4\n")
        self.write("Cargo.toml", '[workspace.package]\nversion = "2.3.4"\n')
        self.write("games/minecraft/gradle.properties", "mod_version=9.8.7\n")
        self.write("docs/releases/release-notes-template.md",
                   (ROOT / "docs/releases/release-notes-template.md").read_text(encoding="utf-8"))
        self.git("init", "-q")
        self.git("config", "core.autocrlf", "false")
        self.git("config", "user.name", "Release fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("add", ".")
        self.git("commit", "-qm", "release fixture")
        self.revision = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", self.revision)
        self.args = argparse.Namespace(streamline_root=self.root / "sdk", output=self.root / "build/releases/2.3.4",
                                       build=False, dry_run=False, powershell="pwsh",
                                       unity_managed_directory=self.root / "references")
        for path in package.required_outputs("9.8.7"):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture-" + path.name.encode())
        overlay = self.root / "target/release/rescaleframe_overlay.dll"
        shutil.copyfile(overlay, self.root / "build/windows-x64/bin/Release/rescaleframe_overlay.dll")
        native = self.root / "build/windows-x64/bin/Release/ReScaleFrame.Minecraft.dll"
        payload = {"ReScaleFrame.Minecraft.dll": native.read_bytes()}
        for name, path in package.minecraft_inputs(self.args).items():
            if name == "ReScaleFrame.Minecraft.dll":
                continue
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("sdk-" + name, encoding="utf-8")
            payload[name] = path.read_bytes()
        self.jar = self.root / "games/minecraft/build/libs/rescaleframecraft-9.8.7.jar"
        with zipfile.ZipFile(self.jar, "w") as bundle:
            bundle.writestr("META-INF/neoforge.mods.toml", '[[mods]]\nmodId="rescaleframecraft"\nversion="9.8.7"\n')
            for name, value in payload.items():
                bundle.writestr("native/windows-x64/" + name, value)
            bundle.writestr("native/windows-x64/files.sha256", "".join(
                hashlib.sha256(value).hexdigest() + "  " + name + "\n" for name, value in payload.items()))
        stamp = int(self.git("show", "-s", "--format=%ct", "HEAD")) + 5
        for path in package.required_outputs("9.8.7"):
            os.utime(path, (stamp, stamp))
        self.addition = self.write("vendor/shader-patch/yyjson/src/yyjson.c", "pinned fixture")
        self.loader = mock.patch.object(package, "load_packager", return_value=mock.Mock(
            source_additions=lambda: {"vendor/shader-patch/yyjson/src/yyjson.c": self.addition}))
        self.loader.start()
        self.addCleanup(self.loader.stop)
        self.packagers = mock.patch.object(package, "run_packager", side_effect=self.stub_packager)
        self.runner = self.packagers.start()
        self.addCleanup(self.packagers.stop)

    def write(self, name, value):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value, encoding="utf-8", newline="\n")
        return path

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, encoding="utf-8").strip()

    def stub_packager(self, game, output, args, revision, version):
        output.mkdir()
        archive = output / f"ReScaleFrame-{version}-{game}-Windows-x64.zip"
        manifest = ({"source_baseline": revision, "version": version, "source_has_uncommitted_changes": False}
                    if game == "DragNWash" else {"source_commit": revision, "version": version})
        with zipfile.ZipFile(archive, "w") as bundle:
            bundle.writestr("package.json" if game == "DragNWash" else "ReScaleFrame/manifest.json", json.dumps(manifest))
        # Legacy sidecars must never escape private integration staging.
        (output / "per-game-source.zip").write_bytes(b"discard")
        (output / "SHA256SUMS.txt").write_text("discard", encoding="ascii")
        return archive

    def test_unified_assets_source_checksums_notes_and_json(self):
        result = package.assemble(self.args)
        expected = package.asset_names("2.3.4", "9.8.7")
        self.assertEqual([asset["name"] for asset in result["assets"]], expected)
        self.assertEqual(result["commit"], self.revision)
        self.assertEqual(result["tag"], "v2.3.4")
        self.assertEqual({path.name for path in self.args.output.iterdir()}, set(expected) | {"SHA256SUMS.txt", "release-notes.md"})
        sums = (self.args.output / "SHA256SUMS.txt").read_text().splitlines()
        self.assertEqual(len(sums), 6)
        for asset, line in zip(result["assets"], sums):
            path = self.args.output / asset["name"]
            self.assertEqual(line, package.digest(path) + "  " + path.name)
            self.assertEqual(asset["size"], path.stat().st_size)
        with zipfile.ZipFile(self.args.output / expected[-1]) as bundle:
            self.assertEqual(bundle.read("ReScaleFrame-2.3.4/VERSION"), b"2.3.4\n")
            self.assertEqual(bundle.read("ReScaleFrame-2.3.4/vendor/shader-patch/yyjson/src/yyjson.c"), b"pinned fixture")
            self.assertFalse(any(".git/" in name for name in bundle.namelist()))
        notes = (self.args.output / "release-notes.md").read_text(encoding="utf-8")
        self.assertEqual(notes.count("**Played on this build: not yet played.**"), 5)
        for game in ("Ace Combat 7", "Red Dead Redemption 2", "Anomaly", "Drag'n Wash", "Minecraft"):
            self.assertIn(game, notes)
        self.assertNotIn("per-game-source.zip", json.dumps(result))
        self.assertEqual(self.runner.call_count, 4)

    def test_dirty_refused_before_build_or_staging(self):
        self.write("VERSION", "2.3.5\n")
        self.args.build = True
        with mock.patch.object(package, "build_outputs") as build:
            with self.assertRaisesRegex(RuntimeError, "dirty tree"):
                package.assemble(self.args)
            build.assert_not_called()
        self.runner.assert_not_called()
        self.assertFalse(self.args.output.parent.exists())

    def test_stale_and_missing_outputs_listed(self):
        paths = package.required_outputs("9.8.7")
        os.utime(paths[0], (1, 1))
        paths[1].unlink()
        with self.assertRaisesRegex(RuntimeError, "Missing:.*AC7.dll") as error:
            package.assemble(self.args)
        self.assertIn("Older than commit: build/windows-x64/bin/Release/ReScaleFrame.Runtime.dll", str(error.exception))
        self.runner.assert_not_called()

    def test_dry_run_lists_full_set_and_writes_nothing(self):
        self.args.dry_run = True
        result = package.assemble(self.args)
        self.assertEqual(len(result["assets"]), 6)
        self.assertTrue(all(asset["sha256"] is None for asset in result["assets"]))
        self.assertFalse(self.args.output.exists())
        self.runner.assert_not_called()

    def test_wrong_asset_name_refused_without_published_directory(self):
        original = self.stub_packager
        def wrong(*args):
            path = original(*args)
            renamed = path.with_name("wrong-version.zip")
            path.rename(renamed)
            return renamed
        self.runner.side_effect = wrong
        with self.assertRaisesRegex(RuntimeError, "asset name"):
            package.assemble(self.args)
        self.assertFalse(self.args.output.exists())

    def test_commit_mismatch_refused(self):
        original = self.stub_packager
        def wrong(game, output, args, revision, version):
            return original(game, output, args, "0" * 40, version)
        self.runner.side_effect = wrong
        with self.assertRaisesRegex(RuntimeError, "source identity"):
            package.assemble(self.args)
        self.assertFalse(self.args.output.exists())

    def test_mutating_build_input_refused(self):
        original = self.stub_packager
        def mutate(*args):
            path = original(*args)
            self.write("build/windows-x64/bin/Release/ReScaleFrame.Game.AC7.dll", "changed")
            return path
        self.runner.side_effect = mutate
        with self.assertRaisesRegex(RuntimeError, "Build input changed"):
            package.assemble(self.args)
        self.assertFalse(self.args.output.exists())

    def test_source_change_mid_assembly_refused(self):
        original = self.stub_packager
        def mutate(*args):
            path = original(*args)
            self.write("VERSION", "changed")
            return path
        self.runner.side_effect = mutate
        with self.assertRaisesRegex(RuntimeError, "source changed"):
            package.assemble(self.args)
        self.assertFalse(self.args.output.exists())

    def test_stale_embedded_minecraft_native_refused(self):
        self.write("build/windows-x64/bin/Release/ReScaleFrame.Minecraft.dll", "new native")
        with self.assertRaisesRegex(RuntimeError, "Minecraft embeds stale"):
            package.assemble(self.args)

    def test_stale_embedded_minecraft_vendor_refused(self):
        self.write("vendor/xess/bin/libxell.dll", "different SDK")
        with self.assertRaisesRegex(RuntimeError, "Minecraft embeds stale/different release input: xess/libxell.dll"):
            package.assemble(self.args)

    def test_renamed_wrong_mod_version_refused(self):
        with zipfile.ZipFile(self.jar) as bundle:
            contents = {name: bundle.read(name) for name in bundle.namelist()}
        contents["META-INF/neoforge.mods.toml"] = b'[[mods]]\nmodId="rescaleframecraft"\nversion="old"\n'
        with zipfile.ZipFile(self.jar, "w") as bundle:
            for name, data in contents.items():
                bundle.writestr(name, data)
        with self.assertRaisesRegex(RuntimeError, "metadata differs"):
            package.assemble(self.args)

    def test_cli_reports_json(self):
        self.args.dry_run = True
        import io
        import contextlib
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(package.main(["--streamline-root", str(self.args.streamline_root), "--dry-run"]), 0)
        self.assertEqual(json.loads(output.getvalue())["commit"], self.revision)

    def test_existing_output_and_unmerged_main_refused(self):
        self.args.output.mkdir(parents=True)
        with self.assertRaisesRegex(RuntimeError, "already exists"):
            package.assemble(self.args)
        self.args.output.rmdir()
        self.git("update-ref", "-d", "refs/remotes/origin/main")
        with self.assertRaisesRegex(RuntimeError, "Fetch origin"):
            package.assemble(self.args)

    def test_build_opt_in_and_dry_build_conflict(self):
        self.args.build = True
        with mock.patch.object(package, "build_outputs") as build:
            package.assemble(self.args)
            build.assert_called_once_with(self.args)
        self.args.output = self.args.output.with_name("unused")
        self.args.dry_run = True
        with mock.patch.object(package, "build_outputs") as build:
            with self.assertRaisesRegex(RuntimeError, "cannot execute"):
                package.assemble(self.args)
            build.assert_not_called()

    def test_build_commands_include_managed_native_overlay_and_minecraft(self):
        self.write("build/windows-x64/CMakeCache.txt", "CMAKE_GENERATOR:INTERNAL=Visual Studio 18 2026\n")
        with mock.patch.object(package, "command") as run:
            package.build_outputs(self.args)
        calls = [[str(value) for value in call.args[0]] for call in run.call_args_list]
        self.assertEqual(calls[0], ["cargo", "build", "--release", "--locked"])
        self.assertEqual(calls[1], ["cmake", "--build", "build/windows-x64", "--config", "Release"])
        self.assertIn("-VS2026", calls[2])
        self.assertTrue(any("stage-native.ps1" in value for value in calls[3]))
        self.assertIn("build", calls[4])
        self.assertIn("-Dorg.gradle.jvmargs=-Xmx768m", calls[4])


class ExistingPackagerCompatibilityTest(unittest.TestCase):
    def test_standalone_dirty_guards_and_internal_commit_guard(self):
        for game in ("AC7", "RDR2", "Anomaly"):
            module = package.load_packager(game)
            with self.subTest(game=game):
                owner = module if game == "Anomaly" else module if game == "AC7" else module.ac7
                with mock.patch.object(owner, "git", return_value=" M source"):
                    with self.assertRaisesRegex(RuntimeError, "Commit"):
                        module.main(["--streamline-root", "unused"])
                with mock.patch.object(module.ac7 if game != "AC7" else module, "git", return_value="new"):
                    with self.assertRaisesRegex(RuntimeError, "commit changed"):
                        module.main(["--streamline-root", "unused"], release_revision="old", game_only=True)

    def test_pinned_source_additions_refuse_mutation(self):
        module = package.load_packager("Anomaly")
        with mock.patch.object(module, "digest", return_value="0" * 64):
            with self.assertRaisesRegex(RuntimeError, "differs from its pin"):
                module.source_additions()


if __name__ == "__main__":
    unittest.main()
