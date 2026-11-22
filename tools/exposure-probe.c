/* SPDX-License-Identifier: MIT */
/* CRT entry point and optional RenderDoc C API for the standalone precision probe. */
#include <stdint.h>
#ifdef RSF_PROBE_RENDERDOC
#include <windows.h>
#include <stdbool.h>
#include <renderdoc_app.h>
#include <stdio.h>
#endif

#ifdef RSF_PROBE_RENDERDOC
static RENDERDOC_API_1_6_0* capture;
#endif

int32_t rsf_probe_capture_open(const char* prefix)
{
#ifdef RSF_PROBE_RENDERDOC
    wchar_t capture_library[1024] = {0};
    const DWORD size = GetEnvironmentVariableW(L"RSF_PROBE_RENDERDOC", capture_library, 1024);
    if (size == 0) return 1;
    if (size >= 1024) return 0;
    HMODULE library = LoadLibraryW(capture_library);
    pRENDERDOC_GetAPI get_api = library ? (pRENDERDOC_GetAPI)(void*)GetProcAddress(library, "RENDERDOC_GetAPI") : NULL;
    void* table = NULL;
    if (!get_api || get_api(eRENDERDOC_API_Version_1_6_0, &table) != 1 || !table) return 0;
    capture = (RENDERDOC_API_1_6_0*)table;
    capture->SetCaptureOptionU32(eRENDERDOC_Option_AllowUnsupportedVendorExtensions, 0x10de);
    capture->SetCaptureFilePathTemplate(prefix);
    capture->SetCaptureKeys(NULL, 0);
    fprintf(stderr, "RenderDoc enabled before D3D11 device creation\n");
#else
    (void)prefix;
#endif
    return 1;
}

void rsf_probe_capture_start(void* device)
{
#ifdef RSF_PROBE_RENDERDOC
    if (capture) capture->StartFrameCapture(device, NULL);
#else
    (void)device;
#endif
}

int32_t rsf_probe_capture_end(void* device)
{
#ifdef RSF_PROBE_RENDERDOC
    if (capture && capture->IsFrameCapturing()) {
        const uint32_t saved = capture->EndFrameCapture(device, NULL);
        fprintf(stderr, "RenderDoc capture saved=%u count=%u\n", saved, capture->GetNumCaptures());
        return saved != 0;
    }
#else
    (void)device;
#endif
    return 1;
}

#ifndef RSF_PROBE_NGX_PARAMS
int32_t rsf_probe_ngx_available(void) { return 0; }
void rsf_probe_ngx_create_parameters(const void* params) { (void)params; }
void rsf_probe_ngx_evaluate_parameters(void* params) { (void)params; }
#endif

extern int32_t rsf_exposure_probe(int32_t argc, const char* const* argv);

int main(int argc, char** argv)
{
    return rsf_exposure_probe(argc, (const char* const*)argv);
}
