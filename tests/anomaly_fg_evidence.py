"""Frame generation: read-only check of the engine sites games/anomaly/src/fg_stage.cpp relies on, against the installed exe.

FG input facts additionally use the guarded CInput primary vtable slot 0. It is driven from guarded boundaries the native observer already hooks
(games/anomaly/src/native_profile.h) and reads HW.pBaseZB (games/anomaly/src/sr_profile.h) for the SR-off depth.
This script proves, on --exe standard (default) or --exe avx, that those sites are what the FG timeline assumes:

  dxRenderDeviceRender::Begin      render begin (simulation end, FRAME/SUBMISSION begin)
  dxRenderDeviceRender::End        contains the only engine Present: HW.m_pSwapChain->Present (vtable +0x40),
                                   behind the secondary-viewport test; end_before/end_after bracket it
  SetCacheXform                    the frame's main camera, before rendering
  CHUDManager::RenderUI            the HUD-less boundary; reached from CLevel::OnRender as a tail jump right after
                                   the scope copy (Render->RenderToTarget(rtSVP), vtable call) and the world render
  CRender::RenderToTarget          target 2 is the secondary viewport (scope); it copies swap-chain buffer 0
  CRender::reset_begin             device reset
  HW.pBaseZB                       the depth view the SR-off pass reads, used by CRender::Render's u_setrt

Needs D:/Anomaly 1.5.3 (not in CTest, like the other Anomaly evidence scripts). Nothing is patched or executed.
Run: py -3 -I tests/anomaly_fg_evidence.py --verify
"""
import argparse
import importlib.util
import pathlib
import re
import struct
import capstone
from capstone import x86

ROOT = pathlib.Path(__file__).resolve().parents[1]
NATIVE = ROOT / 'games/anomaly/src/native_profile.h'
SR_PROFILE = ROOT / 'games/anomaly/src/sr_profile.h'
INPUT_PROFILE = ROOT / 'games/anomaly/src/input_profile.h'
_spec = importlib.util.spec_from_file_location('anomaly_sr_evidence', ROOT / 'tests/anomaly_sr_evidence.py')
sr = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(sr)

FG_SITES = ('IRenderDeviceRender::Begin', 'IRenderDeviceRender::End', 'IRenderDeviceRender::SetCacheXform',
            'CHUDManager::RenderUI', 'CRender::RenderToTarget', 'CRender::reset_begin')


def native_sites():
    text = sr.geometry.legacy_profile(NATIVE)
    sites = {}
    for name, rva, hexbytes in re.findall(r'\{"([^"]+)", (0x[0-9a-f]+), signature\("([0-9a-f]{32})"\)\}', text):
        sites[name] = (int(rva, 16), bytes.fromhex(hexbytes))
    size = int(re.search(r'image_size = (0x[0-9a-f]+)', text).group(1), 16)
    return sites, size


def ints(text, name):
    return int(re.search(name + r'\s*=\s*(0x[0-9a-f]+)', text).group(1), 16)


def indirect_calls(code, base, displacement):
    """Offsets of `call`/`jmp [reg+disp8]` (ff 50..57 / ff 60..67 d8) and `call`/`jmp [reg+disp32]` (ff 90..97 /
    ff a0..a7 d32) with this displacement. The compiler emits the final Present as a tail jump."""
    found = []
    for i in range(len(code) - 6):
        if code[i] != 0xff:
            continue
        modrm = code[i + 1]
        if modrm & 0x07 == 0x04:
            continue   # SIB forms are not used for these vtable calls
        if (0x50 <= modrm <= 0x57 or 0x60 <= modrm <= 0x67) and code[i + 2] == displacement:
            found.append(base + i)
        elif (0x90 <= modrm <= 0x97 or 0xa0 <= modrm <= 0xa7) and struct.unpack_from('<i', code, i + 2)[0] == displacement:
            found.append(base + i)
    return found


