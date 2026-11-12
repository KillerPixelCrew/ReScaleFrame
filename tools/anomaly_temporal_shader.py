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
# X-Ray's DX11 tessellation pass for PN/displacement bump materials (uber_deffer.cpp: VS -> DX11\tess.hs ->
# DX11\tess.ds -> PS); the shadow domain shader (dx11/tess_shadow.ds) never draws into the G-buffer.
TESS_HULL, TESS_DOMAIN = 'dx11/tess.hs', 'dx11/tess.ds'


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
    # Tessellated draws (DX11\tess.hs/ds): the motion VS also hands the PREVIOUS object-space control data to the hull
    # stage, so the domain stage can rebuild the previous PN patch. The extra registers sit after every native field and
    # before SV_Position, which keeps the hull-stage input (p_bumped, no SV_Position) a register prefix of the VS output.
    # The domain stage writes v2p_bumped without them, so the PS interface (p_bumped + SV_Position) is unchanged.
    tess = ('#if defined(RSF_TEMPORAL) && defined({gate})\n'
            '    float4 rsf_previous_position : TEXCOORD14;\n'
            '    float3 rsf_previous_normal : TEXCOORD15;\n'
            '#endif\n')
    for name, gate, anchor in (('v2p_bumped', 'RSF_TESS_VERTEX', r'\n[ \t]*float4[ \t]+hpos[ \t]*:[ \t]*SV_Position;'),
                               ('p_bumped', 'RSF_TESS_CONTROL', r'\n\};')):
        expression = r'(\bstruct\s+' + name + r'\s*\{(?:(?!\n\};).)*?)(' + anchor + r')'
        text, count = re.subn(expression, lambda m: m[1] + '\n' + tess.format(gate=gate).rstrip('\n') + m[2], text,
                              flags=re.S)
        if count != 1:
            raise Refusal('shared IO tessellation anchor: ' + name)
    return text.encode()


# A bump VS permutation compiled for X-Ray's tessellation pass (uber_deffer.cpp r_TessPass adds TESS_PN/TESS_HM to the
# VS, HS, DS and PS of the pass) feeds the hull stage instead of the rasterizer.
TESS_VERTEX_GATE = '#if defined(TESS_PN) || defined(TESS_HM)\n#define RSF_TESS_VERTEX 1\n#endif\n'


