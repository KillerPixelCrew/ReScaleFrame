// SPDX-License-Identifier: MIT
// C++ exception guards and the render bridge used by the Rust GameSDK export.
#include "fallout4_platform.h"
#include "render_hooks_platform.h"
#include <d3d11.h>
#include <cstddef>

namespace {

// A native call that throws reports false; the Rust lifecycle then answers RSF_ERROR_NOT_READY
// as the previous native catch did.
template<class Call> bool guarded(Call&& call) noexcept {
    try { call(); return true; } catch (...) { return false; }
}

}

extern "C" bool rsf_fallout4_native_log(rsf_game_log_fn log, void* user, const char* message) noexcept {
    return guarded([&] { if (log) log(user, message); });
}

extern "C" bool rsf_fallout4_native_host_create(rsf_fallout4_host_create_fn create, const rsf_d3d11_host_setup* setup, rsf_game_host_services* services,
        int32_t* result) noexcept {
    return guarded([&] { *result = create ? create(setup, services) : RSF_ERROR_NOT_READY; });
}

extern "C" bool rsf_fallout4_native_host_set_active(rsf_fallout4_host_active_fn activate, void* user, uint32_t active, int32_t* result) noexcept {
    return guarded([&] { *result = activate ? activate(user, active) : RSF_ERROR_NOT_READY; });
}

extern "C" bool rsf_fallout4_native_creation_begin(rsf_fallout4_creation_begin_fn begin, void* user, rsf_d3d11_creation_scope* scope, int32_t* result) noexcept {
    return guarded([&] { *result = begin ? begin(user, scope) : RSF_ERROR_NOT_READY; });
}

extern "C" bool rsf_fallout4_native_creation_end(rsf_fallout4_creation_end_fn end, void* user, const rsf_d3d11_creation_scope* scope,
        const rsf_d3d11_native_chain* outputs, int32_t* result) noexcept {
    return guarded([&] { *result = end ? end(user, scope, outputs) : RSF_ERROR_NOT_READY; });
}

extern "C" bool rsf_fallout4_native_bind_presentation(rsf_fallout4_bind_presentation_fn bind, void* user, const rsf_d3d11_presentation_binding* binding,
        int32_t* result) noexcept {
    return guarded([&] { *result = bind ? bind(user, binding) : RSF_ERROR_NOT_READY; });
}

extern "C" void rsf_fallout4_publish_render_boundary() noexcept {
    (void)rsf::fallout4::rsf_fallout4_render_publish(rsf::fallout4::render_hook_leaves());
}

// Enforce the original native exception boundary before an exception can reach Rust.
static_assert(sizeof(rsf_fallout4_create_call) == 88 && offsetof(rsf_fallout4_create_call, immediate_context) == 80);
static_assert(sizeof(D3D_DRIVER_TYPE) == sizeof(int32_t) && sizeof(D3D_FEATURE_LEVEL) == sizeof(int32_t));
extern "C" int32_t rsf_fallout4_call_creation(void* original, const rsf_fallout4_create_call* call) noexcept {
    const auto native_create = reinterpret_cast<decltype(&D3D11CreateDeviceAndSwapChain)>(original);
    return native_create(static_cast<IDXGIAdapter*>(call->adapter), D3D_DRIVER_TYPE(call->driver_type),
        static_cast<HMODULE>(call->software), call->flags,
        reinterpret_cast<const D3D_FEATURE_LEVEL*>(call->feature_levels), call->feature_level_count,
        call->sdk_version, static_cast<const DXGI_SWAP_CHAIN_DESC*>(call->swapchain_desc),
        reinterpret_cast<IDXGISwapChain**>(call->swapchain), reinterpret_cast<ID3D11Device**>(call->device),
        reinterpret_cast<D3D_FEATURE_LEVEL*>(call->feature_level),
        reinterpret_cast<ID3D11DeviceContext**>(call->immediate_context));
}
