/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/input_dump.h>
#include <rescaleframe/sha256_file.h>

/* Services contributed by native archives to the Rust dump owner. */
struct rsf_dump_services {
    uint32_t struct_size;
    decltype(&rsf_common_sha256_file) hash;
    decltype(&rsf_dump_texture_bytes) raw;
    decltype(&rsf_dump_texture_png) png;
    decltype(&rsf_dump_pixels_png) pixels_png;
    decltype(&rsf_game_depth_domains_requested) depth_domains_requested;
};
static_assert(sizeof(rsf_dump_services) == 48);
extern "C" void rsf_core_dump_bind_services(const rsf_dump_services*);
