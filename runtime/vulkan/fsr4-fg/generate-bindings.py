"""Regenerate the FSR4 frame-generation SDK records used by the Rust bridge.

Stays committed: the FidelityFX SDK checkout is not part of the repository. Pass --check to
compare the committed output instead of writing it.
"""
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tools"))
from committed_bindings import bindgen, emit, finish  # noqa: E402

sdk = Path(sys.argv[1]).resolve()
revision = subprocess.check_output(["git", "-C", sdk, "rev-parse", "HEAD"], text=True).strip()
if revision != "7539ae8b6568ad29ed78a753404351daf3b5cca7":
    raise SystemExit("Expected the pinned FidelityFX SDK 2.4.0 revision")

include = sdk / "Kits" / "FidelityFX"
result = bindgen(
    include / "framegeneration/include/ffx_framegeneration.h",
    "--allowlist-type", "PfnFfx(CreateContext|DestroyContext|Query|Dispatch|Configure)|ffxQueryDescGetVersions|ffxQueryGetProviderVersion|"
    "ffxDispatchDescFrameGenerationPrepareV2|ffxOverrideVersion|"
    "ffxCreateContextDescFrameGeneration(Version)?|ffxConfigureDescFrameGeneration|"
    "FfxApiCreateContextFramegenerationFlags|FfxApiResourceState",
    "--allowlist-var", "FFX_API_QUERY_DESC_TYPE_GET_(VERSIONS|PROVIDER_VERSION)|"
    "FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION(_VERSION)?|"
    "FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION|FFX_API_DESC_TYPE_OVERRIDE_VERSION|"
    "FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2",
    "--no-prepend-enum-name", "--with-derive-default",
    "--raw-line", "#![allow(dead_code, non_camel_case_types)]",
    "--raw-line", "#![allow(non_snake_case, non_upper_case_globals)]",
    clang=["-x", "c++", "-include", "stddef.h", "-I" + str(include / "api/include")],
)
emit(Path(__file__).with_name("sdk_abi.rs"), result)


directory = Path(__file__).resolve().parent
result = bindgen(
    directory / "bridge_objects.h",
    "--allowlist-type", "^rsf_fsr4fg_native_graphics$",
    "--blocklist-type", "^IDXGISwapChain4$",
    "--raw-line", "#![allow(non_camel_case_types)]",
    "--raw-line", "type IDXGISwapChain4 = ::core::ffi::c_void;",
    clang=["-x", "c++"],
)
emit(directory / "bridge_objects_abi.rs", result)
finish()
