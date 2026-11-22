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
// This façade leaves the caller in control of submission and retirement.
extern "C" {
struct rsf_core_capture12;
struct rsf_core_capture12_platform;
const rsf_core_capture12_platform* rsf_core_capture12_native_platform();
rsf_core_capture12* rsf_core_capture12_create(uint32_t environment);
void rsf_core_capture12_destroy(rsf_core_capture12*);
int32_t rsf_core_capture12_enabled(const rsf_core_capture12*);
int32_t rsf_core_capture12_configure(const rsf_core_capture12*, const char* path, uint64_t first, uint32_t frames);
int32_t rsf_core_capture12_wants(const rsf_core_capture12*, uint64_t frame);
int32_t rsf_core_capture12_force_reset(const rsf_core_capture12*, uint64_t frame);
int32_t rsf_core_capture12_event(const rsf_core_capture12*, const rsf_frame_record*, const char* name,
    uint64_t sequence, uint32_t reset, int32_t result);
int32_t rsf_core_capture12_record(const rsf_core_capture12*, void* list, void* source, uint32_t state,
    const rsf_frame_record*, const char* role, uint32_t slot, uint32_t ordinal, uint32_t reset,
    uint32_t preview, float preview_scale);
int32_t rsf_core_capture12_flush(const rsf_core_capture12*, uint32_t slot, uint32_t ready_only);
int32_t rsf_core_capture12_submitted(const rsf_core_capture12*, void* queue, uint32_t slot, const rsf_frame_record*);
void rsf_core_capture12_abandon(const rsf_core_capture12*, uint32_t slot);
int32_t rsf_core_capture12_flush_after_queue(const rsf_core_capture12*, void* queue,
    int32_t (*wait_submissions)(void*), void* user);
}

namespace rsf {
// Opt-in, bounded GPU evidence. Each caller must establish actual submission
// completion before flushing a slot. No capture work runs without the env var.
class FrameCapture12 {
    rsf_core_capture12* owner = nullptr;

public:
    static const rsf_core_capture12_platform* native_platform() noexcept {
        return rsf_core_capture12_native_platform();
    }

    explicit FrameCapture12(bool environment = true) noexcept
        : owner(rsf_core_capture12_create(environment)) {}
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

    // Diagnostic callers confirm SDK submission for the captured batch before the queue drain.
    bool flush_after_queue(ID3D12CommandQueue* queue, int32_t (*wait_submissions)(void*), void* user) noexcept {
        return rsf_core_capture12_flush_after_queue(owner, queue, wait_submissions, user) != 0;
    }
};
}
