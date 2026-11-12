"""SR Stage 0: read-only check of every games/anomaly/src/sr_profile.h guard and layout against the installed exe.

Needs the installed STANDARD AnomalyDX11.exe (D:/Anomaly 1.5.3), so it is not registered in CTest (same as
tests/anomaly_geometry_evidence.py). Nothing is patched or executed; the file is only read.
Run: py -3 -I tests/anomaly_sr_evidence.py --verify
Every check raises AssertionError naming the site or layout that does not match.
"""
import argparse
import importlib.util
import pathlib
import re
import struct

ROOT = pathlib.Path(__file__).resolve().parents[1]
PROFILE = ROOT / 'games/anomaly/src/sr_profile.h'
NATIVE = ROOT / 'games/anomaly/src/native_profile.h'
GEOMETRY = ROOT / 'games/anomaly/src/geometry_profile.h'

_spec = importlib.util.spec_from_file_location('anomaly_geometry_evidence', ROOT / 'tests/anomaly_geometry_evidence.py')
geometry = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(geometry)

CRENDER_RENDER, CRENDER_RENDER_END = 0x8977c0, 0x898640
RENDER_TO_TARGET = 0x898640
CTOR = 0x853770                 # CRenderTarget::CRenderTarget
REF_RT_CREATE = 0x8a15d0        # resptrcode_crt::create(name, w, h, fmt, samples, uav)
CREATE_RT = 0x873f60            # CResourceManager::_CreateRT
CRT_CREATE = 0x8a1250           # CRT::create
BACKEND_ON_FRAME_BEGIN = 0x842fb0
DEVICE_BEGIN = 0x822cb0         # dxRenderDeviceRender::Begin
SET_CACHE_XFORM = 0x823250
SETUP_STATES = 0x8232a0         # dxRenderDeviceRender::SetupStates
PHASE_SCENE_BEGIN = 0x8e3d20
RENDER_MAIN = 0x898a00          # CRender::render_main(Fmatrix&, bool)
RM_NORMAL = 0x83a4d0


def ints(text, name):
    match = re.search(r'\b' + name + r' = (?:1u << (\d+)|(0x[0-9a-f]+|\d+))', text)
    assert match, ('profile constant', name)
    return 1 << int(match.group(1)) if match.group(1) else int(match.group(2), 0)


class Image(geometry.PE):
    def __init__(self):
        super().__init__()
        self.text_va, self.code = self.text()

    def unique_in_text(self, pattern):
        return self.code.count(pattern) == 1

    def rel_target(self, at, length, disp_at):
        """Target of a rel32/rip-relative operand: instruction at `at`, `length` bytes, disp32 at `at + disp_at`."""
        return at + length + struct.unpack('<i', self.read(at + disp_at, 4))[0]

    def call_target(self, at):
        raw = self.read(at, 5)
        assert raw[0] in (0xe8, 0xe9), hex(at)
        return self.rel_target(at, 5, 1)

    def rip_operands(self, start, end):
        """Targets of rip-relative operands of [REX] op modrm(mod=00, rm=101) disp32 for common load/lea/test opcodes."""
        code = self.read(start, end - start)
        found = []
        for i in range(len(code) - 6):
            j = i + 1 if 0x40 <= code[i] <= 0x4f else i
            op = code[j]
            if op in (0x8b, 0x8d, 0x89, 0x3b, 0x39) and (code[j + 1] & 0xc7) == 0x05:
                found.append((start + i, start + j + 6 + struct.unpack_from('<i', code, j + 2)[0]))
            elif op in (0x80, 0xc6) and (code[j + 1] & 0xc7) == 0x05:   # cmp/mov byte [rip+x], imm8
                found.append((start + i, start + j + 7 + struct.unpack_from('<i', code, j + 2)[0]))
            elif op == 0xf7 and code[j + 1] == 0x05:      # test dword [rip+x], imm32
                found.append((start + i, start + j + 10 + struct.unpack_from('<i', code, j + 2)[0]))
            elif code[i] == 0xf3 and i + 9 <= len(code):                   # movss xmm,[rip] / movss [rip],xmm
                k = i + 2 if 0x40 <= code[i + 1] <= 0x4f else i + 1
                if code[k] == 0x0f and code[k + 1] in (0x10, 0x11) and (code[k + 2] & 0xc7) == 0x05:
                    found.append((start + i, start + k + 7 + struct.unpack_from('<i', code, k + 3)[0]))
        # A REX-prefixed match also matches again one byte later without the prefix (or after an F3 prefix); keep the
        # first decode of each target. This is a byte scanner, not a disassembler: callers only test membership/counts.
        unique, last = [], {}
        for at, target in found:
            if last.get(target, -16) >= at - 2:
                continue
            last[target] = at
            unique.append((at, target))
        return unique

    def string_rvas(self, name):
        return [self.rva(at) for at in self.occurrences(name.encode() + b'\0')
                if self.data[at - 1:at] == b'\0']


def parse(text):
    def native(block):
        body = re.search(block + r'\{\{(.*?)\}\};', text, re.S).group(1)
        return [(n, int(r, 16), h) for n, r, h in re.findall(r'\{"([^"]+)", (0x[0-9a-f]+), signature\("([0-9a-f]{32})"\)\}', body)]
    unique = re.search(r'sr_unique_guards\{\{(.*?)\}\};', text, re.S).group(1)
    first = re.search(r'sr_first_person_sites\{\{(.*?)\}\};', text, re.S).group(1)
    slots = re.search(r'target_slots\{\{(.*?)\}\};', text, re.S).group(1)
    return {
        'functions': native('sr_function_sites'),
        'calls': native('sr_call_sites'),
        'unique': [(n, int(r, 16), h) for n, r, h in re.findall(r'\{"([^"]+)", (0x[0-9a-f]+), "([0-9a-f]+)"\}', unique)],
        'first_person': [(n, int(a, 16), int(b, 16), int(c, 16)) for n, a, b, c in
                         re.findall(r'\{"([^"]+)", (0x[0-9a-f]+), (0x[0-9a-f]+), (0x[0-9a-f]+)\}', first)],
        'slots': [(n, int(o, 16), int(f)) for n, o, f in re.findall(r'\{"(\$user\$[^"]+)", (0x[0-9a-f]+), (\d+)\}', slots)],
    }


def verify_guards(pe, p, log):
    declared = {
        'sr_function_sites': int(re.search(r'array<NativeSite, (\d+)> sr_function_sites', TEXT).group(1)),
        'sr_call_sites': int(re.search(r'array<NativeSite, (\d+)> sr_call_sites', TEXT).group(1)),
        'sr_unique_guards': int(re.search(r'array<UniqueGuard, (\d+)> sr_unique_guards', TEXT).group(1)),
        'sr_first_person_sites': int(re.search(r'array<FirstPersonSite, (\d+)> sr_first_person_sites', TEXT).group(1)),
        'target_slots': int(re.search(r'array<TargetSlot, (\d+)> target_slots', TEXT).group(1)),
    }
    parsed = {'sr_function_sites': p['functions'], 'sr_call_sites': p['calls'], 'sr_unique_guards': p['unique'],
              'sr_first_person_sites': p['first_person'], 'target_slots': p['slots']}
    for key, count in declared.items():
        assert len(parsed[key]) == count, ('profile parse', key, len(parsed[key]), count)
    extended = {name: (rva, h) for name, rva, h in p['unique']}
    for name, rva, h in p['unique']:
        raw = bytes.fromhex(h)
        assert pe.read(rva, len(raw)) == raw, ('unique guard bytes', name)
        assert pe.unique_in_text(raw), ('unique guard not unique', name)
        assert len(raw) > 16 and not pe.unique_in_text(raw[:-1]), ('unique guard is not minimal', name, len(raw))
    for kind in ('functions', 'calls'):
        for name, rva, h in p[kind]:
            raw = bytes.fromhex(h)
            assert pe.read(rva, 16) == raw, (kind, 'guard bytes', name, pe.read(rva, 16).hex())
            if name in extended:
                assert extended[name][0] == rva and extended[name][1].startswith(h), ('extended guard mismatch', name)
            else:
                assert pe.unique_in_text(raw), (kind, '16-byte guard not unique in .text', name)
            log.append(f'guard {kind[:-1]:8s} {rva:#08x} {h} {name}')
    return {name: rva for name, rva, _ in p['functions']}


