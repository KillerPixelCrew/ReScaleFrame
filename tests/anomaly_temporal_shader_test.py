"""Core temporal augmentation tests. Licensed fixtures are read only."""
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
SOURCE = REPO / '.local/anomaly/vanilla/shaders/r3'
TOOL = REPO / 'tools/anomaly_temporal_shader.py'


def load_tool():
    if not TOOL.exists():
        return None
    spec = importlib.util.spec_from_file_location('anomaly_temporal_shader', TOOL)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CoreTests(unittest.TestCase):
    def test_refuses_changed_corpus_escaped_output_and_unknown_root(self):
        tool = load_tool()
        assert tool is not None
        self.assertTrue(hasattr(tool, 'supported_roots'), 'explicit injection allowlist missing')
        roots = tool.supported_roots(SOURCE)
        self.assertEqual(len(roots['vertex']), 18)
        self.assertEqual(len(roots['pixel']), 40)
        self.assertNotIn('deffer_impl_flat.vs', roots['vertex'])
        with tempfile.TemporaryDirectory(prefix='core-safety-', dir=REPO / '.local/anomaly') as folder:
            work = Path(folder)
            modified = work / 'source'
            shutil.copytree(SOURCE, modified)
            (modified / 'skin.h').write_bytes((modified / 'skin.h').read_bytes() + b'\n')
            target = work / 'changed-output'
            with self.assertRaises(tool.Refusal):
                tool.stage(modified, target)
            self.assertFalse(target.exists())
            for bad in (REPO / 'tests/forbidden-core-output', work / '..' / 'escaped'):
                with self.assertRaises(tool.Refusal):
                    tool.stage(SOURCE, bad)
                self.assertFalse(bad.exists())

    def test_real_static_motion_stage_is_not_declaration_only(self):
        tool = load_tool()
        self.assertIsNotNone(tool, 'real temporal transform has not been implemented')
        target = REPO / '.local/anomaly/core-tests/first-stage'
        if target.exists():
            shutil.rmtree(target)
        try:
            result = tool.stage(SOURCE, target)
            self.assertTrue(result['motion_output_implemented'])
            self.assertFalse(result['runtime_ready'])
            owner = (target / 'deffer_base_flat.vs').read_text()
            self.assertIn('rsf_previous_wvp', owner)
            self.assertIn('rsf_native_main', owner)
            self.assertIn('TEXCOORD12', (target / 'common_iostructs.h').read_text())
            self.assertIn('SV_Target2', (target / 'rsf_core_pixel.h').read_text())
            self.assertEqual((target / 'skin.h').read_bytes(), (SOURCE / 'skin.h').read_bytes())
            self.assertIn('rsf_prev_skinning_4', (target / 'deffer_model_bump.vs').read_text())
            duplicate = (target / 'rsf_core_previous_skin.h').read_text()
            self.assertIn('rsf_previous_bones', duplicate)
            self.assertNotIn('sbones_array', duplicate)
            self.assertIn('v.ind.zyx', duplicate)
            with self.assertRaises(tool.Refusal):
                tool.stage(SOURCE, target)
        finally:
            if target.exists():
                shutil.rmtree(target)


class NativeCompilerTests(unittest.TestCase):
    def test_native_fxc_static_and_skin_and_pixel_contract(self):
        tool = load_tool()
        self.assertIsNotNone(tool)
        self.assertTrue(hasattr(tool, 'verify_fxc'), 'native FXC/reflection execution missing')
        with tempfile.TemporaryDirectory(prefix='core-fxc-', dir=REPO / '.local/anomaly') as folder:
            result = tool.verify_fxc(SOURCE, Path(folder) / 'fixture')
            self.assertEqual(result['verified'], result['planned'])
            self.assertGreaterEqual(result['verified'], 88)
            self.assertTrue(all(x['reflection_verified'] for x in result['variants']))
            terrain = [x for x in result['variants'] if x['shader'].startswith('deffer_impl_')]
            self.assertEqual(len(terrain), 2, 'excluded terrain invariance fixture missing')
            self.assertTrue(all(not x['changed'] for x in terrain))
            from anomaly_temporal_shader_verify import run_device
            report = run_device(result, Path(folder) / 'fixture')
            self.assertGreater(report['ps_checks'], 0)
            print(report)


if __name__ == '__main__':
    unittest.main()
