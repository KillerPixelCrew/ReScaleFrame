// SPDX-License-Identifier: MIT
#pragma once

// First member of the SDK capsule. Rust owns these graphics slots. The
// swapchain slot is typed because the SDK retains its address.
struct IDXGISwapChain4;
struct rsf_fsr4fg_native_graphics {
    void* factory;
    void* adapter;
    void* device;
    void* queue;
    void* allocator;
    void* commands;
    void* completion;
    void* shared;
    IDXGISwapChain4* chain;
    void* event;
};
