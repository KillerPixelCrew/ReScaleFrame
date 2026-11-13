#!/usr/bin/env python3
"""Package the exact-build Anomaly DX11 adapter, thin carrier, runtime shader rules and pinned SR/FG runtimes.

--dry-run checks inputs and generates a private rules pack, but emits no release archives and permits dirty
source for development. Final packaging requires a clean, unchanged commit and never reads or writes a game install.
"""
from __future__ import annotations

import argparse
import configparser
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import time
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'tools' / filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


ac7 = load('package_ac7', 'package-ac7.py')
motion = load('anomaly_motion_deploy', 'anomaly_motion_deploy.py')
shader_pack = load('rsf_shader_pack', 'rsf_shader_pack.py')
git, digest, copy = ac7.git, ac7.digest, ac7.copy

CONFIG = """[Loader]
AutoStart=1
RuntimeModule=ReScaleFrame/ReScaleFrame.Runtime.dll
EntryModule=ReScaleFrame/ReScaleFrame.Game.Anomaly.dll
EntryPoint=rsf_anomaly_start

[Anomaly]
Mode=Observe
Overlay=1
SRStage=5
SR=1
SRBackend=1
SRQuality=1
DLSSPreset=0
SRJitter=1
SRMipBias=off
SRColourTransport=0
SRExposure=auto
SRFirstPersonZero=1
Motion=1
MotionInvalidate=1
MotionFoliage=1
FG=0
FGBridge=1
FGBackend=7
FGGeneratedFrames=1
Latency=1
FrameLimitUs=0
StatusOverlay=1
SRStageCapture=0
MotionCapture=0
FGCapture=0
FGDebug=0
FGDevelopment=0
"""

# Retail Streamline 2.14.1, FidelityFX SDK 2.3.0 and XeSS SDK 3.0.2. The FSR/XeSS SR pins are the AC7/RDR2 pins;
# the rest pin the same reviewed SDK set as well, rather than admitting arbitrary files from a supplied directory.
VENDOR_HASHES = {'sl.interposer.dll': '8c87c9499461da561edd529aa9bf7831d67d7b94ebb1c1a5ed54ef4934e1ea4c',
 'sl.common.dll': '82924a8954dd671e09351c5de0eb87ad0eb25b944cc9f9ab955ca1d9950de15d',
 'sl.dlss.dll': '73bf52c0cfaa5900a8f3f4a91306e4625e7cca696dfb305aae44c9f97b582e1f',
 'sl.pcl.dll': 'f13d51cfa05f4cd514df2026049e2db8adf359221713170ad386fd499915b582',
 'sl.dlss_g.dll': 'f4a6b2b14dcc0b1485989e430d3b4e3a44ac1800b92ba1ad74f476e64fb2b09c',
 'sl.reflex.dll': '0ce9725e3e03ea9e7f81d008b57f33ee365973d2e349131c8b1c3e3378fe2db0',
 'nvngx_dlss.dll': '3975567b8943c53acce397f2b72380092f84f162d00b0d2c7d08a1025c563983',
 'nvngx_dlssg.dll': 'ff6e90eb78b827927dff5b4ecc6b1c870c2e9bca29ed9f48c7d348cc9e170b82',
 'amd_fidelityfx_upscaler_dx12.dll': 'aad4fccccb29084a655f9a4f9f613bd22c8c03fb4a0f8840148cc6543b4ccace',
 'amd_fidelityfx_loader_dx12.dll': 'e2191d3c0828be45eb8affd4e3ec0378f2293adc345b772a7170edcf3b021ecb',
 'amd_fidelityfx_framegeneration_dx12.dll': 'e6843252f58b0341a0d80f6a20bc0d840643ee95bc48c22cac08745344577bb6',
 'libxess.dll': '251659dd84a3e84de67c886a4186e01f3eca49b00641906fe38bb6b807e5d5b7',
 'libxess_fg.dll': 'ec5e0c65e075570c6ede72618bb666d0be0c2e10b2ea9762c0fe8cb8e375ab27',
 'libxell.dll': 'd2030dcd694fda8f2ec7e044b13e6db8f0b56d4ba9113a5efad334e3f3ded8c7'}
