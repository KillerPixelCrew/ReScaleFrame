"""Regenerate the FSR4 Vulkan SR SDK descriptor projection.

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
    include / "upscalers/include/ffx_upscale.h",
    "--allowlist-type", "PfnFfx(CreateContext|DestroyContext|Dispatch|Query)|ffxQuery(DescGetVersions|GetProviderVersion)|"
    "ffxOverrideVersion|ffx(CreateContextDescUpscale(Version)?|DispatchDescUpscale|"
    "QueryDescUpscaleGetRenderResolutionFromQualityMode)|FfxApiCreateContextUpscaleFlags|FfxApiResourceState",
    "--allowlist-var", "FFX_API_.*DESC_TYPE_(UPSCALE(_VERSION|_GETRENDERRESOLUTIONFROMQUALITYMODE)?|"
    "GET_VERSIONS|GET_PROVIDER_VERSION|OVERRIDE_VERSION)",
    "--no-prepend-enum-name", "--with-derive-default",
    "--raw-line", "#![allow(dead_code, non_camel_case_types)]",
    "--raw-line", "#![allow(non_snake_case, non_upper_case_globals)]",
    clang=["-x", "c++", "-include", "stddef.h", "-I" + str(include / "api/include")],
)
emit(Path(__file__).with_name("sdk_abi.rs"), result)
finish()
