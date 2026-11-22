"""The exact-build Anomaly profiles, read from their Rust owners for the evidence tools and the packager.

games/anomaly/src/native_profile.rs, geometry_profile.rs and sr_profile.rs hold every per-build record (PROFILES:
Standard, then AVX) and the layout constants both builds share. `native`, `geometry` and `sr` return one build's
records as a dict: every scalar under its lower-case Rust name (shared constants and the build's fields), plus the site
lists as (name, rva, hex) tuples. `path` reads another copy of the owner (the evidence scripts' negative tests)."""
import pathlib
import re

SRC = pathlib.Path(__file__).resolve().parents[1] / 'games/anomaly/src'
NATIVE = SRC / 'native_profile.rs'
GEOMETRY = SRC / 'geometry_profile.rs'
SR = SRC / 'sr_profile.rs'
BUILDS = ('standard', 'avx')

_NAME = r'c"([^"]*)"'
_SITE = re.compile(r'\bsite\(\s*' + _NAME + r',\s*(0x[0-9a-f]+),\s*b"([0-9a-f]{32})",?\s*\)')
_GUARD = re.compile(r'\bUniqueGuard::new\(\s*' + _NAME + r',\s*(0x[0-9a-f]+),\s*c"([0-9a-f]+)",?\s*\)')


def _calls(function, text, arity):
    """name(c"...", a, b, ...) calls in text, as tuples of strings."""
    pattern = rf'\b{function}\(\s*' + _NAME + r',\s*([^,()\s]+)' * (arity - 1) + r',?\s*\)'
    return re.findall(pattern, text)


def sites(text):
    return [(name, int(rva, 16), expected) for name, rva, expected in _SITE.findall(text)]


def guards(text):
    return [(name, int(rva, 16), expected) for name, rva, expected in _GUARD.findall(text)]


def constants(text):
    """Every integer `const NAME: <int> = value;` of a Rust file, lower-cased (1 << n is evaluated)."""
    values = {}
    for name, value in re.findall(r'\bconst ([A-Z][A-Z0-9_]*): (?:u8|u16|u32|u64|usize) = ([^;]+);', text):
        value = value.strip()
        shift = re.fullmatch(r'1 << (\d+)', value)
        if shift:
            values[name.lower()] = 1 << int(shift[1])
        elif re.fullmatch(r'0x[0-9a-f_]+|\d[\d_]*', value):
            values[name.lower()] = int(value.replace('_', ''), 0)
    return values


def _read(path):
    return pathlib.Path(path).read_text(encoding='utf-8')


def _engine():
    """native_profile.rs ENGINE: {(build index, field): its site(...) / UniqueGuard::new(...) source}."""
    body = re.search(r'const ENGINE: \[Engine; 2\] = \[(.*?)\n\];', _read(NATIVE), re.S)[1]
    records = re.split(r'\n    Engine \{', body)[1:]
    return {(index, field): call for index, record in enumerate(records)
            for field, call in re.findall(r'\n        (\w+): ((?:site|UniqueGuard::new)\(.*?\)),', record)}


def _expand(text):
    """Writes the shared engine records the geometry and SR owners name as ENGINE[i].field in place."""
    engine = _engine()

    def replace(match):
        call = engine[(int(match[1]), match[2])]
        return re.search(r'(0x[0-9a-f]+)', call)[1] if match[3] else call
    return re.sub(r'\bENGINE\[(\d)\]\.(\w+)(\.rva)?', replace, text)

def _record(text, build):
    body = re.search(r'static PROFILES: \[Profile; 2\] = \[(.*?)\n\];', text, re.S)[1]
    records = re.split(r'\n    Profile \{', body)[1:]
    assert len(records) == len(BUILDS), 'two exact-build profiles, Standard then AVX'
    return records[BUILDS.index(build)]


def _list(record, field):
    return re.search(rf'\b{field}: \[(.*?)\n        \],', record, re.S)[1]


def _scalars(record):
    return {name: int(value, 0) for name, value in re.findall(r'\n        (\w+): (0x[0-9a-f]+|\d+),', record)}


def _text(record, field):
    return re.search(rf'\b{field}: {_NAME},', record)[1]


def native(build, path=NATIVE):
    text = _read(path)
    record = _record(text, build)
    profile = constants(text) | _scalars(record)
    profile.update(label=_text(record, 'label'), executable=_text(record, 'executable'),
                   sha256=_text(record, 'sha256'), sites=sites(_list(record, 'sites')),
                   unique=guards(_list(record, 'guards')))
    return profile


def geometry(build, path=GEOMETRY):
    text = _expand(_read(path))
    record = _record(text, build)
    profile = constants(text) | _scalars(record)
    # The layout the native geometry readers use lives with them.
    buffer = constants(_read(SRC / 'native_geometry/vertex_constants.rs'))
    profile.update(constant_buffer_slots=buffer['slots'], constant_buffer_size_offset=buffer['size_offset'],
                   constant_buffer_data_offset=buffer['data_offset'],
                   tree_pm_last_lod_offset=constants(_read(SRC / 'native_geometry.rs'))['tree_pm_last_lod_offset'])
    profile.update(sites=sites(_list(record, 'sites')), layout=sites(_list(record, 'layout_sites')),
                   unique=guards(_list(record, 'unique')),
                   hierarchy_vtables=[int(v, 16) for v in
                                      re.findall(r'0x[0-9a-f]+', re.search(r'hierarchy_vtables: \[([^\]]*)\]', record)[1])])
    return profile


def sr(build, path=SR):
    text = _expand(_read(path))
    record = _record(text, build)
    profile = constants(text) | _scalars(record)
    unique = re.search(r'unique_guards: &(\w+),', record)[1]
    unique_table = re.search(rf'const {unique}: \[UniqueGuard; (\d+)\] = \[(.*?)\n\];', text, re.S)
    slots = re.search(r'static TARGET_SLOTS: \[TargetSlot; (\d+)\] = \[(.*?)\n\];', text, re.S)
    world = re.search(r'static WORLD_TARGETS: \[WorldTarget; (\d+)\] = \[(.*?)\n\];', text, re.S)
    profile.update(
        functions=sites(_list(record, 'function_sites')),
        calls=sites(_list(record, 'call_sites')),
        first_person=[(n, int(a, 16), int(b, 16), int(c, 16))
                      for n, a, b, c in _calls('first_person', _list(record, 'first_person_sites'), 4)],
        unique=guards(unique_table[2]), unique_declared=int(unique_table[1]),
        reconstruction=guards(_list(record, 'reconstruction_guards')),
        ssao_set_viewport_code=_text(record, 'ssao_set_viewport_code'),
        slots=[(n, int(o, 16), int(f)) for n, o, f in _calls('slot', slots[2], 3)], slots_declared=int(slots[1]),
        world=[(n, int(o, 16), int(f), a == 'true', h == 'true') for n, o, f, a, h in _calls('world', world[2], 5)],
        world_declared=int(world[1]),
        absent=[int(v, 16) for v in re.findall(r'0x[0-9a-f]+', re.search(r'WORLD_ABSENT_OFFSETS: \[u32; 4\] = \[([^\]]+)\];', text)[1])])
    return profile
