/* SPDX-License-Identifier: MIT */
/*
 * NGX parameter access through the SDK's virtual table. nvsdk_ngx_params.h declares the C++ class
 * only for C++ callers, so this C11 leaf calls its slots directly and takes names and constants
 * from nvsdk_ngx_defs.h. Only the Windows x64 MSVC layout is supported.
 */
#include "sdk.h"
#include <nvsdk_ngx_defs.h>

_Static_assert(RSF_PROBE_NGX_HDR == NVSDK_NGX_DLSS_Feature_Flags_IsHDR, "NGX HDR flag");
_Static_assert(RSF_PROBE_NGX_DEPTH_INVERTED == NVSDK_NGX_DLSS_Feature_Flags_DepthInverted,
    "NGX depth flag");
_Static_assert(RSF_PROBE_NGX_AUTO_EXPOSURE == NVSDK_NGX_DLSS_Feature_Flags_AutoExposure,
    "NGX exposure flag");

/*
 * MSVC lays out overloaded virtuals in reverse declaration order. The SDK declares eight Set
 * overloads and then eight Get overloads (ULL, float, double, uint, int, D3D11, D3D12, void*),
 * so Set takes slots 0-7 and Get slots 8-15. The probe uses the int, float and D3D11 entries.
 * `objdump -dr` of the checked SDK's nvsdk_ngx_d.lib confirms these byte offsets: Set int 0x18,
 * Set float 0x30, Get int 0x58, Get float 0x70. Get D3D11 (0x50) follows the same reversal.
 */
enum {
    NGX_SET_INT = 3,
    NGX_SET_FLOAT = 6,
    NGX_GET_D3D11 = 10,
    NGX_GET_INT = 11,
    NGX_GET_FLOAT = 14,
};

typedef void (*NgxSetInt)(void* parameters, const char* name, int value);
typedef void (*NgxSetFloat)(void* parameters, const char* name, float value);
typedef NVSDK_NGX_Result (*NgxGet)(const void* parameters, const char* name, void* output);

static void* slot(const void* parameters, unsigned index)
{
    return (*(void* const* const*)parameters)[index];
}

int32_t rsf_probe_ngx_available(void) { return 1; }

uint32_t rsf_probe_ngx_get_flags(const void* parameters, int32_t* flags)
{
    return ((NgxGet)slot(parameters, NGX_GET_INT))(
        parameters, NVSDK_NGX_Parameter_DLSS_Feature_Create_Flags, flags);
}

void rsf_probe_ngx_set_flags(const void* parameters, int32_t flags)
{
    ((NgxSetInt)slot(parameters, NGX_SET_INT))(
        (void*)parameters, NVSDK_NGX_Parameter_DLSS_Feature_Create_Flags, flags);
}

void rsf_probe_ngx_set_exposure(void* parameters, const float* pre, const float* scale)
{
    NgxSetFloat set = (NgxSetFloat)slot(parameters, NGX_SET_FLOAT);
    if (pre) set(parameters, NVSDK_NGX_Parameter_DLSS_Pre_Exposure, *pre);
    if (scale) set(parameters, NVSDK_NGX_Parameter_DLSS_Exposure_Scale, *scale);
}

void rsf_probe_ngx_get_exposure(void* parameters, float* pre, float* scale)
{
    NgxGet get = (NgxGet)slot(parameters, NGX_GET_FLOAT);
    get(parameters, NVSDK_NGX_Parameter_DLSS_Pre_Exposure, pre);
    get(parameters, NVSDK_NGX_Parameter_DLSS_Exposure_Scale, scale);
}

void* rsf_probe_ngx_get_resource(void* parameters, int32_t output)
{
    void* resource = NULL;
    const char* name = output ? NVSDK_NGX_Parameter_Output : NVSDK_NGX_Parameter_Color;
    NgxGet get = (NgxGet)slot(parameters, NGX_GET_D3D11);
    return get(parameters, name, &resource) == NVSDK_NGX_Result_Success ? resource : NULL;
}
