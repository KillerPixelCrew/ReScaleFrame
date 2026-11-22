/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <rescaleframe/game_api.h>

extern "C" {
void* rsf_d3d11_host_fixture_texture(void* device, uint32_t width, uint32_t height,
    uint32_t format, uint32_t bindings);
int32_t rsf_d3d11_host_fixture_read_texture(void* device, void* context, void* source,
    uint32_t stride, unsigned char* bytes, size_t length);
void rsf_d3d11_host_fixture_cpu(const rsf_game_host_services* host, uint64_t source,
    uint32_t stage, uint32_t render_expected);
}
