/* SPDX-License-Identifier: MIT */
#pragma once
#include "fg_helpers.h"
#include "generation12_platform.h"

extern "C" const rsf_gen12_checks* rsf_core_fg12_generation_checks(
    rsf_backend_result (*frame)(const rsf_fg_frame*, void*),
    void* (*load_library)(const rsf_generation_setup*, const uint16_t*));

namespace rsf {
// The SDK-independent loader retains the legacy path bound and Windows error log.
inline void* load_generation_library(const rsf_generation_setup* setup, const uint16_t* name)
{
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    rsf_sr_open_desc desc{};
    desc.runtime_directory_utf8 = setup->runtime_directory_utf8;
    desc.log = setup->chain.log;
    desc.log_user = setup->chain.log_user;
    return load_runtime(desc, reinterpret_cast<const wchar_t*>(name));
}

inline const rsf_gen12_checks* generation12_checks()
{
    return rsf_core_fg12_generation_checks(fg_frame, load_generation_library);
}
}