MOTION_NOTICE = """# Anomaly runtime shader patch rules

The package contains only RSF-authored rules and additions. Original shader source
and includes come from the installed game's own filesystem. Generated variants
and cloned source remain local and are not distributed. yyjson is MIT licensed.
"""


def validate_config(text):
    parser = configparser.ConfigParser(interpolation=None, strict=True)
    parser.optionxform = str
    parser.read_string(text)
    expected = configparser.ConfigParser(interpolation=None, strict=True)
    expected.optionxform = str
    expected.read_string(CONFIG)
    if parser.sections() != expected.sections() or any(dict(parser[s]) != dict(expected[s]) for s in expected.sections()):
        raise ValueError('Packaged INI differs from the reviewed portable defaults.')
    for section in parser.sections():
        for key, value in parser[section].items():
            if key.endswith('Dir') or re.search(r'(?:[A-Za-z]:|\\\\|^/|\.\.)', value):
                raise ValueError('Nonportable setting in packaged INI: ' + key)
    return parser


def require_clean(revision=None):
    if git('status', '--porcelain'):
        raise RuntimeError('Commit the intended release source before packaging.')
    current = git('rev-parse', 'HEAD')
    if revision is not None and current != revision:
        raise RuntimeError('Source commit changed during packaging.')
    return current


def safe_relative(name):
    path = PurePosixPath(name)
    if not name or path.as_posix() != name or name == '.' or path.is_absolute() or any(p in ('..', '.') for p in path.parts) or ':' in name or '\\' in name:
        raise ValueError('Unsafe archive entry: ' + name)
    return path


def motion_plan(source=None):
    # The optional argument is retained for callers of the development packager.
    # It is deliberately never opened: release packs need no local game corpus.
    folder = ROOT / 'games/anomaly/patches/motion'
    data = shader_pack.build(folder)
    info = shader_pack.inspect(data)
    destination = ROOT / '.local/anomaly' / ('motion-pack-package-' + uuid.uuid4().hex[:8])
    destination.mkdir(parents=True)
    path = destination / 'motion.rsfpatch'
    path.write_bytes(data)
    rules = json.loads((folder / 'rules.json').read_text(encoding='utf-8'))
    return {'motion.rsfpatch': path}, {'namespace': info['sha256'], 'roots': rules['roots']}


def input_plan(args):
    release = ROOT / 'build/windows-x64/bin/Release'
    fx = args.fidelityfx_root / 'Kits/FidelityFX/signedbin'
    xe = args.xess_root / 'bin'
    plan = {
        'dinput8.dll': ROOT / 'build/windows-x64/thin/bin/Release/dinput8.dll',
        'ReScaleFrame/ReScaleFrame.Runtime.dll': release / 'ReScaleFrame.Runtime.dll',
        'ReScaleFrame/ReScaleFrame.Game.Anomaly.dll': release / 'ReScaleFrame.Game.Anomaly.dll',
        'ReScaleFrame/rescaleframe_overlay.dll': ROOT / 'target/release/rescaleframe_overlay.dll',
        'ReScaleFrame/README.md': ROOT / 'docs/releases/anomaly-install.md',
        'ReScaleFrame/licenses/ReScaleFrame-GPL-3.0.txt': ROOT / 'LICENSE',
        'ReScaleFrame/licenses/GameSDK-MIT.txt': ROOT / 'sdk/game/LICENSE',
        'ReScaleFrame/licenses/MFG-Unlocker-MIT.txt': ROOT / 'runtime/backends/dlss/third_party/mfg_unlocker/LICENSE',
        'ReScaleFrame/licenses/yyjson-MIT.txt': ROOT / 'vendor/shader-patch/yyjson/LICENSE',
        'ReScaleFrame/licenses/ShaderPatch-THIRD_PARTY.md': ROOT / 'runtime/shader_patch/THIRD_PARTY.md',
        'ReScaleFrame/licenses/MinHook.txt': ROOT / 'build/windows-x64/_deps/minhook-src/LICENSE.txt',
        'ReScaleFrame/licenses/Sharpening-THIRD_PARTY.md': ROOT / 'runtime/graphics/src/sharpening/THIRD_PARTY.md',
        'ReScaleFrame/licenses/Streamline.txt': args.streamline_root / 'license.txt',
        'ReScaleFrame/licenses/Streamline-third-party.md': args.streamline_root / '3rd-party-licenses.md',
        'ReScaleFrame/streamline/nvngx_dlss.license.txt': args.streamline_root / 'bin/x64/nvngx_dlss.license.txt',
        'ReScaleFrame/licenses/NVIDIA-Reflex.txt': args.streamline_root / 'bin/x64/reflex.license.txt',
        'ReScaleFrame/fidelityfx/LICENSE.md': args.fidelityfx_root / 'Kits/FidelityFX/docs/license.md',
        'ReScaleFrame/fidelityfx/3rdpartynotice.md': args.fidelityfx_root / '3rdpartynotice.md',
        'ReScaleFrame/xess/LICENSE.txt': args.xess_root / 'LICENSE.txt',
        'ReScaleFrame/xess/third-party-programs.txt': args.xess_root / 'third-party-programs.txt',
    }
    for name in VENDOR_HASHES:
        family, source = ('streamline', args.streamline_root / 'bin/x64' / name) if name in ac7.RUNTIME else (
            ('fidelityfx', fx / name) if name.startswith('amd_') else ('xess', xe / name))
        if digest(source) != VENDOR_HASHES[name]:
            raise RuntimeError('Vendor binary differs from the pinned SDK: ' + name)
        plan['ReScaleFrame/' + family + '/' + name] = source
    for name, source in plan.items():
        safe_relative(name)
        if not source.is_file() or source.is_symlink():
            raise RuntimeError('Missing or linked package input: ' + str(source))
    return plan


