"""Regenerate D3D12 vendor ABI records from the repository-pinned SDK headers."""
import argparse
import os
import subprocess
from pathlib import Path


def generate(bindgen, header, pattern, output, aliases, clang_args):
    subprocess.run([
        bindgen, str(header), "--output", str(output), "--allowlist-type", pattern,
        "--no-recursive-allowlist", "--no-prepend-enum-name", "--with-derive-default",
        "--no-derive-debug", "--no-doc-comments", "--use-core",
        "--rust-edition", "2024", "--rust-target", "1.85",
        "--raw-line", "#![allow(dead_code, non_camel_case_types, non_snake_case)]",
        *[item for alias in aliases for item in ("--raw-line", alias)],
        "--", "--target=x86_64-pc-windows-msvc", "-DCINTERFACE", "-include", "stdbool.h",
        *clang_args,
    ], check=True)
    output.write_bytes(output.read_bytes().replace(b"\r\n", b"\n"))


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xess", type=Path, default=root / "vendor/xess/inc")
    args = parser.parse_args()
    bindgen = os.environ.get("BINDGEN", "bindgen")
    if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
        raise SystemExit("Install bindgen-cli 0.72.1 to regenerate the vendor ABI records")

    aliases = [f"type ID3D12{name} = ::core::ffi::c_void;"
               for name in ("Resource", "DescriptorHeap", "Heap", "PipelineLibrary")]
    generate(bindgen, args.xess / "xess/xess_d3d12.h",
             r"_?xess_(2d|coord|d3d12_init_params|d3d12_execute_params|quality_settings|init_flags|version|result)_t",
             root / "runtime/orchestrator/src/d3d12_sr/xess_abi.rs", aliases, [])


if __name__ == "__main__":
    main()
