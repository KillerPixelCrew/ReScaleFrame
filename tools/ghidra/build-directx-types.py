"""Build DirectX type information for Ghidra from mingw-w64 headers.

Produces a flattened header, a COM vtable slot table, and optionally a Ghidra .gdt archive.
Nothing here touches a game binary. The headers are read from the local mingw-w64 sysroot and
are not vendored into this repository.
"""

import argparse
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import venv
from pathlib import Path


DEFAULT_HEADERS = ["d3d11_4.h", "d3d11on12.h", "d3d12.h", "dxgi1_6.h", "dxgidebug.h"]

# Interfaces a hook or presentation path actually touches. The generated header covers these so
# runtime code can name a vtable slot instead of repeating a magic index.
SHIM_INTERFACES = [
    "IUnknown", "IDXGIFactory", "IDXGIFactory1", "IDXGIFactory2", "IDXGIFactory4",
    "IDXGISwapChain", "IDXGISwapChain1", "IDXGISwapChain3", "IDXGISwapChain4",
    "IDXGIAdapter", "IDXGIDevice", "IDXGIOutput",
    "ID3D11Device", "ID3D11DeviceContext", "ID3D11Texture2D",
    "ID3D12Device", "ID3D12CommandQueue", "ID3D12GraphicsCommandList", "ID3D12Fence",
]
DEFAULT_SYSROOT = Path("/usr/x86_64-w64-mingw32/include")
LANGUAGE_ID = "x86:LE:64:default"
COMPILER_SPEC_ID = "windows"

# Ghidra's C parser rejects GCC extension syntax that survives preprocessing.
DROP_ATTRIBUTES = ("__attribute__", "__extension__", "__declspec")
DROP_KEYWORDS = ("__restrict__", "__restrict", "__inline__", "__forceinline", "__unaligned")

# Compiler builtins have no declaration to preprocess, so give them a parseable spelling.
BUILTIN_DEFINES = ("__builtin_va_list=char *", "__builtin_ms_va_list=char *",
                   "__MINGW_EXTENSION=", "__MINGW_ATTRIB_NONNULL(x)=",
                   "_Float16=short", "__bf16=short", "__float128=long double",
                   "_Complex=", "_Imaginary=", "__complex__=")

VTABLE_PATTERN = re.compile(r"typedef struct (\w+)Vtbl \{(.*?)\n\} \1Vtbl;", re.DOTALL)
METHOD_PATTERN = re.compile(r"\(\s*\*\s*(\w+)\s*\)")
UUID_PATTERN = re.compile(r"__CRT_UUID_DECL\(\s*(\w+)\s*,([^)]*)\)")


def find_compiler(explicit):
    """Return a command prefix that preprocesses Windows headers for x86-64."""
    if explicit:
        return [explicit]
    for name in ("x86_64-w64-mingw32-gcc", "x86_64-w64-mingw32-clang", "x86_64-w64-mingw32-cpp"):
        if shutil.which(name):
            return [name]
    if shutil.which("clang"):
        return ["clang", "--target=x86_64-w64-windows-gnu"]
    raise SystemExit("No mingw-w64 capable preprocessor found. Install mingw-w64-gcc or clang.")


def strip_extensions(text):
    """Remove GCC attribute syntax, keeping the balanced parentheses accounting correct."""
    for keyword in DROP_KEYWORDS:
        text = re.sub(rf"\b{re.escape(keyword)}\b", "", text)
    for keyword in DROP_ATTRIBUTES:
        while True:
            start = text.find(keyword + " ((")
            if start < 0:
                start = text.find(keyword + "((")
            if start < 0:
                break
            cursor = text.index("(", start)
            depth = 0
            for position in range(cursor, len(text)):
                if text[position] == "(":
                    depth += 1
                elif text[position] == ")":
                    depth -= 1
                    if depth == 0:
                        text = text[:start] + text[position + 1:]
                        break
            else:
                raise ValueError(f"Unbalanced {keyword} at offset {start}")
    return text


def strip_function_bodies(text):
    """Reduce inline definitions to declarations. Ghidra's parser cannot read bodies or asm."""
    out = []
    depth = 0
    index = 0
    while index < len(text):
        char = text[index]
        if char in "\"'":
            end = index + 1
            while end < len(text) and text[end] != char:
                end += 2 if text[end] == "\\" else 1
            if depth == 0:
                out.append(text[index:end + 1])
            index = end + 1
            continue
        if char == "{":
            if depth == 0:
                previous = next((value for value in reversed(out) if not value.isspace()), "")
                if previous.endswith(")"):
                    depth = 1
                    out.append(";")
                    index += 1
                    continue
            elif depth:
                depth += 1
        elif char == "}" and depth:
            depth -= 1
            index += 1
            continue
        if depth == 0:
            out.append(char)
        index += 1
    return "".join(out)


