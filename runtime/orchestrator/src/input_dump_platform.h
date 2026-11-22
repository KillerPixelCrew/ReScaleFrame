/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/input_dump.h>
#include <rescaleframe/sha256_file.h>

/* Services contributed by native archives to the Rust dump owner. */
typedef struct rsf_dump_services {
    uint32_t struct_size;
    uint32_t (*hash)(const wchar_t*, uint32_t, unsigned char*);
    rsf_dump_texture_result (*raw)(void*, void*, void*, const rsf_texture_dump_options*);
    rsf_dump_texture_result (*png)(void*, void*, void*, const rsf_texture_dump_options*, rsf_texture_dump_report*);
    rsf_dump_texture_result (*pixels_png)(const rsf_dump_pixels*, const rsf_texture_dump_options*, rsf_texture_dump_report*);
    uint32_t (*depth_domains_requested)(const rsf_game_depth_domains*);
} rsf_dump_services;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_core_dump_bind_services(const rsf_dump_services*);
#ifdef __cplusplus
}
#endif
