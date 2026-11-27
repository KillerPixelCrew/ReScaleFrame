/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/native_sr.h>

#ifdef __cplusplus
extern "C" {
#define RSF_D3D11H_NOEXCEPT noexcept
#else
#define RSF_D3D11H_NOEXCEPT
#endif

/* Host ingress to native code. D02 owns the exact receiver and continuation.
   The host keeps its existing C++ failure values and no-unwind boundaries. */
int32_t rsf_d3d11h_guard_present_install(const rsf_d3d11_present_setup*) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_present_prepare(const rsf_d3d11_present_setup*) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_present_install_scoped(const rsf_d3d11_present_setup*) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_observer_install(rsf_game_log_fn, void*, rsf_observer_present_event_fn) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_present_is_owner(void*) RSF_D3D11H_NOEXCEPT;
void rsf_d3d11h_guard_chain_dispatch(rsf_d3d11_chain_dispatch*) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_creation_begin(void*, uint64_t*) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_creation_end(void*, uint64_t, void*, void*, void*) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_wrap_native(void*, void*, void**) RSF_D3D11H_NOEXCEPT;

void rsf_d3d11h_guard_dump_configure(const char* directory, const char* identity,
    const char* fingerprint, rsf_game_log_fn, void*) RSF_D3D11H_NOEXCEPT;
void rsf_d3d11h_guard_dump_request(void) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_dump_requested(void) RSF_D3D11H_NOEXCEPT;
void rsf_d3d11h_guard_dump_poll(void) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_dump_begin(const rsf_frame_record*, uint32_t) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_dump_wants(const rsf_frame_record*) RSF_D3D11H_NOEXCEPT;
int32_t rsf_d3d11h_guard_dump_inputs_start(void*, const rsf_game_render_pass*) RSF_D3D11H_NOEXCEPT;
void rsf_d3d11h_guard_dump_pass(void*, const rsf_game_render_pass*, uint32_t) RSF_D3D11H_NOEXCEPT;
/* Returns -1 on exception, 0 or 1 from the capture owner on normal return. */
int32_t rsf_d3d11h_guard_dump_texture(void*, const rsf_frame_record*, const char*, const char*,
    void*, uint32_t, float) RSF_D3D11H_NOEXCEPT;
void rsf_d3d11h_guard_dump_result(const rsf_frame_record*, const char*, uint32_t, int32_t) RSF_D3D11H_NOEXCEPT;
void rsf_d3d11h_guard_dump_complete(const rsf_frame_record*, int32_t) RSF_D3D11H_NOEXCEPT;

int32_t rsf_d3d11h_guard_overlay_configure(const rsf_overlay_service_setup*, rsf_render_settings*) RSF_D3D11H_NOEXCEPT;
void rsf_d3d11h_guard_overlay_present(void*) RSF_D3D11H_NOEXCEPT;

#undef RSF_D3D11H_NOEXCEPT
#ifdef __cplusplus
}
#endif
