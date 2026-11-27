/* SPDX-License-Identifier: MIT */
#ifndef RSF_EXPOSURE_PROBE_SDK_H
#define RSF_EXPOSURE_PROBE_SDK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int32_t rsf_probe_renderdoc_available(void);
void* rsf_probe_renderdoc_connect(void* get_api, const char* prefix, uint32_t vendor);
void rsf_probe_renderdoc_start(void* api, void* device);
int32_t rsf_probe_renderdoc_finish(void* api, void* device, uint32_t* saved, uint32_t* count);

#define RSF_PROBE_NGX_HDR 1u
#define RSF_PROBE_NGX_DEPTH_INVERTED 8u
#define RSF_PROBE_NGX_AUTO_EXPOSURE 64u

int32_t rsf_probe_ngx_available(void);
uint32_t rsf_probe_ngx_get_flags(const void* params, int32_t* flags);
void rsf_probe_ngx_set_flags(const void* params, int32_t flags);
void rsf_probe_ngx_set_exposure(void* params, const float* pre, const float* scale);
void rsf_probe_ngx_get_exposure(void* params, float* pre, float* scale);
void* rsf_probe_ngx_get_resource(void* params, int32_t output);

#ifdef __cplusplus
}
#endif
#endif