def verify_input(pe, log):
    text = sr.geometry.profile_text(INPUT_PROFILE)
    sites = [(n, int(r, 16), bytes.fromhex(h)) for n, r, h in re.findall(
        r'\{"(CInput::[^"]+)", (0x[0-9a-f]+), signature\("([0-9a-f]+)"\)\}', text)]
    assert len(sites) == 3, 'input profile entries'
    for name, rva, raw in sites:
        assert len(raw) == 16 and pe.read(rva, 16) == raw and pe.unique_in_text(raw), ('input expected16', name)
    frame, keyboard, mouse = [r for _, r, _ in sites]
    table_rva = int(re.search(r'signature\("[0-9a-f]+"\)\},\s*(0x[0-9a-f]+),', text)[1], 16)
    table = next(t for t in pe.tables('CInput') if t['this_offset'] == 0)
    assert int(table['vtable'], 16) == table_rva and int(table['slots'][0], 16) == frame, 'CInput primary pureFrame slot 0'
    raw = bytes.fromhex(re.findall(r'"([0-9a-f]{64,})"', text)[-1])
    assert pe.read(frame, len(raw)) == raw and pe.unique_in_text(raw), 'CInput complete OnFrame guard'
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64); md.detail = True
    code = list(md.disasm(raw, frame))
    assert code[-1].mnemonic == 'ret' and code[-1].address + code[-1].size == pe.function_end(frame), 'complete input callback'
    calls = [i for i in code if i.mnemonic == 'call' and i.operands[0].type == x86.X86_OP_IMM]
    assert [i.operands[0].imm for i in calls if i.operands[0].imm in (keyboard, mouse)] == [keyboard, mouse], 'keyboard before mouse'
    full = INPUT_PROFILE.read_text(encoding='utf-8')
    offsets = [int(re.search(r'input_' + name + r'_offset = (0x[0-9a-f]+)', full)[1], 16) for name in ('keyboard', 'mouse')]
    for target, offset in zip((keyboard, mouse), offsets):
        call = next(i for i in calls if i.operands[0].imm == target)
        prior = [i for i in code if call.address-16 <= i.address < call.address]
        assert any(i.mnemonic == 'cmp' and i.operands[0].type == x86.X86_OP_MEM and
                   i.operands[0].mem.base == x86.X86_REG_RDI and i.operands[0].mem.disp == offset for i in prior), 'input device field'
        head = pe.read(target, 0x100)
        assert len(indirect_calls(head, target, 0x50)) == 2, 'GetDeviceData initial/reacquire retry, COM slot 10'
        assert bytes.fromhex('ba18000000') in head and b'\x40\x00\x00\x00' in head, '24-byte DIDEVICEOBJECTDATA / 64-entry buffer'
    # Five direct calls: two input reads and three statistics/timer calls. No controller poll occurs in OnFrame.
    assert len(calls) == 5 and not pe.string_rvas('XInputGetState'), 'stock callback has no XInput poll'
    log.append(f'input: OnFrame {frame:#x} primary vtable {table_rva:#x} slot 0; keyboard {keyboard:#x} (+{offsets[0]:#x}), '
               f'mouse {mouse:#x} (+{offsets[1]:#x}); GetDeviceData buffered reads; no native controller poll')