def verify_calls(pe, f, p, log):
    prepare, combine = f['CRenderTarget::phase_scene_prepare'], f['CRenderTarget::phase_combine']
    sunshafts, blur = f['CRenderTarget::phase_sunshafts'], f['CRenderTarget::phase_blur']
    expected_target = {0x897d2c: prepare, 0x897d66: prepare, 0x8985bf: combine, 0x8bf273: sunshafts, 0x8bf27b: blur}
    sites = {rva for _, rva, _ in p['calls']}
    assert sites == set(expected_target), sorted(map(hex, sites))
    for site, target in expected_target.items():
        assert pe.read(site, 1) == b'\xe8' and pe.call_target(site) == target, ('not a real call', hex(site))
    callers = pe.callers(prepare, combine, sunshafts, blur)
    # Sole callers: one function each, no duplicated body (each callee is also exactly one function in .pdata).
    assert sorted(callers[prepare]) == [0x897d2c, 0x897d66], [hex(c) for c in callers[prepare]]
    assert callers[combine] == [0x8985bf] and callers[sunshafts] == [0x8bf273] and callers[blur] == [0x8bf27b]
    assert all(CRENDER_RENDER <= c < CRENDER_RENDER_END for c in callers[prepare] + callers[combine])
    combine_end = pe.function_end(combine, 0x5000)
    combine_end = combine + 0x4583  # .pdata size; function_end stops at the first internal CC pair
    assert combine <= 0x8bf273 < combine_end and combine <= 0x8bf27b < combine_end
    # Both calls take `this` from the same register (mov rcx,r14) and sit in source order: sunshafts is guarded by
    # `!_menu_pp` (test r15b,r15b; jnz) and `ps_sunshafts_mode - 2 <= 1` (R2SS_SCREEN_SPACE, R2SS_COMBINE_SUNSHAFTS);
    # blur is unconditional and immediately follows.
    guard = pe.read(0x8bf255, 0x2a)
    assert guard[:3] == b'\x45\x84\xff' and guard[11] == 0x75, 'menu_pp test before sunshafts'
    assert guard[13:15] == b'\x8b\x05' and pe.rel_target(0x8bf262, 6, 2) == ints(TEXT, 'sunshafts_mode_rva'), 'sunshafts mode'
    assert guard[19:22] == b'\x83\xc0\xfe' and guard[22:25] == b'\x83\xf8\x01' and guard[25] == 0x77
    assert pe.read(0x8bf270, 3) == b'\x49\x8b\xce' and pe.read(0x8bf278, 3) == b'\x49\x8b\xce', 'this = r14'
    # Render passes Target (CRender+0x768) as `this` to both phase_scene_prepare calls and to phase_combine.
    target_offset = ints(TEXT, 'render_target_offset')
    load = b'\x48\x8b\x8e' + struct.pack('<I', target_offset)
    for site in expected_target:
        if CRENDER_RENDER <= site < CRENDER_RENDER_END:
            assert pe.read(site - 7, 7) == load, ('this = RImplementation.Target', hex(site))
    log.append(f'calls: prepare {[hex(c) for c in callers[prepare]]}, combine {hex(callers[combine][0])}, '
               f'sunshafts {hex(callers[sunshafts][0])}, blur {hex(callers[blur][0])}; this = [CRender+{target_offset:#x}]')


def verify_render_modes(pe, f, log):
    table = next(t for t in pe.tables('CRender') if t['this_offset'] == 0)
    assert int(table['vtable'], 16) == ints(TEXT, 'crender_vtable_rva'), table['vtable']
    slots = {ints(TEXT, 'rm_near_slot'): f['CRender::rmNear'], ints(TEXT, 'rm_far_slot'): f['CRender::rmFar'],
             ints(TEXT, 'rm_normal_slot'): RM_NORMAL}
    for slot, rva in slots.items():
        assert int(table['slots'][slot], 16) == rva, ('CRender slot', slot, table['slots'][slot])
    geometry_text = GEOMETRY.read_text()
    assert '{"CRender::rmNear", 0x83a450, signature("40534883ec40488b01ff507848c74424")}' in geometry_text, \
        'rmNear is shared with geometry_profile.h'

    def depth_range(start):
        loads = [t for _, t in pe.rip_operands(start, start + 0x60)]
        values = [struct.unpack('<f', pe.read(t, 4))[0] for t in loads if 0xa4e000 <= t < 0xd00000]
        return values
    assert any(abs(v - 0.02) < 1e-6 for v in depth_range(f['CRender::rmNear'])), 'rmNear MaxDepth 0.02'
    assert any(abs(v - 0.99999) < 1e-6 for v in depth_range(f['CRender::rmFar'])), 'rmFar MinDepth 0.99999'
    log.append(f'CRender vtable {table["vtable"]}: slot55 rmNear, slot56 rmFar, slot57 rmNormal; depth 0.02 / 0.99999')


def verify_xforms(pe, f, log):
    set_p, set_v = f['R_xforms::set_P'], f['R_xforms::set_V']
    xforms = ints(TEXT, 'backend_rva') + ints(TEXT, 'backend_xforms_offset')
    assert xforms == ints(GEOMETRY.read_text(), 'world_matrix_rva'), 'RCache.xforms == geometry world matrix owner'
    # set_P copies [rdx] to this+0xC0 (m_p); set_V to this+0x80 (m_v). `mov eax,[rdx]; lea rdi,[rcx+disp32]; mov [rdi],eax`.
    p_off, v_off = ints(TEXT, 'xforms_project_offset'), ints(TEXT, 'xforms_view_offset')
    assert ('8b02488db9' + struct.pack('<I', p_off).hex() + '8907') in pe.read(set_p, 40).hex(), 'set_P -> m_p'
    assert ('8b02488db9' + struct.pack('<I', v_off).hex() + '8907') in pe.read(set_v, 40).hex() or \
        ('8b02' in pe.read(set_v, 40).hex() and struct.pack('<I', v_off).hex() in pe.read(set_v, 40).hex()), 'set_V -> m_v'
    # SetCacheXform(view, project): set_V(xforms, view) then tail-jump set_P(xforms, project) (dxRenderDeviceRender.cpp:411).
    body = pe.read(SET_CACHE_XFORM, 0x29)
    calls = [(SET_CACHE_XFORM + i, pe.call_target(SET_CACHE_XFORM + i)) for i in range(len(body) - 4)
             if body[i] in (0xe8, 0xe9) and pe.call_target(SET_CACHE_XFORM + i) in (set_p, set_v)]
    assert [t for _, t in calls] == [set_v, set_p], [(hex(a), hex(t)) for a, t in calls]
    assert all(t == xforms for _, t in pe.rip_operands(SET_CACHE_XFORM, SET_CACHE_XFORM + 0x29)
               if t not in (set_p, set_v)), 'SetCacheXform lea rcx,xforms'
    # CRender::Render epilogue: set_P(&Device.mProject), set_V(&Device.mView) (r4_R_render.cpp:506-507 analogue at exit);
    # render_main(Device.mFullTransform) (r4_R_render.cpp:320).
    project, view, full = ints(TEXT, 'device_project_rva'), ints(TEXT, 'device_view_rva'), ints(TEXT, 'device_full_transform_rva')
    def argument_before(call, register_lea=b'\x48\x8d\x15'):
        window = pe.read(call - 24, 24)
        at = window.rfind(register_lea)
        return pe.rel_target(call - 24 + at, 7, 3) if at >= 0 else None   # None: stack matrix (z-prepass m_zfill)
    render_calls = [(a, t) for a, t in pe.calls_in(CRENDER_RENDER, CRENDER_RENDER_END)]
    p_args = {argument_before(a) for a, t in render_calls if t == set_p}
    v_args = {argument_before(a) for a, t in render_calls if t == set_v}
    main_args = {argument_before(a) for a, t in render_calls if t == RENDER_MAIN}
    assert p_args == {project} and v_args == {view} and full in main_args, (p_args, v_args, main_args)
    # First-person sites write Device.mFullTransform = mProject * mView before uploading (accum_point, particles).
    for start in (f['CRenderTarget::accum_point'],):
        stores = {t for _, t in pe.rip_operands(start, start + 0x600)}
        assert any(full <= t < full + 64 for t in stores), 'mFullTransform write'
    log.append(f'set_P -> this+{p_off:#x}, set_V -> this+{v_off:#x}, xforms {xforms:#x}; Device.mView {view:#x}, '
               f'mProject {project:#x}, mFullTransform {full:#x}')
    return set_p


