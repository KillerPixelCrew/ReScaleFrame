// SPDX-License-Identifier: GPL-3.0-only
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
#include <cwchar>

namespace rsf {
// Opt-in, bounded GPU evidence. Each caller must establish actual submission
// completion before flushing a slot. No capture work runs without the env var.
class FrameCapture12 {
    struct Image {
        Microsoft::WRL::ComPtr<ID3D12Resource> buffer;
        Microsoft::WRL::ComPtr<ID3D12Resource> source_lease;
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
        uint64_t bytes = 0;
        rsf_frame_record record{};
        std::string role;
        uint32_t slot = 0, ordinal = 0, dispatch_reset = UINT32_MAX;
        uint64_t core_ticket = 0, completion_value = 0;
        std::string core_name;
        bool legacy = false;
        Microsoft::WRL::ComPtr<ID3D12Fence> completion;
    };
    std::filesystem::path directory;
    uint64_t first = 30, count = 4;
    bool reset_sr = false;
    mutable std::mutex guard;
    std::vector<std::unique_ptr<Image>> images;

public:
    explicit FrameCapture12(bool environment = true) noexcept try {
        if (!environment) return;
        wchar_t path[32768]{};
        const DWORD size = GetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_DIR", path, 32768);
        if (!size || size >= 32768) return;
        directory = path;
        wchar_t value[32]{};
        if (GetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_FIRST", value, 32)) first = std::wcstoull(value, nullptr, 10);
        if (GetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_COUNT", value, 32)) count = (std::min)(uint64_t(16), std::wcstoull(value, nullptr, 10));
        if (GetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_RESET_SR", value, 32)) reset_sr = std::wcscmp(value, L"1") == 0;
        std::filesystem::create_directories(directory);
    } catch (...) { directory.clear(); }

    bool enabled() const noexcept { return !directory.empty() && count; }
    bool wants(uint64_t frame) const noexcept { return enabled() && frame >= first && frame - first < count; }
    bool force_sr_history_reset(uint64_t frame) const noexcept { return reset_sr && wants(frame); }
    bool pending() const { std::lock_guard lock(guard); return !images.empty(); }

    bool event(const rsf_frame_record& identity, const char* name, uint64_t sequence, bool reset, int32_t result) noexcept try {
        if (!wants(identity.frame_id)) return true;
        const auto metadata = directory / "metadata";
        std::filesystem::create_directories(metadata);
        std::ofstream json(metadata / (std::to_string(identity.session_id) + "-" + std::to_string(identity.frame_id) + "-" + name + ".json"));
        LARGE_INTEGER ticks{}, frequency{};
        QueryPerformanceCounter(&ticks); QueryPerformanceFrequency(&frequency);
        json << "{\"frame\":" << identity.frame_id << ",\"session\":" << identity.session_id
            << ",\"view\":" << identity.view_id << ",\"generation\":" << identity.resource_generation
            << ",\"flags\":" << identity.flags << ",\"sdk_sequence\":" << sequence
            << ",\"frame_time_ms\":" << identity.frame_time_ms
            << ",\"reset\":" << (reset ? 1 : 0) << ",\"result\":" << result
            << ",\"cpu_ticks\":" << ticks.QuadPart << ",\"cpu_tick_frequency\":" << frequency.QuadPart
            << ",\"thread\":" << GetCurrentThreadId() << "}\n";
        return bool(json);
    } catch (...) { return false; }

    bool record(ID3D12GraphicsCommandList* list, ID3D12Resource* source, D3D12_RESOURCE_STATES state,
        const rsf_frame_record& identity, const char* role, uint32_t slot = 0, uint32_t ordinal = 0, uint32_t dispatch_reset = UINT32_MAX,
        rsf_dump_view preview = UINT32_MAX, float preview_scale = 0) noexcept try {
        flush_ready();
        const bool legacy = wants(identity.frame_id);
        const bool core = rsf_input_dump_wants(&identity) != 0;
        if ((!legacy && !core) || !list || !source) return false;
        auto image = std::make_unique<Image>();
        const auto desc = source->GetDesc();
        if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count != 1 || desc.DepthOrArraySize != 1) return false;
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        if (FAILED(source->GetDevice(IID_PPV_ARGS(&device)))) return false;
        UINT rows = 0; UINT64 row_bytes = 0;
        device->GetCopyableFootprints(&desc, 0, 1, 0, &image->footprint, &rows, &row_bytes, &image->bytes);
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
        D3D12_RESOURCE_DESC storage{}; storage.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        storage.Width = image->bytes; storage.Height = storage.DepthOrArraySize = storage.MipLevels = 1;
        storage.SampleDesc.Count = 1; storage.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &storage, D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&image->buffer)))) return false;
        image->record = identity; image->role = role; image->slot = slot; image->ordinal = ordinal;
        image->dispatch_reset = dispatch_reset;
        image->source_lease = source;
        image->legacy = legacy;
        if (core) {
            image->core_name = capture_name(role, ordinal);
            image->core_ticket = rsf_input_dump_reserve(&identity, image->core_name.c_str(), role,
                preview == UINT32_MAX ? capture_view(image->core_name) : preview, preview_scale ? preview_scale : capture_scale(role));
        }
        if (!legacy && !image->core_ticket) return false;
        std::lock_guard lock(guard);
        images.push_back(std::move(image)); // Retain allocation before recording references to it.
        Image& target = *images.back();
        transition(list, source, state, D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = source; from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = target.buffer.Get(); to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        to.PlacedFootprint = target.footprint;
        list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
        transition(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, state);
        return true;
    } catch (...) { return false; }

