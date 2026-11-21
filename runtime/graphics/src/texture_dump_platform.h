/* SPDX-License-Identifier: MIT */
#pragma once

#include <rescaleframe/texture_dump.h>
#include <stddef.h>

enum rsf_texture_dump_output : uint32_t {
    RSF_TEXTURE_OUTPUT_TARGA,
    RSF_TEXTURE_OUTPUT_PNG,
    RSF_TEXTURE_OUTPUT_RAW,
};

struct rsf_texture_dump_desc {
    uint32_t width, height, format, mip_levels, array_size, samples;
    uint32_t immediate, context_on_device, texture_on_device;
};

struct rsf_texture_dump_mapping {
    const uint8_t* data;
    uint32_t row_pitch;
};

// Private call table. The Rust owner holds the staging resource, mapping and
// file handles for the duration of the corresponding native leases.
struct rsf_texture_dump_platform {
    uint32_t struct_size;
    uint32_t (*describe)(void*, void*, void*, rsf_texture_dump_desc*);
    void* (*stage)(void*, const rsf_texture_dump_desc*);
    void (*copy)(void*, void*, void*, uint32_t);
    uint32_t (*map)(void*, void*, rsf_texture_dump_mapping*);
    void (*unmap)(void*, void*);
    void* (*open_file)(const char*);
    uint32_t (*write_file)(void*, const uint8_t*, size_t);
    uint32_t (*close_file)(void*);
    uint32_t (*write_png)(const char*, uint32_t, uint32_t, uint32_t, uint32_t, uint8_t*);
};

extern "C" rsf_dump_texture_result rsf_core_texture_dump_texture(
    const rsf_texture_dump_platform*, void*, void*, void*, const rsf_texture_dump_options*,
    rsf_texture_dump_report*, uint32_t);

extern "C" rsf_dump_texture_result rsf_core_texture_dump_pixels(
    const rsf_texture_dump_platform*, const rsf_dump_pixels*, const rsf_texture_dump_options*,
    rsf_texture_dump_report*);