def verify_first_person(pe, f, p, set_p, log):
    rm_near = f['CRender::rmNear']
    hud = 0x84ffa0
    hud_code = pe.read(hud, 0x200)
    first = next(i for i in range(len(hud_code) - 9) if hud_code[i:i + 4] == b'\xf3\x0f\x10\x05')
    fov = hud + first + 8 + struct.unpack_from('<i', hud_code, first + 4)[0]
    project = ints(TEXT, 'device_project_rva')
    listed = set()
    for name, start, set_p_call, near_call in p['first_person']:
        assert pe.call_target(set_p_call) == set_p, ('set_P call', name)
        # Device.mProject.build_projection(...) is stored rip-relative before the upload, and rdx = &Device.mProject.
        # Inlined build_projection stores the non-constant elements with movss [rip],xmm; the particle path calls it
        # out of line with rcx = &Device.mProject.
        before = pe.rip_operands(max(start, set_p_call - 0x600), set_p_call)
        def movss_store(a):
            raw = pe.read(a, 4)
            return raw[0] == 0xf3 and (raw[1:3] == b'\x0f\x11' or (0x40 <= raw[1] <= 0x4f and raw[2:4] == b'\x0f\x11'))
        stores = sum(project <= t < project + 64 and movss_store(a) for a, t in before)
        this_lea = any(t == project and pe.read(a, 3) == b'\x48\x8d\x0d' for a, t in before)
        assert stores >= 3 or this_lea, ('rebuilds Device.mProject', name, stores)
        assert any(t == project and pe.read(a, 3) == b'\x48\x8d\x15' for a, t in before), ('set_P(&Device.mProject)', name)
        raw = pe.read(near_call, 6)
        assert raw == b'\xff\x90\xb8\x01\x00\x00' or (raw[0] == 0xe8 and pe.call_target(near_call) == rm_near), ('rmNear', name)
        assert 0 < near_call - set_p_call < 0x120, (name, hex(near_call - set_p_call))
        end = pe.function_end(start, 0x3000)
        assert start < set_p_call < near_call < end or name.startswith('CRenderTarget::accum_point'), name
        assert fov in {t for _, t in pe.rip_operands(start, start + 0x700)} or \
            fov in {t for _, t in pe.rip_operands(set_p_call - 0x400, set_p_call)}, ('psHUD_FOV read', name)
        listed.add(start)
    # Completeness: every set_P call in .text that is followed within 0x120 bytes by a direct rmNear call or a slot-55
    # virtual call belongs to one of the listed functions.
    pdata = pe.sections[3]
    def owner(rva):
        """Primary .pdata function for rva, following UNW_FLAG_CHAININFO back to the parent entry."""
        raw, size = pdata[2], pdata[3]
        for at in range(raw, raw + size - 12, 12):
            b, e, unwind = struct.unpack_from('<III', pe.data, at)
            if b <= rva < e:
                while True:
                    info = pe.read(unwind, 4)
                    if not (info[0] >> 3) & 0x4:
                        return b
                    codes = (info[2] + 1) & ~1
                    b, e, unwind = struct.unpack('<III', pe.read(unwind + 4 + 2 * codes, 12))
        return None
    found = set()
    for call in pe.callers(set_p)[set_p]:
        tail = pe.read(call + 5, 0x120)
        direct = any(tail[i] == 0xe8 and call + 5 + i + 5 + struct.unpack_from('<i', tail, i + 1)[0] == rm_near
                     for i in range(len(tail) - 5))
        if direct or b'\xff\x90\xb8\x01\x00\x00' in tail:
            found.add(owner(call))
    assert found == listed, (sorted(map(hex, found)), sorted(map(hex, listed)))
    direct = pe.callers(rm_near)[rm_near]
    assert {owner(c) for c in direct} == {f['PS::CParticleEffect::Render'], f['CRenderTarget::accum_point']}, \
        [hex(c) for c in direct]
    particle = next(t for t in pe.tables('CParticleEffect@PS') if t['this_offset'] == 0)
    assert int(particle['slots'][7], 16) == f['PS::CParticleEffect::Render'], 'CParticleEffect::Render vtable slot 7'
    log.append(f'first-person: {len(listed)} set_P+rmNear sites (psHUD_FOV {fov:#x}); direct rmNear callers '
               f'{[hex(c) for c in direct]}; no other set_P is followed by rmNear')


def verify_device_and_hw(pe, f, log):
    native = NATIVE.read_text()
    width, height = ints(native, 'device_width_rva'), ints(native, 'device_height_rva')
    base_rt, base_zb = ints(TEXT, 'hw_base_rt_rva'), ints(TEXT, 'hw_base_zb_rva')
    device, context, chain = ints(TEXT, 'hw_device_rva'), ints(TEXT, 'hw_context_rva'), ints(TEXT, 'hw_swap_chain_rva')
    # CRender::Render early-out: Target->u_setrt(Device.dwWidth, Device.dwHeight, HW.pBaseRT, 0, 0, HW.pBaseZB).
    u_setrt = f['CRenderTarget::u_setrt(W,H)']
    tail_call = next(a for a, t in pe.calls_in(CRENDER_RENDER_END - 0x80, CRENDER_RENDER_END) if t == u_setrt)
    reads = {t for _, t in pe.rip_operands(tail_call - 0x40, tail_call)}
    assert {width, height, base_rt, base_zb} <= reads, sorted(map(hex, reads))
    # u_setrt stores W/H into CRenderTarget+8/+0xC and compares against the RCache RT/ZB caches.
    w_off, h_off = ints(TEXT, 'target_width_offset'), ints(TEXT, 'target_height_offset')
    head = pe.read(u_setrt, 16)
    assert head[4:7] == bytes([0x89, 0x51, w_off]) and head[7:11] == bytes([0x44, 0x89, 0x41, h_off]), head.hex()
    backend = ints(TEXT, 'backend_rva')
    cache = {t for _, t in pe.rip_operands(u_setrt, u_setrt + 0x110)}
    for key in ('backend_rt_offset', 'backend_zb_offset', 'backend_rt_changed_offset'):
        assert backend + ints(TEXT, key) in cache, key
    assert context in cache, 'HW.pContext'
    # phase_scene_prepare: msaa test, rt_Position->pRT, Device dims, HW.pBaseZB.
    prepare = f['CRenderTarget::phase_scene_prepare']
    reads = {t for _, t in pe.rip_operands(prepare, prepare + 0x40)}
    assert {width, height, base_zb, ints(TEXT, 'render_options_word3_rva')} <= reads, sorted(map(hex, reads))
    body = pe.read(prepare, 0x40)
    assert body[6:8] == b'\xf7\x05' and struct.unpack_from('<I', body, 12)[0] == ints(TEXT, 'dx10_msaa_bit'), 'o.dx10_msaa'
    assert b'\x4c\x8b\x89' + struct.pack('<I', 0x378) in body and b'\x4d\x8b\x49\x18' in body, 'rt_Position->pRT'
    assert b'\x48\x8b\x81' + struct.pack('<I', 0x358) in body, 'rt_MSAADepth (MSAA branch)'
    # HW.pDevice in CRT::create; HW.pContext/pSwapChain in RenderToTarget (GetBuffer + CopyResource).
    assert device in {t for _, t in pe.rip_operands(CRT_CREATE, CRT_CREATE + 0x375)}, 'HW.pDevice'
    reads = {t for _, t in pe.rip_operands(RENDER_TO_TARGET, RENDER_TO_TARGET + 0x200)}
    assert {context, chain} <= reads, sorted(map(hex, reads))
    # RImplementation: CBackend::OnFrameBegin calls rmNormal(&RImplementation); Render reads Target at +0x768.
    impl = ints(TEXT, 'render_implementation_rva')
    call = next(a for a, t in pe.calls_in(BACKEND_ON_FRAME_BEGIN, BACKEND_ON_FRAME_BEGIN + 0x40) if t == RM_NORMAL)
    assert pe.read(call - 7, 3) == b'\x48\x8d\x0d' and pe.rel_target(call - 7, 7, 3) == impl, 'RImplementation'
    assert impl + 0x150 == ints(GEOMETRY.read_text(), 'render_phase_rva'), 'RImplementation.phase'
    log.append(f'Device {width:#x}/{height:#x}; HW pDevice {device:#x} pContext {context:#x} swapchain {chain:#x} '
               f'pBaseRT {base_rt:#x} pBaseZB {base_zb:#x}; RImplementation {impl:#x}')


