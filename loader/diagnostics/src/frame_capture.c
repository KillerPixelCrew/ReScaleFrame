/* SPDX-License-Identifier: MIT */

#include "frame_capture_platform.h"

#include <windows.h>

/* renderdoc_app.h uses bool without including stdbool, so it expects C++ or C23. */
#include <stdbool.h>

#include <renderdoc_app.h>

#define RSF_NVIDIA_VENDOR_ID 0x10DE

static int rsf_capture_connect_api(void* library, const char* output_prefix_utf8,
                                   rsf_capture_calls* calls)
{
    pRENDERDOC_GetAPI get_api =
        (pRENDERDOC_GetAPI)(void*)GetProcAddress((HMODULE)library, "RENDERDOC_GetAPI");
    void* table = NULL;
    if (!get_api || get_api(eRENDERDOC_API_Version_1_6_0, &table) != 1 || !table) {
        return 0;
    }
    RENDERDOC_API_1_6_0* api = (RENDERDOC_API_1_6_0*)table;

    if (output_prefix_utf8) {
        api->SetCaptureFilePathTemplate(output_prefix_utf8);
    }
    api->MaskOverlayBits(eRENDERDOC_Overlay_None, eRENDERDOC_Overlay_None);
    api->SetCaptureKeys(NULL, 0);

    /* RenderDoc blocks vendor extensions by default, which makes nvapi_QueryInterface return null.
       On an NVIDIA hybrid-graphics machine that costs the device at the first present and takes
       the game with it. AC7 loads nvapi64.dll, so this matters here. Community Shaders hit the
       same failure and documented the same fix. */
    api->SetCaptureOptionU32(eRENDERDOC_Option_AllowUnsupportedVendorExtensions,
                             RSF_NVIDIA_VENDOR_ID);
    calls->trigger = api->TriggerCapture;
    calls->trigger_multiple = api->TriggerMultiFrameCapture;
    calls->count = api->GetNumCaptures;
    return 1;
}

rsf_capture_result rsf_capture_initialise(const char* library_path_utf8,
                                          const char* output_prefix_utf8)
{
    return rsf_capture_initialise_impl(library_path_utf8, output_prefix_utf8,
                                       rsf_capture_connect_api);
}