def verify(pe):
    log = [f'image sha256 {sr.geometry.HASH}']
    verify_input(pe, log)
    sites, size = native_sites()
    nt = struct.unpack_from('<I', pe.data, 60)[0]
    assert struct.unpack_from('<I', pe.data, nt + 24 + 56)[0] == size, 'SizeOfImage'
    all_sites = [(n, r, h.hex()) for n, (r, h) in sites.items()]
    sr.geometry.verify_unique_guards(pe, sr.geometry.profile_text(NATIVE), 'native', all_sites)
    sr.geometry.verify_detour_prefix(pe, all_sites)
    for name in sites:
        rva, expected = sites[name]
        actual = pe.read(rva, 16)
        assert actual == expected, f'{name} at {rva:#x}: {actual.hex()} != {expected.hex()}'
        log.append(f'{name:36} {rva:#08x} guard {expected.hex()} OK')
    profile = sr.geometry.legacy_profile(SR_PROFILE)
    chain = ints(profile, 'hw_swap_chain_rva')
    base_zb = ints(profile, 'hw_base_zb_rva')
    # End: the only Present, HW.m_pSwapChain->Present(...), is an indirect call at vtable +0x40.
    end = sites['IRenderDeviceRender::End'][0]
    end_stop = pe.function_end(end)
    body = pe.read(end, end_stop - end)
    reads = {t for _, t in pe.rip_operands(end, end_stop)}
    assert chain in reads, f'End does not read HW.m_pSwapChain {chain:#x}: {sorted(map(hex, reads))}'
    presents = indirect_calls(body, end, 0x40)
    assert len(presents) == 1, f'End: expected one vtable+0x40 call, found {[hex(p) for p in presents]}'
    # The Present is the function's last instruction (a tail jump), so the hooked original returns only after it.
    assert body[presents[0] - end - 1:presents[0] - end + 3] == bytes.fromhex('48ff6040'), 'Present tail jump'
    log.append(f'End {end:#x}..{end_stop:#x}: HW.m_pSwapChain {chain:#x} read, Present tail jump at {presents[0]:#x}')
    # RenderUI and RenderToTarget are virtual: find their vtable slots through RTTI (CHUDManager, CRender primary).
    render_ui = sites['CHUDManager::RenderUI'][0]
    rtt = sites['CRender::RenderToTarget'][0]
    ui_slots = {i for t in pe.tables('CHUDManager') if t['this_offset'] == 0 for i, s in enumerate(t['slots']) if s == hex(render_ui)}
    rtt_slots = {i for t in pe.tables('CRender') if t['this_offset'] == 0 for i, s in enumerate(t['slots']) if s == hex(rtt)}
    assert len(ui_slots) == 1 and len(rtt_slots) == 1, f'slots RenderUI {ui_slots} RenderToTarget {rtt_slots}'
    ui_disp, rtt_disp = 8 * ui_slots.pop(), 8 * rtt_slots.pop()
    # CLevel::OnRender (Level.cpp:776-779): `if (IsSVPFrame()) Render->RenderToTarget(rtSVP); HUD().RenderUI();`.
    # The only call of RenderToTarget with rtSVP (edx = 2) is directly followed by g_hud's RenderUI, compiled as a tail
    # jump through vtable + ui_disp. The PDA copy (edx = 1) of the same function precedes it.
    text_va, code = pe.text()
    needle = b'\xff\x90' + struct.pack('<i', rtt_disp)
    sites_svp = []
    at = code.find(needle)
    while at >= 0:
        if b'\xba\x02\x00\x00\x00' in code[at - 0x20:at]:
            sites_svp.append(at)
        at = code.find(needle, at + 1)
    assert len(sites_svp) == 1, f'RenderToTarget(rtSVP) call sites {[hex(text_va + s) for s in sites_svp]}'
    svp = sites_svp[0]
    tail = code[svp + 6:svp + 0x30]
    jump = tail.find(b'\x48\xff\x60' + bytes([ui_disp]))
    assert jump >= 0, f'no RenderUI tail jump after the SVP copy: {tail.hex()}'
    pda = code.rfind(needle, svp - 0x100, svp)
    assert pda >= 0 and b'\xba\x01\x00\x00\x00' in code[pda - 0x20:pda], 'PDA copy before the SVP copy (CLevel::OnRender)'
    log.append(f'CLevel::OnRender: RenderToTarget(rtPDA=1) at {text_va + pda:#x}, RenderToTarget(rtSVP=2) at '
               f'{text_va + svp:#x} (vtable +{rtt_disp:#x}), then HUD().RenderUI tail jump at {text_va + svp + 6 + jump:#x} '
               f'(vtable +{ui_disp:#x} -> {render_ui:#x})')
    # RenderToTarget compares its target against 1 (PDA) and 2 (SVP): the guard starts `mov r8d, edx; sub r8d, 1`.
    assert pe.read(rtt, 16)[6:13] == bytes.fromhex('448bc24183e801'), 'RenderToTarget target switch'
    # HW.pBaseZB is read by CRender::Render's closing u_setrt (sr evidence verifies the full layout).
    zb_reads = {t for _, t in pe.rip_operands(sr.CRENDER_RENDER, sr.CRENDER_RENDER_END)}
    assert base_zb in zb_reads, f'HW.pBaseZB {base_zb:#x} not read by CRender::Render'
    log.append(f'HW.pBaseZB {base_zb:#x} read by CRender::Render; HW.m_pSwapChain {chain:#x}')
    for line in log:
        print(line)
    print(f'PASS: {len(sites)} native guards, {len(FG_SITES)} FG boundaries, End owns the one Present, RenderUI follows the only scope copy '
          'in CLevel::OnRender, HW.pBaseZB')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--verify', action='store_true')
    parser.add_argument('--exe', choices=sorted(sr.geometry.BUILDS), default=sr.geometry.BUILD)
    parser.add_argument('--profile', type=pathlib.Path)
    parser.add_argument('--input-profile', type=pathlib.Path)
    args = parser.parse_args()
    if args.profile:
        NATIVE = args.profile
    if args.input_profile:
        INPUT_PROFILE = args.input_profile
    if args.verify:
        verify(sr.Image())