def verify_options(pe, log):
    word0, word3 = ints(TEXT, 'render_options_word0_rva'), ints(TEXT, 'render_options_word3_rva')
    impl = ints(TEXT, 'render_implementation_rva')
    assert word3 == word0 + 12 and word0 - impl == 0x460, 'CRender::o placement'
    ctor = pe.read(CTOR, 0x2c5d)
    def bt_after_load(word, bit):
        """`mov eax,[rip+word]` followed within 80 bytes by `bt eax,bit` (0F BA E0 ib)."""
        for i in range(len(ctor) - 6):
            if ctor[i:i + 2] == b'\x8b\x05' and CTOR + i + 6 + struct.unpack_from('<i', ctor, i + 2)[0] == word:
                if bytes([0x0f, 0xba, 0xe0, bit]) in ctor[i + 6:i + 86]:
                    return True
        return False
    assert bt_after_load(word0, 26) and bt_after_load(word0, 28), 'mrtmixdepth/fp16_blend (albedo/accum branch)'
    assert bt_after_load(word3, 14), 'dx10_msaa (generic0_r/generic1_r branch)'
    assert b'\x0f\xba\xe0\x0c' in ctor, 'advancedpp (generic2 branch)'
    assert ints(TEXT, 'mrtmixdepth_bit') == 1 << 26 and ints(TEXT, 'dx10_msaa_bit') == 1 << 14
    log.append(f'CRender::o words {word0:#x}/{word3:#x}: mrtmixdepth bit26, fp16_blend bit28, advancedpp bit12, dx10_msaa bit14')


def verify_targets(pe, p, log):
    ctor = pe.read(CTOR, 0x2c5d)
    create_calls = [a - CTOR for a, t in pe.calls_in(CTOR, CTOR + 0x2c5d) if t == REF_RT_CREATE]
    found = {}
    for name, offset, fmt in p['slots']:
        hits = []
        for string in pe.string_rvas(name):
            for i in range(len(ctor) - 7):
                if ctor[i:i + 3] == b'\x48\x8d\x15' and CTOR + i + 7 + struct.unpack_from('<i', ctor, i + 3)[0] == string:
                    hits.append(i)
        assert hits, ('no lea rdx,name in constructor', name)
        for i in hits:
            nxt = min(c for c in create_calls if c > i)
            window = ctor[i:nxt]
            lea = window.find(b'\x49\x8d\x8f')                # lea rcx,[r15+disp32]
            if lea >= 0:
                slot = struct.unpack_from('<I', window, lea + 3)[0]
            else:                                              # accum: mov rcx,rbx with rbx = lea [r15+disp32]
                mov = window.find(b'\x48\x8b\xcb')
                assert mov >= 0, name
                rbx = ctor.rfind(b'\x49\x8d\x9f', 0, i)
                slot = struct.unpack_from('<I', ctor, rbx + 3)[0]
                if mov + 4 < len(window) and window[mov + 3] == 0xeb:   # jmp short to the shared create call
                    to = i + mov + 5 + struct.unpack_from('<b', window, mov + 4)[0]
                    window = ctor[to:min(c for c in create_calls if c > to)]
            assert slot == offset, (name, hex(slot), hex(offset))
            store = window.find(b'\xc7\x44\x24\x20')            # mov dword [rsp+20h], fmt
            if fmt and store >= 0:
                assert struct.unpack_from('<I', window, store + 4)[0] == fmt, (name, 'format')
            elif fmt and name in ('$user$position', '$user$generic0', '$user$generic1', '$user$generic_temp', '$user$generic2'):
                raise AssertionError((name, 'format store not found'))
        found[name] = offset
    assert len(set(found.values())) == len(found), 'duplicate slot offsets'
    # The two first surface_set calls in phase_combine re-point t_LUM_src/t_LUM_dest (phase_combine.cpp:57-58).
    combine = 0x8bc980
    calls = [a for a, t in pe.calls_in(combine, combine + 0x200) if t == 0x87ac30][:2]
    for call, key in zip(calls, ('target_lum_src_texture_offset', 'target_lum_dest_texture_offset')):
        assert struct.pack('<I', ints(TEXT, key)) in pe.read(call - 30, 30), key
    log.append(f'CRenderTarget: {len(found)} ref_rt slots re-derived from constructor create calls; t_LUM src/dest')


