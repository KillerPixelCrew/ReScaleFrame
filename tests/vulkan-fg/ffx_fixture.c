/* SPDX-License-Identifier: MIT */
/* The FidelityFX scenarios drive the production Rust SDK leaf with a fake SDK
   module and fake Vulkan entries (src/ffx_sdk.rs, src/ffx_gpu.rs). The provider
   entries and exception guards are the Runtime archive's (vulkan_ffx_fg.rs); this
   launcher keeps capture and timing output disabled. */
#include "ffx_abi.h"
#include <stdlib.h>
#include <windows.h>

int main(int argc, char** argv)
{
    if (argc != 2) return 1;
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", NULL);
    SetEnvironmentVariableW(L"RSF_VULKAN_TIMING_FILE", NULL);
    return rsf_fixture_ffx_run((uint32_t)strtoul(argv[1], NULL, 10));
}
