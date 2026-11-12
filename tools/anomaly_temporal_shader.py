"""Fail-closed local Anomaly core motion source transform, ABI 1.

Only first-party wrappers are tracked. Generated copies of local licensed input
and its namespace-remapped skin functions remain in .local/anomaly/core*.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil

REPO = Path(__file__).resolve().parents[1]
LOCAL = REPO / '.local/anomaly'
OWNED = REPO / 'games/anomaly/shaders/core'
CORPUS = 'fdc260e0608f259d876c4ae0f11a6f2d8c03c001cf1fe82548794af1c6826a67'
ABI = 1


class Refusal(ValueError):
    """The exact reviewed source or safe output boundary did not match."""


def digest(data):
    return hashlib.sha256(data).hexdigest()


def tree(source):
    result = {}
    for path in sorted(source.rglob('*')):
        if path.is_symlink() or path.is_junction():
            raise Refusal('linked source')
        if path.is_file():
            result[path.relative_to(source).as_posix()] = path.read_bytes()
    return result


def corpus_hash(files):
    return digest(json.dumps([[k, digest(v)] for k, v in sorted(files.items())],
                             separators=(',', ':'), ensure_ascii=True).encode())


def supported_roots(source):
    source = Path(source)
    return {'vertex': sorted(p.name for pattern in ('deffer_base*.vs', 'deffer_model*.vs')
                             for p in source.glob(pattern)),
            'pixel': sorted(p.name for p in source.glob('deffer_base*.ps') if '_atoc_' not in p.name),
            'unchanged_alpha_prepass': sorted(p.name for p in source.glob('deffer_base_atoc*.ps'))}


def shared_io(data):
    text = data.decode()
    for name in ('v2p_flat', 'v2p_bumped', 'p_flat', 'p_bumped'):
        expression = r'(\bstruct\s+' + name + r'\s*\{)(.*?)(\n\};)'
        addition = ('\n#ifdef RSF_TEMPORAL\n'
                    '    float4 rsf_previous_clip : TEXCOORD12;\n'
                    '    nointerpolation float rsf_valid : TEXCOORD13;\n'
                    '#endif\n')
        text, count = re.subn(expression, lambda m: m[1] + addition + m[2] + m[3], text, flags=re.S)
        if count != 1:
            raise Refusal('shared IO anchor: ' + name)
    return text.encode()


def static_vertex(data, shape):
    # Native body is byte-preserved behind a conditional entrypoint alias.
    prefix = '#ifndef RSF_CORE_EXCLUDE\n#define RSF_TEMPORAL 1\n#define main rsf_native_main\n#endif\n'
    suffix = ('\n#ifndef RSF_CORE_EXCLUDE\n#undef main\n#include "rsf_core_vertex.h"\n'
              + shape + ' main(v_in v)\n{\n'
              '    ' + shape + ' o = rsf_native_main(v);\n'
              '    o.rsf_previous_clip = mul(rsf_previous_wvp, v.P);\n'
              '    o.rsf_valid = rsf_draw_valid();\n'
              '    return o;\n}\n#endif\n')
    return prefix.encode() + data + suffix.encode()


def previous_skin(data):
    text = data.decode()
    start = re.search(r'\bfloat3\s+skinning_dir\s*\(', text)
    if start is None:
        raise Refusal('skin function boundary')
    body = text[start.start():text.rfind('#endif')]
    names = ['skinning_dir', 'skinning_pos', 'u_position'] + ['skinning_' + str(i) for i in range(5)]
    # u_position is stateless and kept native. Everything with palette access
    # is cloned under a private namespace, preserving indexing/swizzles exactly.
    names.remove('u_position')
    for name in names:
        body = re.sub(r'\b' + name + r'\b', 'rsf_prev_' + name, body)
    body = re.sub(r'\bsbones_array\b', 'rsf_previous_bones', body)
    return ('#ifndef RSF_CORE_PREVIOUS_SKIN_H\n#define RSF_CORE_PREVIOUS_SKIN_H\n' +
            body + '\n#endif\n').encode()


def model_vertex(data, shape):
    suffix = '\n#undef main\n#include "rsf_core_vertex.h"\n#include "rsf_core_previous_skin.h"\n'
    for mode in ['NONE', '0', '1', '2', '3', '4']:
        input_type = 'v_model' if mode == 'NONE' else 'v_model_skinned_' + mode
        previous = 'v.P' if mode == 'NONE' else 'rsf_prev_skinning_' + mode + '(v).P'
        gate = ' * (rsf_history.z == 1)' if mode == 'NONE' else ''
        if mode in ('1', '2', '3', '4'):
            gate = ' * (rsf_history.y == 1)'
        suffix += ('#ifdef SKIN_' + mode + '\n' + shape + ' main(' + input_type + ' v)\n{\n'
                   '    ' + shape + ' o = rsf_native_main(v);\n'
                   '    o.rsf_previous_clip = mul(rsf_previous_wvp, ' + previous + ');\n'
                   '    o.rsf_valid = rsf_draw_valid()' + gate + ';\n'
                   '    return o;\n}\n#endif\n')
    return b'#define RSF_TEMPORAL 1\n#define main rsf_native_main\n' + data + suffix.encode()


def pixel(data, shape, alpha):
    prefix = ('#ifndef ATOC\n#define RSF_TEMPORAL 1\n'
              '#define main rsf_native_main\n#endif\n')
    suffix = ('\n#ifdef RSF_TEMPORAL\n#undef main\n#include "rsf_core_pixel.h"\n'
              'RSFMotionOutput main(' + shape + ' i, float4 rsf_pixel : SV_Position)\n{\n')
    if alpha:
        suffix += ('#ifdef MSAA_ALPHATEST_DX10_1_ATOC\n'
                   '    f_deffer native = rsf_native_main(i, rsf_pixel);\n#else\n')
    suffix += '    f_deffer native = rsf_native_main(i);\n'
    if alpha:
        suffix += '#endif\n'
    suffix += ('    RSFMotionOutput o;\n'
               '    o.native0 = native.position;\n    o.native1 = native.C;\n'
               '    o.motion = rsf_encode_motion(rsf_pixel, i.rsf_previous_clip, i.rsf_valid);\n'
               '#ifdef EXTEND_F_DEFFER\n    o.native_mask = native.mask;\n#endif\n'
               '    return o;\n}\n#endif\n')
    return prefix.encode() + data + suffix.encode()


def stage(source, output):
    source, output = Path(source), Path(output).absolute()
    # Check every path component before resolve so junctions cannot hide escapes.
    for path in (source.absolute(), output):
        for ancestor in (path, *path.parents):
            if ancestor.is_symlink() or ancestor.is_junction():
                raise Refusal('linked path')
    try:
        relative = output.relative_to(LOCAL)
    except ValueError as error:
        raise Refusal('output must be inside .local/anomaly/core*') from error
    if not relative.parts or not relative.parts[0].startswith('core') or '..' in relative.parts:
        raise Refusal('output must be inside .local/anomaly/core*')
    if output.exists():
        raise Refusal('output already exists')
    original = tree(source)
    if len(original) != 449 or corpus_hash(original) != CORPUS:
        raise Refusal('unreviewed source corpus')
    files = dict(original)
    files['common_iostructs.h'] = shared_io(files['common_iostructs.h'])
    # These two non-core roots include the static owner. Explicitly retain native
    # semantics instead of accidentally emitting an unpaired temporal VS.
    for name in ('deffer_impl_flat.vs', 'deffer_impl_flat_d.vs'):
        files[name] = b'#define RSF_CORE_EXCLUDE 1\n' + files[name]
    for kind in ('flat', 'bump'):
        shape = 'v2p_flat' if kind == 'flat' else 'v2p_bumped'
        name = 'deffer_base_' + kind + '.vs'
        files[name] = static_vertex(files[name], shape)
        name = 'deffer_model_' + kind + '.vs'
        files[name] = model_vertex(files[name], shape)
        for alpha in (False, True):
            name = 'deffer_base_' + ('aref_' if alpha else '') + kind + '.ps'
            files[name] = pixel(files[name], 'p_flat' if kind == 'flat' else 'p_bumped', alpha)
    headers = {p.name: p.read_bytes() for p in OWNED.glob('*.h')}
    files.update(headers)
    files['rsf_core_previous_skin.h'] = previous_skin(original['skin.h'])
    report = {'schema': 1, 'abi': ABI, 'runtime_ready': False, 'motion_output_implemented': True,
              'source_corpus_sha256': CORPUS, 'source_file_count': len(original),
              'supported_roots': supported_roots(source),
              'unsupported': ['software_skinned_SKIN_NONE', 'terrain', 'forward', 'tessellation',
                              'trees', 'details', 'lod', 'particles', 'water', 'sky', 'clouds', 'hud3d'],
              'tool_sha256': digest(Path(__file__).read_bytes()),
              'header_sha256': {k: digest(v) for k, v in headers.items()},
              'files': [{'path': k, 'original_sha256': digest(original[k]) if k in original else None,
                         'staged_sha256': digest(v)} for k, v in sorted(files.items())]}
    report['cache_namespace'] = digest(json.dumps(report, sort_keys=True, separators=(',', ':')).encode())
    try:
        output.mkdir(parents=True)
        for name, data in files.items():
            target = output / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        if tree(output) != files:
            raise Refusal('staging readback failed')
        (output / '.rsf-core.json').write_text(json.dumps(report, indent=2) + '\n')
    except BaseException:
        if output.exists():
            shutil.rmtree(output)
        raise
    return report


def signature(assembly, section):
    match = re.search(r'// ' + section + r' signature:(.*?)(?=\n[^/]|// (?:Input|Output|Patch Constant) signature:)',
                      assembly, re.S)
    if not match:
        raise Refusal('missing FXC signature: ' + section)
    return { (m[1].upper(), int(m[2])): (m[3], m[4], m[5])
             for m in re.finditer(r'^// (\w+)\s+(\d+)\s+([xyzw]+)\s+\d+\s+(\w+)\s+(\w+)', match[1], re.M) }


def verify_fxc(source, output):
    import concurrent.futures
    import os
    import subprocess
    source, output = Path(source), Path(output)
    if output.exists():
        # Verification may be repeated, but never silently replace staging.
        raise Refusal('verification output exists; choose a new directory')
    staged = output / 'r3'
    manifest = stage(source, staged)
    sdk = Path(os.environ.get('ProgramFiles(x86)', 'C:/Program Files (x86)')) / 'Windows Kits/10/bin'
    fxc = os.environ.get('RSF_FXC') or shutil.which('fxc')
    candidates = sorted(sdk.glob('*/x64/fxc.exe'), reverse=True)
    if not fxc and candidates:
        fxc = str(candidates[0])
    if not fxc:
        raise Refusal('FXC required; no compiler found')
    cases = [('deffer_impl_flat.vs', 'vs', 'EXCLUDED', [], False),
             ('deffer_impl_flat_d.vs', 'vs', 'EXCLUDED', [], False)]
    for shader in sorted(source.glob('deffer_base*.vs')) + sorted(source.glob('deffer_model*.vs')):
        modes = ['NONE', '0', '1', '2', '3', '4'] if 'model' in shader.name else ['STATIC']
        for mode in modes:
            for sun in ([], ['USE_R2_STATIC_SUN=1']):
                cases.append((shader.name, 'vs', mode, (['SKIN_' + mode + '=1'] if mode != 'STATIC' else []) + sun, True))
    for shader in sorted(source.glob('deffer_base*.ps')):
        atoc = '_atoc_' in shader.name
        options = [[]]
        if not atoc:
            for samples in (2, 4, 8):
                msaa = ['USE_MSAA=1', 'MSAA_SAMPLES=' + str(samples)]
                options.append(msaa)
                if '_aref_' in shader.name:
                    options.extend([msaa + [axis + '=1'] for axis in
                                    ('MSAA_ALPHATEST_DX10_0_ATOC', 'MSAA_ALPHATEST_DX10_1_ATOC', 'MSAA_ALPHATEST_DX10_1')])
        for defines in options:
            cases.append((shader.name, 'ps', 'ATOC' if atoc else 'PIXEL', defines, not atoc))
    binaries = output / 'dxbc'
    binaries.mkdir()

    def compile_pair(index_case):
        index, (shader, stage_name, mode, defines, changed) = index_case
        pair, assemblies, warnings = [], [], []
        for tag, root in (('old', source), ('new', staged)):
            binary = binaries / (str(index) + '-' + tag + '.dxbc')
            assembly = binary.with_suffix('.asm')
            command = [str(fxc), '/nologo', '/T', stage_name + '_5_0', '/E', 'main', '/D', 'SM_5=1']
            for define in defines:
                command += ['/D', define]
            command += ['/I', str(root.resolve()), '/Fo', str(binary.resolve()), '/Fc', str(assembly.resolve()), str((root / shader).resolve())]
            run = subprocess.run(command, capture_output=True, text=True, timeout=60)
            if run.returncode:
                raise Refusal(shader + ' ' + str(defines) + '\n' + run.stdout + run.stderr)
            pair.append(binary.read_bytes())
            assemblies.append(assembly.read_text())
            warnings.append(run.stderr.count('warning X'))
        old_in, new_in = [signature(a, 'Input') for a in assemblies]
        old_out, new_out = [signature(a, 'Output') for a in assemblies]
        if changed:
            if pair[0] == pair[1]:
                raise Refusal('declaration-only output: ' + shader)
            if stage_name == 'vs':
                if old_in != new_in:
                    raise Refusal('VS input layout changed')
                required = {('TEXCOORD', 12): ('xyzw', 'NONE', 'float'), ('TEXCOORD', 13): ('x', 'NONE', 'float')}
                if new_out != {**old_out, **required}:
                    raise Refusal('VS output reflection mismatch: ' + shader)
                expected_cb = 'RSFCorePrevious'
                offsets = [('rsf_previous_wvp', 0, 64), ('rsf_previous_bones', 64, 3744), ('rsf_history', 3808, 16)]
            else:
                if new_out != {**old_out, ('SV_TARGET', 2): ('xyzw', 'TARGET', 'float')}:
                    raise Refusal('PS MRT/coverage reflection mismatch: ' + shader + str(new_out))
                expected_cb = 'RSFCorePixel'
                offsets = [('rsf_viewport', 0, 16), ('rsf_jitter', 16, 16), ('rsf_pixel_control', 32, 16)]
                if not {('TEXCOORD', 12), ('TEXCOORD', 13), ('SV_POSITION', 0)} <= new_in.keys():
                    raise Refusal('PS motion input reflection missing')
                # Every original material input remains semantically compatible.
                if any(new_in.get(key) != value for key, value in old_in.items()):
                    raise Refusal('PS native input changed')
            if not re.search(r'// ' + expected_cb + r'\s+cbuffer\s+NA\s+NA\s+cb13\s+1', assemblies[1]):
                raise Refusal('reserved b13 reflection missing')
            for name, offset, size in offsets:
                if not re.search(r'\b' + name + r'(?:\[234\])?;\s*// Offset:\s*' + str(offset) + r' Size:\s*' + str(size), assemblies[1]):
                    raise Refusal('constant layout mismatch: ' + name)
            # Original resources must retain both engine names and hardware bindings.
            resource_pattern = r'^// (\S+)\s+(cbuffer|texture|sampler|tbuffer)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\d+)\s*$'
            old_resources = re.findall(resource_pattern, assemblies[0], re.M)
            new_resources = re.findall(resource_pattern, assemblies[1], re.M)
            if any(resource not in new_resources for resource in old_resources):
                raise Refusal('native resource binding metadata changed: ' + shader)
            if any(resource[4] == 'cb13' for resource in old_resources):
                raise Refusal('reserved native constant slot collision')
            # All pre-existing reflected constant offsets/sizes must survive.
            pattern = r'^//\s+(?!row_major)(?:\w+\s+)+(\w+(?:\[\d+\])?);\s*// Offset:\s*(\d+) Size:\s*(\d+)'
            native_constants = re.findall(pattern, assemblies[0], re.M)
            for name, offset, size in native_constants:
                if not re.search(r'\b' + re.escape(name) + r';\s*// Offset:\s*' + offset + r' Size:\s*' + size, assemblies[1]):
                    raise Refusal('native constant metadata changed: ' + name)
        elif pair[0] != pair[1]:
            raise Refusal('alpha prepass changed')
        return {'index': index, 'shader': shader, 'stage': stage_name, 'mode': mode, 'defines': defines,
                'changed': changed, 'reflection_verified': True, 'warnings': warnings,
                'old_sha256': digest(pair[0]), 'new_sha256': digest(pair[1])}

    report = {'compiler': fxc, 'planned': len(cases), 'verified': 0, 'cache_namespace': manifest['cache_namespace'],
              'runtime_ready': False, 'variants': []}
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            for result in executor.map(compile_pair, enumerate(cases)):
                report['variants'].append(result)
                report['verified'] += 1
    finally:
        (output / 'fxc-report.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        result = stage(args.source, args.output)
    except (Refusal, OSError) as error:
        parser.exit(1, str(error) + '\n')
    print(json.dumps({'output': str(args.output), 'cache_namespace': result['cache_namespace']}))


if __name__ == '__main__':
    main()