def preprocess(compiler, sysroot, headers, output):
    """Flatten the selected headers into one translation unit Ghidra can parse."""
    umbrella = output.parent / "directx-umbrella.c"
    umbrella.write_text("".join(f"#include <{name}>\n" for name in headers), encoding="utf-8")
    command = compiler + ["-E", "-P", f"-I{sysroot}"]
    command += [f"-D{define}" for define in BUILTIN_DEFINES] + [str(umbrella)]
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    if result.returncode:
        raise SystemExit(f"Preprocessing failed:\n{result.stderr.strip()[:4000]}")
    text = strip_function_bodies(strip_extensions(result.stdout))
    output.write_text(text, encoding="utf-8")
    return {"command": " ".join(command), "lines": text.count("\n"),
            "warnings": result.stderr.count("warning:")}


def extract_vtables(text):
    """Map each COM interface to its flattened vtable slots, in declaration order."""
    interfaces = {}
    for match in VTABLE_PATTERN.finditer(text):
        name, body = match.group(1), match.group(2)
        methods = []
        depth = 0
        for position, char in enumerate(body):
            if char == "(":
                # A member is a function pointer declared at the top level of the struct body.
                # Parameters can be function pointers too, so only depth zero counts.
                found = depth == 0 and METHOD_PATTERN.match(body, position)
                if found:
                    methods.append({"index": len(methods), "offset": hex(len(methods) * 8),
                                    "name": found.group(1)})
                depth += 1
            elif char == ")":
                depth -= 1
        if methods:
            interfaces[name] = methods
    return interfaces


def extract_iids(sysroot):
    """Collect interface IDs as they appear in memory, so they can be matched inside a binary.

    The declarations sit in the C++ half of the mingw headers, so they survive only in the
    original text and have to be read before preprocessing.
    """
    iids = {}
    for path in sorted(sysroot.glob("*.h")):
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for match in UUID_PATTERN.finditer(text):
            name = match.group(1)
            parts = [part.strip() for part in match.group(2).split(",") if part.strip()]
            if len(parts) != 11:
                continue
            try:
                values = [int(part, 16) for part in parts]
            except ValueError:
                continue
            packed = struct.pack("<IHH", values[0], values[1], values[2]) + bytes(values[3:])
            iids[packed.hex()] = {
                "interface": name,
                "guid": f"{values[0]:08x}-{values[1]:04x}-{values[2]:04x}-"
                        f"{values[3]:02x}{values[4]:02x}-" + "".join(f"{v:02x}" for v in values[5:]),
                "header": path.name,
            }
    return iids


def write_slots_header(path, interfaces, iids):
    """Emit vtable slot indices and interface IDs for in-process code to use directly.

    A hook needs the same numbers this tooling derives, so generate them once instead of
    repeating literal indices in the runtime.
    """
    lines = ["/* Generated by tools/ghidra/build-directx-types.py. Do not edit. */",
             "#ifndef RSF_DIRECTX_SLOTS_H", "#define RSF_DIRECTX_SLOTS_H", "",
             "/* Vtable slot indices, not byte offsets. */"]
    by_interface = {value["interface"]: key for key, value in iids.items()}
    for name in SHIM_INTERFACES:
        methods = interfaces.get(name)
        if not methods:
            continue
        lines.append(f"\n/* {name} */")
        for method in methods:
            lines.append(f"#define RSF_VTBL_{name}_{method['name']} {method['index']}")
        packed = by_interface.get(name)
        if packed:
            body = ", ".join(f"0x{packed[index:index + 2]}" for index in range(0, 32, 2))
            lines.append(f"#define RSF_IID_{name} {{ {body} }}")
    lines += ["", "#endif /* RSF_DIRECTX_SLOTS_H */", ""]
    path.write_text("\n".join(lines), encoding="utf-8")
    return sum(1 for name in SHIM_INTERFACES if name in interfaces)


