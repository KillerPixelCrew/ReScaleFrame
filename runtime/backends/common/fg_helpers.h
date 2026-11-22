// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/frame_generation.h>
#include <dxgi1_4.h>
extern "C" rsf_backend_result rsf_core_fg12_frame(const rsf_fg_frame*, void*);
extern "C" rsf_backend_result rsf_core_fg12_command(void*, void*);
extern "C" rsf_backend_result rsf_core_fg12_setup_header(const rsf_generation_setup*, void**, void**);
extern "C" void rsf_core_fg12_chain_description(const rsf_fg_swapchain_desc*, DXGI_SWAP_CHAIN_DESC1*);
extern "C" const rsf_generation_provider* rsf_core_fg12_disabled_provider(void);

namespace rsf {
inline rsf_backend_result fg_setup_header(const rsf_generation_setup* setup, void** out, void** chain)
{
    return rsf_core_fg12_setup_header(setup, out, chain);
}
inline rsf_backend_result fg_frame(const rsf_fg_frame* frame, void* device)
{
    return rsf_core_fg12_frame(frame, device);
}
inline rsf_backend_result fg_command(void* command, void* device)
{
    return rsf_core_fg12_command(command, device);
}
inline DXGI_SWAP_CHAIN_DESC1 chain_description(const rsf_fg_swapchain_desc& desc)
{
    DXGI_SWAP_CHAIN_DESC1 chain{};
    rsf_core_fg12_chain_description(&desc, &chain);
    return chain;
}
// The provider a vendor backend exposes when its SDK was not compiled in: it validates the setup
// header like the real one, then reports NOT_COMPILED, which is a different thing from failing.
inline const rsf_generation_provider* not_compiled_provider()
{
    return rsf_core_fg12_disabled_provider();
}
}
