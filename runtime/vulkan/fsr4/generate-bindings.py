"""Regenerate the FSR4 Vulkan SR SDK descriptor projection."""
import os
from pathlib import Path
import subprocess
import sys

sdk = Path(sys.argv[1]).resolve()
revision = subprocess.check_output(["git", "-C", sdk, "rev-parse", "HEAD"], text=True).strip()
if revision != "7539ae8b6568ad29ed78a753404351daf3b5cca7":
    raise SystemExit("Expected the pinned FidelityFX SDK 2.4.0 revision")
bindgen = os.environ.get("BINDGEN", "bindgen")
if subprocess.check_output([bindgen, "--version"], text=True).strip() != "bindgen 0.72.1":
    raise SystemExit("Use bindgen-cli 0.72.1 to regenerate the FSR4 SR bindings")

include = sdk / "Kits" / "FidelityFX"
result = subprocess.check_output([
    bindgen, str(include / "upscalers/include/ffx_upscale.h"),
    "--allowlist-type", "PfnFfx(CreateContext|Dispatch|Query)|ffxQuery(DescGetVersions|GetProviderVersion)|"
    "ffxOverrideVersion|ffx(CreateContextDescUpscale(Version)?|DispatchDescUpscale|"
    "QueryDescUpscaleGetRenderResolutionFromQualityMode)|FfxApiCreateContextUpscaleFlags|FfxApiResourceState",
    "--allowlist-var", "FFX_API_.*DESC_TYPE_(UPSCALE(_VERSION|_GETRENDERRESOLUTIONFROMQUALITYMODE)?|"
    "GET_VERSIONS|GET_PROVIDER_VERSION|OVERRIDE_VERSION)",
    "--no-prepend-enum-name", "--with-derive-default", "--no-derive-debug", "--no-doc-comments",
    "--use-core", "--rust-edition", "2024", "--rust-target", "1.85",
    "--raw-line", "#![allow(dead_code, non_camel_case_types)]",
    "--raw-line", "#![allow(non_snake_case, non_upper_case_globals)]",
    "--", "-x", "c++", "--target=x86_64-pc-windows-msvc",
    "-include", "stddef.h", "-I" + str(include / "api/include"),
])
Path(__file__).with_name("sdk_abi.rs").write_bytes(result.replace(b"\r\n", b"\n"))
