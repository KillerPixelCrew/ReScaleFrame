"""Regenerate the standalone bridge bindings from the pinned FidelityFX Vulkan SDK.

Stays committed: the FidelityFX SDK checkout is not part of the repository. Pass --check to
compare the committed output instead of writing it.
"""
import json
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tools"))
from committed_bindings import bindgen, emit, finish  # noqa: E402

sdk = Path(sys.argv[1]).resolve()
vulkan = Path(sys.argv[2]).resolve()
revision = subprocess.check_output(["git", "-C", sdk, "rev-parse", "HEAD"], text=True).strip()
pins = json.loads((Path(__file__).resolve().parents[3] / "eng/dependencies.json").read_text(encoding="utf-8"))
if revision != pins["vendor"]["fidelityfx_vulkan"]["revision"]:
    raise SystemExit("Expected the pinned FidelityFX Vulkan SDK 1.1.4 revision")

include = sdk / "ffx-api" / "include"
types = (
    "FfxApi(ReturnCodes|SurfaceFormat|Resource(Usage|State|Type)|BackbufferTransferFunction|"
    "CreateContext(Upscale|Framegeneration)Flags|DispatchFramegenerationFlags|UiCompositionFlags)|"
    "ffxQueryGetProviderVersion|"
    "PfnFfx(CreateContext|DestroyContext|Configure|Query|Dispatch)|PFN_vkGetDeviceProcAddr|"
    "ffxCreateBackendVKDesc|ffxCreateContextDesc(Upscale|FrameGeneration(Hudless|SwapChainVK)?)|"
    "ffxConfigureDesc(FrameGeneration|FrameGenerationSwapChainRegisterUiResourceVK)|"
    "ffxQueryDesc(UpscaleGetRenderResolutionFromQualityMode|SwapchainReplacementFunctionsVK)|"
    "ffxDispatchDesc(Upscale|FrameGeneration(Prepare(CameraInfo)?|SwapChainWaitForPresentsVK)?)"
)
constants = "FFX_API_.*DESC_TYPE_.*"
result = bindgen(
    include / "ffx_api/vk/ffx_api_vk.h",
    "--allowlist-type", types, "--allowlist-var", constants,
    "--blocklist-type", "Vk(StructureType|Format)",
    "--raw-line", "#![allow(dead_code, non_camel_case_types)]",
    "--raw-line", "#![allow(non_snake_case, non_upper_case_globals)]",
    "--raw-line", "type VkStructureType = ::core::ffi::c_int;",
    "--raw-line", "type VkFormat = ::core::ffi::c_int;",
    "--no-prepend-enum-name", "--with-derive-default",
    # The SDK can throw C++ exceptions; the Runtime contains them with
    # rsf_streamline_vk::sdk_api::guarded_or, which needs an unwinding ABI.
    "--override-abi", "C-unwind=PfnFfx(CreateContext|DestroyContext|Configure|Query|Dispatch)",
    clang=["-x", "c++", "-include", "stddef.h", "-include", "ffx_api/ffx_upscale.h",
           "-include", "ffx_api/ffx_framegeneration.h",
           "-I" + str(include), "-I" + str(vulkan / "Include")],
)
emit(Path(__file__).with_name("sdk_abi.rs"), result)
finish()
