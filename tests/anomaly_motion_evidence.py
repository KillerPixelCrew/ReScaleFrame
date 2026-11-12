"""Motion MRT sites: read-only check of every games/anomaly/src/motion_profile.h value against the installed exe.

Needs the installed STANDARD AnomalyDX11.exe (D:/Anomaly 1.5.3) and capstone, so it is not registered in CTest
(same as tests/anomaly_sr_evidence.py). Nothing is patched or executed; the file is only read and disassembled.
Run: py -3 -I tests/anomaly_motion_evidence.py --verify
Every check raises AssertionError naming the site that does not match.

What is proven, per site:
* create_shader (0x838de0): the function that logs "! CreatePixelShader hr", calls ID3D11Device CreatePixelShader
  (+0x78), CreateVertexShader (+0x60) and CreateGeometryShader (+0x68) with &result->shader (`lea rax, [r15+10h]`),
  then the D3DReflect import on the same buffer, and for VS the D3DGetInputSignatureBlob import. Its only direct
  callers in .text are the two profile call sites, both inside shader_compile (.pdata, chained unwind followed).
* shader_compile (0x83a610): prologue = 4 home-slot stores, 7 pushes, lea rbp,[rsp-13A0h], sub rsp,14A0h, so
  rbp = rsp+100h and the caller frame is fixed. The D3DCompile import call takes pSrcData/SrcDataLen from the r8/r9
  home slots, pDefines = lea [rbp+150h], pFunctionName and Flags from the stack arguments, pSourceName = "" - the
  exact offsets in the profile. No instruction in the body writes those home/argument slots after the prologue.
* phase_scene_begin/end: the G-buffer bind u_setrt(this, this+0x378 rt_Position, ...) / SetMaxAnisotropy(1) bodies,
  with exactly their profile callers in CRender::Render.
"""
import argparse
import importlib.util
import pathlib
import re
import struct

import capstone
from capstone import x86

ROOT = pathlib.Path(__file__).resolve().parents[1]
PROFILE = ROOT / 'games/anomaly/src/motion_profile.h'
_spec = importlib.util.spec_from_file_location('anomaly_geometry_evidence', ROOT / 'tests/anomaly_geometry_evidence.py')
geometry = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(geometry)

CRENDER_RENDER = 0x8977c0
COMPILER = 'd3dcompiler_43.dll'   # games/anomaly/src/motion_profile.h engine_compiler_module
SET_MAX_ANISOTROPY = 0x7fe060


def parse_profile(text):
    sites = {m[1]: (int(m[2], 16), bytes.fromhex(m[3])) for m in
             re.finditer(r'\{"([^"]+)", (0x[0-9a-f]+), signature\("([0-9a-f]{32})"\)\}', text)}
    ints = {m[1]: int(m[2], 0) for m in re.finditer(r'inline constexpr uint32_t (\w+) = (0x[0-9a-f]+|\d+);', text)}
    hexes = {}
    for m in re.finditer(r'inline constexpr char (\w+)\[\] =\s*((?:"[0-9a-f]*"\s*)+);', text):
        hexes[m[1]] = ''.join(re.findall(r'"([0-9a-f]*)"', m[2]))
    return sites, ints, hexes


