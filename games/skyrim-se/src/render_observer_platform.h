// SPDX-License-Identifier: MIT
#pragma once
#include "native_creation_platform.h"

struct rsf_skyrim_jitter_report {
    uintptr_t state;
    uint64_t number;
    uint32_t dimensions[2];
    float projection[2];
    bool readable;
};

struct rsf_skyrim_render_observer_leaves {
    bool (*render_allowed)();
    void (*bind)(uintptr_t, void*, const rsf_d3d11_presentation_binding*);
    void (*configure_resolution)(void*, const rsf_game_host_services*);
    void (*finish_auxiliaries)(const rsf_game_host_services*);
    void (*report_jitter)(const rsf_game_host_services*, const rsf_skyrim_jitter_report*);
    uint32_t (*window_class)(void*, wchar_t*, uint32_t);
};

static_assert(sizeof(rsf_skyrim_jitter_report) == 40 && offsetof(rsf_skyrim_jitter_report, readable) == 32);
static_assert(sizeof(rsf_skyrim_render_observer_leaves) == 48 && sizeof(rsf_d3d11_presentation_binding) == 16);
static_assert(sizeof(wchar_t) == 2);

extern "C" void rsf_skyrim_render_end_enter(const rsf_skyrim_creation_platform*,
    const rsf_skyrim_render_observer_leaves*, rsf_game_host_services*) noexcept;
extern "C" void rsf_skyrim_render_end_leave(const rsf_skyrim_render_observer_leaves*,
    const rsf_game_host_services*) noexcept;
struct rsf_skyrim_upload_observation {
    uint64_t session;
    uint32_t frame;
};
extern "C" bool rsf_skyrim_render_upload_enter(const rsf_skyrim_creation_platform*, uintptr_t renderer,
    rsf_skyrim_upload_observation*) noexcept;
extern "C" void rsf_skyrim_render_upload_leave(const rsf_skyrim_upload_observation*, uint8_t flags) noexcept;
extern "C" void rsf_skyrim_render_jitter_observed(const rsf_skyrim_creation_platform*,
    const rsf_skyrim_render_observer_leaves*, uintptr_t state) noexcept;
extern "C" bool rsf_skyrim_render_accept_window(const rsf_skyrim_render_observer_leaves*, void* window) noexcept;
