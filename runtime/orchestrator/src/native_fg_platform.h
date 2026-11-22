/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_fg.h>
#include <rescaleframe/named_frame_dump.h>

#ifdef __cplusplus
extern "C" {
#endif

/* DXGI formats, D3D12 states, frame phase and dump views named by native_fg.rs. Pinned against the
   platform bindings by const assertions in native_fg/platform.rs. */
#define RSF_NFG_FORMAT_RGBA16_FLOAT 10u
#define RSF_NFG_FORMAT_RG32_FLOAT 16u
#define RSF_NFG_FORMAT_RGB10A2_UNORM 24u
#define RSF_NFG_FORMAT_RGBA8_UNORM 28u
#define RSF_NFG_FORMAT_RGBA8_UNORM_SRGB 29u
#define RSF_NFG_FORMAT_RG16_FLOAT 34u
#define RSF_NFG_FORMAT_R32_FLOAT 41u
#define RSF_NFG_FORMAT_BGRA8_UNORM 87u
#define RSF_NFG_FORMAT_BGRA8_UNORM_SRGB 91u
#define RSF_NFG_STATE_COMMON 0u
#define RSF_NFG_STATE_UNORDERED_ACCESS 0x8u
#define RSF_NFG_STATE_NON_PIXEL_SHADER_RESOURCE 0x40u
#define RSF_NFG_STATE_PRESENT 0u
#define RSF_NFG_PHASE_UI_COMPLETE 5u
#define RSF_NFG_DUMP_VIEW_RAW 0u
#define RSF_NFG_DUMP_VIEW_DEPTH 4u
#define RSF_NFG_DUMP_VIEW_DENSE_MOTION 7u
#define RSF_NFG_SR_DLSS 1u
#define RSF_NFG_FG_BACKEND_FSR3 3u
#define RSF_NFG_FG_BACKEND_FSR4 4u
#define RSF_NFG_STAT_LATENCY_AVAILABLE 4u
#define RSF_NFG_PRESENT_TEST 1u
#define RSF_NFG_RESULT_OK 0
#define RSF_NFG_RESULT_NOT_READY (-8)
#define RSF_NFG_RESULT_FEATURE_FAILED (-9)

typedef struct rsf_nfg_texture_info {
    uint32_t width, height, format, samples;
} rsf_nfg_texture_info;

#ifdef __cplusplus
}
#endif
