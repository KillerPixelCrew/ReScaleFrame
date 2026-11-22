/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/frame_generation.h>

#ifdef __cplusplus
extern "C" {
#endif

struct DXGI_SWAP_CHAIN_DESC1;
rsf_backend_result rsf_core_fg12_frame(const rsf_fg_frame*, void* device);
rsf_backend_result rsf_core_fg12_command(void* command, void* device);
rsf_backend_result rsf_core_fg12_setup_header(const rsf_generation_setup*, void** session, void** chain);
void rsf_core_fg12_chain_description(const rsf_fg_swapchain_desc*, struct DXGI_SWAP_CHAIN_DESC1*);
const rsf_generation_provider* rsf_core_fg12_disabled_provider(void);

#ifdef __cplusplus
}
#endif