    // Called only after the exact recording slot's completion fence has completed.
    bool flush(uint32_t slot = UINT32_MAX, bool ready_only = false) noexcept try {
        std::lock_guard lock(guard);
        bool okay = true;
        for (auto it = images.begin(); it != images.end();) {
            Image& image = **it;
            if (slot != UINT32_MAX && image.slot != slot) { ++it; continue; }
            if (ready_only) {
                if (!image.completion) { ++it; continue; }
                const auto completed = image.completion->GetCompletedValue();
                if (completed == UINT64_MAX) {
                    rsf_input_dump_failed(image.core_ticket, image.core_name.c_str(), RSF_TEXTURE_ERROR_MAP_FAILED);
                    okay = false; it = images.erase(it); continue;
                }
                if (completed < image.completion_value) { ++it; continue; }
            }
            void* data = nullptr;
            const D3D12_RANGE range{0, static_cast<size_t>(image.bytes)};
            if (FAILED(image.buffer->Map(0, &range, &data))) { okay = false; ++it; continue; }
            if (image.core_ticket) {
                rsf_dump_pixels pixels{sizeof(pixels), image.footprint.Footprint.Width, image.footprint.Footprint.Height,
                    uint32_t(image.footprint.Footprint.Format), image.footprint.Footprint.RowPitch,
                    image.bytes - image.footprint.Offset, static_cast<const char*>(data) + image.footprint.Offset};
                okay &= rsf_input_dump_pixels(image.core_ticket, image.core_name.c_str(), &pixels, 2, pixels.format) != 0;
            }
            if (!image.legacy) {
                const D3D12_RANGE written{0, 0}; image.buffer->Unmap(0, &written);
                it = images.erase(it); continue;
            }
            const std::string name = std::to_string(image.record.session_id) + "-" + std::to_string(image.record.frame_id) + "-" + image.role + "-" + std::to_string(image.ordinal);
            const auto file = directory / name;
            auto raw_path = file; raw_path += L".raw";
            std::ofstream raw(raw_path, std::ios::binary);
            raw.write(static_cast<const char*>(data), static_cast<std::streamsize>(image.bytes));
            const D3D12_RANGE written{0, 0}; image.buffer->Unmap(0, &written);
            auto json_path = file; json_path += L".json";
            std::ofstream json(json_path);
            const auto& camera = image.record.camera;
            json << std::setprecision(9) << "{\n\"role\":\"" << image.role << "\",\n\"frame\":" << image.record.frame_id
                << ",\n\"session\":" << image.record.session_id << ",\n\"view\":" << image.record.view_id
                << ",\n\"generation\":" << image.record.resource_generation << ",\n\"flags\":" << image.record.flags
                << ",\n\"frame_time_ms\":" << image.record.frame_time_ms
                << ",\n\"width\":" << image.footprint.Footprint.Width << ",\n\"height\":" << image.footprint.Footprint.Height
                << ",\n\"row_pitch\":" << image.footprint.Footprint.RowPitch << ",\n\"offset\":" << image.footprint.Offset
                << ",\n\"dxgi_format\":" << image.footprint.Footprint.Format << ",\n\"bytes\":" << image.bytes
                << ",\n\"render_size\":[" << camera.render_width << ',' << camera.render_height << "],\n\"output_size\":[" << camera.output_width << ',' << camera.output_height
                << "],\n\"jitter_pixels\":[" << camera.jitter_pixels[0] << ',' << camera.jitter_pixels[1] << "],\n\"matrix_convention\":\"row_vectors\"";
            auto matrix = [&json](const char* key, const float* values) {
                json << ",\n\"" << key << "\":[";
                for (uint32_t i = 0; i < 16; ++i) { if (i) json << ','; json << values[i]; }
                json << ']';
            };
            matrix("view_to_clip", camera.view_to_clip); matrix("clip_to_view", camera.clip_to_view);
            matrix("view_to_world", camera.view_to_world); matrix("world_to_view", camera.world_to_view);
            matrix("clip_to_previous_clip", camera.clip_to_previous_clip);
            if (image.dispatch_reset != UINT32_MAX) json << ",\n\"sdk_dispatch_reset\":" << image.dispatch_reset;
            json << "\n}\n";
            okay &= bool(raw) && bool(json);
            it = images.erase(it);
        }
        return okay;
    } catch (...) { return false; }