def verify_archive(archive):
    with zipfile.ZipFile(archive) as bundle:
        names = bundle.namelist()
        if len(names) != len(set(names)) or bundle.testzip() is not None:
            raise RuntimeError('Duplicate entry or CRC failure in archive.')
        manifest_name = 'ReScaleFrame/manifest.json'
        manifest = json.loads(bundle.read(manifest_name))
        if set(names) != set(manifest['files']) | {manifest_name}:
            raise RuntimeError('Archive entries differ from the manifest allowlist.')
        for name, expected in manifest['files'].items():
            safe_relative(name)
            if hashlib.sha256(bundle.read(name)).hexdigest() != expected:
                raise RuntimeError('Archive hash mismatch: ' + name)
        validate_config(bundle.read('ReScaleFrame.ini').decode('utf-8'))
        return manifest


def archive_stage(stage, archive, stamp):
    with zipfile.ZipFile(archive, 'w') as bundle:
        for path in sorted(stage.rglob('*')):
            if path.is_file():
                entry = zipfile.ZipInfo(path.relative_to(stage).as_posix(), stamp)
                entry.compress_type = zipfile.ZIP_DEFLATED
                entry.external_attr = 0o100644 << 16
                bundle.writestr(entry, path.read_bytes(), compresslevel=9)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--streamline-root', required=True, type=Path)
    parser.add_argument('--fidelityfx-root', type=Path, default=ROOT / 'vendor/fidelityfx')
    parser.add_argument('--xess-root', type=Path, default=ROOT / 'vendor/xess')
    parser.add_argument('--output', type=Path, default=ROOT / 'build/releases/anomaly')
    parser.add_argument('--dry-run', action='store_true', help='validate inputs; permit dirty source; emit no archives')
    args = parser.parse_args()
    revision = git('rev-parse', 'HEAD') if args.dry_run else require_clean()
    version = (ROOT / 'VERSION').read_text(encoding='utf-8').strip()
    validate_config(CONFIG)
    plan = input_plan(args)
    motion_files, report = motion_plan()
    plan.update({'ReScaleFrame/patches/' + n: p for n, p in motion_files.items()})
    if args.dry_run:
        print(json.dumps({'dry_run': True, 'version': version, 'source_commit': revision,
                          'dirty': bool(git('status', '--porcelain')), 'carrier_target': 'rsf_proxy_thin_dinput8',
                          'carrier_sha256': digest(plan['dinput8.dll']), 'motion_namespace': report['namespace'],
                          'motion_roots': len(report['roots']), 'files': sorted(plan)}, indent=2))
        return
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    stage = output / ('stage-anomaly-' + uuid.uuid4().hex[:8])
    stage.mkdir()
    for name, source in plan.items():
        copy(source, stage / name)
    (stage / 'ReScaleFrame.ini').write_text(CONFIG, encoding='utf-8', newline='\n')
    for name, text in {'Anomaly-motion.md': MOTION_NOTICE,
                       'README.md': '# Package notices\n\nRSF is GPL-3.0-only; GameSDK is MIT. Vendor binaries and the\n'
                                    'Motion shader rules and additions are RSF-authored; game source is never packaged.\n'
                                    'See the per-family notices, Sharpening-THIRD_PARTY.md and RUST-DEPENDENCIES.md.\n'}.items():
        (stage / 'ReScaleFrame/licenses' / name).write_text(text, encoding='utf-8', newline='\n')
    ac7.dependency_notices(stage)
    build_profile = (ROOT / 'games/anomaly/src/native_profile.h').read_text(encoding='utf-8')
    hashes = re.findall(r'inline constexpr char (?:dx11|avx)_hash\[\] = "([0-9a-f]{64})"', build_profile)
    if len(hashes) != 2:
        raise RuntimeError('Expected the two exact Anomaly executable profiles.')
    manifest = {'product': 'ReScaleFrame for S.T.A.L.K.E.R. Anomaly', 'version': version, 'source_commit': revision,
                'platform': 'Windows x64', 'streamline': '2.14.1', 'fidelityfx_sdk': '2.3.0', 'xess_sdk': '3.0.2',
                'carrier_target': 'rsf_proxy_thin_dinput8',
                'executables': dict(zip(('AnomalyDX11.exe', 'AnomalyDX11AVX.exe'), hashes)),
                'motion_namespace': report['namespace'], 'motion_source_corpus_sha256': motion.CORPUS,
                'motion_root_count': len(report['roots']), 'compiled_shader_cache_included': False,
                'game_test_scope': 'RTX 4070 Laptop SDR; see README; this rebuilt package is not independently game-tested',
                'files': {p.relative_to(stage).as_posix(): digest(p) for p in sorted(stage.rglob('*')) if p.is_file()}}
    (stage / 'ReScaleFrame/manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    stamp = time.gmtime(int(git('show', '-s', '--format=%ct', revision)))[:6]
    archive = output / f'ReScaleFrame-{version}-Anomaly-Windows-x64.zip'
    archive_stage(stage, archive, stamp)
    verify_archive(archive)
    source = output / f'ReScaleFrame-{version}-Anomaly-source.zip'
    subprocess.run(['git', 'archive', '--format=zip', f'--prefix=ReScaleFrame-{version}/', '-o', str(source), revision],
                   cwd=ROOT, check=True)
    # Corresponding source is the committed authored rules and additions in git archive.
    # Never append generated game-source trees or a local compiled cache.
    # Include the incorporated MIT dependency at the location CMake expects.
    with zipfile.ZipFile(source, 'a') as bundle:
        for name in ('src/yyjson.c', 'src/yyjson.h', 'LICENSE'):
            entry = zipfile.ZipInfo(f'ReScaleFrame-{version}/vendor/shader-patch/yyjson/' + name, stamp)
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = 0o100644 << 16
            bundle.writestr(entry, (ROOT / 'vendor/shader-patch/yyjson' / name).read_bytes(), compresslevel=9)
    with zipfile.ZipFile(source) as bundle:
        if len(bundle.namelist()) != len(set(bundle.namelist())) or bundle.testzip() is not None:
            raise RuntimeError('Corresponding-source archive duplicate entry or CRC failure.')
    require_clean(revision)
    sums = output / 'SHA256SUMS.txt'
    sums.write_text(''.join(f'{digest(p)}  {p.name}\n' for p in (archive, source)), encoding='ascii', newline='\n')
    print(json.dumps({'archive': str(archive), 'source': str(source), 'checksums': str(sums), 'stage': str(stage),
                      'revision': revision, 'entries': len(manifest['files']) + 1,
                      'sha256': {p.name: digest(p) for p in (archive, source)}}, indent=2))


if __name__ == '__main__':
    main()
