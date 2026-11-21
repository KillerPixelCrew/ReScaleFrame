/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/input_dump.h>

/* The frame dump archive supplies platform leaves to the shared Rust owner.
   Handles returned by open_texture and backbuffer transfer one COM reference. */
struct rsf_dump_clock {
    uint64_t tick;
    uint32_t process, year, month, day, hour, minute, second, millisecond;
    uint32_t key_down;
};
struct rsf_dump_surface {
    uint32_t width, height, format;
};
struct rsf_dump_encoding {
    int32_t raw, preview;
};
struct rsf_dump_platform {
    uint32_t struct_size;
    void (*clock)(rsf_dump_clock*);
    void (*local_time)(rsf_dump_clock*);
    void (*sha256)(const wchar_t*, char*);
    void* (*open_texture)(void*, void*, rsf_dump_surface*);
    void* (*backbuffer)(void*);
    void (*encode_texture)(void*, void*, const rsf_texture_dump_options*, uint32_t, rsf_dump_encoding*);
    int32_t (*preview_pixels)(const rsf_dump_pixels*, const rsf_texture_dump_options*);
    uint32_t (*depth_domains_requested)(const rsf_game_depth_domains*);
};
extern "C" void rsf_core_dump_bind_platform(const rsf_dump_platform*);
