"""Read-only installed-binary evidence and local REA client. No tokens in output.

Checks games/anomaly/src/geometry_profile.h against the installed executable of one exact build: --exe standard (default,
AnomalyDX11.exe) or --exe avx (AnomalyDX11AVX.exe). Each header holds one table per build; only the selected build's table
(plus the shared layout) is read. Needs D:/Anomaly 1.5.3, so it is not registered in CTest.
Run: py -3 tests/anomaly_geometry_evidence.py --verify [--exe avx] [--profile copy-of-geometry_profile.h]
The other Anomaly evidence scripts import BUILDS/select/profile_text from here.
"""
import argparse
import hashlib
import json
import pathlib
import re
import struct
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALL = pathlib.Path('D:/Anomaly 1.5.3/bin')
# Script anchors per exact build: functions found independently of the profile (RTTI slot, call-graph position, strings;
# docs/research/anomaly-avx-profile.md). The checks then require the profile to name the same code.
BUILDS = {
    'standard': dict(
        exe='AnomalyDX11.exe', hash='c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e', section='Standard',
        SET_W=0x840960, FLUSH=0x84b930, RENDER_GRAPH=0x84f220, RENDER_HUD=0x84ffa0, RENDER_HUD_UI=0x8505a0,
        RENDER_SORTED=0x850cc0, RENDER_EMISSIVE=0x84ec80, CRENDER_RENDER=0x8977c0, CRENDER_RENDER_END=0x898600,
        SKELETON_RENDER=0x849aa0, IS_SVP_FRAME=0x2d260, DEVICE_END=0x822eb0,
        FVISUAL=0x8cff60, FPROGRESSIVE=0x8d0800, SKELETON_ST=0x8d1610, SKELETON_PM=0x8d1520,
        PHASE_COMPARE=0x8cff83, RM_NEAR=0x83a450, RM_NORMAL=0x83a4d0,
        MODEL_CREATE=0x839940, MODEL_CREATE_CHILD=0x839950, MODEL_DUPLICATE=0x839bd0, MODEL_DELETE=0x839b40,
        BONE_INSTANCE=0x803d90, BONE_TRANSFORM=0x803da0,
        DETAIL_RENDER=0x86a8c0, HW_RENDER=0x8aa320, HW_RENDER_DUMP=0x8aa6c0, HW_RENDER_CALLER=0x86aa4d,
        CONSTANT_BUFFER_FLUSH=0x83de40, TREE_PM_RENDER=0x8d5a40, TREE_ST_RENDER=0x8d5d20, TREE_BASE_RENDER=0x8d5600),
    'avx': dict(
        exe='AnomalyDX11AVX.exe', hash='2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12', section='AVX',
        SET_W=0x837c50, FLUSH=0x842490, RENDER_GRAPH=0x845d20, RENDER_HUD=0x846aa0, RENDER_HUD_UI=0x847000,
        RENDER_SORTED=0x8476a0, RENDER_EMISSIVE=0x8457e0, CRENDER_RENDER=0x88d420, CRENDER_RENDER_END=0x88e286,
        SKELETON_RENDER=0x840790, IS_SVP_FRAME=0x2d180, DEVICE_END=0x81ab40,
        FVISUAL=0x8c5970, FPROGRESSIVE=0x8c6210, SKELETON_ST=0x8c7010, SKELETON_PM=0x8c6f20,
        PHASE_COMPARE=0x8c5993, RM_NEAR=0x831700, RM_NORMAL=0x8317a0,
        MODEL_CREATE=0x830bf0, MODEL_CREATE_CHILD=0x830c00, MODEL_DUPLICATE=0x830e80, MODEL_DELETE=0x830df0,
        BONE_INSTANCE=0x7fd6a0, BONE_TRANSFORM=0x7fd6b0,
        DETAIL_RENDER=0x860170, HW_RENDER=0x8a0180, HW_RENDER_DUMP=0x8a0510, HW_RENDER_CALLER=0x8602e5,
        CONSTANT_BUFFER_FLUSH=0x835130, TREE_PM_RENDER=0x8cb2f0, TREE_ST_RENDER=0x8cb5e0, TREE_BASE_RENDER=0x8caf00),
}
BUILD = None


