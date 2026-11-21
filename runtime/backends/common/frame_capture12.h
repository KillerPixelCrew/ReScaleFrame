// SPDX-License-Identifier: MIT
#pragma once
#include "d3d12_helpers.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/capture_names.h>
#include <wrl/client.h>
#include <array>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <memory>
#include <mutex>
#include <vector>
#include <algorithm>
#include <cstring>
#include <cwchar>

// Rust (runtime/orchestrator/src/frame_capture12.rs) owns configuration,
// admission, pending identity, slot and fence completion, retirement, core
// ticket refusal and publishing, and the buffer, source and fence leases.
// These native leaves only issue typed D3D12 and Win32 calls.
extern "C" {
struct rsf_core_capture12;
struct rsf_core_capture12_layout { uint64_t bytes, offset; uint32_t width, height, format, row_pitch; };
struct rsf_core_capture12_platform {
    void* (*allocate)(void* source, rsf_core_capture12_layout*);
    void (*copy)(void* list, void* source, uint32_t state, void* buffer, const rsf_core_capture12_layout*);
    const uint8_t* (*map)(void* buffer, uint64_t bytes);
    void (*unmap)(void* buffer);
    void* (*signal)(void* queue);
    uint64_t (*completed)(void* fence);
    int32_t (*drain)(void* queue);
    uint32_t (*name)(const char* role, uint32_t ordinal, char* name, uint32_t capacity, uint32_t* view, float* scale);
    int (*wants)(const rsf_frame_record*);
    uint64_t (*reserve)(const rsf_frame_record*, const char*, const char*, rsf_dump_view, float);
    void (*failed)(uint64_t, const char*, int32_t);
    int (*pixels)(uint64_t, const char*, const rsf_dump_pixels*, uint32_t, uint32_t);
};
static_assert(sizeof(rsf_core_capture12_platform) == 12 * sizeof(void*));
static_assert(sizeof(rsf_core_capture12_layout) == 32 && sizeof(rsf_dump_pixels) == 40);
rsf_core_capture12* rsf_core_capture12_create(uint32_t environment, const rsf_core_capture12_platform*);
void rsf_core_capture12_destroy(rsf_core_capture12*);
int32_t rsf_core_capture12_enabled(const rsf_core_capture12*);
int32_t rsf_core_capture12_configure(const rsf_core_capture12*, const char* path, uint64_t first, uint32_t frames);
int32_t rsf_core_capture12_wants(const rsf_core_capture12*, uint64_t frame);
int32_t rsf_core_capture12_force_reset(const rsf_core_capture12*, uint64_t frame);
uint64_t rsf_core_capture12_pending(const rsf_core_capture12*);
int32_t rsf_core_capture12_event(const rsf_core_capture12*, const rsf_frame_record*, const char* name,
    uint64_t sequence, uint32_t reset, int32_t result);
int32_t rsf_core_capture12_record(const rsf_core_capture12*, void* list, void* source, uint32_t state,
    const rsf_frame_record*, const char* role, uint32_t slot, uint32_t ordinal, uint32_t reset,
    uint32_t preview, float preview_scale);
int32_t rsf_core_capture12_flush(const rsf_core_capture12*, uint32_t slot, uint32_t ready_only);
int32_t rsf_core_capture12_submitted(const rsf_core_capture12*, void* queue, uint32_t slot, const rsf_frame_record*);
void rsf_core_capture12_abandon(const rsf_core_capture12*, uint32_t slot);
int32_t rsf_core_capture12_flush_after_queue(const rsf_core_capture12*, void* queue);
}

namespace rsf {
// Opt-in, bounded GPU evidence. Each caller must establish actual submission
// completion before flushing a slot. No capture work runs without the env var.
class FrameCapture12 {
    rsf_core_capture12* owner = nullptr;

