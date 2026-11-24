"""Resolves games/anomaly/src/discovery.rs in an installed Anomaly executable, read-only, for the evidence tools.

The adapter finds every site at start-up from the patterns and derivations in discovery.rs. This module repeats that
discovery on the executable file: each pattern must match exactly once in the executable sections, vtable slots, the
position binder and the first-person call pairs are derived the same way, and every site's expected code must match
at the found address. The evidence scripts then require each value of the recorded profiles (the fixtures' data) to
be the discovered site, so their checks run on what the patterns resolve to. Any failure raises AssertionError naming
the site. `RSF_ANOMALY_DISCOVERY` names another copy of discovery.rs (the negative tests), `RSF_ANOMALY_DIR` another
game folder.
"""
import functools
import importlib.util
import os
import pathlib
import re
import struct

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / 'games/anomaly/src'
EXES = {'standard': 'AnomalyDX11.exe', 'avx': 'AnomalyDX11AVX.exe'}

_STRING = r'c"((?:[^"\\]|\\\n)*)"'
# Recorded names that differ from the discovered site's.
ALIASES = {'position binder body': 'cl_pos_decompress_params::setup'}


@functools.lru_cache(maxsize=None)
def _derive():
    """tools/derive-site-patterns.py (capstone and numpy), loaded only when an executable is resolved."""
    spec = importlib.util.spec_from_file_location('derive_site_patterns', ROOT / 'tools/derive-site-patterns.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _text(literal):
    return re.sub(r'\\\n\s*', '', literal)


def _pattern(text):
    return [None if token == '??' else int(token, 16) for token in text.split()]


def source():
    return pathlib.Path(os.environ.get('RSF_ANOMALY_DISCOVERY', SRC / 'discovery.rs'))


def install():
    return pathlib.Path(os.environ.get('RSF_ANOMALY_DIR', 'D:/Anomaly 1.5.3')) / 'bin'


def parse(text):
    """The Site variants in order, the pattern entries and the derived sites' (feature, name, check)."""
    variants = [v for v in re.findall(r'^    (\w+),$', re.search(r'enum Site \{\n(.*?)\n\}', text, re.S)[1], re.M)]
    constants = {m[1]: _text(m[2]) for m in re.finditer(r'const (\w+): &CStr = ' + _STRING + ';', text)}
    body = re.search(r'const SIGNATURES: \[Entry; PATTERN_SITES\] = \[\n(.*?)\n\];', text, re.S)[1]
    entries = []
    for m in re.finditer(r'// Site::(\w+)\n\s*(at|after|called|global)\((.*?)\),\n', body + '\n', re.S):
        variant, kind, args = m[1], m[2], m[3]
        strings = [_text(s) for s in re.findall(_STRING, args)]
        numbers = [int(n) for n in re.findall(r'(?:^|,)\s*(\d+)\s*(?=,)', re.sub(_STRING, '""', args))]
        name, pattern = strings[0], strings[1]
        find = {'at': ('at', 0), 'after': ('at', numbers[0] if numbers else 0), 'called': ('rip', 1, 5),
                'global': ('rip', *numbers[:2])}[kind]
        check = strings[2] if kind == 'called' else ''
        entries.append((variant, name, pattern, find, check))
    derived = [(m[1], m[2], constants.get(m[3], _text(m[4] or ''))) for m in re.finditer(
        r'\(\s*(\w+),\s*c"([^"]+)",\s*(?:(\w+)|' + _STRING + r'),?\s*\)', re.search(
            r'const DERIVED: .*? = \[\n(.*?)\n\];', text, re.S)[1])]
    assert [v for v, *_ in entries] == variants[:len(entries)], 'SIGNATURES follows the Site order'
    assert len(entries) + len(derived) == len(variants), 'every derived site is described'
    numbers = {name: int(value, 0) for name, value in re.findall(r'const (\w+): usize = (0x[0-9a-f]+|\d+);', text)}
    slots = [(m[1], int(m[2])) for m in re.finditer(r'\(\s*Site::(\w+),\s*(\d+),?\s*\)', re.search(
        r'const RM_SLOTS: .*? = \[(.*?)\];', text, re.S)[1])]
    first_person = re.findall(r'\(\s*Site::(\w+),\s*\[\s*Site::(\w+),\s*Site::(\w+),?\s*\],?\s*\)', text)
    operands = [(m[1], int(m[2]), int(m[3]), m[4]) for m in re.finditer(
        r'\(\s*Site::(\w+),\s*(\d+),\s*(\d+),\s*Site::(\w+),?\s*\)', text)]
    return variants, entries, derived, numbers, slots, first_person, operands


def _matches(image, start, text):
    """The site's expected code: one of the `|`-separated patterns matches at start."""
    return any(_derive().same(image, start, _pattern(candidate)) for candidate in text.split('|'))


@functools.lru_cache(maxsize=None)
def _resolve(path, discovery):
    derive = _derive()
    image = derive.Image(path, False)
    nt = struct.unpack_from('<I', image.bytes, 0x3c)[0]
    base = struct.unpack_from('<Q', image.bytes, nt + 24 + 24)[0]
    variants, entries, derived, numbers, slots, first_person, operands = parse(
        pathlib.Path(discovery).read_text(encoding='utf-8'))
    names = {v: n for (v, n, *_) in entries} | {v: n for v, (_, n, _) in zip(variants[len(entries):], derived)}
    guards = {}
    rva = {}
    for variant, name, pattern, find, check in entries:
        hits = image.matches(derive.regex_of(_pattern(pattern)), limit=2)
        assert len(hits) == 1, (f'site {name!r}', 'was not found' if not hits else 'matched more than once')
        if find[0] == 'at':
            rva[variant] = hits[0] + find[1]
            guards[variant] = (check or pattern, hits[0] if not check else rva[variant])
        else:
            _, disp, end = find
            rva[variant] = hits[0] + end + struct.unpack_from('<i', image.bytes, hits[0] + disp)[0]
            if check:
                guards[variant] = (check, rva[variant])
    for variant, (_, _, check) in zip(variants[len(entries):], derived):
        if check:
            guards[variant] = (check, None)

    def pointer(at):
        value = struct.unpack_from('<Q', image.bytes, at)[0] - base
        assert 0 < value < len(image.bytes), ('unreadable pointer', hex(at))
        return value

    rva['VertexConstantBuffers'] -= numbers['VERTEX_CONSTANTS_STRIDE']
    for variant, slot in slots:
        rva[variant] = pointer(rva['RendererVtable'] + 8 * slot)
    rva['SkeletonPmRender'] = pointer(rva['SkeletonPmVtable'] + 8 * numbers['SKELETON_PM_RENDER_SLOT'])
    at = rva['BinderRegistration']
    rva['PositionSetupVtable'] = pointer(at + 7 + struct.unpack_from('<i', image.bytes, at + 3)[0])
    rva['PositionSetup'] = pointer(rva['PositionSetupVtable'])

    def call(at):
        return image.bytes[at] == 0xe8 and at + 5 + struct.unpack_from('<i', image.bytes, at + 1)[0]

    window, span = numbers['FIRST_PERSON_WINDOW'], numbers['FIRST_PERSON_SPAN']
    for function, set_p, near in first_person:
        start = rva[function]
        first = next((at for at in range(start, start + window) if call(at) == rva['SetP']), None)
        assert first is not None, (f'site {names[set_p]!r}', 'was not found')
        second = next((at for at in range(first + 5, first + span) if image.bytes[at:at + 6] == bytes.fromhex(
            'ff90b8010000') or call(at) == rva['RmNear']), None)
        assert second is not None, (f'site {names[near]!r}', 'was not found')
        rva[set_p], rva[near] = first, second
    for variant, (text, start) in guards.items():
        start = rva[variant] if start is None else start
        assert _matches(image, start, text), (f'site {names[variant]!r}', 'does not hold its expected code')
    for site, disp, end, target in operands:
        at = rva[site]
        assert at + end + struct.unpack_from('<i', image.bytes, at + disp)[0] == rva[target], (
            f'site {names[site]!r}', 'does not read its global')
    return {names[v]: rva[v] for v in variants}, {v: rva[v] for v in variants}


def sites(build):
    """{site name: RVA} discovered in the installed executable of `build`."""
    return _resolve(str(install() / EXES[build]), str(source()))[0]


def variants(build):
    return _resolve(str(install() / EXES[build]), str(source()))[1]


def builder_fields(owner):
    """{record field: Site variant} of the owner's `discovered` builder (`field: found.rva(Site::X)`)."""
    text = (SRC / owner).read_text(encoding='utf-8')
    return dict(re.findall(r'(\w+): found\.rva\(Site::(\w+)\)', text))


def require_sites(build, owner, pairs):
    """Every recorded (name, RVA) of `owner` is the discovered site of that name."""
    found = sites(build)
    for name, rva in pairs:
        site = ALIASES.get(name, name)
        assert site in found, (owner, 'no discovered site', name)
        assert found[site] == rva, (owner, f'recorded {name} {rva:#x} is not the discovered {found[site]:#x}')


def require_fields(build, owner, values):
    """Every recorded field the owner's builder fills from a site equals that discovered site."""
    found, fields = variants(build), builder_fields(owner)
    for field, rva in values.items():
        if field in fields:
            assert found[fields[field]] == rva, (owner, f'recorded {field} {rva:#x} is not the discovered '
                                                        f'{fields[field]} {found[fields[field]]:#x}')


def require_record(build, owner, record):
    """A profile record of anomaly_profiles: its site lists, first-person sites, vtables and RVA fields."""
    for key in ('sites', 'layout', 'functions', 'calls', 'unique', 'reconstruction'):
        require_sites(build, owner, [(name, rva) for name, rva, *_ in record.get(key, [])])
    text = (SRC / owner).read_text(encoding='utf-8')
    found = variants(build)
    for name, function, set_p, near in record.get('first_person', []):
        m = re.search(r'first_person\(\s*c"' + re.escape(name) + r'",\s*Site::(\w+),\s*Site::(\w+),\s*Site::(\w+)', text)
        assert m, (owner, 'first-person site not built', name)
        assert [found[v] for v in m.groups()] == [function, set_p, near], (owner, 'first-person site', name)
    if 'hierarchy_vtables' in record:
        listed = re.findall(r'Site::(\w+)', re.search(r'hierarchy_vtables: \[(\s*Site::.*?)\]', text, re.S)[1])
        assert [found[v] for v in listed] == record['hierarchy_vtables'], (owner, 'hierarchy vtables')
    require_fields(build, owner, {k: v for k, v in record.items() if isinstance(v, int)})
