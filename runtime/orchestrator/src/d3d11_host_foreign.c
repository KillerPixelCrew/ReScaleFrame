/* SPDX-License-Identifier: MIT */
/* No-unwind boundaries for the D3D11 host's native calls.
   MSVC raises a C++ exception as SEH code 0xE06D7363, so a filter on that code stops the same
   exceptions the former catch-all stopped. Access violations and other faults still propagate.
   Entries that were noexcept without a handler keep their terminate semantics: a C++ exception
   there ends the process instead of unwinding into Rust. */
#include "d3d11_host_foreign.h"
#include <rescaleframe/input_dump.h>
#include <stdlib.h>
#include <string.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <rescaleframe/foreign_guard.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

static rsf_overlay_api overlay = {sizeof(rsf_overlay_api), RSF_OVERLAY_API_VERSION};

int32_t rsf_d3d11h_guard_present_install(const rsf_d3d11_present_setup* setup)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11_present_install(setup);
    } RSF_FOREIGN_CATCH { return 0; }
}

int32_t rsf_d3d11h_guard_present_prepare(const rsf_d3d11_present_setup* setup)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11_present_prepare(setup);
    } RSF_FOREIGN_CATCH { return 0; }
}

int32_t rsf_d3d11h_guard_present_install_scoped(const rsf_d3d11_present_setup* setup)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11_present_install_scoped(setup);
    } RSF_FOREIGN_CATCH { return 0; }
}

int32_t rsf_d3d11h_guard_observer_install(rsf_game_log_fn log, void* user, rsf_observer_present_event_fn present)
{
    RSF_FOREIGN_TRY {
        rsf_observer_options observe;
        memset(&observe, 0, sizeof(observe));
        observe.struct_size = sizeof(observe);
        observe.abi_version = RSF_OBSERVER_ABI_VERSION;
        observe.format = UINT32_MAX;
        observe.minimum_width = UINT32_MAX;
        observe.capacity = 1;
        observe.log = log;
        observe.log_user = user;
        observe.on_present_event = present;
        observe.on_present_event_user = user;
        return rsf_observer_install(&observe) == RSF_OBSERVER_OK;
    } RSF_FOREIGN_CATCH { return 0; }
}

int32_t rsf_d3d11h_guard_present_is_owner(void* chain)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11_present_is_owner(chain);
    } RSF_FOREIGN_CATCH { abort(); }
}

void rsf_d3d11h_guard_chain_dispatch(rsf_d3d11_chain_dispatch* request)
{
    RSF_FOREIGN_TRY {
        rsf_d3d11_chain_dispatch_owned(request);
    } RSF_FOREIGN_CATCH { abort(); }
}

int32_t rsf_d3d11h_guard_creation_begin(void* window, uint64_t* token)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11_present_creation_begin(window, token);
    } RSF_FOREIGN_CATCH { abort(); }
}

int32_t rsf_d3d11h_guard_creation_end(void* window, uint64_t token, void* device, void* context, void* outer)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11_present_creation_end(window, token, device, context, outer);
    } RSF_FOREIGN_CATCH { abort(); }
}

/* If wrapping throws, the host sees E_FAIL before changing the caller slot
   or consuming its reference. */
int32_t rsf_d3d11h_guard_wrap_native(void* device, void* native_chain, void** facade)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11_present_wrap_native_chain(device, native_chain, facade);
    } RSF_FOREIGN_CATCH { return E_FAIL; }
}

void rsf_d3d11h_guard_dump_configure(const char* directory, const char* identity,
    const char* fingerprint, rsf_game_log_fn log, void* user)
{
    RSF_FOREIGN_TRY {
        rsf_input_dump_configure(directory, identity, fingerprint, log, user);
    } RSF_FOREIGN_CATCH {}
}

void rsf_d3d11h_guard_dump_request(void)
{
    RSF_FOREIGN_TRY {
        rsf_input_dump_request();
    } RSF_FOREIGN_CATCH {}
}

int32_t rsf_d3d11h_guard_dump_requested(void)
{
    RSF_FOREIGN_TRY {
        return rsf_input_dump_requested();
    } RSF_FOREIGN_CATCH { return 0; }
}

void rsf_d3d11h_guard_dump_poll(void)
{
    RSF_FOREIGN_TRY {
        rsf_input_dump_poll();
    } RSF_FOREIGN_CATCH {}
}

int32_t rsf_d3d11h_guard_dump_begin(const rsf_frame_record* record, uint32_t backend)
{
    RSF_FOREIGN_TRY {
        return rsf_input_dump_begin_record(record, backend);
    } RSF_FOREIGN_CATCH { return 0; }
}

int32_t rsf_d3d11h_guard_dump_wants(const rsf_frame_record* record)
{
    RSF_FOREIGN_TRY {
        return rsf_input_dump_wants(record);
    } RSF_FOREIGN_CATCH { return 0; }
}

int32_t rsf_d3d11h_guard_dump_inputs_start(void* context, const rsf_game_render_pass* pass)
{
    RSF_FOREIGN_TRY {
        rsf_input_dump_pass_metadata(pass);
        rsf_input_dump_pass(context, pass, 1);
        return 1;
    } RSF_FOREIGN_CATCH { return 0; }
}

void rsf_d3d11h_guard_dump_pass(void* context, const rsf_game_render_pass* pass, uint32_t begin)
{
    RSF_FOREIGN_TRY {
        rsf_input_dump_pass(context, pass, begin);
    } RSF_FOREIGN_CATCH {}
}

int32_t rsf_d3d11h_guard_dump_texture(void* context, const rsf_frame_record* record, const char* name,
    const char* description, void* texture, uint32_t view, float scale)
{
    RSF_FOREIGN_TRY {
        return rsf_input_dump_texture_record(context, record, name, description, texture, view, scale);
    } RSF_FOREIGN_CATCH { return -1; }
}

void rsf_d3d11h_guard_dump_result(const rsf_frame_record* record, const char* stage, uint32_t backend, int32_t result)
{
    RSF_FOREIGN_TRY {
        rsf_input_dump_result(record, stage, backend, result);
    } RSF_FOREIGN_CATCH {}
}

void rsf_d3d11h_guard_dump_complete(const rsf_frame_record* record, int32_t result)
{
    RSF_FOREIGN_TRY {
        rsf_input_dump_complete(record, result);
    } RSF_FOREIGN_CATCH {}
}

int32_t rsf_d3d11h_guard_overlay_configure(const rsf_overlay_service_setup* panel, rsf_render_settings* applied)
{
    RSF_FOREIGN_TRY {
        if (rsf_get_overlay_api(&overlay) != RSF_OK || overlay.configure(panel) != RSF_OK) return RSF_ERROR_NOT_READY;
        memset(applied, 0, sizeof(*applied));
        applied->struct_size = sizeof(*applied);
        applied->abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
        return overlay.settings_get(applied);
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}

void rsf_d3d11h_guard_overlay_present(void* chain)
{
    RSF_FOREIGN_TRY {
        if (overlay.present) (void)overlay.present(chain, NULL);
    } RSF_FOREIGN_CATCH {}
}
