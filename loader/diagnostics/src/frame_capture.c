/* SPDX-License-Identifier: GPL-3.0-only */

#include <rescaleframe/frame_capture.h>

#include <windows.h>

/* renderdoc_app.h uses bool without including stdbool, so it expects C++ or C23. */
#include <stdbool.h>

#include <renderdoc_app.h>

#define RSF_NVIDIA_VENDOR_ID 0x10DE

static RENDERDOC_API_1_6_0* api;

rsf_capture_result rsf_capture_initialise(const char* library_path_utf8,
                                          const char* output_prefix_utf8)
{
    if (!library_path_utf8) {
        return RSF_CAPTURE_ERROR_INVALID_ARGUMENT;
    }
    if (api) {
        return RSF_CAPTURE_OK;
    }

    wchar_t path[MAX_PATH * 2];
    if (MultiByteToWideChar(CP_UTF8, 0, library_path_utf8, -1, path, MAX_PATH * 2) == 0) {
        return RSF_CAPTURE_ERROR_INVALID_ARGUMENT;
    }
    /* Load only from the path we were given. Falling back to the search path could pick up an
       unrelated build and make a failure hard to explain. */
    HMODULE library = LoadLibraryW(path);
    if (!library) {
        return RSF_CAPTURE_ERROR_LIBRARY_MISSING;
    }

    pRENDERDOC_GetAPI get_api =
        (pRENDERDOC_GetAPI)(void*)GetProcAddress(library, "RENDERDOC_GetAPI");
    if (!get_api) {
        return RSF_CAPTURE_ERROR_API_MISSING;
    }
    if (get_api(eRENDERDOC_API_Version_1_6_0, (void**)&api) != 1 || !api) {
        api = NULL;
        return RSF_CAPTURE_ERROR_API_MISSING;
    }

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
    return RSF_CAPTURE_OK;
}

rsf_capture_result rsf_capture_trigger(uint32_t frames)
{
    if (!api) {
        return RSF_CAPTURE_ERROR_NOT_READY;
    }
    if (frames <= 1) {
        api->TriggerCapture();
    } else {
        api->TriggerMultiFrameCapture(frames);
    }
    return RSF_CAPTURE_OK;
}

uint32_t rsf_capture_count(void)
{
    return api ? api->GetNumCaptures() : 0u;
}
