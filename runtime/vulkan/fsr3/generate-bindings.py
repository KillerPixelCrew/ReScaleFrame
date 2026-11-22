"""Regenerate the standalone bridge bindings from the pinned FidelityFX Vulkan SDK."""
import os
from pathlib import Path
import subprocess
import sys

sdk = Path(sys.argv[1]).resolve()
vulkan = Path(sys.argv[2]).resolve()
revision = subprocess.check_output(["git", "-C", sdk, "rev-parse", "HEAD"], text=True).strip()
if revision != "c6efa6bf7f2027b3ec94f28578bb5965eabb9e55":
    raise SystemExit("Expected the pinned FidelityFX Vulkan SDK 1.1.4 revision")
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Use bindgen-cli 0.72.1 to regenerate the FSR3 bindings")

include = sdk / "ffx-api" / "include"
types = (
    "FfxApi(ReturnCodes|SurfaceFormat|Resource(Usage|State|Type)|BackbufferTransferFunction|"
    "CreateContextUpscaleFlags|DispatchFramegenerationFlags|UiCompositionFlags)|"
    "PfnFfx(CreateContext|DestroyContext|Configure|Query|Dispatch)|PFN_vkGetDeviceProcAddr|"
    "ffxCreateBackendVKDesc|ffxCreateContextDesc(Upscale|FrameGeneration(Hudless|SwapChainVK)?)|"
    "ffxConfigureDesc(FrameGeneration|FrameGenerationSwapChainRegisterUiResourceVK)|"
    "ffxQueryDesc(UpscaleGetRenderResolutionFromQualityMode|SwapchainReplacementFunctionsVK)|"
    "ffxDispatchDesc(Upscale|FrameGeneration(Prepare(CameraInfo)?|SwapChainWaitForPresentsVK)?)"
)
constants = "FFX_API_.*DESC_TYPE_.*"
result = subprocess.check_output([
    bindgen, str(include / "ffx_api/vk/ffx_api_vk.h"),
    "--allowlist-type", types, "--allowlist-var", constants,
    "--blocklist-type", "Vk(StructureType|Format)",
    "--raw-line", "#![allow(dead_code, non_camel_case_types, non_snake_case, non_upper_case_globals)]",
    "--raw-line", "type VkStructureType = ::core::ffi::c_int;",
    "--raw-line", "type VkFormat = ::core::ffi::c_int;",
    "--no-prepend-enum-name", "--with-derive-default", "--no-derive-debug", "--no-doc-comments",
    "--use-core", "--rust-edition", "2024", "--rust-target", "1.85",
    "--", "-x", "c++", "--target=x86_64-pc-windows-msvc",
    "-include", "stddef.h", "-include", "ffx_api/ffx_upscale.h",
    "-include", "ffx_api/ffx_framegeneration.h",
    "-I" + str(include), "-I" + str(vulkan / "Include"),
])
Path(__file__).with_name("sdk_abi.rs").write_bytes(result.replace(b"\r\n", b"\n"))
