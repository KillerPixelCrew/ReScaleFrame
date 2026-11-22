// SPDX-License-Identifier: MIT
// No-unwind boundaries for the D3D11 host's native calls.
#include "d3d11_host_foreign.h"
#include <rescaleframe/input_dump.h>
#include <windows.h>

namespace {
rsf_overlay_api overlay{sizeof(rsf_overlay_api), RSF_OVERLAY_API_VERSION};
}

extern "C" int rsf_d3d11h_guard_present_install(const rsf_d3d11_present_setup* setup) noexcept try
{
    return rsf_d3d11_present_install(setup);
} catch (...) { return 0; }

extern "C" int rsf_d3d11h_guard_present_prepare(const rsf_d3d11_present_setup* setup) noexcept try
{
    return rsf_d3d11_present_prepare(setup);
} catch (...) { return 0; }

extern "C" int rsf_d3d11h_guard_present_install_scoped(const rsf_d3d11_present_setup* setup) noexcept try
{
    return rsf_d3d11_present_install_scoped(setup);
} catch (...) { return 0; }

extern "C" int32_t rsf_d3d11h_guard_observer_install(rsf_game_log_fn log, void* user, rsf_observer_present_event_fn present) noexcept try
{
    rsf_observer_options observe{}; observe.struct_size = sizeof(observe); observe.abi_version = RSF_OBSERVER_ABI_VERSION;
    observe.format = UINT32_MAX; observe.minimum_width = UINT32_MAX; observe.capacity = 1;
    observe.log = log; observe.log_user = user; observe.on_present_event = present; observe.on_present_event_user = user;
    return rsf_observer_install(&observe) == RSF_OBSERVER_OK;
} catch (...) { return 0; }

extern "C" int32_t rsf_d3d11h_guard_present_is_owner(void* chain) noexcept { return rsf_d3d11_present_is_owner(chain); }
extern "C" void rsf_d3d11h_guard_chain_dispatch(rsf_d3d11_chain_dispatch* request) noexcept { rsf_d3d11_chain_dispatch_owned(request); }
extern "C" int32_t rsf_d3d11h_guard_creation_begin(void* window, uint64_t* token) noexcept { return rsf_d3d11_present_creation_begin(window, token); }
extern "C" int32_t rsf_d3d11h_guard_creation_end(void* window, uint64_t token, void* device, void* context, void* outer) noexcept
{
    return rsf_d3d11_present_creation_end(window, token, device, context, outer);
}

// If wrapping throws, the host sees E_FAIL before changing the caller slot
// or consuming its reference.
extern "C" int32_t rsf_d3d11h_guard_wrap_native(void* device, void* native_chain, void** facade) noexcept try
{
    return rsf_d3d11_present_wrap_native_chain(device, native_chain, facade);
} catch (...) { return E_FAIL; }

extern "C" void rsf_d3d11h_guard_dump_configure(const char* directory, const char* identity,
    const char* fingerprint, rsf_game_log_fn log, void* user) noexcept try
{
    rsf_input_dump_configure(directory, identity, fingerprint, log, user);
} catch (...) {}
extern "C" void rsf_d3d11h_guard_dump_request() noexcept try { rsf_input_dump_request(); } catch (...) {}
extern "C" int32_t rsf_d3d11h_guard_dump_requested() noexcept try { return rsf_input_dump_requested(); } catch (...) { return 0; }
extern "C" void rsf_d3d11h_guard_dump_poll() noexcept try { rsf_input_dump_poll(); } catch (...) {}
extern "C" int32_t rsf_d3d11h_guard_dump_begin(const rsf_frame_record* record, uint32_t backend) noexcept try
{
    return rsf_input_dump_begin_record(record, backend);
} catch (...) { return 0; }
extern "C" int32_t rsf_d3d11h_guard_dump_wants(const rsf_frame_record* record) noexcept try { return rsf_input_dump_wants(record); } catch (...) { return 0; }
extern "C" int32_t rsf_d3d11h_guard_dump_inputs_start(void* context, const rsf_game_render_pass* pass) noexcept try
{
    rsf_input_dump_pass_metadata(pass);
    rsf_input_dump_pass(context, pass, 1);
    return 1;
} catch (...) { return 0; }
extern "C" void rsf_d3d11h_guard_dump_pass(void* context, const rsf_game_render_pass* pass, uint32_t begin) noexcept try
{
    rsf_input_dump_pass(context, pass, begin);
} catch (...) {}
extern "C" int32_t rsf_d3d11h_guard_dump_texture(void* context, const rsf_frame_record* record, const char* name, const char* description,
    void* texture, uint32_t view, float scale) noexcept try
{
    return rsf_input_dump_texture_record(context, record, name, description, texture, view, scale);
} catch (...) { return -1; }
extern "C" void rsf_d3d11h_guard_dump_result(const rsf_frame_record* record, const char* stage, uint32_t backend, int32_t result) noexcept try
{
    rsf_input_dump_result(record, stage, backend, result);
} catch (...) {}
extern "C" void rsf_d3d11h_guard_dump_complete(const rsf_frame_record* record, int32_t result) noexcept try
{
    rsf_input_dump_complete(record, result);
} catch (...) {}

extern "C" int32_t rsf_d3d11h_guard_overlay_configure(const rsf_overlay_service_setup* panel, rsf_render_settings* applied) noexcept try
{
    if (rsf_get_overlay_api(&overlay) != RSF_OK || overlay.configure(panel) != RSF_OK) return RSF_ERROR_NOT_READY;
    *applied = {sizeof(rsf_render_settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    return overlay.settings_get(applied);
} catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" void rsf_d3d11h_guard_overlay_present(void* chain) noexcept try
{
    if (overlay.present) (void)overlay.present(chain, nullptr);
} catch (...) {}
