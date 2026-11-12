/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/d3d11_sr_api.h>
#include <rescaleframe/native_sr.h>
#include <atomic>
#include <cstring>

namespace {
// A requested history break, consumed by the next evaluate. The pipeline has one owner thread; the
// flag is atomic only so a reset requested from that thread's callbacks is never torn.
std::atomic<uint32_t> reset_requested{0};

rsf_dlss_pipeline_result evaluate(void* context, const rsf_game_render_pass* pass) noexcept
{
    // Checked before the copy, which reads the whole structure.
    if (!context || !pass || pass->struct_size < sizeof(rsf_game_render_pass)) return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    if (!reset_requested.load(std::memory_order_acquire)) return rsf_native_sr_evaluate(context, pass);
    rsf_game_render_pass copy;
    std::memcpy(&copy, pass, sizeof(copy));
    copy.flags |= RSF_GAME_RENDER_RESET;
    const auto result = rsf_native_sr_evaluate(context, &copy);
    // Kept until a pass reaches the backend: a refused pass carried no history to break.
    if (result == RSF_DLSS_PIPELINE_OK) reset_requested.store(0, std::memory_order_release);
    return result;
}
rsf_dlss_pipeline_result reset() noexcept
{
    reset_requested.store(1, std::memory_order_release);
    return RSF_DLSS_PIPELINE_OK;
}
rsf_dlss_pipeline_result stop() noexcept
{
    reset_requested.store(0, std::memory_order_release);
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
        &reset,
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
