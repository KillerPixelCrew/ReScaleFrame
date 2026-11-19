// SPDX-License-Identifier: MIT
#include <rescaleframe/fg_session.h>
#include <windows.h>
#include <d3d12.h>

namespace {
uint64_t completed(void* fence)
{
    return static_cast<ID3D12Fence*>(fence)->GetCompletedValue();
}
}

extern "C" rsf_backend_result rsf_core_fg_session_create(const rsf_fg_host*, rsf_fg_session**,
    uint64_t (*completed)(void*), decltype(&rsf_frame_allows_fg), decltype(&rsf_frame_has_rendered_source));

extern "C" rsf_backend_result rsf_fg_session_create(const rsf_fg_host* host, rsf_fg_session** out)
{
    return rsf_core_fg_session_create(host, out, completed, rsf_frame_allows_fg, rsf_frame_has_rendered_source);
}
