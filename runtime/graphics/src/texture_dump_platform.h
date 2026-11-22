/* SPDX-License-Identifier: MIT */
#pragma once

#include <rescaleframe/texture_dump.h>
#include <stddef.h>

enum rsf_texture_dump_output : uint32_t {
    RSF_TEXTURE_OUTPUT_TARGA,
    RSF_TEXTURE_OUTPUT_PNG,
    RSF_TEXTURE_OUTPUT_RAW,
};

extern "C" rsf_dump_texture_result rsf_core_texture_dump_texture(
    void*, void*, void*, const rsf_texture_dump_options*,
    rsf_texture_dump_report*, uint32_t);

extern "C" rsf_dump_texture_result rsf_core_texture_dump_pixels(
    const rsf_dump_pixels*, const rsf_texture_dump_options*,
    rsf_texture_dump_report*);
