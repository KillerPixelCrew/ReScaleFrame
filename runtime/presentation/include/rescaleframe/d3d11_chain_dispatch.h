// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

#define RSF_D3D11_DISPATCH_RELEASE 1u
#define RSF_D3D11_DISPATCH_PRESENT 2u
#define RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE 3u
#define RSF_D3D11_DISPATCH_RESIZE_BUFFERS 4u
#define RSF_D3D11_DISPATCH_PRESENT1 5u
#define RSF_D3D11_DISPATCH_SET_COLOR_SPACE1 6u
#define RSF_D3D11_DISPATCH_RESIZE_BUFFERS1 7u

typedef struct rsf_d3d11_chain_dispatch {
    uint32_t struct_size, abi_version, method, handled;
    void* receiver;
    union {
        struct { uint32_t sync_interval, flags; } present;
        /* Synchronous borrowed DXGI_PRESENT_PARAMETERS, including its native arrays. */
        struct { uint32_t sync_interval, flags; const void* parameters; } present1;
        struct { uint32_t fullscreen; void* target; } fullscreen;
        struct { uint32_t buffer_count, width, height, format, flags; } resize_buffers;
        struct { uint32_t color_space; } color_space;
        /* Borrowed arrays use native ResizeBuffers1 buffer_count semantics. */
        struct {
            uint32_t buffer_count, width, height, format, flags;
            const uint32_t* node_masks;
            void* const* present_queues;
        } resize_buffers1;
    } args;
    union { int32_t hresult; uint32_t references; } result;
} rsf_d3d11_chain_dispatch;