def verify_crt_texture(pe, log):
    create_rt = pe.read(CREATE_RT, 0x200)
    # xr_new<CRT>: `mov edx, 50h` into the allocator, then the CRT constructor (0x8a1140) on the result.
    alloc = create_rt.find(b'\xba' + struct.pack('<I', ints(TEXT, 'crt_size')))
    ctor_call = [a - CREATE_RT for a, t in pe.calls_in(CREATE_RT, CREATE_RT + 0x200) if t == 0x8a1140]
    assert alloc >= 0 and ctor_call and 0 < ctor_call[0] - alloc < 0x30, 'CRT allocation size'
    assert CRT_CREATE in [t for _, t in pe.calls_in(CREATE_RT, CREATE_RT + 0x200)], '_CreateRT -> CRT::create'
    body = pe.read(CRT_CREATE, 0x375).hex()
    o = {k: ints(TEXT, k) for k in ('crt_surface_offset', 'crt_rtv_offset', 'crt_dsv_offset', 'crt_uav_offset',
                                    'crt_texture_offset', 'crt_width_offset', 'crt_height_offset', 'crt_format_offset',
                                    'crt_order_offset')}
    patterns = {'crt_surface_offset': f'4c8d71{o["crt_surface_offset"]:02x}',   # lea r14,[rcx+10h] (CreateTexture2D out)
                'crt_rtv_offset': f'4c8d4f{o["crt_rtv_offset"]:02x}',           # lea r9,[rdi+18h] CreateRenderTargetView
                'crt_dsv_offset': f'4c8d4f{o["crt_dsv_offset"]:02x}',           # lea r9,[rdi+20h] CreateDepthStencilView
                'crt_uav_offset': f'4c8d4f{o["crt_uav_offset"]:02x}',           # lea r9,[rdi+28h] CreateUnorderedAccessView
                'crt_texture_offset': f'488d77{o["crt_texture_offset"]:02x}',   # lea rsi,[rdi+30h] pTexture
                'crt_width_offset': f'8977{o["crt_width_offset"]:02x}',         # mov [rdi+38h],esi
                'crt_height_offset': f'895f{o["crt_height_offset"]:02x}',       # mov [rdi+3Ch],ebx
                'crt_format_offset': f'894f{o["crt_format_offset"]:02x}',       # mov [rdi+40h],ecx
                'crt_order_offset': f'488947{o["crt_order_offset"]:02x}'}       # mov [rdi+48h],rax (rdtsc)
    for key, pattern in patterns.items():
        assert pattern in body, key
    # D24S8 (75) -> DXGI 44 R24G8_TYPELESS texture, 45 D24_UNORM_S8_UINT view (dx10SH_RT.cpp:79-82,121).
    # cmp ecx,4Bh; jz -> mov ebx,2 (depth); lea r13d,[rbx+2Ah] = 44 texture format; view format 45 when it is 44.
    raw = pe.read(CRT_CREATE, 0x375)
    at = raw.find(bytes.fromhex('83f94b74'))
    to = at + 5 + raw[at + 4]
    assert at >= 0 and raw[to:to + 9].hex() == 'bb02000000448d6b2a', 'D24S8 -> R24G8_TYPELESS'
    assert re.search(r'83f82c75..c745..2d000000', body), 'R24G8_TYPELESS -> D24_UNORM_S8_UINT view'
    # mov rdx,[r14] (pSurface); mov rcx,[rsi] (pTexture); call CTexture::surface_set.
    assert pe.read(0x8a15a3, 6).hex() == '498b16488b0e' and pe.call_target(0x8a15a9) == 0x87ac30, \
        'CRT::create -> pTexture->surface_set(pSurface)'
    s = pe.read(0x87ac30, 0x90).hex()
    t = {k: ints(TEXT, k) for k in ('texture_surface_offset', 'texture_desc_cache_offset', 'texture_desc_offset', 'texture_srv_offset')}
    assert f'48897b{t["texture_surface_offset"]:02x}' in s, 'CTexture::pSurface'
    assert '488b8b' + struct.pack('<I', t['texture_srv_offset']).hex() in s, 'CTexture::m_pSRView'
    assert f'48897b{t["texture_desc_cache_offset"]:02x}' in s, 'CTexture::desc_cache'
    assert f'488d53{t["texture_desc_offset"]:02x}' in s, 'CTexture::desc (GetDesc out)'
    log.append('CRT 0x50: surface 0x10 rtv 0x18 dsv 0x20 uav 0x28 texture 0x30 w 0x38 h 0x3c fmt 0x40 order 0x48; '
               'CTexture surface 0x50 desc_cache 0x70 desc 0x78 srv 0xa8')


def verify_backend(pe, f, log):
    backend = ints(TEXT, 'backend_rva')
    begin = pe.read(DEVICE_BEGIN, 21)       # push rbx; sub rsp,20h; mov rbx,rcx; lea rcx,[RCache]; call OnFrameBegin
    assert begin[9:12] == b'\x48\x8d\x0d' and pe.rel_target(DEVICE_BEGIN + 9, 7, 3) == backend, 'Begin: RCache.OnFrameBegin'
    assert pe.call_target(DEVICE_BEGIN + 16) == BACKEND_ON_FRAME_BEGIN
    invalidate = f['CBackend::Invalidate']
    assert pe.calls_in(BACKEND_ON_FRAME_BEGIN, BACKEND_ON_FRAME_BEGIN + 0x30)[0][1] == invalidate, 'OnFrameBegin -> Invalidate'
    body = pe.read(invalidate, pe.function_end(invalidate, 0x400) - invalidate)
    for key in ('backend_rt_offset', 'backend_zb_offset', 'backend_ctable_offset', 'backend_texture_list_offset',
                'backend_textures_ps_offset', 'backend_textures_vs_offset', 'backend_textures_gs_offset',
                'backend_textures_hs_offset', 'backend_textures_ds_offset', 'backend_textures_cs_offset',
                'backend_rt_changed_offset'):
        assert struct.pack('<I', ints(TEXT, key)) in body, key
    # textures_gs/hs/ds/cs/ps: lea rdi,[rbx+disp32]; mov ecx,16; rep stosq. textures_vs: four qword stores.
    for key in ('backend_textures_gs_offset', 'backend_textures_hs_offset', 'backend_textures_ds_offset',
                'backend_textures_cs_offset', 'backend_textures_ps_offset'):
        assert (b'\x48\x8d\xbb' + struct.pack('<I', ints(TEXT, key)) + bytes.fromhex('b910000000f348ab')) in body or \
            (b'\x48\x8d\xbb' + struct.pack('<I', ints(TEXT, key)) + bytes.fromhex('33c0b910000000f348ab')) in body, key
    vs = ints(TEXT, 'backend_textures_vs_offset')
    for k in range(ints(TEXT, 'backend_textures_vs_count')):
        assert b'\x48\x89\xab' + struct.pack('<I', vs + 8 * k) in body, ('textures_vs', k)
    log.append(f'RCache {backend:#x}: pRT +0x578 pZB +0x598 ctable +0x5c8 T +0x640 textures_ps +0x658[16] vs +0x6d8[4] '
               f'gs/hs/ds/cs +0x6f8/+0x778/+0x7f8/+0x878[16]; Invalidate {invalidate:#x}')


def verify_samplers(pe, f, log):
    cache, bias, aniso = ints(TEXT, 'sampler_cache_rva'), ints(TEXT, 'user_mip_bias_rva'), ints(TEXT, 'user_anisotropy_rva')
    set_bias, set_aniso = f['dx10SamplerStateCache::SetMipLODBias'], f['dx10SamplerStateCache::SetMaxAnisotropy']
    body = pe.read(SETUP_STATES, 0x3a)
    reads = [t for _, t in pe.rip_operands(SETUP_STATES, SETUP_STATES + 0x3a)]
    assert reads.count(cache) == 2 and aniso in reads and bias in reads, [hex(r) for r in reads]
    targets = [pe.call_target(SETUP_STATES + i) for i in range(len(body) - 4) if body[i] in (0xe8, 0xe9)
               and pe.call_target(SETUP_STATES + i) in (set_bias, set_aniso)]
    assert targets == [set_aniso, set_bias], [hex(t) for t in targets]
    assert pe.read(SETUP_STATES + 0x22, 4) == b'\xf3\x0f\x10\x0d', 'bias passed in xmm1'
    field = ints(TEXT, 'sampler_mip_lod_bias_offset')
    assert pe.read(set_bias + 9, 8) == b'\xf3\x0f\x10\x81' + struct.pack('<I', field), 'm_uiMipLODBias'
    scene_begin = [t for _, t in pe.rip_operands(PHASE_SCENE_BEGIN, PHASE_SCENE_BEGIN + 0x20)]
    assert cache in scene_begin and aniso in scene_begin and \
        pe.calls_in(PHASE_SCENE_BEGIN, PHASE_SCENE_BEGIN + 0x20)[0][1] == set_aniso, 'phase_scene_begin SetMaxAnisotropy'
    # All three references are tail jumps (E9): SetupStates and the two r__tf_mipbias console handlers.
    jumps = [pe.text_va + i for i in range(len(pe.code) - 5) if pe.code[i] == 0xe9 and
             pe.text_va + i + 5 + struct.unpack_from('<i', pe.code, i + 1)[0] == set_bias]
    calls = pe.callers(set_bias)[set_bias]
    assert SETUP_STATES + 0x35 in jumps and len(jumps) + len(calls) == 3, ([hex(j) for j in jumps], [hex(c) for c in calls])
    log.append(f'SSManager {cache:#x}: SetMipLODBias {set_bias:#x} (xmm1, field +{field:#x}; references '
               f'{[hex(j) for j in jumps + calls]}), ps_r__tf_Mipbias {bias:#x}, SetMaxAnisotropy {set_aniso:#x}')


