/* SPDX-License-Identifier: MIT */
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/native_window.h>

namespace {
void present_event(void*, const rsf_observer_present_event* event) noexcept { rsf_native_window_present(event); }
void latency_event(void*, const rsf_observer_present_event* event) noexcept { rsf_native_fg_present(event); }
}

extern "C" RSF_RUNTIME_API rsf_result rsf_get_d3d11_fg_api(rsf_d3d11_fg_api* api)
{
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (api->abi_version != RSF_D3D11_FG_API_VERSION) return RSF_ERROR_ABI_MISMATCH;
    const rsf_d3d11_fg_api complete{
        sizeof(rsf_d3d11_fg_api),
        RSF_D3D11_FG_API_VERSION,
        &rsf_native_fg_set_log,
        &rsf_native_fg_options_set,
        &rsf_native_fg_status_get,
        &rsf_native_fg_reflex_ordering_set,
        &rsf_native_fg_dispatch_cpu,
        &rsf_native_fg_dispatch_pass,
        &rsf_native_fg_bind_source,
        &rsf_native_fg_capture_scoped,
        &rsf_native_fg_inputs_ready,
        &rsf_native_fg_dump_inputs,
        &rsf_native_fg_prepare,
        &rsf_native_fg_retire,
        &present_event,
        &latency_event,
    };
    *api = complete;
    return RSF_OK;
}