def static_vertex(data, shape):
    # Native body is byte-preserved behind a conditional entrypoint alias.
    gate = TESS_VERTEX_GATE if shape == 'v2p_bumped' else ''
    prefix = '#ifndef RSF_CORE_EXCLUDE\n#define RSF_TEMPORAL 1\n' + gate + '#define main rsf_native_main\n#endif\n'
    tess = ''
    if shape == 'v2p_bumped':
        # Static geometry has no previous deformation: the previous object-space control point is the vertex itself, the
        # normal the native body's own unpack (unpack_bx4 of the swizzled Nh), so rsf_previous_wv carries all motion.
        tess = ('#ifdef RSF_TESS_VERTEX\n'
                '    o.rsf_previous_position = v.P;\n'
                '    o.rsf_previous_normal = unpack_bx4(unpack_D3DCOLOR(v.Nh.xyz));\n'
                '#endif\n')
    suffix = ('\n#ifndef RSF_CORE_EXCLUDE\n#undef main\n#include "rsf_core_vertex.h"\n'
              + shape + ' main(v_in v)\n{\n'
              '    ' + shape + ' o = rsf_native_main(v);\n'
              '    o.rsf_previous_clip = mul(rsf_previous_wvp, v.P);\n'
              '    o.rsf_valid = rsf_draw_valid();\n' + tess +
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
        source = 'v' if mode == 'NONE' else 'rsf_prev_skinning_' + mode + '(v)'
        previous = source + '.P'
        gate = ' * (rsf_history.z == 1)' if mode == 'NONE' else ''
        if mode in ('1', '2', '3', '4'):
            gate = ' * (rsf_history.y == 1)'
        tess = ''
        if shape == 'v2p_bumped':
            # The previous skinned object-space position and normal, scaled exactly as the native body scales N (2 * N
            # before m_WV), so the domain stage's previous patch matches the current one's construction.
            tess = ('#ifdef RSF_TESS_VERTEX\n'
                    '    o.rsf_previous_position = ' + previous + ';\n'
                    '    o.rsf_previous_normal = 2 * ' + source + '.N;\n'
                    '#endif\n')
        suffix += ('#ifdef SKIN_' + mode + '\n' + shape + ' main(' + input_type + ' v)\n{\n'
                   '    ' + shape + ' o = rsf_native_main(v);\n'
                   '    o.rsf_previous_clip = mul(rsf_previous_wvp, ' + previous + ');\n'
                   '    o.rsf_valid = rsf_draw_valid()' + gate + ';\n' + tess +
                   '    return o;\n}\n#endif\n')
    gate = TESS_VERTEX_GATE if shape == 'v2p_bumped' else ''
    return ('#define RSF_TEMPORAL 1\n' + gate + '#define main rsf_native_main\n').encode() + data + suffix.encode()


def insert_after(text, pattern, insertion, what):
    """Insert first-party text after the one native statement matching `pattern`; refuse on 0 or 2+ matches."""
    matches = list(re.finditer(pattern, text))
    if len(matches) != 1:
        raise Refusal('tessellation anchor (' + str(len(matches)) + ' matches): ' + what)
    end = matches[0].end()
    return text[:end] + insertion + text[end:]


def tess_hull(data):
    # The hull stage copies each control point field by field; the RSF fields are copied the same way.
    text = insert_after(data.decode(), r'\n[ \t]*ouput\.M3[ \t]*=[ \t]*ip\[i\]\.M3;',
                        '\n#ifdef RSF_TEMPORAL\n'
                        '\touput.rsf_previous_clip = ip[i].rsf_previous_clip;\n'
                        '\touput.rsf_valid = ip[i].rsf_valid;\n'
                        '\touput.rsf_previous_position = ip[i].rsf_previous_position;\n'
                        '\touput.rsf_previous_normal = ip[i].rsf_previous_normal;\n'
                        '#endif', 'tess.hs control point copy')
    return ('#define RSF_TEMPORAL 1\n#define RSF_TESS_CONTROL 1\n' + text).encode()


def tess_domain(data):
    """The previous surface point at the SAME domain location: same uvw (after the native edge snap), the previous PN
    patch built from the previous control points with the native ComputePNPatch/ComputePatchVertex (plus this frame's
    delivered-minus-textbook patch difference carried rigidly, rsf_core_tess.h rsf_tess_carry_patch), the native
    displacement (same height texels: the texture coordinates do not animate) along the previous normal, the native
    edge rule (minc == 0 keeps the flat point), then the previous projection."""
    text = data.decode()
    # After tess.h, so the header sees PNPatch.
    text = insert_after(text, r'#include[ \t]+"DX11\\tess\.h"', '\n#include "rsf_core_tess.h"', 'tess.ds tess.h include')
    text = insert_after(text, r'\n[ \t]*output\.position[ \t]*=[ \t]*bp\[0\]\.position\*w \+ bp\[1\]\.position\*v \+ '
                              r'bp\[2\]\.position\*u;',
                        '\n#ifdef RSF_TEMPORAL\n'
                        '\tfloat3 rsf_pp[3] =\n\t{\n'
                        '\t\trsf_tess_view_position(bp[0].rsf_previous_position),\n'
                        '\t\trsf_tess_view_position(bp[1].rsf_previous_position),\n'
                        '\t\trsf_tess_view_position(bp[2].rsf_previous_position)\n\t};\n'
                        '\tfloat3 rsf_pn[3] =\n\t{\n'
                        '\t\trsf_tess_view_normal(bp[0].rsf_previous_normal),\n'
                        '\t\trsf_tess_view_normal(bp[1].rsf_previous_normal),\n'
                        '\t\trsf_tess_view_normal(bp[2].rsf_previous_normal)\n\t};\n'
                        '\tfloat3 rsf_prev = rsf_pp[0]*w + rsf_pp[1]*v + rsf_pp[2]*u;\n'
                        '\tfloat3 rsf_prev_flat = rsf_prev;\n'
                        '\tfloat3 rsf_prev_normal = normalize(rsf_pn[0]*w + rsf_pn[1]*v + rsf_pn[2]*u);\n'
                        '#endif', 'tess.ds flat position')
    text = insert_after(text, r'\n[ \t]*ComputePatchVertex\(P, N, uvw, input\.patch, output\.position\.xyz, Normal\);',
                        '\n#\tifdef RSF_TEMPORAL\n'
                        '\t{\n'
                        '\t\tfloat3 rsf_unit[3] = { normalize(rsf_pn[0]), normalize(rsf_pn[1]), normalize(rsf_pn[2]) };\n'
                        '\t\tfloat3 rsf_current_unit[3] = { normalize(N[0]), normalize(N[1]), normalize(N[2]) };\n'
                        '\t\tPNPatch rsf_patch, rsf_reference;\n'
                        '\t\tComputePNPatch(rsf_pp, rsf_unit, rsf_patch);\n'
                        '\t\tComputePNPatch(P, rsf_current_unit, rsf_reference);\n'
                        '\t\trsf_tess_carry_patch(P, rsf_pp, input.patch, rsf_reference, rsf_patch);\n'
                        '\t\tComputePatchVertex(rsf_pp, rsf_pn, uvw, rsf_patch, rsf_prev, rsf_prev_normal);\n'
                        '\t}\n'
                        '#\tendif', 'tess.ds PN evaluation')
    for tcd in ('output.tcdbump', '0'):
        text = insert_after(text, r'\n[ \t]*ComputeDisplacedVertex\(output\.position\.xyz, Normal, output\.tcdh, ' +
                            re.escape(tcd) + r'\);',
                            '\n#\t\tifdef RSF_TEMPORAL\n'
                            '\tComputeDisplacedVertex(rsf_prev, rsf_prev_normal, output.tcdh, ' + tcd + ');\n'
                            '#\t\tendif', 'tess.ds displacement ' + tcd)
    text = insert_after(text, r'\n[ \t]*\[flatten\]if\( minc==0 \)\s*\n[ \t]*output\.position\.xyz = triPos;',
                        '\n#ifdef RSF_TEMPORAL\n'
                        '\t[flatten]if( minc==0 )\n'
                        '\t\trsf_prev = rsf_prev_flat;\n'
                        '#endif', 'tess.ds edge rule')
    text = insert_after(text, r'\n[ \t]*output\.hpos[ \t]*=[ \t]*mul\(m_P, float4\(output\.position\.xyz,1\)\);',
                        '\n#ifdef RSF_TEMPORAL\n'
                        '\toutput.rsf_previous_clip = mul(rsf_previous_projection, float4(rsf_prev, 1));\n'
                        '\toutput.rsf_valid = rsf_tess_valid(bp[0].rsf_valid, bp[1].rsf_valid, bp[2].rsf_valid);\n'
                        '#endif', 'tess.ds projection')
    return ('#define RSF_TEMPORAL 1\n#define RSF_TESS_CONTROL 1\n' + text).encode()


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
    files[TESS_HULL] = tess_hull(files[TESS_HULL])
    files[TESS_DOMAIN] = tess_domain(files[TESS_DOMAIN])
    headers = {p.name: p.read_bytes() for p in OWNED.glob('*.h')}
    files.update(headers)
    files['rsf_core_previous_skin.h'] = previous_skin(original['skin.h'])
    report = {'schema': 1, 'abi': ABI, 'runtime_ready': False, 'motion_output_implemented': True,
              'source_corpus_sha256': CORPUS, 'source_file_count': len(original),
              'supported_roots': supported_roots(source),
              'tessellation_roots': [TESS_HULL, TESS_DOMAIN],
              'unsupported': ['software_skinned_SKIN_NONE', 'terrain', 'forward', 'tessellated_shadow_domain',
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

    # Tessellation pass (uber_deffer.cpp r_TessPass adds the same TESS_* and detail defines to all four stages). Kept
    # apart from 'variants', whose VS/PS cases the device harness (tests/anomaly_temporal_shader_verify.py) consumes.
    tess_sets = [['TESS_PN=1'], ['TESS_PN=1', 'USE_TDETAIL=1', 'USE_TDETAIL_BUMP=1'], ['TESS_PN=1', 'TESS_HM=1', 'USE_TDETAIL=1'],
                 ['TESS_HM=1', 'USE_LM_HEMI=1']]
    tess_cases = []
    for tess in tess_sets:
        for mode in ['NONE', '0', '1', '2', '3', '4']:
            tess_cases.append(('deffer_model_bump.vs', 'vs', mode, ['SKIN_' + mode + '=1'] + tess))
        tess_cases.append(('deffer_base_bump.vs', 'vs', 'STATIC', tess))
        tess_cases.append((TESS_HULL, 'hs', 'HULL', tess))
        tess_cases.append((TESS_DOMAIN, 'ds', 'DOMAIN', tess))
        for shader in ('deffer_base_bump.ps', 'deffer_base_aref_bump.ps'):
            tess_cases.append((shader, 'ps', 'PIXEL', tess))
    control = {('TEXCOORD', 12): ('xyzw', 'NONE', 'float'), ('TEXCOORD', 13): ('x', 'NONE', 'float'),
               ('TEXCOORD', 14): ('xyzw', 'NONE', 'float'), ('TEXCOORD', 15): ('xyz', 'NONE', 'float')}
    to_pixel = {k: v for k, v in control.items() if k[1] in (12, 13)}

    def compile_tess(index_case):
        index, (shader, stage_name, mode, defines) = index_case
        pair, assemblies = [], []
        for tag, root in (('old', source), ('new', staged)):
            binary = binaries / ('tess-' + str(index) + '-' + tag + '.dxbc')
            assembly = binary.with_suffix('.asm')
            command = [str(fxc), '/nologo', '/T', stage_name + '_5_0', '/E', 'main', '/D', 'SM_5=1', '/Zpr']
            for define in defines:
                command += ['/D', define]
            command += ['/I', str(root.resolve()), '/Fo', str(binary.resolve()), '/Fc', str(assembly.resolve()), str((root / shader).resolve())]
            run = subprocess.run(command, capture_output=True, text=True, timeout=60)
            if run.returncode:
                raise Refusal(shader + ' ' + str(defines) + '\n' + run.stdout + run.stderr)
            pair.append(binary.read_bytes())
            assemblies.append(assembly.read_text())
        old_in, new_in = [signature(a, 'Input') for a in assemblies]
        old_out, new_out = [signature(a, 'Output') for a in assemblies]
        if pair[0] == pair[1]:
            raise Refusal('declaration-only tessellation output: ' + shader)
        if stage_name == 'vs':
            if old_in != new_in or new_out != {**old_out, **control}:
                raise Refusal('tessellation VS interface mismatch: ' + shader + ' ' + str(defines))
        elif stage_name == 'ps':
            if not {('TEXCOORD', 12), ('TEXCOORD', 13), ('SV_POSITION', 0)} <= new_in.keys() or \
                    {('TEXCOORD', 14), ('TEXCOORD', 15)} & new_in.keys():
                raise Refusal('tessellation PS input mismatch: ' + shader)
        else:
            if signature(assemblies[0], 'Patch Constant') != signature(assemblies[1], 'Patch Constant'):
                raise Refusal('patch-constant signature changed: ' + shader)
            if new_in != {**old_in, **control} or new_out != {**old_out, **(control if stage_name == 'hs' else to_pixel)}:
                raise Refusal('tessellation ' + stage_name + ' interface mismatch: ' + shader + ' ' + str(defines))
            if stage_name == 'ds':
                if not re.search(r'// RSFTessPrevious\s+cbuffer\s+NA\s+NA\s+cb13\s+1', assemblies[1]):
                    raise Refusal('RSFTessPrevious b13 reflection missing')
                for name, offset, size in (('rsf_previous_wv', 0, 64), ('rsf_previous_projection', 64, 64), ('rsf_tess_history', 128, 16)):
                    if not re.search(r'\b' + name + r';\s*// Offset:\s*' + str(offset) + r' Size:\s*' + str(size), assemblies[1]):
                        raise Refusal('tessellation constant layout mismatch: ' + name)
            elif 'cb13' in assemblies[1]:
                raise Refusal('the hull stage must not bind b13')
        resource_pattern = r'^// (\S+)\s+(cbuffer|texture|sampler|tbuffer)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\d+)\s*$'
        old_resources = re.findall(resource_pattern, assemblies[0], re.M)
        new_resources = re.findall(resource_pattern, assemblies[1], re.M)
        if any(resource not in new_resources for resource in old_resources):
            raise Refusal('native resource binding metadata changed: ' + shader)
        return {'index': index, 'shader': shader, 'stage': stage_name, 'mode': mode, 'defines': defines,
                'reflection_verified': True, 'old_sha256': digest(pair[0]), 'new_sha256': digest(pair[1])}

    report = {'compiler': fxc, 'planned': len(cases), 'verified': 0, 'cache_namespace': manifest['cache_namespace'],
              'runtime_ready': False, 'variants': [], 'tess_planned': len(tess_cases), 'tess_verified': 0, 'tess_variants': []}
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            for result in executor.map(compile_pair, enumerate(cases)):
                report['variants'].append(result)
                report['verified'] += 1
            for result in executor.map(compile_tess, enumerate(tess_cases)):
                report['tess_variants'].append(result)
                report['tess_verified'] += 1
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