def verify_stage2(pe, p, log):
    """SR Stage 2 data: the world-target table, the absent targets, the SSAO helper's static viewport and its callers,
    the half-size rule of rt_half_depth, and the SSAO dispatch in phase_combine."""
    slots = {name: (offset, fmt) for name, offset, fmt in p['slots']}
    body = re.search(r'sr_world_targets\{\{(.*?)\}\};', TEXT, re.S).group(1)
    world = [(n, int(o, 16), int(f), a == 'true', h == 'true') for n, o, f, a, h in
             re.findall(r'\{"(\$user\$[^"]+)", (0x[0-9a-f]+), (\d+), (true|false), (true|false)\}', body)]
    declared = int(re.search(r'array<WorldTarget, (\d+)> sr_world_targets', TEXT).group(1))
    assert len(world) == declared == 9, ('sr_world_targets parse', len(world), declared)
    order = ['$user$generic0', '$user$generic_temp', '$user$position', '$user$albedo', '$user$accum', '$user$generic1',
             '$user$generic2', '$user$half_depth', '$user$ssao_temp']
    assert [w[0] for w in world] == order, 'sr_world_targets order must match sr::Slot'
    for name, offset, fmt, optional, half in world:
        assert slots.get(name) == (offset, fmt), ('world target differs from target_slots', name)
        assert optional == (name in ('$user$generic2', '$user$half_depth', '$user$ssao_temp')), ('optional', name)
        assert half == (name == '$user$half_depth'), ('half_allowed', name)
    absent = [int(x, 16) for x in re.findall(r'0x[0-9a-f]+', re.search(r'sr_world_absent_offsets\{(.*?)\};', TEXT).group(1))]
    by_offset = {offset: name for name, (offset, _) in slots.items()}
    assert [by_offset.get(o) for o in absent] == ['$user$msaadepth', '$user$generic0_r', '$user$generic1_r', '$user$accum_temp'], absent
    assert set(absent).isdisjoint({w[1] for w in world})
    # rt_half_depth (r4_rendertarget.cpp:705-720): `test al,4` (ssao_opt_data), Device.dwWidth/dwHeight into r8d/r9d,
    # `test al,8` (ssao_half_data) then `shr r8d,1; shr r9d,1`, R16F (0x6f) or R32F on NVIDIA, slot +0x590.
    word0 = ints(TEXT, 'render_options_word0_rva')
    width, height = ints(NATIVE.read_text(), 'device_width_rva'), ints(NATIVE.read_text(), 'device_height_rva')
    at = 0x855433
    code = pe.read(at, 0x55)
    assert code[:2] == b'\x8b\x05' and pe.rel_target(at, 6, 2) == word0, 'half_depth: o word0 load'
    assert code[6:10] == b'\xa8\x04\x74\x78', 'half_depth: test o.ssao_opt_data'
    assert code[10:13] == b'\x44\x8b\x05' and pe.rel_target(at + 10, 7, 3) == width, 'half_depth: Device.dwWidth'
    assert code[17:20] == b'\x44\x8b\x0d' and pe.rel_target(at + 17, 7, 3) == height, 'half_depth: Device.dwHeight'
    assert code[24:34] == bytes.fromhex('a808740641d1e841d1e9'), 'half_depth: o.ssao_half_data -> w/2, h/2'
    assert code[44:47] == b'\x48\x8d\x15' and pe.rel_target(at + 44, 7, 3) in pe.string_rvas('$user$half_depth'), 'half_depth name'
    assert code[51:56] == bytes.fromhex('b86f000000') and code[69:76] == b'\x49\x8d\x8f' + struct.pack('<I', 0x590), 'half_depth R16F, +0x590'
    bits = {k: ints(TEXT, k) for k in ('ssao_blur_on_bit', 'ssao_opt_data_bit', 'ssao_half_data_bit', 'ssao_hdao_bit', 'ssao_ultra_bit')}
    assert bits['ssao_opt_data_bit'] == 4 and bits['ssao_half_data_bit'] == 8
    # phase_combine (phase_combine.cpp:61-77): hdao (0x20 && 0x40), else opt_data (4) -> phase_downsamp, else blur_on (2)
    # -> phase_ssao.
    at = 0x8bca75
    code = pe.read(at, 0x38)
    assert code[:2] == b'\x8b\x05' and pe.rel_target(at, 6, 2) == word0, 'phase_combine: o word0 load'
    assert code[6:8] == bytes([0xa8, bits['ssao_hdao_bit']]) and code[10:12] == bytes([0xa8, bits['ssao_ultra_bit']])
    hdao, downsamp, ssao = pe.call_target(0x8bca8e), pe.call_target(0x8bca9c), pe.call_target(0x8bcaaa)
    assert code[0x20:0x22] == bytes([0xa8, bits['ssao_opt_data_bit']]) and code[0x2e:0x30] == bytes([0xa8, bits['ssao_blur_on_bit']])
    assert (hdao, downsamp, ssao) == (0x8f9690, 0x8f8400, 0x8f89d0), [hex(x) for x in (hdao, downsamp, ssao)]
    callers = pe.callers(hdao, downsamp, ssao)
    assert callers[hdao] == [0x8bca8e] and callers[downsamp] == [0x8bca9c] and callers[ssao] == [0x8bcaaa], callers
    # set_viewport: the code bytes the adapter checks at install, both data addresses from their displacements.
    helper = ints(TEXT, 'ssao_set_viewport_rva')
    expected = bytes.fromhex(re.search(r'ssao_set_viewport_code =\s*"([0-9a-f]+)"', TEXT).group(1))
    assert len(expected) == 0x46 and pe.read(helper, len(expected)) == expected, 'set_viewport code'
    viewport, guard = ints(TEXT, 'ssao_viewport_rva'), ints(TEXT, 'ssao_viewport_guard_rva')
    assert expected[0x34:0x36] == b'\x39\x05' and pe.rel_target(helper + 0x34, 6, 2) == guard, 'cmp [guard], tls epoch'
    assert expected[0x3a] == 0x7f, 'jg: the initialisation path'
    assert expected[0x3f:0x42] == b'\x4c\x8d\x05' and pe.rel_target(helper + 0x3f, 7, 3) == viewport, 'lea r8,[viewport]'
    tail = pe.read(helper + 0x46, 0x42)
    assert tail[:2] == b'\xba\x01' and bytes.fromhex('ffa060010000') in tail, 'RSSetViewports(1, &viewport) via vtable +0x160'
    # Initialisation path: only Width (+8) and Height (+0xC) are stored (movss [rip+x], xmm7 / xmm6).
    assert pe.read(helper + 0x80, 4) == b'\xf3\x0f\x11\x3d' and pe.rel_target(helper + 0x80, 8, 4) == viewport + ints(TEXT, 'ssao_viewport_width_offset')
    assert pe.read(helper + 0x88, 4) == b'\xf3\x0f\x11\x35' and pe.rel_target(helper + 0x88, 8, 4) == viewport + ints(TEXT, 'ssao_viewport_height_offset')
    # .data initialiser {0, 0, w, h, 0, 1.0f}; the guard lies past the section's raw data (zero-initialised .bss).
    assert pe.read(viewport, 24) == struct.pack('<6f', 0, 0, 0, 0, 0, 1.0), pe.read(viewport, 24).hex()
    section = next(s for s in pe.sections if s[0] <= guard < s[0] + s[1])
    assert guard >= section[0] + section[3] and viewport < section[0] + section[3], 'guard in .bss, viewport in .data'
    # Every reference to either address is inside set_viewport, and set_viewport has exactly the four stock callers.
    refs = [(a, t) for a, t in pe.rip_operands(pe.text_va, pe.text_va + len(pe.code)) if t in (viewport, guard)]
    assert refs and all(helper <= a < helper + 0x98 for a, _ in refs), [hex(a) for a, _ in refs]
    calls = pe.callers(helper)[helper]
    assert sorted(calls) == [0x8f84ff, 0x8f89a7, 0x8f8b90, 0x8f957f], [hex(c) for c in calls]
    assert all(downsamp <= c < helper for c in calls), 'callers inside phase_downsamp/phase_ssao'
    log.append(f'Stage 2: {len(world)} world targets (= target_slots), absent {[hex(o) for o in absent]}; rt_half_depth halves with '
               f'ssao_half_data; SSAO dispatch hdao {hdao:#x} downsamp {downsamp:#x} ssao {ssao:#x}; set_viewport {helper:#x} '
               f'static viewport {viewport:#x} (.data), guard {guard:#x} (.bss), callers {[hex(c) for c in sorted(calls)]}')