class Image(geometry.PE):
    def __init__(self):
        super().__init__()
        self.text_va, self.code = self.text()
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
        self.md.detail = True
        self.imports = self.import_slots()

    def import_slots(self):
        nt = struct.unpack_from('<I', self.data, 60)[0]
        directory = struct.unpack_from('<II', self.data, nt + 24 + 112 + 8)[0]   # IMAGE_DIRECTORY_ENTRY_IMPORT
        result = {}
        at = directory
        while True:
            lookup, _, _, name, thunk = struct.unpack('<IIIII', self.read(at, 20))
            if not name:
                break
            dll = self.cstring(name).lower()
            index = 0
            while True:
                entry = struct.unpack('<Q', self.read((lookup or thunk) + index * 8, 8))[0]
                if not entry:
                    break
                if not entry & (1 << 63):
                    result[thunk + index * 8] = (dll, self.cstring((entry & 0x7fffffff) + 2))
                index += 1
            at += 20
        return result

    def cstring(self, rva):
        raw = self.read(rva, 256)
        return raw[:raw.index(b'\0')].decode()

    def function(self, rva):
        """[start, end) of the .pdata function containing rva, following chained unwind info to the primary."""
        nt = struct.unpack_from('<I', self.data, 60)[0]
        pdata, size = struct.unpack_from('<II', self.data, nt + 24 + 112 + 3 * 8)
        best = None
        for i in range(size // 12):
            begin, end, unwind = struct.unpack('<III', self.read(pdata + i * 12, 12))
            if begin <= rva < end:
                best = (begin, end, unwind)
                break
        assert best, ('no .pdata entry', hex(rva))
        begin, end, unwind = best
        while self.read(unwind, 1)[0] >> 3 & 0x4:   # UNW_FLAG_CHAININFO: the primary entry follows the codes
            count = self.read(unwind + 2, 1)[0]
            chained = unwind + 4 + ((count + 1) & ~1) * 2
            begin, end, unwind = struct.unpack('<III', self.read(chained, 12))
        return begin, end

    def disasm(self, start, end):
        return list(self.md.disasm(self.read(start, end - start), start))

    def direct_callers(self, target):
        result = []
        at = 0
        while True:
            at = self.code.find(b'\xe8', at)
            if at < 0 or at + 5 > len(self.code):
                return result
            rva = self.text_va + at
            if rva + 5 + struct.unpack_from('<i', self.code, at + 1)[0] == target:
                result.append(rva)
            at += 1

    def import_of(self, instruction):
        """Name of the import a `call rel32` reaches through its `jmp [rip+x]` thunk, or a direct IAT call."""
        if instruction.mnemonic != 'call':
            return None
        op = instruction.operands[0]
        if op.type == x86.X86_OP_IMM:
            thunk = list(self.md.disasm(self.read(op.imm, 6), op.imm))
            if not thunk or thunk[0].mnemonic != 'jmp' or thunk[0].operands[0].type != x86.X86_OP_MEM:
                return None
            slot = thunk[0].address + thunk[0].size + thunk[0].operands[0].mem.disp
        elif op.type == x86.X86_OP_MEM and op.mem.base == x86.X86_REG_RIP:
            slot = instruction.address + instruction.size + op.mem.disp
        else:
            return None
        return self.imports.get(slot)


def mem(op):
    return op.mem if op.type == x86.X86_OP_MEM else None


def verify(pe, log):
    sites, ints, hexes = parse_profile(PROFILE.read_text())
    names = ['create_shader', 'CRenderTarget::phase_scene_begin', 'CRenderTarget::phase_scene_end']
    for name in names:
        rva, expected = sites[name]
        assert pe.read(rva, 16) == expected, ('guard', name)
        start, _ = pe.function(rva)
        assert start == rva, ('not a function start', name)
    log('guards: 3 detour prologues match and start .pdata functions')

    # ---- create_shader -------------------------------------------------------------------------------------------
    create, _ = sites['create_shader']
    begin, end = pe.function(create)
    code = pe.disasm(begin, end)
    strings = {pe.cstring(i.address + i.size + i.operands[1].mem.disp) for i in code
               if i.mnemonic == 'lea' and len(i.operands) == 2 and mem(i.operands[1]) and i.operands[1].mem.base == x86.X86_REG_RIP
               and pe.read(i.address + i.size + i.operands[1].mem.disp, 1)[0] in b'!.'}
    assert '! CreatePixelShader hr == 0x%08x' in strings and '! D3DXFindShaderComment hr == 0x%08x' in strings, 'create_shader strings'
    vcalls = [i for i in code if i.mnemonic == 'call' and mem(i.operands[0]) and i.operands[0].mem.base != x86.X86_REG_RIP]
    slots = sorted({i.operands[0].mem.disp for i in vcalls})
    for slot in (0x60, 0x68, 0x78):   # ID3D11Device::CreateVertexShader / CreateGeometryShader / CreatePixelShader
        assert slot in slots, ('device create slot', hex(slot))
    shader_offset = ints['shader_object_offset']
    outparams = [i for i in code if i.mnemonic == 'lea' and mem(i.operands[1]) and i.operands[1].mem.base == x86.X86_REG_R15
                 and i.operands[1].mem.disp == shader_offset]
    assert len(outparams) >= 3, ('shader object out-parameter lea [r15+0x10]', len(outparams))
    compiler = re.search(r'engine_compiler_module\[\] = L"([^"]+)"', PROFILE.read_text())[1].lower()
    assert compiler == COMPILER, ('engine compiler', compiler)
    imported = [pe.import_of(i) for i in code if i.mnemonic == 'call']
    assert imported.count((COMPILER, 'D3DReflect')) >= 3, ('D3DReflect calls', imported)
    assert (COMPILER, 'D3DGetInputSignatureBlob') in imported, 'input signature extraction'
    log('create_shader: Create{Vertex,Geometry,Pixel}Shader(+0x60/+0x68/+0x78) into result+0x%x, D3DReflect, '
        'D3DGetInputSignatureBlob on the same buffer' % shader_offset)

    # ---- callers: exactly the two profile call sites, inside shader_compile ---------------------------------------
    calls = sorted(pe.direct_callers(create))
    profile_calls = sorted(rva for name, (rva, _) in sites.items() if name.startswith('shader_compile -> create_shader'))
    assert calls == profile_calls, ('create_shader callers', [hex(c) for c in calls])
    compile_rva = ints['shader_compile_rva']
    for call in calls:
        assert pe.function(call) == pe.function(compile_rva), ('call outside shader_compile', hex(call))
        _, expected = next(v for n, v in sites.items() if v[0] == call)
        assert pe.read(call, 16) == expected, ('call guard', hex(call))
    assert ints['call_length'] == 5
    log('callers: create_shader has exactly the 2 direct calls %s, both in shader_compile' % [hex(c) for c in calls])

    # ---- shader_compile frame ----------------------------------------------------------------------------------
    assert pe.read(compile_rva, len(hexes['shader_compile_prologue']) // 2).hex() == hexes['shader_compile_prologue'], 'prologue'
    begin, end = pe.function(compile_rva)
    body = pe.disasm(begin, end)
    pushes = 0
    rbp_from_entry = frame = None
    homes = {}
    for i in body[:24]:
        if i.mnemonic == 'push':
            pushes += 1
        elif i.mnemonic == 'mov' and mem(i.operands[0]) and i.operands[0].mem.base == x86.X86_REG_RSP and pushes == 0:
            homes[i.operands[0].mem.disp] = i.reg_name(i.operands[1].reg)
        elif i.mnemonic == 'lea' and i.operands[0].reg == x86.X86_REG_RBP:
            rbp_from_entry = -8 * pushes + i.operands[1].mem.disp
        elif i.mnemonic == 'mov' and i.operands[0].reg == x86.X86_REG_EAX and i.operands[1].type == x86.X86_OP_IMM:
            frame = i.operands[1].imm
        elif i.mnemonic == 'sub' and i.operands[0].reg == x86.X86_REG_RSP:
            break
    assert pushes == 7 and frame == 0x14a0 and rbp_from_entry is not None, ('frame shape', pushes, frame)
    rsp_from_entry = -8 * pushes - frame
    rbp = rbp_from_entry - rsp_from_entry                       # rbp relative to the body rsp
    entry = -rsp_from_entry                                        # entry rsp relative to the body rsp
    assert rbp == 0x100 and entry == 0x14d8, ('rbp/entry', hex(rbp), hex(entry))
    # Entry-relative home slots: [rsp+8] rcx, +10 rbx, +18 r8 (pSrcData), +20 r9d (SrcDataLen).
    assert homes.get(0x18) == 'r8' and homes.get(0x20) == 'r9d', ('home slots', homes)
    assert ints['frame_source_offset'] == entry + 0x18 and ints['frame_length_offset'] == entry + 0x20
    assert ints['frame_entry_offset'] == entry + 0x28 and ints['frame_flags_offset'] == entry + 0x38
    # The D3DCompile call and its operands.
    call = next(i for i in body if pe.import_of(i) == (COMPILER, 'D3DCompile'))
    setup = ints['d3dcompile_setup_rva']
    assert pe.read(setup, len(hexes['d3dcompile_setup']) // 2).hex() == hexes['d3dcompile_setup'], 'D3DCompile setup guard'
    assert setup < call.address and call.address + call.size == setup + len(hexes['d3dcompile_setup']) // 2, 'setup ends at the call'
    window = [i for i in body if setup <= i.address < call.address]
    def rbp_operand(register, mnemonic):
        found = [i for i in window if i.mnemonic == mnemonic and i.operands[0].type == x86.X86_OP_REG and
                 i.operands[0].reg == register and mem(i.operands[1]) and i.operands[1].mem.base == x86.X86_REG_RBP]
        assert len(found) == 1, ('operand', register, mnemonic)
        return found[0].operands[1].mem.disp + rbp
    assert rbp_operand(x86.X86_REG_RCX, 'mov') == ints['frame_source_offset'], 'pSrcData'
    assert rbp_operand(x86.X86_REG_EDX, 'mov') == ints['frame_length_offset'], 'SrcDataLen'
    assert rbp_operand(x86.X86_REG_R9, 'lea') == ints['frame_defines_offset'], 'pDefines'
    loads = {i.operands[1].mem.disp + rbp for i in window if i.mnemonic == 'mov' and i.operands[0].type == x86.X86_OP_REG and
             i.operands[0].reg in (x86.X86_REG_RAX, x86.X86_REG_EAX) and mem(i.operands[1]) and i.operands[1].mem.base == x86.X86_REG_RBP}
    assert ints['frame_entry_offset'] in loads and ints['frame_flags_offset'] in loads, ('entry/flags loads', loads)
    name = [i for i in window if i.mnemonic == 'lea' and i.operands[0].reg == x86.X86_REG_R8 and i.operands[1].mem.base == x86.X86_REG_RIP]
    assert len(name) == 1 and name[0].address + name[0].size + name[0].operands[1].mem.disp == ints['source_name_rva'], 'pSourceName'
    assert pe.read(ints['source_name_rva'], 1) == b'\0', 'pSourceName is ""'
    # The defines array: 128 entries of 16 bytes, all stores into it are rbp-indexed at +0x150.
    defines = ints['frame_defines_offset'] - rbp
    assert ints['frame_defines_capacity'] == 128
    # Nothing writes the argument/home slots after the prologue.
    protected = {ints[k] - rbp for k in ('frame_source_offset', 'frame_length_offset', 'frame_entry_offset', 'frame_flags_offset')}
    sub = next(i for i in body if i.mnemonic == 'sub' and i.operands[0].reg == x86.X86_REG_RSP)
    writes = []
    for i in body:
        if i.address <= sub.address or not i.operands:
            continue
        first = i.operands[0]
        if mem(first) and first.mem.base == x86.X86_REG_RBP and first.mem.index == 0 and first.mem.disp in protected and \
                i.mnemonic not in ('cmp', 'test'):
            writes.append((hex(i.address), i.mnemonic, i.op_str))
    assert not writes, ('frame slots written in the body', writes)
    log('shader_compile: rbp=rsp+0x100, caller frame offsets source=0x%x length=0x%x entry=0x%x flags=0x%x defines=0x%x '
        '(lea [rbp+0x%x]); pSourceName=""; slots never rewritten' % (ints['frame_source_offset'], ints['frame_length_offset'],
                                                                     ints['frame_entry_offset'], ints['frame_flags_offset'],
                                                                     ints['frame_defines_offset'], defines))

    # ---- G-buffer window ---------------------------------------------------------------------------------------
    begin_rva, _ = sites['CRenderTarget::phase_scene_begin']
    end_rva, _ = sites['CRenderTarget::phase_scene_end']
    render = pe.function(CRENDER_RENDER)
    begin_calls = pe.direct_callers(begin_rva)
    end_calls = pe.direct_callers(end_rva)
    assert len(begin_calls) == 3 and all(pe.function(c) == render for c in begin_calls), ('phase_scene_begin callers', begin_calls)
    assert len(end_calls) == 2 and all(pe.function(c) == render for c in end_calls), ('phase_scene_end callers', end_calls)
    code = pe.disasm(*pe.function(begin_rva))
    first_call = next(i for i in code if i.mnemonic in ('call', 'jmp') and i.operands[0].type == x86.X86_OP_IMM)
    assert first_call.operands[0].imm == SET_MAX_ANISOTROPY, 'phase_scene_begin starts with SetMaxAnisotropy'
    position = [i for i in code if i.mnemonic == 'lea' and mem(i.operands[1]) and i.operands[1].mem.disp == 0x378]
    assert position, 'phase_scene_begin passes this+0x378 (rt_Position) to u_setrt'
    code = pe.disasm(*pe.function(end_rva))
    first = next(i for i in code if i.mnemonic == 'call')
    one = [i for i in code if i.address < first.address and i.mnemonic == 'mov' and i.operands[0].type == x86.X86_OP_REG and
           i.operands[0].reg == x86.X86_REG_EDX and i.operands[1].type == x86.X86_OP_IMM and i.operands[1].imm == 1]
    assert first.operands[0].imm == SET_MAX_ANISOTROPY and one, 'phase_scene_end starts with SetMaxAnisotropy(1)'
    log('window: phase_scene_begin callers %s, phase_scene_end callers %s, all in CRender::Render' %
        ([hex(c) for c in begin_calls], [hex(c) for c in end_calls]))
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--verify', action='store_true', required=True)
    parser.parse_args()
    pe = Image()
    verify(pe, print)
    print('anomaly motion evidence: all motion_profile.h sites verified against', geometry.EXE)


if __name__ == '__main__':
    main()
