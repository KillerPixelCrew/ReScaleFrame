/* SPDX-License-Identifier: MIT */
#pragma once

#include <rescaleframe/texture_dump.h>
#include <stddef.h>

enum rsf_texture_dump_output : uint32_t {
    RSF_TEXTURE_OUTPUT_TARGA,
    RSF_TEXTURE_OUTPUT_PNG,
    RSF_TEXTURE_OUTPUT_RAW,
};

// WIC implementation used by the Rust preview writer.
struct rsf_texture_dump_platform {
    uint32_t struct_size;
    uint32_t (*write_png)(const char*, uint32_t, uint32_t, uint32_t, uint32_t, uint8_t*);
};

extern "C" rsf_dump_texture_result rsf_core_texture_dump_texture(
    const rsf_texture_dump_platform*, void*, void*, void*, const rsf_texture_dump_options*,
    rsf_texture_dump_report*, uint32_t);

extern "C" rsf_dump_texture_result rsf_core_texture_dump_pixels(
    const rsf_texture_dump_platform*, const rsf_dump_pixels*, const rsf_texture_dump_options*,
    rsf_texture_dump_report*);
