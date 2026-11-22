// SPDX-License-Identifier: MIT
// NGX parameter access through the SDK's C++ virtual interface.
#include "sdk.h"
#include <d3d11.h>
#include <nvsdk_ngx_params.h>

static_assert(RSF_PROBE_NGX_HDR == NVSDK_NGX_DLSS_Feature_Flags_IsHDR);
static_assert(RSF_PROBE_NGX_DEPTH_INVERTED == NVSDK_NGX_DLSS_Feature_Flags_DepthInverted);
static_assert(RSF_PROBE_NGX_AUTO_EXPOSURE == NVSDK_NGX_DLSS_Feature_Flags_AutoExposure);

int32_t rsf_probe_ngx_available() { return 1; }

uint32_t rsf_probe_ngx_get_flags(const void* parameters, int32_t* flags)
{
    return static_cast<const NVSDK_NGX_Parameter*>(parameters)->Get(
        NVSDK_NGX_Parameter_DLSS_Feature_Create_Flags, flags);
}

void rsf_probe_ngx_set_flags(const void* parameters, int32_t flags)
{
    const_cast<NVSDK_NGX_Parameter*>(static_cast<const NVSDK_NGX_Parameter*>(parameters))->Set(
        NVSDK_NGX_Parameter_DLSS_Feature_Create_Flags, flags);
}

void rsf_probe_ngx_set_exposure(void* parameters, const float* pre, const float* scale)
{
    auto* params = static_cast<NVSDK_NGX_Parameter*>(parameters);
    if (pre) params->Set(NVSDK_NGX_Parameter_DLSS_Pre_Exposure, *pre);
    if (scale) params->Set(NVSDK_NGX_Parameter_DLSS_Exposure_Scale, *scale);
}

void rsf_probe_ngx_get_exposure(void* parameters, float* pre, float* scale)
{
    auto* params = static_cast<const NVSDK_NGX_Parameter*>(parameters);
    params->Get(NVSDK_NGX_Parameter_DLSS_Pre_Exposure, pre);
    params->Get(NVSDK_NGX_Parameter_DLSS_Exposure_Scale, scale);
}

void* rsf_probe_ngx_get_resource(void* parameters, int32_t output)
{
    const auto* params = static_cast<const NVSDK_NGX_Parameter*>(parameters);
    ID3D11Resource* resource = nullptr;
    const char* name = output ? NVSDK_NGX_Parameter_Output : NVSDK_NGX_Parameter_Color;
    return params->Get(name, &resource) == NVSDK_NGX_Result_Success ? resource : nullptr;
}
