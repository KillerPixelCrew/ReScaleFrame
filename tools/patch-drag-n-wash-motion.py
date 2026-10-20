# SPDX-License-Identifier: GPL-3.0-only
# /// script
# requires-python = ">=3.12"
# dependencies = ["UnityPy==1.25.0"]
# ///
"""Build an artifact-local character-motion shader patch from the installed game.

Game bytecode and decompiled sources stay in the caller's ignored output directory.
This command never writes the game installation. Pair the generated shader pass with
CharacterMotion.cs, which binds its per-renderer vertex histories and render targets.
"""

import argparse
import copy
import ctypes
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

import UnityPy
from UnityPy.helpers import CompressionHelper

ASSET_SHA256 = "0948b788c15132fccb447a120daa4ad991ebf7878db7365757381dbb7c0ab4f0"
SHADER = "WalkNWashLit"
PASS = "ReScaleFrameMotion"
CONTROL_REGISTER = 256


def digest(data):
    return hashlib.sha256(data).hexdigest()


def compile_shader(source, profile):
    compiler = ctypes.WinDLL("d3dcompiler_47.dll")
    entry = compiler.D3DCompile
    entry.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p,
                     ctypes.c_void_p, ctypes.c_void_p, ctypes.c_char_p,
                     ctypes.c_char_p, ctypes.c_uint, ctypes.c_uint,
                     ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_void_p)]
    entry.restype = ctypes.c_long
    code, errors = ctypes.c_void_p(), ctypes.c_void_p()
    encoded = source.encode("utf-8")
    result = entry(encoded, len(encoded), b"rsf-motion", None, None, b"main",
                   profile.encode(), (1 << 11) | (1 << 13), 0,
                   ctypes.byref(code), ctypes.byref(errors))

    def blob(pointer):
        table = ctypes.cast(pointer, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p))).contents
        address = ctypes.WINFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p)(table[3])(pointer)
        size = ctypes.WINFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p)(table[4])(pointer)
        data = ctypes.string_at(address, size)
        ctypes.WINFUNCTYPE(ctypes.c_ulong, ctypes.c_void_p)(table[2])(pointer)
        return data

    message = blob(errors).decode(errors="replace") if errors else ""
    if result < 0:
        raise RuntimeError(f"{profile} compilation failed: {message}")
    return blob(code)


def segments(shader):
    if shader["platforms"] != [4] or shader["stageCounts"] != [2]:
        raise ValueError("Expected the fingerprinted Windows D3D shader payload")
    compressed = bytes(shader["compressedBlob"])
    return [CompressionHelper.decompress_lz4(compressed[o:o+n], size)
            for o, n, size in zip(shader["offsets"][0], shader["compressedLengths"][0],
                                  shader["decompressedLengths"][0])]


def program(raw, index):
    offset, size, segment = struct.unpack_from("<III", raw[0], 4 + index * 12)
    return raw[segment][offset:offset+size]


def code_range(blob):
    # Unity's 202012090 shader-program envelope, with aligned keyword strings.
    if struct.unpack_from("<I", blob)[0] != 202012090:
        raise ValueError("Unexpected GPU program envelope version")
    count = struct.unpack_from("<I", blob, 24)[0]
    cursor = 28
    for _ in range(count):
        size = struct.unpack_from("<I", blob, cursor)[0]
        cursor = (cursor + 4 + size + 3) & ~3
    length = struct.unpack_from("<I", blob, cursor)[0]
    start = blob.index(b"DXBC", cursor + 4, cursor + 4 + length)
    size = struct.unpack_from("<I", blob, start + 24)[0]
    if start + size != cursor + 4 + length:
        raise ValueError("Unexpected Direct3D code suffix")
    return cursor, start, size


