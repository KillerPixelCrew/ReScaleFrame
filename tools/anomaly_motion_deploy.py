#!/usr/bin/env python3
"""Build the local motion-shader deployment the Anomaly adapter loads with [Anomaly] Motion=1.

The output holds licensed game shader bodies, so it is written only beneath .local/anomaly/motion-deploy* and is
never tracked or shipped. It is generated from the user's own pinned r3 corpus by the existing core and foliage
transforms (tools/anomaly_temporal_shader.py, tools/anomaly_foliage_shader.py); this tool only merges them, adds
the manifest and verifies the result.

Layout (copy the whole directory to <game>/bin/ReScaleFrame/anomaly-motion, or point MotionShaderDir at it):
    manifest.txt        rsf-anomaly-motion-manifest 1 / namespace / one `root` line per allowlisted root
                        (stage vs/ps/hs/ds; the tessellation roots are named dx11/tess.hs and dx11/tess.ds)
    original/r3/...     the untouched corpus: the adapter recompiles each engine permutation from it and requires a
                        byte-identical result before it trusts the inputs it read from the engine
    motion/r3/...       the merged core + foliage transform
    .rsf-motion.json    provenance (corpus, tool hashes, core/foliage namespaces, file hashes)
The adapter writes verified variants to cache/<namespace prefix>/ inside the deployment at run time.

Run: py -3 tools/anomaly_motion_deploy.py --source .local/anomaly/vanilla/shaders/r3 --output .local/anomaly/motion-deploy
"""
import argparse
import hashlib
import importlib.util
import json
import re
import shutil
import sys
import uuid
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
LOCAL = REPO / '.local/anomaly'
CORPUS = 'fdc260e0608f259d876c4ae0f11a6f2d8c03c001cf1fe82548794af1c6826a67'
ABI = 1
BLOCKED = 'deffer_tree_s_bump_d.vs'   # recursively includes itself (foliage README)


class Refusal(ValueError):
    """The input or output boundary did not match the reviewed conditions."""


