"""Read-only evidence regression suite for both installed Anomaly builds.

Runs the four evidence scripts, then proves rejection of altered guards, RVAs and
layout/frame values in temporary profile copies. Never writes the game install.
Not registered in CTest because the copyrighted executables are external inputs.
"""
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
CASES = {
    'geometry': ('geometry_profile.h', 'world_matrix_rva'),
    'sr': ('sr_profile.h', 'device_project_rva'),
    'motion': ('motion_profile.h', 'source_offset'),
    'fg': ('native_profile.h', 'image_size'),
}


def run(script, build, profile=None):
    args = [sys.executable, '-I', str(ROOT / 'tests' / ('anomaly_' + script + '_evidence.py')),
            '--verify', '--exe', build]
    if profile:
        args.extend(['--profile', str(profile)])
    return subprocess.run(args, capture_output=True, text=True, timeout=120)


def main():
    checks = 0
    for build, section in (('standard', 'Standard'), ('avx', 'AVX')):
        for domain, (header, field) in CASES.items():
            result = run(domain, build)
            assert result.returncode == 0, (domain, build, result.stdout, result.stderr)
            checks += 1
            text = (ROOT / 'games/anomaly/src' / header).read_text()
            start = text.index('// ---- ' + section + ' 1.5.3 DX11')
            end = text.find('// ---- ', start + 10)
            end = len(text) if end < 0 else end
            selected = text[start:end]
            guard = re.search(r'signature\("([0-9a-f]{32})"\)', selected)
            rva = re.search(r'\{"[^"]+", (0x[0-9a-f]+), signature', selected)
            layout = re.search(r'\.' + field + r' = (0x[0-9a-f]+)', selected)
            assert guard and rva and layout, (domain, field)
            mutations = [('guard', guard), ('rva', rva), ('layout', layout)]
            if domain == 'sr':
                sun = re.search(r'\{"CRender::render_sun_cascades", (0x[0-9a-f]+), "([0-9a-f]+)"', selected)
                assert sun
                # Alter the extended guard beyond expected16, and the separately guarded call's displacement.
                mutations.append(('sun_extended', sun))
                sun_call = re.search(r'\{"CRender::Render -> render_sun_cascades[^\n]+signature\("([0-9a-f]+)"', selected)
                assert sun_call
                mutations.append(('sun_call', sun_call))
                for field in ('get_constant_buffer_rva', 'position_setup_vtable_rva'):
                    match = re.search(r'\.' + field + r' = (0x[0-9a-f]+)', selected)
                    assert match
                    mutations.append((field, match))
                for name in ('position binder body', 'R_constants::GetCBuffer',
                             'position binder registration', 'constant buffer reflected size'):
                    match = re.search(r'\{"' + name + r'", (0x[0-9a-f]+), "([0-9a-f]+)"', selected)
                    assert match
                    mutations.append(('reconstruction_guard', match))
            with tempfile.TemporaryDirectory(prefix='rsf-anomaly-evidence-') as temp:
                for kind, match in mutations:
                    group = 2 if kind in ('sun_extended', 'reconstruction_guard') else 1
                    old = match[group]
                    if kind in ('guard', 'reconstruction_guard'):
                        new = ('00' if old[:2] != '00' else 'ff') + old[2:]
                    elif kind == 'sun_extended':
                        new = old[:32] + ('00' if old[32:34] != '00' else 'ff') + old[34:]
                    elif kind == 'sun_call':
                        new = old[:2] + ('00' if old[2:4] != '00' else 'ff') + old[4:]
                    else:
                        new = hex(int(old, 16) + 1)
                    altered = selected[:match.start(group)] + new + selected[match.end(group):]
                    path = pathlib.Path(temp) / header
                    path.write_text(text[:start] + altered + text[end:])
                    result = run(domain, build, path)
                    assert result.returncode != 0 and 'AssertionError' in result.stderr, (domain, build, kind, result.stdout, result.stderr)
                    checks += 1
            extra = '; cascade and reconstruction guard/owner mutations refused' if domain == 'sr' else ''
            print(f'PASS: {build} {domain}: original accepted; guard/RVA/layout mutations refused{extra}', flush=True)
    print(f'PASS: {checks} checks (8 originals, 40 negative mutations)')


if __name__ == '__main__':
    main()