def select(name):
    """Select the exact build: sets EXE, HASH and every anchor below as module globals (also for importing scripts)."""
    global BUILD, EXE, HASH
    BUILD = name
    build = BUILDS[name]
    EXE = INSTALL / build['exe']
    HASH = build['hash']
    globals().update({k: v for k, v in build.items() if k.isupper()})


def profile_text(path, text=None):
    """The shared part of a profile header plus the selected build's section ("// ---- Standard ..." / "// ---- AVX ...")."""
    text = pathlib.Path(path).read_text() if text is None else text
    marks = {name: text.find('// ---- ' + BUILDS[name]['section'] + ' 1.5.3 DX11') for name in BUILDS}
    assert all(at >= 0 for at in marks.values()), ('profile has no per-build sections', str(path))
    starts = sorted(marks.values())
    begin = marks[BUILD]
    end = next((at for at in starts if at > begin), len(text))
    return text[:starts[0]] + text[begin:end]


def legacy_profile(path):
    """Selected C++ aggregate as named fields for the evidence parsers."""
    text = profile_text(path)
    tag = 'standard' if BUILD == 'standard' else 'avx'
    for domain in ('sr', 'motion'):
        text = text.replace(domain + '_' + tag + '_unique', domain + '_unique_guards')
    text = re.sub(r'\.(\w+) = \{\{(.*?)\}\},', lambda m: m[1] + '{{' + m[2] + '}};', text, flags=re.S)
    text = re.sub(r'\.(\w+) =\s*((?:"[0-9a-f]*"\s*)+),', lambda m: 'inline constexpr char ' + m[1] + '[] = ' + m[2] + ';', text)
    text = re.sub(r'\.(\w+) = (0x[0-9a-f]+|\d+)(?:,|(?=\}))', r'inline constexpr uint32_t \1 = \2;', text)
    # Motion's CallerFrame is an embedded aggregate, named frame_* in the evidence parser.
    text = re.sub(r'inline constexpr uint32_t (source|length|entry|flags|defines)_offset', r'inline constexpr uint32_t frame_\1_offset', text)
    return text


def verify_detour_prefix(pe, sites):
    """MinHook copies whole instructions through its five-byte relative relay jump.

    HDE64 cannot decode VEX. Require every copied instruction to be legacy integer code;
    retain the decoded lengths and RIP operands for review without executing game code.
    """
    import capstone
    from capstone import x86
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    result = []
    for name, rva, _ in sites:
        rva = int(rva, 16) if isinstance(rva, str) else rva
        used, parts, rip = 0, [], []
        for i in md.disasm(pe.read(rva, 32), rva):
            assert i.bytes[0] not in (0xc4, 0xc5, 0x62), ('VEX/EVEX in detour prefix', name, hex(i.address))
            used += i.size
            parts.append(i.size)
            rip.extend(i.address + i.size + op.mem.disp for op in i.operands
                       if op.type == x86.X86_OP_MEM and op.mem.base == x86.X86_REG_RIP)
            if used >= 5:
                break
        assert used >= 5, ('short detour', name)
        result.append((name, hex(rva), parts, list(map(hex, rip))))
    return result


def build_from_args(argv=None):
    """--exe standard|avx from the command line (other arguments are left to the caller's parser)."""
    pre = argparse.ArgumentParser(add_help=False)
    pre.add_argument('--exe', choices=sorted(BUILDS), default='standard')
    known, _ = pre.parse_known_args(argv)
    select(known.exe)
    return known.exe


