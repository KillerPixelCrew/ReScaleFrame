"""Read-only installed-standard-binary evidence and local REA client. No tokens in output.

Needs the installed STANDARD AnomalyDX11.exe (D:/Anomaly 1.5.3), so it is not registered in CTest.
Run: py -3 tests/anomaly_geometry_evidence.py --verify
"""
import argparse
import hashlib
import json
import pathlib
import re
import struct
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
EXE = pathlib.Path('D:/Anomaly 1.5.3/bin/AnomalyDX11.exe')
HASH = 'c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e'
SET_W = 0x840960

class PE:
    def __init__(self):
        self.data = EXE.read_bytes()
        assert hashlib.sha256(self.data).hexdigest() == HASH
        nt = struct.unpack_from('<I', self.data, 60)[0]
        n, opt = struct.unpack_from('<H', self.data, nt + 6)[0], struct.unpack_from('<H', self.data, nt + 20)[0]
        self.base = struct.unpack_from('<Q', self.data, nt + 24 + 24)[0]
        self.sections = []
        for i in range(n):
            at = nt + 24 + opt + i * 40
            vs, va, size, raw = struct.unpack_from('<IIII', self.data, at + 8)
            self.sections.append((va, max(vs, size), raw, size))
    def offset(self, rva):
        for va, size, raw, _ in self.sections:
            if va <= rva < va + size:
                return raw + rva - va
        raise ValueError(hex(rva))
    def rva(self, offset):
        for va, _, raw, size in self.sections:
            if raw <= offset < raw + size:
                return va + offset - raw
        raise ValueError(hex(offset))
    def read(self, rva, size):
        at = self.offset(rva)
        return self.data[at:at + size]
    def occurrences(self, needle):
        at = 0
        while True:
            at = self.data.find(needle, at)
            if at < 0:
                break
            yield at
            at += 1
    def tables(self, name):
        for text in self.occurrences(('.?AV' + name + '@@\0').encode()):
            type_rva = self.rva(text - 16)
            for ref in self.occurrences(struct.pack('<I', type_rva)):
                col = ref - 12
                try:
                    col_rva = self.rva(col)
                    signature, offset, cd, typ, hierarchy, own = struct.unpack_from('<IIIIII', self.data, col)
                except (ValueError, struct.error):
                    continue
                if signature != 1 or own != col_rva:
                    continue
                for pointer in self.occurrences(struct.pack('<Q', self.base + col_rva)):
                    table = self.rva(pointer + 8)
                    entries = [hex(v - self.base) for v in struct.unpack('<' + 'Q' * 65, self.read(table, 65 * 8))]
                    yield {'class': name, 'vtable': hex(table), 'this_offset': offset, 'slots': entries}
    def set_w_world_address(self):
        """Count of `lea rcx,[rip+x]` operands feeding direct calls to R_xforms::set_W."""
        text_va, text_size, raw, _ = self.sections[0]
        code = self.data[raw:raw + text_size]
        callers = {}
        for at in range(24, len(code) - 5):
            if code[at] != 0xE8 or text_va + at + 5 + struct.unpack_from('<i', code, at + 1)[0] != SET_W:
                continue
            for back in range(at - 24, at - 6):
                if code[back:back + 3] == b'\x48\x8d\x0d':
                    target = text_va + back + 7 + struct.unpack_from('<i', code, back + 3)[0]
                    callers[target] = callers.get(target, 0) + 1
        return callers

def rea(name, args):
    session = json.loads((ROOT / '.local/anomaly/rea-session.json').read_text())
    request = urllib.request.Request(f"http://127.0.0.1:{session['port']}/call", data=json.dumps({'name': name, 'arguments': args}).encode(), headers={'Authorization': 'Bearer ' + session['token'], 'Content-Type': 'application/json'})
    record = json.load(urllib.request.urlopen(request, timeout=1800))
    result = record.get('result', {})
    content = result.get('structuredContent')
    if content is None:
        content = [item.get('text', '') for item in result.get('content', [])]
    if isinstance(content, dict):
        content = content.get('normalized_result', content)
        if isinstance(content, dict) and 'items' in content:
            content = [{k: item[k] for k in ('address', 'status', 'pseudocode') if k in item} for item in content['items']]
    print(json.dumps({'record': record['id'], 'result': content}, indent=2), flush=True)

