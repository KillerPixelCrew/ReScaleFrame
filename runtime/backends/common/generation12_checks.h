/* SPDX-License-Identifier: MIT */
#pragma once
#include "fg_helpers.h"
#include "generation12_platform.h"

extern "C" rsf_backend_result rsf_core_fg12_setup(const rsf_generation_setup*);
extern "C" uint32_t rsf_core_fg12_streamline_loaded(void);

namespace rsf {
inline const rsf_gen12_checks generation12_checks{
    [](const rsf_generation_setup* setup, void** out, void** chain) {
        const auto header = fg_setup_header(setup, out, chain);
        return header == RSF_BACKEND_OK ? rsf_core_fg12_setup(setup) : header;
    },
    fg_frame, fg_command, rsf_core_fg12_streamline_loaded,
    [](const rsf_generation_setup* setup, const uint16_t* name) -> void* {
        static_assert(sizeof(wchar_t) == sizeof(uint16_t));
        rsf_sr_open_desc desc{};
        desc.runtime_directory_utf8 = setup->runtime_directory_utf8;
        desc.log = setup->chain.log;
        desc.log_user = setup->chain.log_user;
        return load_runtime(desc, reinterpret_cast<const wchar_t*>(name));
    },
};
}
