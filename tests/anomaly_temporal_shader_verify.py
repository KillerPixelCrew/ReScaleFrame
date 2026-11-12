"""Strict standalone FXC + MSVC + D3D11 execution. Writes only local evidence."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

REPO = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('anomaly_temporal_shader', REPO / 'tools/anomaly_temporal_shader.py')
assert SPEC is not None and SPEC.loader is not None
TOOL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TOOL)


def run_device(result, root):
    root = Path(root).absolute()
    # The production verifier already constrains this to .local/anomaly/core*.
    relative = root.relative_to(REPO / '.local/anomaly')
    if not relative.parts[0].startswith('core') or '..' in relative.parts:
        raise ValueError('device output must remain local to core evidence')
    native = REPO / 'tests/anomaly_temporal_shader_device.cpp'
    output = root / 'device.exe'
    configured = os.environ.get('RSF_VCVARS64')
    candidates = sorted(Path('C:/Program Files/Microsoft Visual Studio').glob('*/**/vcvars64.bat'), reverse=True)
    if configured:
        vcvars = configured
    elif candidates:
        vcvars = str(candidates[0])
    else:
        raise RuntimeError('MSVC required; set RSF_VCVARS64')
    command = ('call "' + vcvars + '" >nul && cl /nologo /EHsc /W4 /WX /std:c++20 "' + str(native) +
               '" /Fo"' + str(root / 'device.obj') + '" /Fe"' + str(output) +
               '" /link d3d11.lib d3dcompiler.lib dxgi.lib')
    build_script = root / 'build-device.cmd'
    build_script.write_text('@echo off\n' + command + '\n')
    build = subprocess.run(['cmd.exe', '/d', '/c', str(build_script)], capture_output=True, text=True)
    (root / 'device-build.log').write_text(build.stdout + build.stderr)
    if build.returncode:
        raise RuntimeError(build.stdout + build.stderr)
    listing = root / 'device-cases.tsv'
    listing.write_text(''.join(str(x['index']) + '\t' + x['stage'] + '\t' + x['mode'] + '\t' +
                              next((d.split('=')[1] for d in x['defines'] if d.startswith('MSAA_SAMPLES=')), '1') +
                              '\t' + str(2 if 'MSAA_ALPHATEST_DX10_0_ATOC=1' in x['defines'] else int('_aref_' in x['shader'])) + '\n'
                              for x in result['variants'] if x['changed']))
    run = subprocess.run([str(output), str(root / 'dxbc'), str(listing)],
                         capture_output=True, text=True, timeout=180)
    (root / 'device-run.log').write_text(run.stdout + run.stderr)
    if run.returncode:
        raise RuntimeError(run.stdout + run.stderr)
    report = json.loads(run.stdout)
    expected_vs = sum(x['changed'] and x['stage'] == 'vs' for x in result['variants'])
    expected_ps = sum(x['changed'] and x['stage'] == 'ps' for x in result['variants'])
    if (report['vs_checks'] != expected_vs * 2 or report['zero_motion_checks'] != expected_ps or
            report['invalid_checks'] != expected_vs + expected_ps * 2):
        raise RuntimeError('device coverage counts do not match the FXC case list')
    report.update({'native_fixture_sha256': TOOL.digest(native.read_bytes()), 'msvc_environment': vcvars,
                   'cache_namespace': result['cache_namespace'], 'runtime_ready': False,
                   'changed_vs_variants': expected_vs, 'changed_ps_variants': expected_ps})
    (root / 'device-report.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=REPO / '.local/anomaly/vanilla/shaders/r3')
    parser.add_argument('--output', type=Path, required=True, help='fresh .local/anomaly/core* directory')
    args = parser.parse_args()
    result = TOOL.verify_fxc(args.source, args.output)
    report = run_device(result, args.output)
    print(json.dumps({'fxc_pairs': result['verified'], **report}, indent=2))


if __name__ == '__main__':
    main()