def verify(pe):
    profile = ROOT / 'games/anomaly/src/geometry_profile.h'
    assert profile.exists(), 'missing installed geometry profile'
    text = profile.read_text()
    sites = re.findall(r'\{"([^"]+)", (0x[0-9a-f]+), signature\("([0-9a-f]{32})"\)\}', text)
    assert len(sites) >= 13, 'missing verified lifecycle, visual or layout sites'
    for name, rva, expected in sites:
        assert pe.read(int(rva, 16), 16).hex() == expected, name
    wanted = {'CRender': {36: 0x839940, 37: 0x839950, 38: 0x839bd0, 39: 0x839b40},
              'Fvisual': {7: 0x8cff60}, 'FProgressive': {7: 0x8d0800},
              'CSkeletonX_ST': {7: 0x8d1610}, 'CSkeletonX_PM': {7: 0x8d1520},
              'FTreeVisual_ST': {7: 0x8d5d20}, 'FTreeVisual_PM': {7: 0x8d5a40}}
    for name, slots in wanted.items():
        table = next(t for t in pe.tables(name) if t['this_offset'] == 0)
        for slot, rva in slots.items():
            assert int(table['slots'][slot], 16) == rva, (name, slot)
    listed = re.search(r'hierarchy_vtables\{([^}]*)\}', text)
    assert listed, 'missing hierarchy vtable list'
    expected_tables = {int(v, 16) for v in re.findall(r'0x[0-9a-f]+', listed.group(1))}
    found_tables = {int(next(t for t in pe.tables(name) if t['this_offset'] == 0)['vtable'], 16)
                    for name in ('FHierrarhyVisual', 'CKinematics', 'CKinematicsAnimated', 'FLOD')}
    assert expected_tables == found_tables, (sorted(map(hex, expected_tables)), sorted(map(hex, found_tables)))
    world = int(re.search(r'world_matrix_rva = (0x[0-9a-f]+)', text).group(1), 16)
    callers = pe.set_w_world_address()
    assert callers and max(callers, key=callers.get) == world, (hex(world), callers)
    # set_W copies the first 16 floats of its argument to this+0 (m_w): mov eax,[rdx]; lea rsi,[rcx+0x100]; mov [rcx],eax.
    assert '8b02488db1000100008901' in pe.read(SET_W, 48).hex(), 'set_W copy shape'
    # LL_GetBoneInstance: (id & 0xffff) * 0xa8 + [IKinematics + 0x50]; LL_GetTransform_R adds 0x40.
    assert pe.read(0x803d90, 10).hex() == '0fb7c24869c0a8000000', 'bone instance stride'
    assert '4883c040' in pe.read(0x803da0, 20).hex(), 'render transform offset'
    print(f'PASS: installed standard hash, {len(sites)} expected16 guards, lifecycle/visual RTTI slots, '
          f'{len(found_tables)} hierarchy vtables, world matrix {hex(world)} ({callers[world]} set_W callers), '
          'bone stride/render-transform offset')

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--class', dest='classes', action='append')
    parser.add_argument('--read', nargs=2)
    parser.add_argument('--rea', nargs=2)
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args()
    if args.rea:
        rea(args.rea[0], json.loads(args.rea[1]))
    else:
        pe = PE()
        if args.verify:
            verify(pe)
        if args.read:
            print(pe.read(int(args.read[0], 0), int(args.read[1], 0)).hex())
        for name in args.classes or []:
            print(json.dumps(list(pe.tables(name)), indent=2))