def verify_stage3(pe, f, p, log):
    """SR Stage 3 data: the adapter writes Device.mProject/mFullTransform and reads mView as three adjacent Fmatrix
    globals, calls R_xforms::set_P (the extended guard) and SetMipLODBias (xmm1), and at install checks every
    first-person site for `call set_P` then `call [rax+0x1b8]` (slot 55) or a direct `call rmNear`."""
    view, project, full = ints(TEXT, 'device_view_rva'), ints(TEXT, 'device_project_rva'), ints(TEXT, 'device_full_transform_rva')
    assert project == view + 64 and full == project + 64, 'Device.mView, mProject, mFullTransform are adjacent Fmatrix'
    assert ints(TEXT, 'rm_near_slot') * 8 == 0x1b8, 'slot 55 is call [rax+0x1b8]'
    set_p, rm_near, rm_far = f['R_xforms::set_P'], f['CRender::rmNear'], f['CRender::rmFar']
    unique = {name: (rva, h) for name, rva, h in p['unique']}
    assert unique['R_xforms::set_P'][0] == set_p and pe.read(set_p, len(bytes.fromhex(unique['R_xforms::set_P'][1]))).hex() == \
        unique['R_xforms::set_P'][1], 'set_P extended guard'
    forms = {'slot55': 0, 'direct': 0}
    for name, start, set_p_call, near_call in p['first_person']:
        assert pe.read(set_p_call, 1) == b'\xe8' and pe.call_target(set_p_call) == set_p, ('set_P call', name)
        raw = pe.read(near_call, 6)
        if raw == b'\xff\x90\xb8\x01\x00\x00':
            forms['slot55'] += 1
        else:
            assert raw[0] == 0xe8 and pe.call_target(near_call) == rm_near != rm_far, ('rmNear call', name)
            forms['direct'] += 1
        assert 0 < near_call - set_p_call < 0x120, name
    assert forms == {'slot55': 4, 'direct': 2}, forms
    # The bias parameter really is xmm1: SetMipLODBias compares it with the cached field first (`movss xmm0,[rcx+field]`
    # then ucomiss xmm0,xmm1), so an unchanged value returns without rebuilding the samplers.
    set_bias = f['dx10SamplerStateCache::SetMipLODBias']
    body = pe.read(set_bias, 0x20)
    assert b'\x0f\x2e\xc1' in body or b'\x0f\x2e\xc8' in body, ('ucomiss with xmm1', body.hex())
    log.append(f'Stage 3: Device.mView/mProject/mFullTransform {view:#x}/{project:#x}/{full:#x} adjacent; set_P {set_p:#x} '
               f'extended guard; first-person sites {forms["slot55"]} via slot 55, {forms["direct"]} direct rmNear; '
               f'SetMipLODBias {set_bias:#x} compares xmm1 with the cached bias')


# ---- Stage 3 jitter burst measurement (no executable needed) -------------------------------------------------------
def _np():
    try:
        import numpy
    except ImportError:
        return None
    return numpy


def _upsampled_dft(np, data, size, factor, offsets):
    """Matrix-multiply DFT of `data` (frequency domain) upsampled by `factor` in a size x size window at `offsets`
    (Guizar-Sicairos, Thurman and Fienup 2008, as in scikit-image's phase_cross_correlation)."""
    rows, cols = data.shape
    col_kernel = np.exp((-1j * 2 * np.pi / (cols * factor)) *
                        (np.fft.ifftshift(np.arange(cols))[:, None] - np.floor(cols / 2)) * (np.arange(size) - offsets[1])[None, :])
    row_kernel = np.exp((-1j * 2 * np.pi / (rows * factor)) *
                        (np.arange(size)[:, None] - offsets[0]) * (np.fft.ifftshift(np.arange(rows))[None, :] - np.floor(rows / 2)))
    return row_kernel @ data @ col_kernel


def image_shift(np, reference, moving, factor=100):
    """Displacement (dx, dy) in pixels of the content of `moving` relative to `reference` (+x right, +y down), from
    cross-correlation of Hann-windowed, mean-removed images with 1/factor pixel refinement."""
    window = np.outer(np.hanning(reference.shape[0]), np.hanning(reference.shape[1]))
    a = (reference - reference.mean()) * window
    b = (moving - moving.mean()) * window
    # Plain (not whitened) cross-correlation: whitening equalises the noise floor (film grain, rain) with the signal
    # and biases the subpixel peak; the band-limited correlation peak of a pure translation sits at the offset.
    product = np.fft.fft2(a) * np.conj(np.fft.fft2(b))
    correlation = np.fft.ifft2(product)
    peak = np.array(np.unravel_index(np.argmax(np.abs(correlation)), correlation.shape), dtype=float)
    shape = np.array(correlation.shape, dtype=float)
    peak[peak > np.trunc(shape / 2)] -= shape[peak > np.trunc(shape / 2)]
    region = np.ceil(factor * 1.5)
    centre = np.trunc(region / 2)
    shift = np.round(peak * factor) / factor
    refined = np.conj(_upsampled_dft(np, np.conj(product), int(region), factor, centre - shift * factor))
    fine = np.array(np.unravel_index(np.argmax(np.abs(refined)), refined.shape), dtype=float)
    shift = shift + (fine - centre) / factor
    # `shift` registers moving onto reference; the content moved by its negative.
    return -shift[1], -shift[0]


def _luminance(np, path, width, height, dxgi):
    raw = path.read_bytes()
    if dxgi == 10:   # R16G16B16A16_FLOAT
        rgba = np.frombuffer(raw, dtype='<f2').astype(np.float64).reshape(height, width, 4)
    elif dxgi in (24,):   # R10G10B10A2_UNORM
        words = np.frombuffer(raw, dtype='<u4').reshape(height, width)
        rgba = np.stack([(words >> s) & 0x3ff for s in (0, 10, 20)] + [words >> 30], axis=-1).astype(np.float64) / 1023.0
    elif dxgi in (28, 29, 87, 91):   # R8G8B8A8 / B8G8R8A8 UNORM(_SRGB)
        rgba = np.frombuffer(raw, dtype=np.uint8).reshape(height, width, 4).astype(np.float64) / 255.0
        if dxgi in (87, 91):
            rgba = rgba[..., [2, 1, 0, 3]]
    else:
        raise AssertionError(('unsupported DXGI format', dxgi, path.name))
    rgb = np.nan_to_num(np.clip(rgba[..., :3], 0.0, 65504.0))
    return rgb @ np.array([0.2126, 0.7152, 0.0722])


