// SPDX-License-Identifier: MIT
#include <rescaleframe/fg_session.h>

extern "C" uint64_t rsf_d3d12_fence_completed(void* fence);
extern "C" rsf_backend_result rsf_core_fg_session_create(const rsf_fg_host*, rsf_fg_session**,
    uint64_t (*completed)(void*), decltype(&rsf_frame_allows_fg), decltype(&rsf_frame_has_rendered_source));

extern "C" rsf_backend_result rsf_fg_session_create(const rsf_fg_host* host, rsf_fg_session** out)
{
    return rsf_core_fg_session_create(host, out, rsf_d3d12_fence_completed, rsf_frame_allows_fg, rsf_frame_has_rendered_source);
}
