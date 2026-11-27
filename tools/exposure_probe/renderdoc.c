/* SPDX-License-Identifier: MIT */
#include "sdk.h"
#include <stddef.h>

#ifdef RSF_PROBE_RENDERDOC
#include <stdbool.h>
#include <renderdoc_app.h>

int32_t rsf_probe_renderdoc_available(void) { return 1; }

void* rsf_probe_renderdoc_connect(void* address, const char* prefix, uint32_t vendor)
{
    pRENDERDOC_GetAPI get_api = (pRENDERDOC_GetAPI)address;
    void* table = NULL;
    if (get_api(eRENDERDOC_API_Version_1_6_0, &table) != 1 || !table) return NULL;

    RENDERDOC_API_1_6_0* api = table;
    api->SetCaptureOptionU32(eRENDERDOC_Option_AllowUnsupportedVendorExtensions, vendor);
    api->SetCaptureFilePathTemplate(prefix);
    api->SetCaptureKeys(NULL, 0);
    return api;
}

void rsf_probe_renderdoc_start(void* table, void* device)
{
    RENDERDOC_API_1_6_0* api = table;
    api->StartFrameCapture(device, NULL);
}

int32_t rsf_probe_renderdoc_finish(void* table, void* device, uint32_t* saved, uint32_t* count)
{
    RENDERDOC_API_1_6_0* api = table;
    if (!api->IsFrameCapturing()) return 0;
    *saved = api->EndFrameCapture(device, NULL);
    *count = api->GetNumCaptures();
    return 1;
}
#else
int32_t rsf_probe_renderdoc_available(void) { return 0; }
void* rsf_probe_renderdoc_connect(void* address, const char* prefix, uint32_t vendor)
{
    (void)address;
    (void)prefix;
    (void)vendor;
    return NULL;
}
void rsf_probe_renderdoc_start(void* api, void* device) { (void)api; (void)device; }
int32_t rsf_probe_renderdoc_finish(void* api, void* device, uint32_t* saved, uint32_t* count)
{
    (void)api;
    (void)device;
    (void)saved;
    (void)count;
    return 0;
}
#endif

#ifndef RSF_PROBE_NGX_PARAMS
int32_t rsf_probe_ngx_available(void) { return 0; }
uint32_t rsf_probe_ngx_get_flags(const void* params, int32_t* flags)
{
    (void)params;
    (void)flags;
    return 0;
}
void rsf_probe_ngx_set_flags(const void* params, int32_t flags) { (void)params; (void)flags; }
void rsf_probe_ngx_set_exposure(void* params, const float* pre, const float* scale)
{
    (void)params;
    (void)pre;
    (void)scale;
}
void rsf_probe_ngx_get_exposure(void* params, float* pre, float* scale)
{
    (void)params;
    (void)pre;
    (void)scale;
}
void* rsf_probe_ngx_get_resource(void* params, int32_t output)
{
    (void)params;
    (void)output;
    return NULL;
}
#endif