select('standard')

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
    def text(self):
        text_va, text_size, raw, _ = self.sections[0]
        return text_va, self.data[raw:raw + text_size]
    def callers(self, *targets):
        """Direct `call rel32` sites per target, over the whole .text section."""
        text_va, code = self.text()
        wanted = set(targets)
        found = {t: [] for t in wanted}
        at = code.find(b'\xe8')
        while at >= 0:
            if at + 5 <= len(code):
                target = text_va + at + 5 + struct.unpack_from('<i', code, at + 1)[0]
                if target in wanted:
                    found[target].append(text_va + at)
            at = code.find(b'\xe8', at + 1)
        return found
    def function_end(self, rva, limit=0x1800):
        """First 16-byte-padding (CC CC) after the entry; the image pads every function this way."""
        code = self.read(rva, limit)
        for i in range(16, len(code) - 1):
            if code[i] == 0xcc and code[i + 1] == 0xcc:
                return rva + i
        return rva + limit
    def calls_in(self, start, end):
        code = self.read(start, end - start)
        return [(start + i, start + i + 5 + struct.unpack_from('<i', code, i + 1)[0])
                for i in range(len(code) - 5) if code[i] == 0xe8]
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
    def float_loads(self, start, size, register=None):
        """Targets of rip-relative scalar-float loads, legacy `F3 [REX] 0F 10 /r` (movss, standard build) or VEX
        `C5 {FA|7A} 10 /r` (vmovss, AVX build). `register` limits them to xmm<register> (0..7 without REX.R)."""
        return {target for _, target in self.float_load_sites(start, size, register)}
    def float_load_sites(self, start, size, register=None):
        code = self.read(start, size)
        found = []
        for i in range(len(code) - 9):
            if code[i] == 0xf3:
                j = i + 2 if 0x40 <= code[i + 1] <= 0x4f else i + 1
                if code[j:j + 2] == b'\x0f\x10' and (code[j + 2] & 0xc7) == 0x05 and \
                        (register is None or ((code[j + 2] >> 3) & 7) == register and j == i + 1):
                    found.append((start + i, start + j + 7 + struct.unpack_from('<i', code, j + 3)[0]))
            elif code[i] == 0xc5 and (code[i + 1] & 0x7f) == 0x7a and code[i + 2] == 0x10 and (code[i + 3] & 0xc7) == 0x05 and \
                    (register is None or ((code[i + 3] >> 3) & 7) == register and code[i + 1] & 0x80):
                found.append((start + i, start + i + 8 + struct.unpack_from('<i', code, i + 4)[0]))
        return found
    def first_float_load(self, start, size, register):
        sites = self.float_load_sites(start, size, register)
        assert sites, ('no scalar float load into xmm%d' % register, hex(start))
        return sites[0][1]


UNIQUE = re.compile(r'\{"([^"]+)", (0x[0-9a-f]+), "([0-9a-f]+)"\}')


def verify_unique_guards(pe, text, domain, sites):
    """The `<domain>_<build>_unique` extended guards of the selected section: each matches the image, is unique in .text and
    minimal, starts with the 16-byte guard of the same site, and every listed site whose 16 bytes are not unique has one."""
    block = re.search(domain + r'_(?:standard|avx)_unique\{\{(.*?)\}\};', text, re.S)
    assert block, ('missing extended unique guards', domain)
    guards = [(n, int(r, 16), bytes.fromhex(h)) for n, r, h in UNIQUE.findall(block.group(1))]
    declared = int(re.search(r'array<UniqueGuard, (\d+)> ' + domain + r'_(?:standard|avx)_unique', text).group(1))
    assert len(guards) == declared, ('unique guard parse', domain, len(guards), declared)
    _, code = pe.text()
    by_rva = {}
    for name, rva, raw in guards:
        assert pe.read(rva, len(raw)) == raw, ('unique guard bytes', name)
        assert code.count(raw) == 1, ('unique guard not unique', name)
        assert len(raw) > 16 and code.count(raw[:-1]) > 1, ('unique guard is not minimal', name, len(raw))
        by_rva[rva] = (name, raw)
    for name, rva, expected in sites:
        rva = int(rva, 16) if isinstance(rva, str) else rva
        expected = bytes.fromhex(expected) if isinstance(expected, str) else expected
        if rva in by_rva:
            assert by_rva[rva][0] == name and by_rva[rva][1].startswith(expected), ('unique guard does not extend the site', name)
        else:
            assert code.count(expected) == 1, ('16-byte guard not unique and no extended guard', domain, name)
    return len(guards)


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

