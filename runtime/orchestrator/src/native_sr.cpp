// SPDX-License-Identifier: MIT
// Exception boundaries for the D3D11 SR and FG pipeline calls.
#include "native_sr_completion.h"
#include <rescaleframe/native_fg.h>
#include <rescaleframe/game_api.h>

namespace {
constexpr int leaf_fault = INT32_MIN;
}

extern "C" int32_t rsf_native_sr_resize_guarded(uint32_t width, uint32_t height) noexcept try
{
    return rsf_dlss_pipeline_resize_output(width, height, nullptr, nullptr);
} catch (...) {
    return leaf_fault;
}

extern "C" int32_t rsf_native_sr_frame_guarded(void* context,
    const rsf_dlss_pipeline_frame* frame) noexcept try
{
    return rsf_dlss_pipeline_on_frame(context, frame);
} catch (...) {
    return leaf_fault;
}

extern "C" int32_t rsf_native_sr_fg_capture_guarded(void* context,
    const rsf_dlss_frame* frame) noexcept try
{
    rsf_native_fg_capture(context, frame);
    return 1;
} catch (...) {
    return 0;
}

extern "C" rsf_dlss_pipeline_result rsf_core_native_sr_evaluate(
    void* context, const rsf_game_render_pass* pass, uint32_t* ordered);

extern "C" int32_t rsf_native_sr_evaluate_ordered_guarded(
    void* context, const rsf_game_render_pass* pass, uint32_t* ordered) noexcept try
{
    return rsf_core_native_sr_evaluate(context, pass, ordered);
} catch (...) {
    if (ordered) *ordered = 0;
    return RSF_ERROR_NOT_READY;
}

extern "C" RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_native_sr_evaluate(void* context,
    const rsf_game_render_pass* pass)
{
    return rsf_core_native_sr_evaluate(context, pass, nullptr);
}
