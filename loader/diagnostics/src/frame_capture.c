/* SPDX-License-Identifier: GPL-3.0-only */

#include <rescaleframe/frame_capture.h>

#include <windows.h>

/* renderdoc_app.h uses bool without including stdbool, so it expects C++ or C23. */
#include <stdbool.h>

#include <renderdoc_app.h>

#define RSF_NVIDIA_VENDOR_ID 0x10DE

/* Borrowed API table backed by a deliberately retained loaded RenderDoc DLL. Initialization
   publishes this once; the module has no teardown or synchronization for concurrent setup. */
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
    /* Use the supplied library path without a fallback search attempt. */
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

    /* Allow NVIDIA extensions: the observed hybrid-device path requires nvapi on first
       Present. This follows Community Shaders' documented RenderDoc workaround. */
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