def verify_context(pe, text):
    """Draw-time call site, phase, view and first-person boundaries. Raises AssertionError with the failing name."""
    profile = {n: int(r, 16) for n, r, _ in re.findall(r'\{"([^"]+)", (0x[0-9a-f]+), signature\("([0-9a-f]{32})"\)\}', text)}
    assert profile['CBackend::constants.flush'] == FLUSH
    assert profile['R_dsgraph_structure::r_dsgraph_render_hud'] == RENDER_HUD
    assert profile['R_dsgraph_structure::r_dsgraph_render_sorted'] == RENDER_SORTED
    assert profile['R_dsgraph_structure::r_dsgraph_render_emissive'] == RENDER_EMISSIVE
    callers = pe.callers(FLUSH, RENDER_HUD, RENDER_HUD_UI, RENDER_SORTED, RENDER_EMISSIVE, RENDER_GRAPH, IS_SVP_FRAME, SKELETON_RENDER)

    # constants.flush: a loop over 14 constant-buffer slots (mov edi,0xe), no arguments, called from every inlined
    # CBackend::Render. Its callers are the draw sites, so it runs once per D3D draw (or dispatch).
    body = pe.read(FLUSH, 0x80).hex()
    assert 'bf0e000000' in body and body.count('e8') >= 6 and 'c3' in body, 'flush body'
    assert len(callers[FLUSH]) >= 100, len(callers[FLUSH])
    # In every hooked visual body, and in the shared skeleton _Render, each flush call is followed within 40 bytes by the
    # D3D11 context call DrawIndexed (vtable slot 12: FF 50 60 or FF 90 60 00 00 00); no engine call sits between them.
    draw_ranges = {'Fvisual::Render': FVISUAL, 'FProgressive::Render': FPROGRESSIVE, 'FTreeVisual_ST::Render': TREE_ST_RENDER,
                   'FTreeVisual_PM::Render': TREE_PM_RENDER, 'CSkeletonX::_Render': SKELETON_RENDER}
    flush_before_draw = 0
    for name, start in draw_ranges.items():
        end = pe.function_end(start, 0x2000)
        sites = [site for site in callers[FLUSH] if start <= site < end]
        assert sites, name
        for site in sites:
            tail = pe.read(site + 5, 48).hex()
            assert 'ff5060' in tail or 'ff9060000000' in tail, (name, hex(site))
            flush_before_draw += 1
    for name, start in (('CSkeletonX_ST::Render', SKELETON_ST), ('CSkeletonX_PM::Render', SKELETON_PM)):
        assert SKELETON_RENDER in [target for _, target in pe.calls_in(start, pe.function_end(start))], name

    # CRender::phase: Fvisual::Render's `cmp dword [phase], 1` (PHASE_SMAP) and the stores in CRender::Render.
    phase = int(re.search(r'render_phase_rva = (0x[0-9a-f]+)', text).group(1), 16)
    cmp_at = PHASE_COMPARE
    raw = pe.read(cmp_at, 7)
    assert raw[:2] == b'\x83\x3d' and raw[6] == 1 and cmp_at + 7 + struct.unpack_from('<i', raw, 2)[0] == phase, 'phase compare'
    # The writes are `this`-relative: CRender::Render does `mov [rsi+0x150],1` (PHASE_SMAP, Z-fill) and later
    # `mov [rsi+0x150],r14d` (PHASE_NORMAL before render_main). The global CRender (RImplementation) is phase-0x150.
    render = pe.read(CRENDER_RENDER, CRENDER_RENDER_END - CRENDER_RENDER)
    assert render.count(b'\xc7\x86\x50\x01\x00\x00\x01\x00\x00\x00') == 1, 'phase = PHASE_SMAP store'
    assert render.count(b'\x44\x89\xb6\x50\x01\x00\x00') == 1, 'phase = PHASE_NORMAL store'
    crender = phase - 0x150
    text_va, code = pe.text()
    lea_refs = 0
    at = code.find(b'\x48\x8d')
    while at >= 0:
        if at + 7 <= len(code) and (code[at + 2] & 0xc7) == 0x05 and \
                text_va + at + 7 + struct.unpack_from('<i', code, at + 3)[0] == crender:
            lea_refs += 1
        at = code.find(b'\x48\x8d', at + 1)
    assert lea_refs >= 50, ('RImplementation', hex(crender), lea_refs)

    # View: IsSVPFrame is `isActive && dwFrame % frameDelay == 0`; End passes &Device.m_SecondViewport to it and
    # reads isCamReady at +2. dwFrame is the same global the observer reads as engine_frame.
    sv = pe.read(IS_SVP_FRAME, 32)
    assert sv[:3] == b'\x80\x39\x00' and sv[5:9] == b'\x0f\xb6\x49\x01' and sv[11:13] == b'\x8b\x05' and sv[17:19] == b'\xf7\xf1', 'IsSVPFrame shape'
    frame = IS_SVP_FRAME + 17 + struct.unpack_from('<i', sv, 13)[0]
    assert frame == int(re.search(r'device_frame_rva = (0x[0-9a-f]+)', profile_text(ROOT / 'games/anomaly/src/native_profile.h')).group(1), 16), hex(frame)
    call_at = callers[IS_SVP_FRAME]
    assert len(call_at) >= 1 and any(DEVICE_END <= c < DEVICE_END + 0x100 for c in call_at), 'End calls IsSVPFrame'
    call_in_end = next(c for c in call_at if DEVICE_END <= c < DEVICE_END + 0x100)
    lea = pe.read(call_in_end - 7, 7)
    assert lea[:3] == b'\x48\x8d\x0d', 'lea rcx,[Device.m_SecondViewport]'
    viewport = call_in_end - 7 + 7 + struct.unpack_from('<i', lea, 3)[0]
    assert viewport == int(re.search(r'second_viewport_rva = (0x[0-9a-f]+)', text).group(1), 16), hex(viewport)
    after = pe.read(call_in_end + 5, 24)
    cam = after.find(b'\x38\x05')
    assert cam >= 0 and call_in_end + 5 + cam + 6 + struct.unpack_from('<i', after, cam + 2)[0] == viewport + 2, 'isCamReady'

    # First-person: render_hud / render_sorted / render_emissive / render_hud_ui all read psHUD_FOV (movss/mulss of one
    # global); render_hud, sorted and emissive bracket their HUD work with CRender vtable slots 55 (rmNear, 0x1b8) and 57
    # (rmNormal, 0x1c8). They are called from CRender::Render, and render_sorted right after render_graph(1).
    def float_reads(start, size):
        return pe.float_loads(start, size)
    # render_hud's first float load is psHUD_FOV (psHUD_FOV * 83 -> deg2rad -> build_projection); the others share it.
    # The shared Device globals (aspect, near/far) are also common, so the HUD FOV is pinned by that first load.
    fov = {pe.first_float_load(RENDER_HUD, 0x200, register=0)}
    for name, start in (('hud_ui', RENDER_HUD_UI), ('sorted', RENDER_SORTED), ('emissive', RENDER_EMISSIVE)):
        assert fov <= float_reads(start, 0x300), name
    for name, start in (('hud', RENDER_HUD), ('sorted', RENDER_SORTED), ('emissive', RENDER_EMISSIVE)):
        end = pe.function_end(start, 0x1000)
        code = pe.read(start, end - start)
        near, normal = code.find(b'\xff\x90\xb8\x01\x00\x00'), code.find(b'\xff\x90\xc8\x01\x00\x00')
        assert 0 < near < normal, (name, near, normal)
    hud_sites = [c for c in callers[RENDER_HUD] if CRENDER_RENDER <= c < CRENDER_RENDER_END]
    assert len(hud_sites) == 2, [hex(c) for c in callers[RENDER_HUD]]
    assert any(CRENDER_RENDER <= c < CRENDER_RENDER_END for c in callers[RENDER_EMISSIVE])
    assert any(0 < c - g <= 0x30 for c in callers[RENDER_SORTED] for g in callers[RENDER_GRAPH]), 'render_sorted follows render_graph'
    # rmNear sets viewport depth 0..0.02, rmNormal 0..1 (movss xmm1,[rip+x] holds MaxDepth).
    def max_depth(start):
        return struct.unpack('<f', pe.read(pe.first_float_load(start, 0x60, register=1), 4))[0]
    assert abs(max_depth(RM_NEAR) - 0.02) < 1e-6 and max_depth(RM_NORMAL) == 1.0, 'rmNear/rmNormal viewport depth'

    # Static level geometry: r_dsgraph_render_graph and render_landscape call set_W(&xforms, &Fidentity); the same
    # global is the source of 18 set_W callers. Its identity contents are set at startup, so the classifier compares the
    # live matrix and the log reports how many draws matched.
    graph = pe.read(RENDER_GRAPH, 0x90)
    set_w = next(i for i in range(len(graph) - 5) if graph[i] == 0xe8 and
                 RENDER_GRAPH + i + 5 + struct.unpack_from('<i', graph, i + 1)[0] == SET_W)
    lea_rdx, lea_rcx = graph[set_w - 14:set_w - 7], graph[set_w - 7:set_w]
    assert lea_rdx[:3] == b'\x48\x8d\x15' and lea_rcx[:3] == b'\x48\x8d\x0d'
    identity = RENDER_GRAPH + set_w - 14 + 7 + struct.unpack_from('<i', lea_rdx, 3)[0]
    world = RENDER_GRAPH + set_w - 7 + 7 + struct.unpack_from('<i', lea_rcx, 3)[0]
    assert world == int(re.search(r'world_matrix_rva = (0x[0-9a-f]+)', text).group(1), 16)
    return {'flush_callers': len(callers[FLUSH]), 'draw_flush_sites': flush_before_draw, 'phase': hex(phase), 'RImplementation': hex(crender),
            'viewport': hex(viewport), 'psHUD_FOV': hex(next(iter(fov))), 'Fidentity': hex(identity)}


