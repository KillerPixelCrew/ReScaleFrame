// SPDX-License-Identifier: MIT
// Optional NGX parameter tracing uses the SDK's C++ virtual interface.
#include <d3d11.h>
#include <wrl/client.h>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <nvsdk_ngx_params.h>

using Microsoft::WRL::ComPtr;
extern "C" int32_t rsf_probe_ngx_available() { return 1; }

extern "C" void rsf_probe_ngx_evaluate_parameters(void* parameters)
{
    auto* params = static_cast<NVSDK_NGX_Parameter*>(parameters);
    static std::atomic_bool reported = false;
    char override_value[64]{};
    if (GetEnvironmentVariableA("RSF_PROBE_PRE_EXPOSURE", override_value, sizeof(override_value))) {
        const float value = std::strtof(override_value, nullptr);
        if (value > 0) params->Set(NVSDK_NGX_Parameter_DLSS_Pre_Exposure, value);
    }
    if (GetEnvironmentVariableA("RSF_PROBE_EXPOSURE_SCALE", override_value, sizeof(override_value))) {
        const float value = std::strtof(override_value, nullptr);
        if (value > 0) params->Set(NVSDK_NGX_Parameter_DLSS_Exposure_Scale, value);
    }
    if (!reported.exchange(true, std::memory_order_relaxed)) {
        float pre = 0, scale = 0;
        params->Get(NVSDK_NGX_Parameter_DLSS_Pre_Exposure, &pre);
        params->Get(NVSDK_NGX_Parameter_DLSS_Exposure_Scale, &scale);
        std::fprintf(stderr, "NGX_EVALUATE preExposure=%g exposureScale=%g\n", double(pre), double(scale));
        const char* names[] = {NVSDK_NGX_Parameter_Color, NVSDK_NGX_Parameter_Output};
        for (const auto* name : names) {
            ID3D11Resource* resource = nullptr;
            if (params->Get(name, &resource) == NVSDK_NGX_Result_Success && resource) {
                ComPtr<ID3D11Texture2D> texture;
                if (SUCCEEDED(resource->QueryInterface(IID_PPV_ARGS(&texture)))) {
                    D3D11_TEXTURE2D_DESC description{}; texture->GetDesc(&description);
                    std::fprintf(stderr, "NGX_RESOURCE %s %ux%u format=%u bind=%x\n", name,
                                 description.Width, description.Height, unsigned(description.Format), description.BindFlags);
                }
            }
        }
    }
}

extern "C" void rsf_probe_ngx_create_parameters(const void* parameters)
{
    const auto* params = static_cast<const NVSDK_NGX_Parameter*>(parameters);
    int flags = 0;
    const auto result = params->Get(NVSDK_NGX_Parameter_DLSS_Feature_Create_Flags, &flags);
    char ldr[8]{};
    if (GetEnvironmentVariableA("RSF_PROBE_LDR", ldr, sizeof(ldr)) && ldr[0] == '1') {
        flags &= ~int(NVSDK_NGX_DLSS_Feature_Flags_IsHDR);
        const_cast<NVSDK_NGX_Parameter*>(params)->Set(NVSDK_NGX_Parameter_DLSS_Feature_Create_Flags, flags);
    }
    std::fprintf(stderr, "NGX_CREATE flags_get=%x flags=%x HDR=%u AutoExposure=%u DepthInverted=%u\n",
                 unsigned(result), unsigned(flags), (unsigned(flags) & 1u) != 0,
                 (unsigned(flags) & 64u) != 0, (unsigned(flags) & 8u) != 0);
}