def load(name):
    spec = importlib.util.spec_from_file_location(name, REPO / 'tools' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(data):
    return hashlib.sha256(data).hexdigest()


def tree(root):
    result = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink() or path.is_junction():
            raise Refusal('linked path: ' + str(path))
        if path.is_file():
            result[path.relative_to(root).as_posix()] = path.read_bytes()
    return result


INCLUDE = re.compile(rb'^[ \t]*#[ \t]*include[ \t]*"([^"]+)"', re.M)


def includes(files, name, seen=None):
    """Case-insensitive include closure of `name` inside the r3 tree (the engine includer's first lookup)."""
    seen = set() if seen is None else seen
    lower = {k.lower(): k for k in files}
    key = lower.get(name.lower())
    if key is None or key in seen:
        return seen
    seen.add(key)
    for match in INCLUDE.finditer(files[key]):
        includes(files, match[1].decode().replace('\\', '/'), seen)   # "DX11\tess.h" is dx11/tess.h in the tree
    return seen


def family(name):
    # Tessellation pass (uber_deffer.cpp r_TessPass): the hull and domain shaders of the G-buffer pass. The shadow
    # domain shader (dx11/tess_shadow.ds) is not transformed and stays stock.
    if name == 'dx11/tess.hs':
        return 'hs', 'tess_hull'
    if name == 'dx11/tess.ds':
        return 'ds', 'tess_domain'
    if name.startswith('deffer_base') and name.endswith('.ps') and '_atoc_' not in name:
        return 'ps', 'core_pixel'
    if name.startswith('deffer_base') and name.endswith('.vs'):
        return 'vs', 'core_static'
    if name.startswith('deffer_model') and name.endswith('.vs'):
        return 'vs', 'core_model'
    if name.startswith('deffer_tree') and name.endswith('.vs'):
        return 'vs', 'foliage_tree'
    if name.startswith('deffer_detail') and name.endswith('.vs'):
        return 'vs', 'foliage_detail'
    return None


def build(source, output):
    source, output = Path(source).absolute(), Path(output).absolute()
    for path in (source, output):
        for ancestor in (path, *path.parents):
            if ancestor.is_symlink() or ancestor.is_junction():
                raise Refusal('linked path')
    try:
        relative = output.relative_to(LOCAL)
    except ValueError as error:
        raise Refusal('output must be inside .local/anomaly/motion-deploy*') from error
    if not relative.parts or not relative.parts[0].startswith('motion-deploy') or '..' in relative.parts:
        raise Refusal('output must be inside .local/anomaly/motion-deploy*')
    if output.exists():
        raise Refusal('output already exists')
    core_tool, foliage_tool = load('anomaly_temporal_shader'), load('anomaly_foliage_shader')
    original = tree(source)
    if len(original) != 449 or core_tool.corpus_hash(original) != CORPUS:
        raise Refusal('unreviewed source corpus')
    # The core tool only stages beneath .local/anomaly/core*; use a fresh private name and remove it afterwards.
    scratch = LOCAL / ('core-motion-deploy-' + uuid.uuid4().hex)
    try:
        core_report = core_tool.stage(source, scratch)
        motion = tree(scratch)
    finally:
        if scratch.exists():
            shutil.rmtree(scratch)
    motion.pop('.rsf-core.json', None)
    patches, foliage_report = foliage_tool.transform(source)
    for name, data in patches.items():
        motion[name] = data
    allowlist = []
    for name in sorted(original):
        kind = family(name)
        if kind is None:
            continue
        closure = includes(motion, name)
        if any(item.lower() == BLOCKED for item in closure):
            continue
        changed = [item for item in closure if motion.get(item) != original.get(item)]
        if not changed:
            continue   # nothing in this root's include closure is transformed: no motion variant exists
        allowlist.append((kind[0], kind[1], name, digest(original[name]), digest(motion[name])))
    tool_hash = digest(Path(__file__).read_bytes())
    report = {'schema': 1, 'abi': ABI, 'runtime_ready': False, 'source_corpus_sha256': CORPUS,
              'core_cache_namespace': core_report['cache_namespace'],
              'foliage_patch_sha256': {k: digest(v) for k, v in sorted(patches.items())},
              'deploy_tool_sha256': tool_hash, 'blocked_roots': [BLOCKED],
              'motion_files': {k: digest(v) for k, v in sorted(motion.items())},
              'roots': [{'stage': s, 'family': f, 'name': n, 'original_sha256': o, 'motion_sha256': m}
                        for s, f, n, o, m in allowlist]}
    report['namespace'] = digest(json.dumps(report, sort_keys=True, separators=(',', ':')).encode())
    originals = {}
    for s, f, n, o, m in allowlist:
        if o in originals:
            raise Refusal('two allowlisted roots share original source: ' + originals[o] + ', ' + n)
        originals[o] = n
    lines = ['rsf-anomaly-motion-manifest 1', 'namespace ' + report['namespace']]
    lines += [f'root {s} {f} {n} {o} {m}' for s, f, n, o, m in allowlist]
    try:
        for subtree, files in (('original', original), ('motion', motion)):
            for name, data in files.items():
                target = output / subtree / 'r3' / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
            if tree(output / subtree / 'r3') != files:
                raise Refusal('readback failed: ' + subtree)
        (output / 'manifest.txt').write_text('\n'.join(lines) + '\n', encoding='ascii', newline='\n')
        (output / '.rsf-motion.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    except BaseException:
        if output.exists():
            shutil.rmtree(output)
        raise
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        report = build(args.source, args.output)
    except (Refusal, OSError) as error:
        parser.exit(2, str(error) + '\n')
    counts = {}
    for root in report['roots']:
        counts[root['family']] = counts.get(root['family'], 0) + 1
    print(json.dumps({'namespace': report['namespace'], 'roots': counts, 'output': str(args.output)}, indent=2))


if __name__ == '__main__':
    sys.exit(main())
