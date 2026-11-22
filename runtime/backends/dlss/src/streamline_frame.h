/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/streamline_host.h>

#ifdef __cplusplus
extern "C" {
#endif
/* Internal to this backend module, never part of the public DLL or game ABI. The shared D3D12
   DLSS SR sets its frame's common constants through the host that owns the SDK tokens.
   `constants` points at an sl::Constants-layout record (rsf_slvk_sdk_constants). Returns nonzero
   when the host accepted them. */
uint32_t rsf_streamline_common_set(rsf_streamline_host* host, uint64_t source, uint32_t viewport,
    const void* constants);
#ifdef __cplusplus
}
#endif
