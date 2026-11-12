"""First-party tests. Licensed shaders are read only from the local corpus."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import shutil
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[1]
SOURCE = REPO / ".local/anomaly/vanilla/shaders/r3"
TOOL = REPO / "tools/anomaly_foliage_shader.py"


def load_tool():
    if not TOOL.exists():
        return None
    spec = importlib.util.spec_from_file_location("anomaly_foliage_shader", TOOL)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class TransformTests(unittest.TestCase):
    def test_real_corpus_transform_preserves_current_body_and_wave_branch(self):
        tool = load_tool()
        self.assertIsNotNone(tool, "foliage temporal transformation is missing")
        assert tool is not None
        if not SOURCE.exists():
            self.skipTest("licensed local fixture unavailable")
        patches, report = tool.transform(SOURCE)
        self.assertEqual(set(patches), {
            "deffer_tree_flat.vs", "deffer_tree_bump.vs",
            "deffer_tree_s_flat.vs", "deffer_tree_s_bump.vs",
            "deffer_detail_w_flat.vs", "deffer_detail_s_flat.vs",
            "rsf_anomaly_foliage.h"})
        self.assertFalse(report["runtime_ready"])
        self.assertEqual(report["blocked_roots"], ["deffer_tree_s_bump_d.vs"])
        for name, patched in patches.items():
            if not name.endswith(".vs"):
                continue
            original = (SOURCE / name).read_bytes().decode("utf-8-sig")
            original_body = tool.entrypoint(original)["body"]
            current = tool.entrypoint(patched.decode(), "rsf_foliage_current")
            self.assertEqual(current["body"], original_body)
            previous = tool.entrypoint(patched.decode(), "rsf_foliage_previous")
            if "tree" in name:
                if "_s_" not in name:
                    self.assertIn("USE_TREEWAVE", previous["body"])
                self.assertIn("rsf_previous_tree_xform", previous["body"])
            else:
                self.assertIn("rsf_previous_rows", previous["body"])
            self.assertIn(b"rsf_previous_clip", patched)
            self.assertIn(b"rsf_valid", patched)


class StageTests(unittest.TestCase):
    def test_stage_is_bounded_readback_verified_and_rejects_modified_source(self):
        tool = load_tool()
        assert tool is not None
        self.assertTrue(hasattr(tool, "stage"), "bounded foliage staging missing")
        if not SOURCE.exists():
            self.skipTest("licensed local fixture unavailable")
        staging = REPO / ".local/anomaly/staging"
        staging.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=staging, prefix="foliage-test-") as folder:
            root = Path(folder)
            output = root / "generated"
            manifest = tool.stage(SOURCE, output)
            self.assertFalse(manifest["runtime_ready"])
            self.assertEqual(len(manifest["changed_files"]), 7)
            self.assertEqual((output / "common_iostructs.h").read_bytes(),
                             (SOURCE / "common_iostructs.h").read_bytes())
            self.assertEqual((output / "deffer_tree_flat_d.vs").read_bytes(),
                             (SOURCE / "deffer_tree_flat_d.vs").read_bytes())
            with self.assertRaises(tool.Refusal):
                tool.stage(SOURCE, output)
            with self.assertRaises(tool.Refusal):
                tool.stage(SOURCE, REPO / "tests/anomaly_foliage_shader_forbidden")
            changed = root / "changed-source"
            shutil.copytree(SOURCE, changed)
            with (changed / "deffer_tree_flat.vs").open("ab") as stream:
                stream.write(b" ")
            with self.assertRaises(tool.Refusal):
                tool.stage(changed, root / "bad-output")
            self.assertFalse((root / "bad-output").exists())
            # A pinned source copy can itself live under the permitted local root.
            pinned = root / "pinned-source"
            shutil.copytree(SOURCE, pinned)
            with patch.object(tool.shutil, "copytree", side_effect=AssertionError("nested source must be refused before copying")):
                with self.assertRaises(tool.Refusal):
                    tool.stage(pinned, pinned / "nested-output")
            self.assertFalse((pinned / "nested-output").exists())

    def test_source_race_cannot_redefine_expected_stage_bytes(self):
        tool = load_tool()
        assert tool is not None
        if not SOURCE.exists():
            self.skipTest("licensed local fixture unavailable")
        staging = REPO / ".local/anomaly/staging"
        staging.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=staging, prefix="foliage-race-") as folder:
            root = Path(folder)
            source = root / "source"
            shutil.copytree(SOURCE, source)
            original_copy = shutil.copytree
            def raced_copy(src, dst, *args, **kwargs):
                result = original_copy(src, dst, *args, **kwargs)
                if Path(src) == source:
                    for base in (source, Path(dst)):
                        asset = base / "common.h"
                        asset.write_bytes(asset.read_bytes() + b" ")
                return result
            output = root / "output"
            with patch.object(tool.shutil, "copytree", raced_copy):
                with self.assertRaises(tool.Refusal):
                    tool.stage(source, output)
            self.assertFalse(output.exists())
            raced_destination = root / "raced-destination"
            original_mkdir = Path.mkdir
            def raced_mkdir(path, *args, **kwargs):
                if path == raced_destination:
                    original_mkdir(path, parents=True)
                    (path / "other-owner.txt").write_text("must survive")
                    raise FileExistsError("another owner created destination")
                return original_mkdir(path, *args, **kwargs)
            with patch.object(Path, "mkdir", raced_mkdir):
                with self.assertRaises(FileExistsError):
                    tool.stage(SOURCE, raced_destination)
            self.assertEqual((raced_destination / "other-owner.txt").read_text(), "must survive")

    def test_cli_emits_content_addressed_family_abi_and_refuses_existing_output(self):
        if not SOURCE.exists():
            self.skipTest("licensed local fixture unavailable")
        staging = REPO / ".local/anomaly/staging"
        staging.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=staging, prefix="foliage-cli-") as folder:
            output = Path(folder) / "generated"
            command = [sys.executable, str(TOOL), "--source", str(SOURCE), "--output", str(output)]
            run = subprocess.run(command, capture_output=True, text=True, timeout=30)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            report = json.loads(run.stdout)
            self.assertIn("cache_namespace", report, "replacement cache must pin transform and injected header")
            self.assertEqual(len(report["cache_namespace"]), 64)
            self.assertEqual(report["tool_sha256"], hashlib.sha256(TOOL.read_bytes()).hexdigest())
            self.assertEqual(report["abi"], 1)
            self.assertEqual(report["previous_valid_member"], "rsf_valid")
            again = subprocess.run(command, capture_output=True, text=True, timeout=30)
            self.assertEqual(again.returncode, 2)


def prepare_compiler_fixture(output):
    """Merge independent worker artifacts ONLY inside the local test staging."""
    tool = load_tool()
    assert tool is not None
    tool.stage(SOURCE, output)
    core_path = REPO / "tools/anomaly_temporal_shader.py"
    if not core_path.exists():
        raise unittest.SkipTest("shared core implementation unavailable; no fake integration")
    spec = importlib.util.spec_from_file_location("rsf_core_fixture", core_path)
    assert spec is not None and spec.loader is not None
    core = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(core)
    shared = output / "common_iostructs.h"
    shared.write_bytes(core.shared_io(shared.read_bytes()))
    for header in core.OWNED.glob("*.h"):
        shutil.copyfile(header, output / header.name)
    for kind, shape in (("flat", "p_flat"), ("bump", "p_bumped")):
        for alpha in (False, True):
            name = "deffer_base_" + ("aref_" if alpha else "") + kind + ".ps"
            (output / name).write_bytes(core.pixel((SOURCE / name).read_bytes(), shape, alpha))


def find_fxc():
    sdk = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Windows Kits/10/bin"
    choices = sorted(sdk.glob("*/x64/fxc.exe"), reverse=True)
    return os.environ.get("RSF_FXC") or shutil.which("fxc") or (str(choices[0]) if choices else None)


def compile_shader(fxc, source, output, name, target="vs_5_0", defines=()):
    command = [str(fxc), "/nologo", "/T", target, "/E", "main", "/D", "SM_5=1",
               "/I", str(source), "/Fo", str(output.with_suffix(".dxbc")),
               "/Fc", str(output.with_suffix(".asm"))]
    for define in defines:
        command += ["/D", define]
    run = subprocess.run(command + [str(source / name)], capture_output=True, text=True, timeout=30)
    if run.returncode:
        raise AssertionError(name + "\n" + run.stdout + run.stderr)
    return output.with_suffix(".asm").read_text(), output.with_suffix(".dxbc").read_bytes()


class CompilerTests(unittest.TestCase):
    def test_real_fxc_variants_link_shared_contract_and_change_dxbc(self):
        tool = load_tool()
        assert tool is not None
        fxc = find_fxc()
        if not fxc or not SOURCE.exists():
            self.skipTest("FXC or licensed local corpus unavailable")
        staging = REPO / ".local/anomaly/staging"
        staging.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=staging, prefix="foliage-fxc-") as folder:
            folder = Path(folder)
            output = folder / "merged"
            prepare_compiler_fixture(output)
            names = sorted(path.name for path in SOURCE.glob("deffer_tree*.vs")
                           if path.name != "deffer_tree_s_bump_d.vs")
            names += ["deffer_detail_w_flat.vs", "deffer_detail_s_flat.vs"]
            variants = []
            for index, name in enumerate(names):
                original_asm, original = compile_shader(fxc, SOURCE, folder / f"original-{index}", name)
                assembly, changed = compile_shader(fxc, output, folder / f"changed-{index}", name)
                self.assertNotEqual(original, changed)
                self.assertRegex(assembly, r"TEXCOORD\s+12")
                self.assertRegex(assembly, r"TEXCOORD\s+13")
                self.assertRegex(assembly, r"RSFFoliagePrevious\s+cbuffer\s+NA\s+NA\s+cb13\s+1")
                def inputs(text):
                    return text.split("// Input signature:")[1].split("// Output signature:")[0]
                self.assertEqual(inputs(original_asm), inputs(assembly))
                self.assertRegex(assembly, r"rsf_previous_rows\[244\];\s*// Offset:\s*176 Size:\s*3904")
                self.assertRegex(assembly, r"rsf_previous_item_state\[61\];\s*// Offset:\s*4080 Size:\s*976")
                variants.append({"shader": name, "previous_cb13": True, "target": "vs_5_0", "defines": ["SM_5=1"],
                                 "reflection": assembly.split("\nvs_5_0")[0],
                                 "original_sha256": hashlib.sha256(original).hexdigest(),
                                 "changed_sha256": hashlib.sha256(changed).hexdigest()})
            pixels = []
            for index, name in enumerate(("deffer_base_flat.ps", "deffer_base_aref_flat.ps",
                                          "deffer_base_bump.ps", "deffer_base_aref_bump.ps")):
                assembly, bytecode = compile_shader(fxc, output, folder / f"pixel-{index}", name, "ps_5_0")
                self.assertRegex(assembly, r"SV_Target\s+2")
                pixels.append({"shader": name, "target": "ps_5_0", "defines": ["SM_5=1"],
                               "reflection": assembly.split("\nps_5_0")[0],
                               "sha256": hashlib.sha256(bytecode).hexdigest()})
            (REPO / ".local/anomaly/foliage-compile-report.json").write_text(
                json.dumps({"compiler": str(fxc), "runtime_ready": False, "variants": variants, "pixels": pixels,
                            "source_corpus_sha256": tool.CORPUS_SHA256,
                            "foliage_tool_sha256": hashlib.sha256(TOOL.read_bytes()).hexdigest(),
                            "core_tool_sha256": hashlib.sha256((REPO / "tools/anomaly_temporal_shader.py").read_bytes()).hexdigest(),
                            "core_headers_sha256": {header.name: hashlib.sha256(header.read_bytes()).hexdigest()
                                                    for header in (REPO / "games/anomaly/shaders/core").glob("*.h")},
                            "shared_core": "actual worker shared_io and pixel transformations"}, indent=2) + "\n")
            self.assertEqual(len(variants), 13)


class DeviceTests(unittest.TestCase):
    def test_real_fxc_vertex_programs_execute_unchanged_current_and_previous_deformation(self):
        cpp = REPO / "tests/anomaly_foliage_shader_device.cpp"
        self.assertTrue(cpp.exists(), "real foliage device fixture is missing")
        fxc = find_fxc()
        compiler = shutil.which("g++")
        if not fxc or not compiler or not SOURCE.exists():
            self.skipTest("Windows D3D11 fixture requires FXC, g++ and licensed corpus")
        staging = REPO / ".local/anomaly/staging"
        staging.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=staging, prefix="foliage-device-") as folder:
            folder = Path(folder)
            output = folder / "merged"
            prepare_compiler_fixture(output)
            cases = [("tree", "deffer_tree_flat.vs", ()),
                     ("tree_bump", "deffer_tree_bump.vs", ()),
                     ("tree_disabled", "deffer_tree_flat.vs", ("USE_TREEWAVE=1",)),
                     ("tree_still", "deffer_tree_s_flat.vs", ()),
                     ("tree_still_bump", "deffer_tree_s_bump.vs", ()),
                     ("detail_wave", "deffer_detail_w_flat.vs", ()),
                     ("detail_still", "deffer_detail_s_flat.vs", ())]
            for tag, name, defines in cases:
                compile_shader(fxc, SOURCE, folder / (tag + "-original"), name, defines=defines)
                compile_shader(fxc, output, folder / (tag + "-changed"), name, defines=defines)
            for kind in ("flat", "bump"):
                name = "deffer_base_" + kind + ".ps"
                compile_shader(fxc, SOURCE, folder / ("pixel-" + kind + "-original"), name, "ps_5_0")
                compile_shader(fxc, output, folder / ("pixel-" + kind + "-changed"), name, "ps_5_0")
            exe = folder / "fixture.exe"
            build = subprocess.run([compiler, "-std=c++20", "-Wall", "-Wextra", "-Werror", "-O2", "-static",
                                    str(cpp), "-o", str(exe), "-ld3d11", "-ld3dcompiler", "-ldxguid"],
                                   capture_output=True, text=True, timeout=120)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(exe), str(folder)], capture_output=True, text=True, timeout=90)
            (REPO / ".local/anomaly/foliage-device-report.txt").write_text(run.stdout + run.stderr)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn("previous-state GPU reference matches", run.stdout)
            self.assertIn("missing history invalidates", run.stdout)
            self.assertIn("MRT2 rendered", run.stdout)


if __name__ == "__main__":
    unittest.main()