    static void* allocate_leaf(void* raw, rsf_core_capture12_layout* out) noexcept {
        auto* source = static_cast<ID3D12Resource*>(raw);
        const auto desc = source->GetDesc();
        if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count != 1 || desc.DepthOrArraySize != 1) return nullptr;
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        if (FAILED(source->GetDevice(IID_PPV_ARGS(&device)))) return nullptr;
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
        UINT rows = 0; UINT64 row_bytes = 0, bytes = 0;
        device->GetCopyableFootprints(&desc, 0, 1, 0, &footprint, &rows, &row_bytes, &bytes);
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
        D3D12_RESOURCE_DESC storage{}; storage.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        storage.Width = bytes; storage.Height = storage.DepthOrArraySize = storage.MipLevels = 1;
        storage.SampleDesc.Count = 1; storage.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ID3D12Resource* buffer = nullptr;
        if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &storage, D3D12_RESOURCE_STATE_COPY_DEST,
            nullptr, IID_PPV_ARGS(&buffer)))) return nullptr;
        *out = {bytes, footprint.Offset, footprint.Footprint.Width, footprint.Footprint.Height,
            uint32_t(footprint.Footprint.Format), footprint.Footprint.RowPitch};
        return buffer;
    }
    static void copy_leaf(void* list, void* raw, uint32_t state, void* buffer, const rsf_core_capture12_layout* layout) noexcept {
        auto* commands = static_cast<ID3D12GraphicsCommandList*>(list);
        auto* source = static_cast<ID3D12Resource*>(raw);
        const auto before = static_cast<D3D12_RESOURCE_STATES>(state);
        transition(commands, source, before, D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = source; from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = static_cast<ID3D12Resource*>(buffer); to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        to.PlacedFootprint = {layout->offset, {static_cast<DXGI_FORMAT>(layout->format), layout->width, layout->height, 1, layout->row_pitch}};
        commands->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
        transition(commands, source, D3D12_RESOURCE_STATE_COPY_SOURCE, before);
    }
    static const uint8_t* map_leaf(void* buffer, uint64_t bytes) noexcept {
        void* data = nullptr;
        const D3D12_RANGE range{0, static_cast<size_t>(bytes)};
        return SUCCEEDED(static_cast<ID3D12Resource*>(buffer)->Map(0, &range, &data)) ? static_cast<const uint8_t*>(data) : nullptr;
    }
    static void unmap_leaf(void* buffer) noexcept {
        const D3D12_RANGE written{0, 0};
        static_cast<ID3D12Resource*>(buffer)->Unmap(0, &written);
    }
    static void* signal_leaf(void* raw) noexcept {
        auto* queue = static_cast<ID3D12CommandQueue*>(raw);
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        Microsoft::WRL::ComPtr<ID3D12Fence> fence;
        if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device))) ||
            FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence))) || FAILED(queue->Signal(fence.Get(), 1))) return nullptr;
        return fence.Detach();
    }
    static uint64_t completed_leaf(void* fence) noexcept { return static_cast<ID3D12Fence*>(fence)->GetCompletedValue(); }
    static int32_t drain_leaf(void* raw) noexcept {
        auto* queue = static_cast<ID3D12CommandQueue*>(raw);
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        Microsoft::WRL::ComPtr<ID3D12Fence> fence;
        if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device))) || FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return 0;
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!event) return 0;
        const bool complete = SUCCEEDED(queue->Signal(fence.Get(), 1)) && SUCCEEDED(fence->SetEventOnCompletion(1, event)) && WaitForSingleObject(event, 15000) == WAIT_OBJECT_0;
        CloseHandle(event);
        return complete;
    }
    static uint32_t name_leaf(const char* role, uint32_t ordinal, char* out, uint32_t capacity, uint32_t* view, float* scale) noexcept try {
        const auto name = capture_name(role, ordinal);
        if (name.size() >= UINT32_MAX) return 0;
        const auto size = uint32_t(name.size() + 1);
        if (size <= capacity) {
            std::memcpy(out, name.c_str(), size);
            *view = capture_view(name); *scale = capture_scale(role);
        }
        return size;
    } catch (...) { return 0; }
    static const rsf_core_capture12_platform* platform() noexcept {
        static const rsf_core_capture12_platform value{allocate_leaf, copy_leaf, map_leaf, unmap_leaf, signal_leaf,
            completed_leaf, drain_leaf, name_leaf, rsf_input_dump_wants, rsf_input_dump_reserve, rsf_input_dump_failed,
            rsf_input_dump_pixels};
        return &value;
    }

public:
    explicit FrameCapture12(bool environment = true) noexcept
        : owner(rsf_core_capture12_create(environment, platform())) {}
    ~FrameCapture12() { rsf_core_capture12_destroy(owner); }
    FrameCapture12(const FrameCapture12&) = delete;
    FrameCapture12& operator=(const FrameCapture12&) = delete;

    bool enabled() const noexcept { return rsf_core_capture12_enabled(owner) != 0; }
    // Configure an owner explicitly before recording, without changing the process environment.
    bool configure(const char* path, uint64_t first_frame, uint32_t frames) noexcept {
        return rsf_core_capture12_configure(owner, path, first_frame, frames) != 0;
    }
    bool wants(uint64_t frame) const noexcept { return rsf_core_capture12_wants(owner, frame) != 0; }
    bool force_sr_history_reset(uint64_t frame) const noexcept { return rsf_core_capture12_force_reset(owner, frame) != 0; }
    bool pending() const { return rsf_core_capture12_pending(owner) != 0; }

    bool event(const rsf_frame_record& identity, const char* name, uint64_t sequence, bool reset, int32_t result) noexcept {
        return rsf_core_capture12_event(owner, &identity, name, sequence, reset, result) != 0;
    }

    bool record(ID3D12GraphicsCommandList* list, ID3D12Resource* source, D3D12_RESOURCE_STATES state,
        const rsf_frame_record& identity, const char* role, uint32_t slot = 0, uint32_t ordinal = 0, uint32_t dispatch_reset = UINT32_MAX,
        rsf_dump_view preview = UINT32_MAX, float preview_scale = 0) noexcept {
        return rsf_core_capture12_record(owner, list, source, uint32_t(state), &identity, role, slot, ordinal,
            dispatch_reset, uint32_t(preview), preview_scale) != 0;
    }

    // Called only after the exact recording slot's completion fence has completed.
    bool flush(uint32_t slot = UINT32_MAX, bool ready_only = false) noexcept {
        return rsf_core_capture12_flush(owner, slot, ready_only) != 0;
    }

    // Fence only copies actually submitted on this queue. Polling later never waits the CPU.
    bool submitted(ID3D12CommandQueue* queue, uint32_t slot = UINT32_MAX,
        const rsf_frame_record* identity = nullptr) noexcept {
        return rsf_core_capture12_submitted(owner, queue, slot, identity) != 0;
    }
    bool flush_ready() noexcept { return flush(UINT32_MAX, true); }
    void abandon(uint32_t slot) noexcept { rsf_core_capture12_abandon(owner, slot); }

    // Diagnostic caller only, after the SDK has submitted its callback lists on this queue.
    bool flush_after_queue(ID3D12CommandQueue* queue) noexcept {
        return rsf_core_capture12_flush_after_queue(owner, queue) != 0;
    }
};
}
