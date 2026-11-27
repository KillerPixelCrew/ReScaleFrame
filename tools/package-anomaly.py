#!/usr/bin/env python3
"""Package the exact-build Anomaly DX11 adapter, thin carrier, runtime shader rules and pinned SR/FG runtimes.

--dry-run checks inputs and generates a private rules pack, but emits no release archives and permits dirty
source for development. Final packaging requires a clean, unchanged commit and never reads or writes a game install.
"""
from __future__ import annotations

import argparse
import configparser
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import time
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]


sys.path.insert(0, str(Path(__file__).resolve().parent))
import anomaly_motion_deploy as motion  # noqa: E402
import anomaly_profiles  # noqa: E402
import rsf_shader_pack as shader_pack  # noqa: E402
from package_common import (RUNTIME, STREAMLINE_SDK, VENDOR_HASHES, copy, dependency_notices, digest, git,  # noqa: E402
                            release_identity)

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

# The vendor DLLs are the reviewed SDK set pinned in eng/dependencies.json (VENDOR_HASHES), rather than arbitrary files
# admitted from a supplied directory.
MOTION_NOTICE = """# Anomaly runtime shader patch rules

The package contains only RSF-authored rules and additions. Original shader source
and includes come from the installed game's own filesystem. Generated variants
and cloned source remain local and are not distributed.
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
        'ReScaleFrame/licenses/ReScaleFrame-MIT.txt': ROOT / 'LICENSE',
        'ReScaleFrame/licenses/Sharpening-GPL-3.0.txt': ROOT / 'runtime/graphics/src/sharpening/LICENSE-GPL-3.0.txt',
        'ReScaleFrame/licenses/GameSDK-MIT.txt': ROOT / 'sdk/game/LICENSE',
        'ReScaleFrame/licenses/AMD-Anti-Lag-2-MIT.txt': ROOT / 'runtime/orchestrator/ANTILAG2-LICENSE.txt',
        'ReScaleFrame/licenses/MFG-Unlocker-MIT.txt': ROOT / 'runtime/backends/dlss/third_party/mfg_unlocker/LICENSE',
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
        family, source = ('streamline', args.streamline_root / 'bin/x64' / name) if name in RUNTIME else (
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


def main(argv=None, *, release_revision=None, game_only=False):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--streamline-root', type=Path, default=STREAMLINE_SDK)
    parser.add_argument('--fidelityfx-root', type=Path, default=ROOT / 'vendor/fidelityfx')
    parser.add_argument('--xess-root', type=Path, default=ROOT / 'vendor/xess')
    parser.add_argument('--output', type=Path, default=ROOT / 'build/releases/anomaly')
    parser.add_argument('--dry-run', action='store_true', help='validate inputs; permit dirty source; emit no archives')
    args = parser.parse_args(argv)
    if args.dry_run and release_revision is not None:
        raise ValueError('Release-set callers cannot bypass source checks with Anomaly dry-run.')
    revision = (release_identity(release_revision) if release_revision is not None else
                git('rev-parse', 'HEAD') if args.dry_run else require_clean())
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
                       'README.md': '# Package notices\n\nRSF is MIT; its sharpening shaders are GPL-3.0-only ports of OptiScaler. Vendor binaries and the\n'
                                    'Motion shader rules and additions are RSF-authored; game source is never packaged.\n'
                                    'See the per-family notices, Sharpening-THIRD_PARTY.md and RUST-DEPENDENCIES.md.\n'}.items():
        (stage / 'ReScaleFrame/licenses' / name).write_text(text, encoding='utf-8', newline='\n')
    dependency_notices(stage)
    builds = [anomaly_profiles.native(build, discovered=False) for build in anomaly_profiles.BUILDS]
    manifest = {'product': 'ReScaleFrame for S.T.A.L.K.E.R. Anomaly', 'version': version, 'source_commit': revision,
                'platform': 'Windows x64', 'streamline': '2.14.1', 'fidelityfx_sdk': '2.3.0', 'xess_sdk': '3.0.2',
                'carrier_target': 'rsf_proxy_thin_dinput8',
                'executables': {build['executable']: build['sha256'] for build in builds},
                'motion_namespace': report['namespace'], 'motion_source_corpus_sha256': motion.CORPUS,
                'motion_root_count': len(report['roots']), 'compiled_shader_cache_included': False,
                'game_test_scope': 'RTX 4070 Laptop SDR; see README; this rebuilt package is not independently game-tested',
                'files': {p.relative_to(stage).as_posix(): digest(p) for p in sorted(stage.rglob('*')) if p.is_file()}}
    (stage / 'ReScaleFrame/manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    stamp = time.gmtime(int(git('show', '-s', '--format=%ct', revision)))[:6]
    archive = output / f'ReScaleFrame-{version}-Anomaly-Windows-x64.zip'
    archive_stage(stage, archive, stamp)
    verify_archive(archive)
    if game_only:
        release_identity(revision)
        return {'archive': str(archive), 'revision': revision}
    source = output / f'ReScaleFrame-{version}-Anomaly-source.zip'
    subprocess.run(['git', 'archive', '--format=zip', f'--prefix=ReScaleFrame-{version}/', '-o', str(source), revision],
                   cwd=ROOT, check=True)
    # Corresponding source is the committed authored rules and additions in git archive.
    # Never append generated game-source trees or a local compiled cache.
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