def verify_foliage(pe, text):
    """Tree and grass sites: the staged vertex constants, the detail render window and the progressive tree's slide window."""
    number = lambda name: int(re.search(name + r' = (0x[0-9a-f]+|\d+)', text).group(1), 0)
    callers = pe.callers(FLUSH, CONSTANT_BUFFER_FLUSH, HW_RENDER, HW_RENDER_DUMP, TREE_BASE_RENDER)

    # CBackend::constants.flush walks six per-stage tables of 14 ref_cbuffer pointers (R_constants::flush_cache order:
    # VS, PS, GS, HS, DS, CS), each entry calling dx10ConstantBuffer::Flush. The VS table is the first, 0x70 bytes below
    # the `lea rbx` base.
    body = pe.read(FLUSH, 0x70)
    assert body[10:13] == b'\x48\x8d\x1d', 'flush: lea rbx,[rip+table]'
    base = FLUSH + 10 + 7 + struct.unpack_from('<i', body, 13)[0]
    vertex_table = number('vertex_constant_buffers_rva')
    assert base - 0x70 == vertex_table, (hex(base), hex(vertex_table))
    assert number('constant_buffer_slots') == 14 and b'\xbf\x0e\x00\x00\x00' in body, 'flush: 14 slots'
    loads = [b'\x48\x8b\x4b\x90', b'\x48\x8b\x0b', b'\x48\x8b\x4b\x70', b'\x48\x8b\x8b\xe0\x00\x00\x00',
             b'\x48\x8b\x8b\x50\x01\x00\x00', b'\x48\x8b\x8b\xc0\x01\x00\x00']
    at = 0
    for load in loads:
        at = body.find(load, at)
        assert at >= 0, ('flush table load', load.hex())
        at += len(load)
    assert sorted(callers[CONSTANT_BUFFER_FLUSH]) == [FLUSH + 0x1f, FLUSH + 0x2c, FLUSH + 0x3a, FLUSH + 0x4b, FLUSH + 0x5c, FLUSH + 0x6d], \
        [hex(c) for c in callers[CONSTANT_BUFFER_FLUSH]]
    # dx10ConstantBuffer::Flush: `if (m_bChanged +0x68) { UpdateSubresource(m_pBuffer +0x50, 0, 0, m_pBufferData +0x60, 0, 0) }`;
    # UpdateSubresource is ID3D11DeviceContext vtable slot 48 (+0x180). It copies at call time, so the CPU copy is
    # free again the moment the flush returns.
    flush = pe.read(CONSTANT_BUFFER_FLUSH, 0x50).hex()
    for part in ('80796800', '488b5350', '488b4360', '41ff9280010000', 'c6436800'):
        assert part in flush, ('Flush body', part)
    assert number('constant_buffer_data_offset') == 0x60 and number('constant_buffer_size_offset') == 0x58
    # m_uiBufferSize (+0x58) sits between those two pointers in the source layout (xr_resource_named 0x10, shared_str,
    # type, crc, two vectors = 0x50 m_pBuffer, u32 size, void* data, bool changed); +0x50/+0x60/+0x68 are proven above and
    # the draw-time reader additionally refuses any buffer whose reported size differs from the shader reflection's.

    # CDetailManager::Render -> hw_Render -> hw_Render_dump (called for wave 0, wave 1 and still): the only two flush
    # sites of the dump (full batch, last partial batch) are each followed by DrawIndexed, so every grass draw is inside
    # CDetailManager::Render.
    render_end = pe.function_end(DETAIL_RENDER, 0x400)
    assert HW_RENDER in [target for _, target in pe.calls_in(DETAIL_RENDER, render_end)], 'Render calls hw_Render'
    assert sorted(callers[HW_RENDER]) == [HW_RENDER_CALLER], [hex(c) for c in callers[HW_RENDER]]
    hw_end = pe.function_end(HW_RENDER, 0x600)
    dump_calls = [site for site, target in pe.calls_in(HW_RENDER, hw_end) if target == HW_RENDER_DUMP]
    assert len(dump_calls) == 3, [hex(c) for c in dump_calls]
    assert sorted(callers[HW_RENDER_DUMP]) == sorted(dump_calls), 'hw_Render_dump is only called from hw_Render'
    dump_end = pe.function_end(HW_RENDER_DUMP, 0x2000)
    sites = [site for site in callers[FLUSH] if HW_RENDER_DUMP <= site < dump_end]
    assert len(sites) == 2, [hex(c) for c in sites]
    for site in sites:
        assert 'ff5060' in pe.read(site + 5, 48).hex(), ('DrawIndexed after the dump flush', hex(site))

    # FTreeVisual_PM::Render calls FTreeVisual::Render, then reads `last_lod` (+0x108) and writes it back when the LOD
    # argument is non-negative (pSWI +0x100). FTreeVisual_ST::Render draws the whole mesh (no slide window; the stage
    # reports lod -1 for it).
    pm = pe.read(TREE_PM_RENDER, 0x200).hex()
    assert number('tree_pm_last_lod_offset') == 0x108
    assert '448b8b08010000' in pm and '44898b08010000' in pm and '488b8300010000' in pm, 'PM last_lod'
    assert TREE_BASE_RENDER in [target for _, target in pe.calls_in(TREE_PM_RENDER, TREE_PM_RENDER + 0x30)]
    return {'vs_constant_table': hex(vertex_table), 'flush_table_entries': 6, 'hw_Render_dump': hex(HW_RENDER_DUMP),
            'dump_flush_sites': [hex(s) for s in sites], 'tree_pm_last_lod': '+0x108'}

