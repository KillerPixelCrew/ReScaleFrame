"""Portable Anomaly release defaults and packaging refusal paths, without a game or SDK dependency."""
import configparser
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('package_anomaly', ROOT / 'tools/package-anomaly.py')
package = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = package
SPEC.loader.exec_module(package)


class AnomalyPackageTest(unittest.TestCase):
    def test_source_patterns_stay_minimal(self):
        rules = json.loads((ROOT / 'games/anomaly/patches/motion/rules.json').read_text())

        def check_patterns(value):
            if isinstance(value, dict):
                for key, child in value.items():
                    if key in ('anchor', 'start', 'end'):
                        self.assertLessEqual(len(child.encode()), 48, key)
                    check_patterns(child)
            elif isinstance(value, list):
                for child in value:
                    check_patterns(child)

        check_patterns(rules)

    def test_rules_pack_without_a_game_corpus(self):
        files, report = package.motion_plan(Path('does-not-exist'))
        self.assertEqual(set(files), {'motion.rsfpatch'})
        info = package.shader_pack.inspect(files['motion.rsfpatch'].read_bytes())
        self.assertEqual(info['metadata']['id'], 'org.rescaleframe.anomaly.motion')
        self.assertEqual(len(report['roots']), 73)
        self.assertNotIn(b'original/r3/', files['motion.rsfpatch'].read_bytes())

    def test_portable_defaults(self):
        ini = package.validate_config(package.CONFIG)
        self.assertEqual(ini['Loader']['RuntimeModule'], 'ReScaleFrame/ReScaleFrame.Runtime.dll')
        self.assertEqual(ini['Loader']['EntryPoint'], 'rsf_anomaly_start')
        for key, value in {'Mode': 'Observe', 'Overlay': '1', 'SRStage': '5', 'SR': '1', 'SRBackend': '1',
                           'Motion': '1', 'FG': '0', 'FGBridge': '1', 'FGBackend': '7', 'SRStageCapture': '0',
                           'MotionCapture': '0', 'FGCapture': '0'}.items():
            self.assertEqual(ini['Anomaly'][key], value)
        self.assertFalse(any(key.endswith('Dir') for key in ini['Anomaly']))

    def test_bad_defaults_refused(self):
        for altered in (package.CONFIG.replace('Motion=1\n', ''),
                        package.CONFIG.replace('SRStageCapture=0', 'SRStageCapture=1'),
                        package.CONFIG.replace('FG=0', 'FG=1'),
                        package.CONFIG + 'SRStreamlineDir=D:/local/sdk\n',
                        package.CONFIG.replace('ReScaleFrame/ReScaleFrame.Runtime.dll', '/tmp/runtime.dll'),
                        package.CONFIG + 'SR=1\n'):
            with self.subTest(config=altered[-80:]), self.assertRaises((ValueError, configparser.Error)):
                package.validate_config(altered)

    def test_source_refusals(self):
        with mock.patch.object(package, 'git', return_value=' M tools/package-anomaly.py'):
            with self.assertRaisesRegex(RuntimeError, 'Commit'):
                package.require_clean()
        with mock.patch.object(package, 'git', side_effect=['', 'new_commit']):
            with self.assertRaisesRegex(RuntimeError, 'changed'):
                package.require_clean('old_commit')

    def test_vendor_mutation_refused(self):
        args = type('Args', (), {'streamline_root': Path('sdk'), 'fidelityfx_root': Path('fx'), 'xess_root': Path('xe')})()
        with mock.patch.object(package, 'digest', return_value='0' * 64):
            with self.assertRaisesRegex(RuntimeError, 'pinned SDK'):
                package.input_plan(args)
        self.assertEqual(len(package.VENDOR_HASHES), 14)
        self.assertTrue(set(package.ac7.RUNTIME) <= package.VENDOR_HASHES.keys())

    def test_archive_paths(self):
        for name in ('../outside.dll', '/absolute.dll', 'C:/absolute.dll', 'path\\outside.dll', '.', 'path/./file'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                package.safe_relative(name)
        self.assertEqual(str(package.safe_relative('ReScaleFrame/anomaly-motion/original/r3/shared/common.h')),
                         'ReScaleFrame/anomaly-motion/original/r3/shared/common.h')

    def make_bundle(self, path, extra=False, corrupt=False):
        files = {'ReScaleFrame.ini': package.CONFIG.encode(), 'ReScaleFrame/native.dll': b'native fixture'}
        manifest = {'files': {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}}
        with zipfile.ZipFile(path, 'w') as bundle:
            for name, data in files.items():
                bundle.writestr(name, data + (b'corrupt' if corrupt and name.endswith('.dll') else b''))
            bundle.writestr('ReScaleFrame/manifest.json', json.dumps(manifest))
            if extra:
                bundle.writestr('unreviewed.pdb', b'not allowlisted')
        return manifest

    def test_archive_matches_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'bundle.zip'
            expected = self.make_bundle(path)
            self.assertEqual(package.verify_archive(path), expected)

    def test_archive_extra_entry_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'extra.zip'
            self.make_bundle(path, extra=True)
            with self.assertRaisesRegex(RuntimeError, 'allowlist'):
                package.verify_archive(path)

    def test_archive_changed_entry_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'changed.zip'
            self.make_bundle(path, corrupt=True)
            with self.assertRaisesRegex(RuntimeError, 'hash mismatch'):
                package.verify_archive(path)


if __name__ == '__main__':
    unittest.main()