def wrap_source(source, vertex):
    source = re.sub(r"^// ---- Created with 3Dmigoto[^\r\n]*\r?\n", "", source, count=1)
    match = re.search(r"void main\((.*?)\)\s*\{", source, re.S)
    if not match:
        raise ValueError("Decompiler entry point did not match")
    arguments = [arg.strip() for arg in match[1].split(",")]
    names = [re.search(r"\b([vo]\d+)\s*:", arg).group(1) for arg in arguments]
    source = source[:match.start()] + source[match.start():].replace("void main(", "void rsf_original(", 1)
    # Entry-point shaders can leave unused components undefined. A callable HLSL
    # function must initialize each out parameter, including those unused components.
    entry_brace = source.index("{", source.index("void rsf_original("))
    initializers = "".join(f"\n  {name} = 0;" for name, arg in zip(names, arguments) if arg.startswith("out "))
    source = source[:entry_brace+1] + initializers + source[entry_brace+1:]
    if vertex:
        # The selected DepthOnly variants all expose their actual deformed world position
        # as o1.xyz/TEXCOORD0. Keep every original input, output, clip and deformation call.
        if "out float4 o1 : TEXCOORD0" not in match[1] or "register(t31)" in source or "register(u1)" in source:
            raise ValueError("Unexpected character vertex signature or reserved binding collision")
        source = re.sub(r"float4 cb0\[(\d+)\];", "float4 cb0[265];", source, count=1)
        declarations = """
StructuredBuffer<float4> rsf_previous : register(t31);
RWStructuredBuffer<float4> rsf_current : register(u1);
float4 rsf_project(float3 p, uint baseRegister) {
    return cb0[baseRegister] * p.x + cb0[baseRegister+1] * p.y +
        cb0[baseRegister+2] * p.z + cb0[baseRegister+3];
}
"""
        extra = ["uint rsf_vertex : SV_VertexID", "out float4 rsf_now : TEXCOORD14",
                 "out float4 rsf_before : TEXCOORD15"]
        body = """
    uint4 control = asuint(cb0[256]);
    rsf_now = rsf_project(o1.xyz, 257);
    rsf_before = rsf_now;
    if (rsf_vertex < control.z) {
        if ((control.w & 1) != 0) {
            float4 previous = rsf_previous[rsf_vertex];
            if (asuint(previous.w) == control.x)
                rsf_before = rsf_project(previous.xyz, 261);
        }
        rsf_current[rsf_vertex] = float4(o1.xyz, asfloat(control.y));
    }
    if ((control.w & 2) != 0) rsf_before = rsf_now;
"""
    else:
        declarations = ""
        extra = ["float4 rsf_now : TEXCOORD14", "float4 rsf_before : TEXCOORD15"]
        body = """
    o0 = float4((rsf_now.xy / rsf_now.w - rsf_before.xy / rsf_before.w)
        * float2(0.5, -0.5), 0, 0);
"""
    return source + declarations + "\nvoid main(\n" + ",\n".join(arguments + extra) + ") {\n" + \
        "    rsf_original(" + ", ".join(names) + ");\n" + body + "}\n"


def replace_code(blob, code, vertex):
    length_at, start, size = code_range(blob)
    prefix = bytearray(blob[length_at+4:start])
    if len(prefix) != 38 or prefix[0] != 2:
        raise ValueError("Unexpected Direct3D program header")
    if vertex:
        # Reserve the high SRV binding for the previous-position stream. The UAV is
        # explicitly bound by the graph pass, rather than inherited from a material.
        # Matched player D3D12CommonShader::CreateFromSource copies the five bytes
        # after the version into SRV/CBV/sampler/UAV/flags counts. IsValidOnTier
        # checks the SRV, CBV and sampler fields against 128, 14 and 16.
        prefix[1] = 32
        prefix[4] = 2
    payload = bytes(prefix) + code
    tail = blob[(start + size + 3) & ~3:]
    rebuilt = blob[:length_at] + struct.pack("<I", len(payload)) + payload
    return rebuilt + b"\0" * (-len(rebuilt) % 4) + tail


