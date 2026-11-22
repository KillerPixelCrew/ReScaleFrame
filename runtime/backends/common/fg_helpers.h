// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/frame_generation.h>
#include "sr_helpers.h"
#include <dxgi1_4.h>
extern "C" rsf_backend_result rsf_core_fg12_frame(const rsf_fg_frame*, void*,
    int (*allows_fg)(const rsf_frame_record*), int (*rendered)(const rsf_frame_record*));
extern "C" rsf_backend_result rsf_core_fg12_command(void*, void*);

namespace rsf {
inline rsf_backend_result fg_setup_header(const rsf_generation_setup* setup, void** out, void** chain)
{
    if (out) *out = nullptr;
    if (chain) *chain = nullptr;
    if (!setup || !out || !chain || setup->struct_size < sizeof(*setup) ||
        setup->chain.struct_size < sizeof(rsf_fg_swapchain_desc)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_FG_ABI_VERSION || setup->chain.abi_version != RSF_BACKEND_ABI_VERSION)
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    return RSF_BACKEND_OK;
}
inline rsf_backend_result fg_frame(const rsf_fg_frame* frame, void* device)
{
    return rsf_core_fg12_frame(frame, device, rsf_frame_allows_fg, rsf_frame_has_rendered_source);
}
inline rsf_backend_result fg_command(void* command, void* device)
{
    return rsf_core_fg12_command(command, device);
}
inline DXGI_SWAP_CHAIN_DESC1 chain_description(const rsf_fg_swapchain_desc& desc)
{
    DXGI_SWAP_CHAIN_DESC1 chain{};
    chain.Width = desc.width; chain.Height = desc.height; chain.Format = static_cast<DXGI_FORMAT>(desc.format);
    chain.SampleDesc.Count = 1; chain.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    chain.BufferCount = desc.buffer_count; chain.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    chain.Flags = desc.allow_tearing ? DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING : 0;
    return chain;
}
// The provider a vendor backend exposes when its SDK was not compiled in: it validates the setup
// header like the real one, then reports NOT_COMPILED, which is a different thing from failing.
inline const rsf_generation_provider* not_compiled_provider()
{
    struct Stub {
        static rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
        {
            const auto result = fg_setup_header(setup, out, chain);
            return result != 0 ? result : RSF_BACKEND_ERROR_NOT_COMPILED;
        }
        static rsf_backend_result configure(void*, const rsf_fg_options*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result begin(void*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result marker(void*, rsf_latency_marker, uint64_t, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result prepare(void*, void*, const rsf_fg_frame*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result after(void*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result status(void*, rsf_fg_status*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result retirement(void*, rsf_fg_retirement*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static void destroy(void*) {}
        static rsf_backend_result abort_frame(void*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
    };
    static const rsf_generation_provider provider{sizeof(provider), Stub::create, Stub::configure, Stub::begin,
        Stub::marker, Stub::prepare, Stub::after, Stub::status, Stub::retirement, Stub::destroy, Stub::abort_frame, nullptr};
    return &provider;
}
}
