/* SPDX-License-Identifier: MIT */
#include <rescaleframe/d3d11_present_api.h>

namespace {
constexpr rsf_d3d11_present_api complete{
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
} // namespace

static_assert(sizeof(rsf_d3d11_present_api) == 184 && RSF_D3D11_PRESENT_API_VERSION == 1);
static_assert(RSF_OK == 0 && RSF_ERROR_INVALID_ARGUMENT == -1 && RSF_ERROR_ABI_MISMATCH == -2);
extern "C" rsf_result rsf_core_d3d11p_get_api(rsf_d3d11_present_api*, const rsf_d3d11_present_api*);

extern "C" RSF_RUNTIME_API rsf_result rsf_get_d3d11_present_api(rsf_d3d11_present_api* api) {
    return rsf_core_d3d11p_get_api(api, &complete);
}