def measure_burst(directory, region=None, final_region=None, tolerance=0.15, out=None):
    """Reads a Stage 3 burst (burst.json, world_NN.bin, final_NN.bin) and compares the measured displacement of every
    world frame against frame 0 with the jitter difference the adapter recorded. Returns the summary dict; `ok` is
    False when any world error exceeds `tolerance` render pixels or a final region moved more than `tolerance`."""
    import json
    np = _np()
    if np is None:
        raise SystemExit('numpy is required for --measure-jitter')
    directory = pathlib.Path(directory)
    index = json.loads((directory / 'burst.json').read_text())
    frames = [f for f in index['frames'] if f['world']]
    assert len(frames) >= 2, 'need at least two world frames'
    def crop(image, box, border=4):
        if box:
            x0, y0, x1, y1 = box
            return image[y0:y1, x0:x1]
        return image[border:-border, border:-border]
    base = frames[0]
    reference = crop(_luminance(np, directory / f'world_{base["index"]:02d}.bin', *base['world_size'], base['world_dxgi']), region)
    final_reference = None
    if final_region and base['final']:
        final_reference = crop(_luminance(np, directory / f'final_{base["index"]:02d}.bin', *base['final_size'], base['final_dxgi']),
                               final_region)
    rows, worst, worst_final = [], 0.0, 0.0
    for f in frames[1:]:
        moving = crop(_luminance(np, directory / f'world_{f["index"]:02d}.bin', *f['world_size'], f['world_dxgi']), region)
        dx, dy = (float(v) for v in image_shift(np, reference, moving))
        jx = (f['jitter_px'][0] if f['applied'] else 0.0) - (base['jitter_px'][0] if base['applied'] else 0.0)
        jy = (f['jitter_px'][1] if f['applied'] else 0.0) - (base['jitter_px'][1] if base['applied'] else 0.0)
        error = max(abs(dx - jx), abs(dy - jy))
        worst = max(worst, error)
        row = {'index': f['index'], 'sequence_index': f['sequence_index'], 'expected': [round(jx, 4), round(jy, 4)],
               'measured': [round(dx, 4), round(dy, 4)], 'error_px': round(error, 4)}
        if final_reference is not None and f['final']:
            fdx, fdy = (float(v) for v in image_shift(np, final_reference, crop(_luminance(np, directory / f'final_{f["index"]:02d}.bin',
                                                                        *f['final_size'], f['final_dxgi']), final_region)))
            row['final_region_shift'] = [round(fdx, 4), round(fdy, 4)]
            worst_final = max(worst_final, abs(fdx), abs(fdy))
        rows.append(row)
    summary = {'directory': str(directory), 'region': region, 'final_region': final_region, 'frames': rows,
               'worst_world_error_px': round(float(worst), 4), 'tolerance_px': tolerance,
               'worst_final_region_shift_px': round(float(worst_final), 4) if final_reference is not None else None}
    summary['ok'] = bool(worst <= tolerance and (final_reference is None or worst_final <= tolerance))
    text = json.dumps(summary, indent=1)
    print(text)
    if out:
        pathlib.Path(out).write_text(text)
    return summary


def measure_self_test():
    """Synthetic burst: a band-limited random image shifted by Fourier translation by the first 8 Halton(2,3) samples
    (mean-free noise added), plus a static 'UI' image. The measurement must recover each offset within 0.03 px."""
    import json
    import tempfile
    np = _np()
    if np is None:
        print('SKIP: numpy unavailable')
        return 77
    rng = np.random.default_rng(7)
    h, w = 96, 144
    spectrum = np.fft.fft2(rng.standard_normal((h, w)))
    fy, fx = np.fft.fftfreq(h)[:, None], np.fft.fftfreq(w)[None, :]
    spectrum *= np.exp(-(fx ** 2 + fy ** 2) / (2 * 0.08 ** 2))   # smooth, band-limited content
    def halton(i, b):
        r, f = 0.0, 1.0
        while i:
            f /= b
            r += f * (i % b)
            i //= b
        return r
    samples = [(halton(i + 1, 2) - 0.5, halton(i + 1, 3) - 0.5) for i in range(8)]
    ui = (rng.random((h * 2, w * 2)) > 0.5).astype(np.float64)
    with tempfile.TemporaryDirectory() as temp:
        root = pathlib.Path(temp)
        frames = []
        for i, (jx, jy) in enumerate(samples):
            shifted = np.real(np.fft.ifft2(spectrum * np.exp(-2j * np.pi * (fx * jx + fy * jy))))
            shifted = 0.5 + 0.1 * shifted / np.abs(shifted).max() + rng.normal(0, 0.002, shifted.shape)
            rgba = np.stack([shifted, shifted, shifted, np.ones_like(shifted)], axis=-1).astype('<f2')
            (root / f'world_{i:02d}.bin').write_bytes(rgba.tobytes())
            final = np.stack([ui * 255] * 3 + [np.full_like(ui, 255)], axis=-1).astype(np.uint8)
            (root / f'final_{i:02d}.bin').write_bytes(final.tobytes())
            frames.append({'index': i, 'sequence_index': i, 'applied': True, 'jitter_px': [jx, jy], 'world': True,
                           'world_size': [w, h], 'world_dxgi': 10, 'final': True, 'final_size': [w * 2, h * 2], 'final_dxgi': 28})
        (root / 'burst.json').write_text(json.dumps({'frames': frames}))
        summary = measure_burst(root, final_region=[10, 10, 200, 150], tolerance=0.03)
    assert summary['ok'], summary
    print(f'PASS: measured {len(summary["frames"])} synthetic subpixel offsets, worst error {summary["worst_world_error_px"]} px')
    return 0


def verify(pe):
    assert PROFILE.exists(), 'missing games/anomaly/src/sr_profile.h'
    p = parse(TEXT)
    log = [f'image sha256 {geometry.HASH}']
    f = verify_guards(pe, p, log)
    verify_calls(pe, f, p, log)
    verify_render_modes(pe, f, log)
    set_p = verify_xforms(pe, f, log)
    verify_first_person(pe, f, p, set_p, log)
    verify_device_and_hw(pe, f, log)
    verify_options(pe, log)
    verify_targets(pe, p, log)
    verify_crt_texture(pe, log)
    verify_backend(pe, f, log)
    verify_samplers(pe, f, log)
    verify_stage2(pe, p, log)
    verify_stage3(pe, f, p, log)
    for line in log:
        print(line)
    print(f'PASS: {len(p["functions"])} function guards, {len(p["calls"])} call-site guards, {len(p["unique"])} '
          f'extended unique guards, {len(p["first_person"])} first-person sites, {len(p["slots"])} CRenderTarget slots, '
          'CRT/CTexture/RCache/R_xforms/SSManager layouts, Stage 2 world targets and SSAO viewport static, Stage 3 '
          'projection/first-person/mip-bias sites')


TEXT = PROFILE.read_text() if PROFILE.exists() else ''

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--verify', action='store_true')
    parser.add_argument('--read', nargs=2)
    parser.add_argument('--profile', help='verify another copy of sr_profile.h (negative tests)')
    parser.add_argument('--measure-jitter', metavar='DIR', help='measure a Stage 3 burst (sr-jitter-* folder); no exe needed')
    parser.add_argument('--region', nargs=4, type=int, metavar=('X0', 'Y0', 'X1', 'Y1'),
                        help='world sub-rectangle in render pixels (for example the first-person weapon)')
    parser.add_argument('--final-region', nargs=4, type=int, metavar=('X0', 'Y0', 'X1', 'Y1'),
                        help='back-buffer sub-rectangle that must not move (HUD/UI), output pixels')
    parser.add_argument('--tolerance', type=float, default=0.15, help='pass threshold in pixels (default 0.15)')
    parser.add_argument('--out', help='also write the measurement summary JSON here')
    parser.add_argument('--measure-self-test', action='store_true', help='synthetic check of the burst measurement')
    args = parser.parse_args()
    if args.profile:
        PROFILE = pathlib.Path(args.profile)
        TEXT = PROFILE.read_text()
    if args.measure_self_test:
        raise SystemExit(measure_self_test())
    if args.measure_jitter:
        raise SystemExit(0 if measure_burst(args.measure_jitter, args.region, args.final_region, args.tolerance, args.out)['ok'] else 1)
    image = Image()
    if args.verify:
        verify(image)
    if args.read:
        print(image.read(int(args.read[0], 0), int(args.read[1], 0)).hex())
