// SPDX-License-Identifier: MIT
#pragma once
// Synchronous render-input reader and receipt boundary.
#include "render_records.h"
#include <rescaleframe/game_renderer.h>

RSF_FALLOUT4_RENDER_ABI_BEGIN

typedef void (*NativeInputReport)(const char*);

// The readers transfer an owned reference for each nonnull slot.
// The native caller attaches them to the World or releases unused references.

bool rsf_fallout4_inputs_claim_exception() RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_inputs_read(NativeInputReferences* lease,
    NativeInputReport, const NativeDepthSnapshot* producer, rsf_game_render_pass*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_inputs_read_raster(NativeInputReferences* lease,
    const rsf_game_render_pass* frame, const NativeDepthSnapshot* producer, rsf_game_render_pass*) RSF_FALLOUT4_RENDER_NOEXCEPT;
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeCamera) == 288);
RSF_FALLOUT4_RENDER_ASSERT(offsetof(NativeCamera, projection) == 64);
RSF_FALLOUT4_RENDER_ASSERT(offsetof(NativeCamera, view_projection_unjittered) == 128);
RSF_FALLOUT4_RENDER_ASSERT(offsetof(NativeCamera, previous_view_projection_unjittered) == 192);
RSF_FALLOUT4_RENDER_ASSERT(offsetof(NativeCamera, position_adjust) == 256);
RSF_FALLOUT4_RENDER_ASSERT(offsetof(NativeCamera, previous_position_adjust) == 268);
RSF_FALLOUT4_RENDER_ASSERT(offsetof(NativeCamera, jitter_ndc) == 280);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(CameraRaster) == 32 && offsetof(CameraRaster, crop) == 16);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeInputState) == 128 && offsetof(NativeInputState, frame) == 56 &&
    offsetof(NativeInputState, ratio) == 92 && offsetof(NativeInputState, viewport) == 100 &&
    offsetof(NativeInputState, delta_seconds) == 116 && offsetof(NativeInputState, default_viewport) == 120 &&
    offsetof(NativeInputState, dynamic_viewport) == 121);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeInputSwapFacts) == 24 && offsetof(NativeInputSwapFacts, read) == 8);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeInputResourceFacts) == 80 && offsetof(NativeInputResourceFacts, read) == 24 &&
    offsetof(NativeInputResourceFacts, view_format) == 60 && offsetof(NativeInputResourceFacts, reserved) == 76);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeInputReceipt) == 912 && offsetof(NativeInputReceipt, native) == 128 &&
    offsetof(NativeInputReceipt, views) == 416 && offsetof(NativeInputReceipt, rtv) == 656 &&
    offsetof(NativeInputReceipt, swap) == 736 && offsetof(NativeInputReceipt, expected_renderer) == 760 &&
    offsetof(NativeInputReceipt, expected) == 784 && offsetof(NativeInputReceipt, after_frame) == 808 &&
    offsetof(NativeInputReceipt, camera_read) == 836 && offsetof(NativeInputReceipt, device_match) == 852 &&
    offsetof(NativeInputReceipt, viewport) == 864 && offsetof(NativeInputReceipt, confirmed_viewport) == 888);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeInputHistoryReceipt) == 96 && offsetof(NativeInputHistoryReceipt, raster) == 32);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeFramebufferReceipt) == 344 && offsetof(NativeFramebufferReceipt, swap) == 256 &&
    offsetof(NativeFramebufferReceipt, target_texture) == 280 && offsetof(NativeFramebufferReceipt, platform) == 304 &&
    offsetof(NativeFramebufferReceipt, rtv_mip) == 340);

bool rsf_fallout4_inputs_same_resources(const void* current, const void* held,
    bool owners) RSF_FALLOUT4_RENDER_NOEXCEPT;
void* rsf_fallout4_input_object(const void* inputs, uint32_t slot) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_inputs_report_reader(NativeInputReport, const char* stage,
    const NativeInputReceipt*, const rsf_game_render_pass*) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_inputs_report_mismatch(NativeInputReport, const NativeInputState* first,
    const NativeInputState* second, uint32_t mask) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_inputs_report_history(NativeInputReport,
    const NativeInputHistoryReceipt*) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_inputs_report_framebuffer(NativeInputReport, const char* stage, const char* owner_stage,
    const NativeFramebufferReceipt*) RSF_FALLOUT4_RENDER_NOEXCEPT;

RSF_FALLOUT4_RENDER_ABI_END
