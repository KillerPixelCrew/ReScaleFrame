"""Regenerate D3D12 vendor ABI records from the repository-pinned SDK headers."""
import argparse
import os
import subprocess
from pathlib import Path


def generate(bindgen, header, pattern, output, aliases, clang_args, constants='NOTHING'):
    subprocess.run([
        bindgen, str(header), "--output", str(output), "--allowlist-type", pattern,
        "--allowlist-var", constants, "--no-recursive-allowlist", "--no-prepend-enum-name", "--with-derive-default",
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
    parser.add_argument("--fidelityfx", type=Path, default=root / "vendor/fidelityfx/Kits/FidelityFX")
    args = parser.parse_args()
    bindgen = os.environ.get("BINDGEN", "bindgen")
    if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
        raise SystemExit("Install bindgen-cli 0.72.1 to regenerate the vendor ABI records")

    aliases = [f"type ID3D12{name} = ::core::ffi::c_void;"
               for name in ("Resource", "DescriptorHeap", "Heap", "PipelineLibrary")]
    generate(bindgen, args.xess / "xess/xess_d3d12.h",
             r"_?xess_(2d|coord|d3d12_init_params|d3d12_execute_params|quality_settings|init_flags|version|result)_t",
             root / "runtime/orchestrator/src/d3d12_sr/xess_abi.rs", aliases, [])


    generate(bindgen, args.xess / "xess_fg/xefg_swapchain_d3d12.h",
             r"_?xefg_swapchain_(d3d12_(resource_data|init_params)|init_flags|ui_mode|version|2d|frame_constant_data|properties|"
             r"present_status|result|resource_type|resource_validity)_t|xefg_swapchain_ui_composition_state_t|"
             r"_?xell_sleep_params_t",
             root / "runtime/orchestrator/src/d3d12_generation/xess_abi.rs",
             ["#![allow(clippy::useless_transmute)]",
              "type ID3D12Resource = ::core::ffi::c_void;", "type D3D12_RESOURCE_STATES = u32;",
              "type ID3D12Heap = ::core::ffi::c_void;", "type ID3D12PipelineLibrary = ::core::ffi::c_void;",
              "type IDXGISwapChain = ::core::ffi::c_void;"],
             ["-x", "c++", "-std=c++20", "-include", "d3d12.h", "-include", "dxgi1_5.h",
              "-include", str(args.xess / "xell/xell.h")])
    api = args.fidelityfx / "api/include"
    generate(bindgen, root / "runtime/backends/fsr/src/ffx_descriptor_types.h",
             r"ffx(ApiHeader|StructType_t|ApiMessage|Context|ReturnCode_t|AllocationCallbacks|Alloc|Dealloc|"
             r"CreateContextDescHeader|DispatchDescHeader|QueryDescHeader|QueryDescGetVersions|OverrideVersion|"
             r"QueryGetProviderVersion|CreateBackendDX12Desc|CreateContextDescUpscale|DispatchDescUpscale|"
             r"QueryDescUpscaleGetRenderResolutionFromQualityMode)|PfnFfx(CreateContext|DestroyContext|Query|Dispatch)|"
             r"rsf_ffx_descriptor_type|FfxApi(Resource|ResourceDescription|Dimensions2D|FloatCoords2D|ReturnCodes|CreateContextUpscaleFlags|ResourceState)",
             root / "runtime/orchestrator/src/d3d12_sr/fsr_abi.rs",
             ["type ID3D12Device = ::core::ffi::c_void;", "type wchar_t = u16;"],
             [f"-I{api}", f"-I{args.fidelityfx / 'upscalers/include'}"],
             r"FFX_API_(CREATE_CONTEXT_DESC_TYPE_(BACKEND_DX12|UPSCALE)|DISPATCH_DESC_TYPE_UPSCALE|"
             r"QUERY_DESC_TYPE_(GET_VERSIONS|GET_PROVIDER_VERSION|UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE)|"
             r"DESC_TYPE_OVERRIDE_VERSION)")

    generate(bindgen, root / "runtime/backends/fsr/src/ffx_generation_descriptor_types.h",
             r"ffx(CreateContextDescFrameGeneration(Version)?|DispatchDescFrameGeneration(PrepareV2)?|"
             r"CallbackDescFrameGenerationPresent|ConfigureDescFrameGeneration|ConfigureDescHeader)|"
             r"PfnFfxConfigure|FfxApi(Rect2D|PresentCallbackFunc|FrameGenerationDispatchFunc)|rsf_ffx_fg_descriptor_type",
             root / "runtime/orchestrator/src/d3d12_generation/fsr_abi.rs",
             ["use crate::d3d12_sr::ffx_abi::*;"],
             [f"-I{api}", f"-I{args.fidelityfx / 'framegeneration/include'}"])


if __name__ == "__main__":
    main()