def build_shader(data, decompiler, output, reuse_compiled=False):
    result = copy.deepcopy(data)
    raw = segments(data)
    count = struct.unpack_from("<I", raw[0])[0]
    if len(raw[0]) != 4 + count * 12:
        raise ValueError("Unexpected segmented shader table")
    subshader, = result["m_ParsedForm"]["m_SubShaders"]
    if any(p["m_State"]["m_Name"] == PASS for p in subshader["m_Passes"]):
        raise ValueError("Source already contains the repair")
    original = next(p for p in subshader["m_Passes"] if p["m_State"]["m_Name"] == "DepthOnly")
    motion = copy.deepcopy(original)
    motion["m_State"]["m_Name"] = PASS
    motion["m_State"]["rtBlend0"]["colMask"]["val"] = 12.0
    motion["m_State"]["m_Tags"]["tags"] = [
        (key, PASS if key.upper() == "LIGHTMODE" else value)
        for key, value in motion["m_State"]["m_Tags"]["tags"]]
    new_segment = bytearray()
    new_entries = []
    all_names = motion["m_NameIndices"]

    def name_index(name):
        if any(item[0] == name for item in all_names):
            return next(item[1] for item in all_names if item[0] == name)
        index = max(item[1] for item in all_names) + 1
        all_names.append([name, index])
        return index

    vertex_params = motion["progVertex"]["m_CommonParameters"]
    globals_index = name_index("$Globals")
    globals_buffer = next(c for c in vertex_params["m_ConstantBuffers"] if c["m_NameIndex"] == globals_index)
    if globals_buffer["m_Size"] > CONTROL_REGISTER * 16:
        raise ValueError("Reserved constants overlap original globals")
    globals_buffer["m_Size"] = 265 * 16
    globals_buffer["m_VectorParams"].append(dict(m_NameIndex=name_index("_RSFMotionControl"),
        m_Index=4096, m_ArraySize=0, m_Type=0, m_Dim=4))
    for name, offset in [("_RSFMotionCurrentVP", 4112), ("_RSFMotionPreviousVP", 4176)]:
        globals_buffer["m_MatrixParams"].append(dict(m_NameIndex=name_index(name), m_Index=offset,
            m_ArraySize=0, m_Type=0, m_RowCount=4))
    vertex_params["m_BufferParams"].append(dict(m_NameIndex=name_index("_RSFMotionPrevious"), m_Index=31, m_ArraySize=1))
    vertex_params["m_UAVParams"].append(dict(m_NameIndex=name_index("_RSFMotionCurrent"), m_Index=1, m_OriginalIndex=1))

    for stage, profile in [("progVertex", "vs_5_0"), ("progFragment", "ps_5_0")]:
        variants = motion[stage]["m_PlayerSubPrograms"][3]
        for ordinal, variant in enumerate(variants):
            blob = program(raw, variant["m_BlobIndex"])
            _, start, size = code_range(blob)
            path = output / f"{stage}-{ordinal}.dxbc"
            path.write_bytes(blob[start:start+size])
        subprocess.run([str(decompiler), "-D", *[str(output / f"{stage}-{i}.dxbc") for i in range(len(variants))]],
                       check=True, stdout=subprocess.DEVNULL)
        for ordinal, variant in enumerate(variants):
            path = output / f"{stage}-{ordinal}.hlsl"
            source = wrap_source(path.read_text(), stage == "progVertex")
            generated = path.with_suffix(".patched.hlsl")
            compiled = path.with_suffix(".patched.dxbc")
            previous_source = re.sub(r"^// ---- Created with 3Dmigoto[^\r\n]*\r?\n", "", generated.read_text(), count=1) if generated.exists() else None
            if reuse_compiled and compiled.exists() and previous_source == source:
                code = compiled.read_bytes()
                if code[:4] != b"DXBC" or struct.unpack_from("<I", code, 24)[0] != len(code):
                    raise ValueError(f"Invalid cached program: {compiled}")
            else:
                code = compile_shader(source, profile)
                generated.write_text(source)
                compiled.write_bytes(code)
            blob = replace_code(program(raw, variant["m_BlobIndex"]), code, stage == "progVertex")
            variant["m_BlobIndex"] = count + len(new_entries)
            new_entries.append((len(new_segment), len(blob), len(raw)))
            new_segment.extend(blob)
            new_segment.extend(b"\0" * (-len(new_segment) % 4))
    raw[0] = struct.pack("<I", count + len(new_entries)) + raw[0][4:] + b"".join(struct.pack("<III", *entry) for entry in new_entries)
    raw.append(bytes(new_segment))
    encoded = [CompressionHelper.compress_lz4(chunk) for chunk in raw]
    offsets, cursor = [], 0
    for chunk in encoded:
        offsets.append(cursor)
        cursor += len(chunk)
    result["offsets"] = [offsets]
    result["compressedLengths"] = [[len(chunk) for chunk in encoded]]
    result["decompressedLengths"] = [[len(chunk) for chunk in raw]]
    result["compressedBlob"] = list(b"".join(encoded))
    subshader["m_Passes"].append(motion)
    return result, len(new_entries)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--decompiler", type=Path, required=True)
    parser.add_argument("--source-asset", type=Path, help="Verified original asset backup when the installation is already patched")
    parser.add_argument("--reuse-compiled", action="store_true", help="Reuse compiled variants only when their regenerated HLSL is unchanged")
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(args.game.resolve()):
        raise ValueError("The output must be outside the game installation")
    source = args.source_asset or args.game / "DragNWash_Data/globalgamemanagers.assets"
    data = source.read_bytes()
    if digest(data) != ASSET_SHA256:
        raise ValueError("Refusing an unrecognized game asset fingerprint")
    args.output.mkdir(parents=True, exist_ok=True)
    env = UnityPy.load(str(source))
    target = next(obj for obj in env.objects if obj.type.name == "Shader" and obj.read().m_ParsedForm.m_Name == SHADER)
    original_shader = target.read_typetree()
    patched, programs = build_shader(original_shader, args.decompiler.resolve(), args.output, args.reuse_compiled)
    target.save_typetree(patched)
    rebuilt = target.assets_file.save()
    output = args.output / source.name
    output.write_bytes(rebuilt)
    check = UnityPy.load(str(output))
    before = {o.path_id: o.get_raw_data() for o in UnityPy.load(str(source)).objects}
    changed = [o.path_id for o in check.objects if o.get_raw_data() != before[o.path_id]]
    if changed != [target.path_id]:
        raise ValueError(f"Unexpected changed objects: {changed}")
    checked_shader = next(o.read_typetree() for o in check.objects if o.path_id == target.path_id)
    for before_sub, after_sub in zip(original_shader["m_ParsedForm"]["m_SubShaders"], checked_shader["m_ParsedForm"]["m_SubShaders"]):
        if after_sub["m_Passes"][:-1] != before_sub["m_Passes"]:
            raise ValueError("An original shader pass changed")
    if original_shader["m_ParsedForm"]["m_PropInfo"] != checked_shader["m_ParsedForm"]["m_PropInfo"]:
        raise ValueError("Original material properties changed")
    report = dict(source_sha256=digest(data), patched_sha256=digest(rebuilt), shader=SHADER,
                  pass_name=PASS, programs=programs, changed_objects=changed, output=str(output.resolve()),
                  status="staged; Unity execution and visual acceptance required")
    (args.output / "patch.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