PROFILE = ROOT / 'games/anomaly/src/geometry_profile.h'


def verify(pe):
    assert PROFILE.exists(), 'missing installed geometry profile'
    text = profile_text(PROFILE)
    sites = re.findall(r'\{"([^"]+)", (0x[0-9a-f]+), signature\("([0-9a-f]{32})"\)\}', text)
    assert len(sites) >= 21, 'missing verified lifecycle, visual, context or layout sites'
    for name, rva, expected in sites:
        assert pe.read(int(rva, 16), 16).hex() == expected, name
    profile = {n: int(r, 16) for n, r, _ in sites}
    wanted = {'CRender': {36: MODEL_CREATE, 37: MODEL_CREATE_CHILD, 38: MODEL_DUPLICATE, 39: MODEL_DELETE},
              'Fvisual': {7: FVISUAL}, 'FProgressive': {7: FPROGRESSIVE},
              'CSkeletonX_ST': {7: SKELETON_ST}, 'CSkeletonX_PM': {7: SKELETON_PM},
              'FTreeVisual_ST': {7: TREE_ST_RENDER}, 'FTreeVisual_PM': {7: TREE_PM_RENDER}}
    wanted['CRender'].update({55: RM_NEAR, 57: RM_NORMAL})   # rmNear, rmNormal
    named = {'CRender': {36: 'CRender::model_Create', 37: 'CRender::model_CreateChild', 38: 'CRender::model_Duplicate',
                         39: 'CRender::model_Delete', 55: 'CRender::rmNear', 57: 'CRender::rmNormal'},
             'Fvisual': {7: 'Fvisual::Render'}, 'FProgressive': {7: 'FProgressive::Render'},
             'CSkeletonX_ST': {7: 'CSkeletonX_ST::Render'}, 'CSkeletonX_PM': {7: 'CSkeletonX_PM::Render'},
             'FTreeVisual_ST': {7: 'FTreeVisual_ST::Render'}, 'FTreeVisual_PM': {7: 'FTreeVisual_PM::Render'}}
    for name, slots in named.items():
        for slot, site in slots.items():
            assert profile[site] == wanted[name][slot], ('profile site is not the RTTI slot', site)
    guards = verify_unique_guards(pe, text, 'geometry', sites)
    verify_detour_prefix(pe, sites[:17])
    for name, slots in wanted.items():
        table = next(t for t in pe.tables(name) if t['this_offset'] == 0)
        for slot, rva in slots.items():
            assert int(table['slots'][slot], 16) == rva, (name, slot)
    listed = re.search(r'hierarchy_vtables = \{([^}]*)\}', text)
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
    assert pe.read(BONE_INSTANCE, 10).hex() == '0fb7c24869c0a8000000', 'bone instance stride'
    assert '4883c040' in pe.read(BONE_TRANSFORM, 20).hex(), 'render transform offset'
    assert profile['CKinematics::LL_GetBoneInstance'] == BONE_INSTANCE and profile['CKinematics::LL_GetTransform_R'] == BONE_TRANSFORM
    assert profile['R_xforms::set_W'] == SET_W and profile['Fvisual::Render phase compare'] == PHASE_COMPARE
    assert profile['CSecondVPParams::IsSVPFrame'] == IS_SVP_FRAME and profile['dx10ConstantBuffer::Flush'] == CONSTANT_BUFFER_FLUSH
    assert profile['CDetailManager::Render'] == DETAIL_RENDER
    context = verify_context(pe, text)
    foliage = verify_foliage(pe, text)
    print(f'PASS: installed {BUILD} build ({EXE.name}, sha256 {HASH}), {len(sites)} expected16 guards, {guards} extended unique '
          f'guards, lifecycle/visual/rmNear/rmNormal RTTI slots, '
          f'{len(found_tables)} hierarchy vtables, world matrix {hex(world)} ({callers[world]} set_W callers), '
          f'bone stride/render-transform offset; draw-time/context: {context}; foliage: {foliage}')

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--class', dest='classes', action='append')
    parser.add_argument('--read', nargs=2)
    parser.add_argument('--rea', nargs=2)
    parser.add_argument('--verify', action='store_true')
    parser.add_argument('--exe', choices=sorted(BUILDS), default='standard', help='which exact executable (default standard)')
    parser.add_argument('--profile', help='verify another copy of geometry_profile.h (negative tests)')
    args = parser.parse_args()
    select(args.exe)
    if args.profile:
        PROFILE = pathlib.Path(args.profile)
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