    // Fence only copies actually submitted on this queue. Polling later never waits the CPU.
    bool submitted(ID3D12CommandQueue* queue, uint32_t slot = UINT32_MAX) noexcept try {
        std::lock_guard lock(guard);
        if (images.empty() || !queue) return true;
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        Microsoft::WRL::ComPtr<ID3D12Fence> fence;
        if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device))) ||
            FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence))) || FAILED(queue->Signal(fence.Get(), 1))) return false;
        for (auto& image : images) if (!image->completion && (slot == UINT32_MAX || image->slot == slot)) {
            image->completion = fence; image->completion_value = 1;
        }
        return true;
    } catch (...) { return false; }
    bool flush_ready() noexcept { return flush(UINT32_MAX, true); }
    void abandon(uint32_t slot) noexcept try {
        std::lock_guard lock(guard);
        for (auto it = images.begin(); it != images.end();) {
            if ((*it)->slot != slot || (*it)->completion) { ++it; continue; }
            rsf_input_dump_failed((*it)->core_ticket, (*it)->core_name.c_str(), RSF_TEXTURE_ERROR_MAP_FAILED);
            it = images.erase(it);
        }
    } catch (...) {}

    // Diagnostic caller only, after the SDK has submitted its callback lists on this queue.
    bool flush_after_queue(ID3D12CommandQueue* queue) noexcept {
        if (!pending() || !queue) return true;
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        Microsoft::WRL::ComPtr<ID3D12Fence> fence;
        if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device))) || FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return false;
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!event) return false;
        const bool complete = SUCCEEDED(queue->Signal(fence.Get(), 1)) && SUCCEEDED(fence->SetEventOnCompletion(1, event)) && WaitForSingleObject(event, 15000) == WAIT_OBJECT_0;
        CloseHandle(event);
        return complete && flush();
    }
};
}
