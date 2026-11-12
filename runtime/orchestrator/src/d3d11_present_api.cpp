/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/d3d11_present_api.h>

extern "C" RSF_RUNTIME_API rsf_result rsf_get_d3d11_present_api(rsf_d3d11_present_api* api) {
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (api->abi_version != RSF_D3D11_PRESENT_API_VERSION) return RSF_ERROR_ABI_MISMATCH;
    const rsf_d3d11_present_api complete{
        sizeof(rsf_d3d11_present_api),
        RSF_D3D11_PRESENT_API_VERSION,
        &rsf_d3d11_present_install,
        &rsf_d3d11_present_prepare,
        &rsf_d3d11_present_create_for_hwnd,
        &rsf_d3d11_present_host,
        &rsf_d3d11_present_graphics,
        &rsf_d3d11_present_queue,
        &rsf_d3d11_present_command_list,
        &rsf_d3d11_present_retire,
        &rsf_d3d11_present_is_owner,
        &rsf_d3d11_present_has_owner,
        &rsf_d3d11_present_provider,
        &rsf_d3d11_present_session,
        &rsf_d3d11_present_backend,
        &rsf_d3d11_present_backend_choices,
        &rsf_d3d11_present_request,
        &rsf_d3d11_present_switch_result,
        &rsf_d3d11_present_generation,
        &rsf_d3d11_present_begin,
        &rsf_d3d11_present_marker,
        &rsf_d3d11_present_acquire,
        &rsf_d3d11_present_input,
        &rsf_d3d11_present_abort,
    };
    *api = complete;
    return RSF_OK;
}
