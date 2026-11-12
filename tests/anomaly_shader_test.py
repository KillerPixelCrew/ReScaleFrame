"""First-party synthetic fixtures and optional read-only vanilla asset checks."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
TOOL = REPO / "tools/anomaly_shader_tool.py"


def load_tool():
    if not TOOL.is_file():
        return None
    spec = importlib.util.spec_from_file_location("anomaly_shader_tool", TOOL)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class InventoryTests(unittest.TestCase):
    def test_hashes_windows_includes_and_lexical_entry_inputs(self):
        tool = load_tool()
        self.assertIsNotNone(tool, "shader inventory tooling is not implemented")
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "shared").mkdir()
            # Independently authored examples, not copied game shaders.
            data = b'#include "SHARED\\types.h"\r\nOutput main(Input vertex) { return (Output)0; }\r\n'
            (root / "sample.vs").write_bytes(data)
            (root / "shared/types.h").write_text(
                '// float4 fake : SECRET;\nstruct Input { float3 point : POSITION0; };\n'
                'struct Output { float4 clip : SV_Position; };\n', encoding="utf-8")
            result = tool.inventory(root)
            self.assertEqual(result["file_count"], 2)
            self.assertFalse(result["runtime_ready"])
            item = next(x for x in result["files"] if x["path"] == "sample.vs")
            self.assertEqual(item["sha256"], hashlib.sha256(data).hexdigest())
            self.assertEqual(item["includes"][0]["resolved"], "shared/types.h")
            self.assertEqual(item["entrypoints"][0]["inputs"][0]["fields"][0]["semantic"], "POSITION0")
            self.assertNotIn("SECRET", str(result))
            self.assertEqual(result["semantic_status"], "lexical_candidates_not_compiler_reflection")

    def test_lua_comments_and_include_lines_do_not_create_false_evidence(self):
        tool = load_tool()
        assert tool is not None
        local = REPO / ".local/anomaly"
        local.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=local) as folder:
            root = Path(folder)
            (root / "fixture.s").write_text('-- shader:dx10texture("fake", "$user$ui")\n'
                                          'shader:dx10texture("real", "$user$ui")\n'
                                          'shader:dx10sampler("smp_fixture")\n')
            (root / "fixture.vs").write_text('\n\n#include "types.h"\n')
            (root / "types.h").write_text('SamplerState smp_fixture;\n')
            result = tool.inventory(root)
            files = {item["path"]: item for item in result["files"]}
            self.assertEqual(files["fixture.s"]["material_bindings"], [{"sampler": "real", "texture": "$user$ui"}])
            self.assertEqual(files["fixture.vs"]["includes"][0]["line"], 3)
            self.assertIn("material_samplers", files["fixture.s"], "material sampler inventory is missing")
            self.assertEqual(files["fixture.s"]["material_samplers"], ["smp_fixture"])
            self.assertEqual(files["types.h"]["samplers"], ["smp_fixture"])


    def test_real_fixture_matches_pinned_corpus_and_conditional_input_candidates(self):
        tool = load_tool()
        assert tool is not None
        root = REPO / ".local/anomaly/vanilla/shaders/r3"
        if not root.exists():
            self.skipTest("licensed local vanilla fixture is not available")
        result = tool.inventory(root)
        self.assertEqual(result["file_count"], 449)
        self.assertEqual(result["corpus_sha256"], "fdc260e0608f259d876c4ae0f11a6f2d8c03c001cf1fe82548794af1c6826a67")
        files = {item["path"]: item for item in result["files"]}
        inputs = files["deffer_base_flat.vs"]["entrypoints"][0]["inputs"]
        self.assertEqual({item["struct"] for item in inputs}, {"v_static", "v_static_color"})
        self.assertEqual(result["missing_includes"], [])
        self.assertIn(["deffer_tree_s_bump_d.vs", "deffer_tree_s_bump_d.vs"], result["include_cycles"])
        self.assertIn({"sampler": "s_pdascreen", "texture": "$user$ui"}, files["pda_overlay.s"]["material_bindings"])
        self.assertEqual(len(files["deffer_model_flat.vs"]["entrypoints"]), 6)


class TransformationTests(unittest.TestCase):
    def test_inserts_owned_include_only_after_exact_hash_and_anchor(self):
        tool = load_tool()
        self.assertIsNotNone(tool)
        self.assertTrue(hasattr(tool, "inject_include"), "bounded include insertion is missing")
        original = b"// first-party synthetic fixture\r\n#define EXAMPLE_GUARD\r\nfloat marker;\r\n"
        anchor = original.splitlines(keepends=True)[1]
        profile = {"source_sha256": hashlib.sha256(original).hexdigest(),
                   "anchor_line": 2, "anchor_sha256": hashlib.sha256(anchor).hexdigest()}
        transformed = tool.inject_include(original, profile, "rsf_anomaly_temporal.h")
        self.assertEqual(transformed, original[:original.index(b"float marker;")] +
                         b'#include "rsf_anomaly_temporal.h"\r\n' + b"float marker;\r\n")
        for changed, rule in ((original + b" ", profile),
                              (original, {**profile, "anchor_sha256": "0" * 64}),
                              (original, {**profile, "anchor_line": 0})):
            with self.assertRaises(tool.Refusal):
                tool.inject_include(changed, rule, "rsf_anomaly_temporal.h")
        for name in ("../bad.h", "a/b.h", 'bad"name.h', "C:evil.h"):
            with self.assertRaises(tool.Refusal):
                tool.inject_include(original, profile, name)


class StagingTests(unittest.TestCase):
    def test_stages_complete_pinned_tree_without_changing_originals(self):
        tool = load_tool()
        assert tool is not None
        self.assertTrue(hasattr(tool, "stage"), "verified staging transaction is missing")
        local = REPO / ".local/anomaly"
        local.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=local) as folder:
            work = Path(folder)
            source = work / "vanilla"
            source.mkdir()
            original = b"#define SYNTHETIC_GUARD\nfloat synthetic_value;\n"
            (source / "common.h").write_bytes(original)
            (source / "extra.ps").write_bytes(b"// independently authored fixture\n")
            injected = b"#define RSF_TEST_ONLY 1\n"
            profile = {"schema": 1, "file_count": 2,
                       "corpus_sha256": tool.tree_digest(tool.read_tree(source)),
                       "patch": {"path": "common.h", "source_sha256": tool.digest(original),
                                 "anchor_line": 1, "anchor_sha256": tool.digest(original.splitlines(keepends=True)[0])},
                       "include": {"name": "rsf_anomaly_temporal.h", "sha256": tool.digest(injected)}}
            # Test sandbox remains beneath the actual permitted local staging root.
            target = REPO / ".local/anomaly/staging" / ("test-" + work.name)
            try:
                report = tool.stage(source, target, profile, injected)
                self.assertFalse(report["runtime_ready"])
                self.assertEqual(report["source_file_count"], 2)
                self.assertEqual(report["staged_file_count"], 3)
                self.assertEqual((source / "common.h").read_bytes(), original)
                self.assertEqual((target / "extra.ps").read_bytes(), (source / "extra.ps").read_bytes())
                self.assertEqual((target / "rsf_anomaly_temporal.h").read_bytes(), injected)
                self.assertEqual(len(report["files"]), 3)
                with self.assertRaises(tool.Refusal):
                    tool.stage(source, target, profile, injected)
            finally:
                import shutil
                if target.exists():
                    shutil.rmtree(target)
            for bad_profile, bad_payload, bad_target in (
                ({**profile, "corpus_sha256": "0" * 64}, injected, target),
                (profile, injected + b" ", target),
                ({**profile, "patch": {**profile["patch"], "path": "../common.h"}}, injected, target),
                (profile, injected, work / "outside-staging"),
                ({**profile, "patch": {**profile["patch"], "anchor_sha256": "0" * 64}}, injected, target),
            ):
                with self.assertRaises(tool.Refusal):
                    tool.stage(source, bad_target, bad_profile, bad_payload)
                self.assertFalse(bad_target.exists())
            (source / "extra.ps").write_bytes(b"unexpected mod\n")
            with self.assertRaises(tool.Refusal):
                tool.stage(source, target, profile, injected)
            self.assertFalse(target.exists())


class CliTests(unittest.TestCase):
    def test_cli_stages_real_fixture_and_refuses_nonlocal_inventory_output(self):
        import subprocess
        import sys
        import json
        import shutil
        source = REPO / ".local/anomaly/vanilla/shaders/r3"
        if not source.exists():
            self.skipTest("licensed local vanilla fixture is not available")
        target = REPO / ".local/anomaly/staging/unittest-cli-real"
        self.assertFalse(target.exists(), "test destination already exists")
        try:
            run = subprocess.run([sys.executable, str(TOOL), "stage-include", "--source", str(source),
                                  "--output", str(target)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            report = json.loads((target / ".rsf-stage.json").read_text())
            self.assertEqual(report["source_file_count"], 449)
            self.assertEqual(report["staged_file_count"], 450)
            self.assertFalse(report["runtime_ready"])
            planned = subprocess.run([sys.executable, str(TOOL), "plan", "--source", str(source),
                                      "--output", str(target / "patch-plan.json")], capture_output=True, text=True)
            self.assertEqual(planned.returncode, 0, planned.stdout + planned.stderr)
            self.assertFalse(json.loads((target / "patch-plan.json").read_text())["runtime_ready"])
            for item in report["files"]:
                self.assertEqual(hashlib.sha256((target / item["path"]).read_bytes()).hexdigest(), item["staged_sha256"])
                if item["original_sha256"] is not None:
                    self.assertEqual(hashlib.sha256((source / item["path"]).read_bytes()).hexdigest(), item["original_sha256"])
            escaped = REPO / "tests/anomaly_shader_forbidden_output.json"
            run = subprocess.run([sys.executable, str(TOOL), "inventory", "--source", str(source),
                                  "--output", str(escaped)], capture_output=True, text=True)
            self.assertNotEqual(run.returncode, 0)
            self.assertFalse(escaped.exists())
        finally:
            if target.exists():
                shutil.rmtree(target)


class PlanningTests(unittest.TestCase):
    def test_real_plan_enumerates_wrappers_skin_modes_and_blocks_recursive_source(self):
        tool = load_tool()
        assert tool is not None
        self.assertTrue(hasattr(tool, "patch_plan"), "machine-readable previous-state planning is missing")
        root = REPO / ".local/anomaly/vanilla/shaders/r3"
        if not root.exists():
            self.skipTest("licensed local vanilla fixture is not available")
        plan = tool.patch_plan(tool.inventory(root))
        self.assertFalse(plan["runtime_ready"])
        entries = {entry["path"]: entry for entry in plan["geometry_candidates"]}
        self.assertEqual(entries["deffer_model_flat.vs"]["skin_modes"],
                         ["SKIN_NONE", "SKIN_0", "SKIN_1", "SKIN_2", "SKIN_3", "SKIN_4"])
        self.assertEqual(len([x for x in entries.values() if x["family"] == "deferred_static"]), 12)
        self.assertEqual(len([x for x in entries.values() if x["family"] == "terrain"]), 2)
        self.assertEqual(entries["deffer_tree_s_bump_d.vs"]["status"], "blocked_include_cycle")
        self.assertIn("previous_wave_wind_and_instance_transform", entries["deffer_tree_flat.vs"]["previous_state"])
        self.assertIn("previous_per_instance_rows", entries["deffer_detail_s_flat.vs"]["previous_state"])
        self.assertIn("previous_surface_after_domain_displacement", entries["dx11/tess.ds"]["previous_state"])
        self.assertIn("not_provided", plan["motion_mrt"]["status"])
        self.assertTrue("colour_contract" in plan, "source-reviewed colour seams are missing from the plan")
        self.assertEqual(plan["colour_contract"]["pre_tonemap_local_value"], "combine_1.ps:144-150 color before tonemap")
        self.assertFalse(plan["colour_contract"]["complete_stock_fp16_scene_texture_available"])
        self.assertEqual(plan["depth_contract"]["view_z_channel"], "s_position.z")
        with self.assertRaises(tool.Refusal):
            tool.patch_plan({**tool.inventory(root), "corpus_sha256": "0" * 64})


class CompilerFixtureTests(unittest.TestCase):
    def test_local_fxc_bytecode_is_unchanged_for_reviewed_geometry_and_pda_variants(self):
        import json
        import os
        import shutil
        import subprocess
        tool = load_tool()
        assert tool is not None
        source = REPO / ".local/anomaly/vanilla/shaders/r3"
        if not source.exists():
            self.skipTest("licensed local vanilla fixture is not available")
        sdk = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Windows Kits/10/bin"
        compilers = sorted(sdk.glob("*/x64/fxc.exe"), reverse=True)
        fxc = os.environ.get("RSF_FXC") or shutil.which("fxc") or (str(compilers[0]) if compilers else None)
        if not fxc:
            self.skipTest("Windows SDK fxc is not installed; set RSF_FXC explicitly")
        corpus = tool.inventory(source)
        plan = tool.patch_plan(corpus)
        cases = []
        for item in plan["geometry_candidates"]:
            if item["status"] == "blocked_include_cycle":
                continue
            if item["stage"] == "vs":
                modes = item["skin_modes"] or [None]
                for mode in modes:
                    cases.append((item["path"], "vs_5_0", [mode + "=1"] if mode else []))
            elif item["path"] in ("dx11/tess.ds", "dx11/tess.hs"):
                for axes in plan["tessellation_axes"]:
                    for detail in ([], ["USE_TDETAIL=1", "USE_TDETAIL_BUMP=1", "USE_LM_HEMI=1"]):
                        cases.append((item["path"], item["stage"] + "_5_0", [axis + "=1" for axis in axes] + detail))
        cases.append(("pda_overlay.ps", "ps_5_0", []))
        target = REPO / ".local/anomaly/staging/unittest-compiler"
        self.assertFalse(target.exists(), "test destination already exists")
        report = {"runtime_ready": False, "compiler": str(fxc), "scope": "declaration_only_bytecode_invariance",
                  "source_corpus_sha256": corpus["corpus_sha256"], "variants": []}
        try:
            profile = json.loads((REPO / "tools/anomaly_shader_vanilla_profile.json").read_text())
            tool.stage(source, target, profile, (REPO / "tools/anomaly_shader_constants.h").read_bytes())
            binary_dir = target / "compile-check"
            binary_dir.mkdir()
            for index, (shader, shader_target, defines) in enumerate(cases):
                with self.subTest(shader=shader, target=shader_target, defines=defines):
                    pair = []
                    warnings = []
                    for tag, root in (("original", source), ("staged", target)):
                        binary = binary_dir / (str(index) + "-" + tag + ".dxbc")
                        command = [str(fxc), "/nologo", "/T", shader_target, "/E", "main", "/D", "SM_5=1"]
                        for define in defines:
                            command += ["/D", define]
                        command += ["/I", str(root), "/Fo", str(binary), str(root / shader)]
                        run = subprocess.run(command, capture_output=True, text=True, timeout=30)
                        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
                        pair.append(binary.read_bytes())
                        warnings.append(run.stderr.count("warning X"))
                    self.assertEqual(pair[0], pair[1], "declaration-only include changed compiled DXBC")
                    report["variants"].append({"shader": shader, "target": shader_target, "defines": defines,
                                               "identical_dxbc_sha256": tool.digest(pair[0]),
                                               "original_staged_warning_counts": warnings})
        finally:
            report["planned_variant_count"] = len(cases)
            report["verified_variant_count"] = len(report["variants"])
            (REPO / ".local/anomaly/shader-compile-check.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
            if target.exists():
                shutil.rmtree(target)
        self.assertEqual(report["verified_variant_count"], report["planned_variant_count"])


if __name__ == "__main__":
    unittest.main()
