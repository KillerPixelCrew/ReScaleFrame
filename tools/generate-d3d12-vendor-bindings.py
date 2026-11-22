"""Regenerate D3D12 vendor ABI records from the repository-pinned SDK headers.

Stays committed: the XeSS, FidelityFX and NVAPI headers live in the gitignored vendor/ checkout.
Pass --check to compare the committed outputs instead of writing them.
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from committed_bindings import bindgen, emit, finish  # noqa: E402


def generate(header, pattern, output, aliases, clang_args, constants='NOTHING', *, recursive=False):
    emit(output, bindgen(
        header, "--allowlist-type", pattern,
        "--allowlist-var", constants, *([] if recursive else ["--no-recursive-allowlist"]),
        "--no-prepend-enum-name", "--with-derive-default",
        "--raw-line", "#![allow(dead_code, non_camel_case_types, non_snake_case)]",
        *[item for alias in aliases for item in ("--raw-line", alias)],
        clang=["-DCINTERFACE", "-include", "stdbool.h", *clang_args],
    ))


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xess", type=Path, default=root / "vendor/xess/inc")
    parser.add_argument("--fidelityfx", type=Path, default=root / "vendor/fidelityfx/Kits/FidelityFX")
    parser.add_argument("--vulkan", type=Path, help="Vulkan SDK Include directory for XeSS Vulkan records")
    parser.add_argument("--nvapi", type=Path, help="Regenerate only the NVAPI records from this SDK directory")
    args = parser.parse_args()

    if args.nvapi:
        generate(args.nvapi / "nvapi.h",
                 r"NV_(GET_SLEEP_STATUS|SET_SLEEP_MODE|LATENCY_MARKER|SET_REFLEX_SYNC|ASYNC_FRAME_MARKER)_PARAMS",
                 root / "runtime/orchestrator/src/reflex_nvapi_abi.rs",
                 ["#![allow(non_upper_case_globals, clippy::useless_transmute)]"],
                 ["-include", "sal.h"],
                 recursive=True)
        finish()
        return

    aliases = [f"type ID3D12{name} = ::core::ffi::c_void;"
               for name in ("Resource", "DescriptorHeap", "Heap", "PipelineLibrary")]
    generate(args.xess / "xess/xess_d3d12.h",
             r"_?xess_(2d|coord|d3d12_init_params|d3d12_execute_params|quality_settings|init_flags|version|result)_t",
             root / "runtime/orchestrator/src/d3d12_sr/xess_abi.rs", aliases, [])


    generate(args.xess / "xess_fg/xefg_swapchain_d3d12.h",
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
    if args.vulkan:
        generate(args.xess / "xess/xess_vk.h",
                 r"_?xess_(vk_(init_params_t|execute_params_t|image_view_info)|2d_t|coord_t|quality_settings_t|version_t)|"
                 r"VkImageSubresourceRange",
                 root / "runtime/orchestrator/src/vulkan_sr/xess_sdk_abi.rs",
                 [f"type Vk{name} = *mut ::core::ffi::c_void;"
                  for name in ("Image", "ImageView", "DeviceMemory", "PipelineCache")]
                 + ["type VkFormat = i32;", "type VkImageAspectFlags = u32;"],
                 ["-x", "c++", "-std=c++20", "-DVK_NO_PROTOTYPES", f"-I{args.vulkan}"])
    api = args.fidelityfx / "api/include"
    generate(root / "runtime/backends/fsr/src/ffx_descriptor_types.h",
             r"ffx(ApiHeader|StructType_t|ApiMessage|Context|ReturnCode_t|AllocationCallbacks|Alloc|Dealloc|"
             r"CreateContextDescHeader|DispatchDescHeader|QueryDescHeader|QueryDescGetVersions|OverrideVersion|"
             r"QueryGetProviderVersion|CreateBackendDX12Desc|CreateContextDescUpscale|DispatchDescUpscale|"
             r"QueryDescUpscaleGetRenderResolutionFromQualityMode)|PfnFfx(CreateContext|DestroyContext|Query|Dispatch)|"
             r"rsf_ffx_descriptor_type|FfxApi(Resource|ResourceDescription|Dimensions2D|FloatCoords2D|ReturnCodes|CreateContextUpscaleFlags|ResourceState|SurfaceFormat|ResourceType|ResourceUsage)",
             root / "runtime/orchestrator/src/d3d12_sr/fsr_abi.rs",
             ["type ID3D12Device = ::core::ffi::c_void;", "type wchar_t = u16;"],
             [f"-I{api}", f"-I{args.fidelityfx / 'upscalers/include'}"],
             r"FFX_API_(CREATE_CONTEXT_DESC_TYPE_(BACKEND_DX12|UPSCALE)|DISPATCH_DESC_TYPE_UPSCALE|"
             r"QUERY_DESC_TYPE_(GET_VERSIONS|GET_PROVIDER_VERSION|UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE)|"
             r"DESC_TYPE_OVERRIDE_VERSION)")

    generate(root / "runtime/backends/fsr/src/ffx_generation_descriptor_types.h",
             r"ffx(CreateContextDescFrameGeneration(Version|SwapChain(ForHwnd|Version)DX12)?|DispatchDescFrameGeneration(PrepareV2)?|"
             r"CallbackDescFrameGenerationPresent|ConfigureDescFrameGeneration|ConfigureDescHeader)|"
             r"PfnFfxConfigure|FfxApi(Rect2D|PresentCallbackFunc|FrameGenerationDispatchFunc)|rsf_ffx_fg_descriptor_type",
             root / "runtime/orchestrator/src/d3d12_generation/fsr_abi.rs",
             ["use crate::d3d12_sr::ffx_abi::*;",
              "use windows::Win32::Graphics::Dxgi::{DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FULLSCREEN_DESC};",
              "type HWND = *mut ::core::ffi::c_void;",
              *[f"type {name} = ::core::ffi::c_void;" for name in ("IDXGISwapChain4", "IDXGIFactory", "ID3D12CommandQueue")]],
             [f"-I{api}", f"-I{args.fidelityfx / 'framegeneration/include'}"],
             r"FFX_FRAMEGENERATION(_SWAPCHAIN_DX12)?_VERSION_(MAJOR|MINOR|PATCH)")
    finish()


if __name__ == "__main__":
    main()
