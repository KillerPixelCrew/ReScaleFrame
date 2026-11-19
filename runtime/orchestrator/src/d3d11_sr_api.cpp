/* SPDX-License-Identifier: MIT */
#include <rescaleframe/d3d11_sr_api.h>
#include <rescaleframe/native_sr.h>
extern "C" rsf_dlss_pipeline_result rsf_core_sr_evaluate(void* context,
    const rsf_game_render_pass* pass, decltype(&rsf_native_sr_evaluate) native);
extern "C" rsf_dlss_pipeline_result rsf_core_sr_request_reset();
extern "C" void rsf_core_sr_clear_reset();

namespace {
rsf_dlss_pipeline_result evaluate(void* context, const rsf_game_render_pass* pass) noexcept
{
    return rsf_core_sr_evaluate(context, pass, rsf_native_sr_evaluate);
}
rsf_dlss_pipeline_result stop() noexcept
{
    rsf_core_sr_clear_reset();
    return rsf_dlss_pipeline_stop();
}
rsf_dlss_pipeline_result set_quality(uint32_t quality, uint32_t* width, uint32_t* height) noexcept
{
    if (quality > RSF_DLSS_QUALITY_ULTRA_PERFORMANCE) return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    return rsf_dlss_pipeline_set_quality(static_cast<rsf_dlss_quality>(quality), width, height);
}
}

extern "C" RSF_RUNTIME_API rsf_result rsf_get_d3d11_sr_api(rsf_d3d11_sr_api* api)
{
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (api->abi_version != RSF_D3D11_SR_API_VERSION) return RSF_ERROR_ABI_MISMATCH;
    const rsf_d3d11_sr_api complete{
        sizeof(rsf_d3d11_sr_api),
        RSF_D3D11_SR_API_VERSION,
        RSF_DLSS_PIPELINE_ABI_VERSION,
        &rsf_dlss_pipeline_start,
        &evaluate,
        &rsf_core_sr_request_reset,
        &stop,
        &rsf_dlss_pipeline_get_status,
        &rsf_dlss_pipeline_select_backend,
        &set_quality,
        &rsf_dlss_pipeline_set_preset,
        &rsf_dlss_pipeline_resize_output,
        &rsf_dlss_pipeline_output_texture,
        &rsf_dlss_pipeline_request_dump,
        &rsf_dlss_pipeline_set_sharpening,
    };
    *api = complete;
    return RSF_OK;
}