def build_gdt(header, output, ghidra_home, interfaces):
    """Parse the flattened header into a .gdt archive using Ghidra's own C parser."""
    import pyghidra

    pyghidra.start(install_dir=ghidra_home)
    from ghidra.app.util.cparser.C import CParserUtils
    from ghidra.util.task import ConsoleTaskMonitor
    from java.lang import String
    import jpype

    if output.exists():
        output.unlink()
    empty = jpype.JArray(String)(0)
    archive = CParserUtils.parseHeaderFiles(
        None, jpype.JArray(String)([str(header)]), empty, empty,
        str(output), LANGUAGE_ID, COMPILER_SPEC_ID, ConsoleTaskMonitor())
    count = sum(1 for _ in archive.getAllDataTypes())
    result = {"data_types": count, "verified_vtables": 0, "mismatched_vtables": []}
    for name, methods in interfaces.items():
        parsed = archive.getDataType(f"/directx.h/{name}Vtbl")
        if parsed is None:
            result["mismatched_vtables"].append({"interface": name, "problem": "absent"})
        elif parsed.getLength() != len(methods) * 8:
            result["mismatched_vtables"].append(
                {"interface": name, "expected": len(methods) * 8, "parsed": parsed.getLength()})
        else:
            result["verified_vtables"] += 1
    archive.save()
    archive.close()
    return result


def bootstrap_pyghidra(ghidra_home, venv_dir):
    """Install Ghidra's bundled pyghidra wheels into a private venv and return its interpreter."""
    scripts = "Scripts" if os.name == "nt" else "bin"
    suffix = ".exe" if os.name == "nt" else ""
    interpreter = venv_dir / scripts / f"python{suffix}"
    if not interpreter.exists():
        distribution = ghidra_home / "Ghidra" / "Features" / "PyGhidra" / "pypkg" / "dist"
        if not distribution.is_dir():
            raise SystemExit(f"No bundled PyGhidra distribution under {distribution}")
        print(f"Creating {venv_dir} from {distribution}")
        venv.EnvBuilder(with_pip=True).create(venv_dir)
        subprocess.run([str(interpreter), "-m", "pip", "install", "--quiet", "--no-index",
                        "--find-links", str(distribution), "pyghidra"], check=True)
    return interpreter


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="output directory, keep it untracked")
    parser.add_argument("--sysroot", type=Path, default=DEFAULT_SYSROOT,
                        help="mingw-w64 include directory")
    parser.add_argument("--header", action="append", dest="headers", default=None,
                        help="header to include, repeatable, defaults to the DX11/DX12/DXGI set")
    parser.add_argument("--compiler", default=None, help="preprocessor command to use")
    parser.add_argument("--ghidra-home", type=Path, default=os.environ.get("GHIDRA_INSTALL_DIR"),
                        help="Ghidra installation, required for --gdt")
    parser.add_argument("--gdt", action="store_true", help="also build a Ghidra .gdt archive")
    parser.add_argument("--no-bootstrap", action="store_true",
                        help="fail instead of creating a pyghidra venv")
    args = parser.parse_args()

    headers = args.headers or DEFAULT_HEADERS
    if not args.sysroot.is_dir():
        raise SystemExit(f"No mingw-w64 headers at {args.sysroot}. Install mingw-w64-headers.")
    missing = [name for name in headers if not (args.sysroot / name).exists()]
    if missing:
        raise SystemExit(f"Missing headers in {args.sysroot}: {', '.join(missing)}")

    args.output.mkdir(parents=True, exist_ok=True)
    header = args.output / "directx.h"
    report = {"sysroot": str(args.sysroot), "headers": headers}
    report["preprocess"] = preprocess(find_compiler(args.compiler), args.sysroot, headers, header)

    interfaces = extract_vtables(header.read_text(encoding="utf-8"))
    iids = extract_iids(args.sysroot)
    vtables = args.output / "directx-vtables.json"
    vtables.write_text(json.dumps({"source": report, "interfaces": interfaces, "iids": iids},
                                  indent=2) + "\n", encoding="utf-8")
    report["interfaces"] = len(interfaces)
    report["iids"] = len(iids)
    report["slots_header"] = write_slots_header(args.output / "directx-slots.h", interfaces, iids)

    if args.gdt:
        if not args.ghidra_home:
            raise SystemExit("--gdt needs --ghidra-home or GHIDRA_INSTALL_DIR")
        ghidra_home = Path(args.ghidra_home)
        try:
            import pyghidra  # noqa: F401
        except ImportError:
            if args.no_bootstrap:
                raise SystemExit("pyghidra is not importable and bootstrapping is disabled")
            interpreter = bootstrap_pyghidra(ghidra_home, args.output / "pyghidra-venv")
            os.execv(str(interpreter), [str(interpreter), os.path.abspath(__file__),
                                        *sys.argv[1:], "--no-bootstrap"])
        report["gdt"] = build_gdt(header, args.output / "directx.gdt", ghidra_home, interfaces)

    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
