// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "capture_api.h"
#include "../../common/include/rescaleframe/sha256_file.h"
#include <atomic>
#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <map>
#include <memory>
#include <mutex>
#include <vector>

/* Private ABI for one pinned Intel binary, recovered from setter RVA20E1C0 and
   resource notifier RVAs20AB10/20AC40. This is not a public Intel SDK interface.
   Exact fingerprint admission precedes registration. Unknown callbacks stay0. */
namespace rsf_xess_capture {
struct DiagnosticsV2 {
    uint32_t version = 2, reserved = 0;
    void* user = nullptr;
    void* before_present = nullptr;
    void* before_commands = nullptr;
    void* after_commands = nullptr;
    void* constants = nullptr;
    void* before_inputs = nullptr;
    void (*input)(void*, uint64_t, uint32_t, const xefg_swapchain_d3d12_resource_data_t*, ID3D12GraphicsCommandList*) = nullptr;
    void* after_inputs = nullptr;
    void (*before_outputs)(void*, uint64_t, ID3D12GraphicsCommandList*, uint32_t) = nullptr;
    void (*output)(void*, uint64_t, uint32_t, ID3D12Resource*, ID3D12GraphicsCommandList*, uint32_t) = nullptr;
    void* after_outputs = nullptr;
};
static_assert(sizeof(DiagnosticsV2) == 0x60 && offsetof(DiagnosticsV2, input) == 0x38 && offsetof(DiagnosticsV2, output) == 0x50);
static_assert(sizeof(xefg_swapchain_d3d12_resource_data_t) == 0x28);
using SetDiagnostics = int (*)(xefg_swapchain_handle_t, const DiagnosticsV2*);

class Capture {
    struct Image {
        ID3D12Resource* readback = nullptr;
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
        uint64_t bytes = 0, token = 0, source_resource = 0, command_list = 0;
        uint32_t ordinal = 0, kind = 0, generated_count = 0, actual_count = 0;
        bool generated = false;
        rsf_frame_record source{};
        ~Image() { if (readback) readback->Release(); }
    };
    std::filesystem::path directory;
    uint64_t first = 30, count = 4, retained_bytes = 0;
    bool probe = false;
    SetDiagnostics set_diagnostics = nullptr;
    xefg_swapchain_handle_t context = nullptr;
    struct Source { rsf_frame_record record; uint32_t generated_count, actual_count; };
    std::map<uint32_t, Source> sources;
    std::vector<std::unique_ptr<Image>> pending;
    mutable std::mutex guard;
    std::atomic<uint64_t> callbacks{0};
    bool wants(uint64_t frame) const { return count && frame >= first && frame - first < count; }
    void failure(const char* reason) noexcept try {
        std::ofstream(directory / "xess-capture-errors.log", std::ios::app) << reason << '\n';
    } catch (...) {}
    static bool fingerprint(HMODULE module) {
        wchar_t path[32768]{};
        DWORD size = GetModuleFileNameW(module, path, 32768);
        if (!size || size >= 32768) return false;
        unsigned char actual[32]{};
        constexpr unsigned char expected[32]{0xec,0x5e,0x0c,0x65,0xe0,0x75,0x57,0x0c,0x6e,0xde,0x72,0x61,0x8b,0xb6,0x66,0xd0,
            0xbe,0x0c,0x2e,0x10,0xb2,0xea,0x97,0x62,0xc0,0xfe,0x8c,0xb8,0xe3,0x75,0xab,0x27};
        return rsf::sha256_file(path, actual) && std::memcmp(actual, expected, sizeof(actual)) == 0;
    }
    static void transition(ID3D12GraphicsCommandList* commands, ID3D12Resource* resource,
        D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
        if (before == after) return;
        D3D12_RESOURCE_BARRIER barrier{}; barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition.pResource = resource; barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        barrier.Transition.StateBefore = before; barrier.Transition.StateAfter = after;
        commands->ResourceBarrier(1, &barrier);
    }
    void observe(uint64_t token, uint32_t kind, ID3D12Resource* resource,
        ID3D12GraphicsCommandList* commands, uint32_t ordinal, bool generated, D3D12_RESOURCE_STATES state) noexcept try {
        std::lock_guard lock(guard);
        const auto source = sources.find(static_cast<uint32_t>(token));
        if (source == sources.end() || !resource || !commands) return;
        if (generated) callbacks.fetch_add(1);
        if (!wants(source->second.record.frame_id)) return;
        const auto desc = resource->GetDesc();
        // Preserve the exact borrowed resource and command-list identities in the
        // receipt. No callback pointer or opaque ordinal is a temporal fraction.
        std::ofstream events(directory / "xess-diagnostic-callbacks.jsonl", std::ios::app);
        events << "{\"frame\":" << source->second.record.frame_id << ",\"token\":" << token
            << ",\"kind\":" << kind << ",\"ordinal\":" << ordinal << ",\"generated\":" << (generated ? "true" : "false")
            << ",\"resource\":" << reinterpret_cast<uint64_t>(resource) << ",\"commands\":" << reinterpret_cast<uint64_t>(commands)
            << ",\"width\":" << desc.Width << ",\"height\":" << desc.Height << ",\"format\":" << desc.Format
            << ",\"state\":" << state << ",\"fraction_established\":false}\n";
        if (probe) return;
        if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count != 1 || desc.DepthOrArraySize != 1 ||
            desc.MipLevels != 1 || desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM || desc.Width != source->second.record.output_width ||
            desc.Height != source->second.record.output_height || ordinal >= 15) { failure("callback-resource-contract"); return; }
        for (const auto& image : pending)
            if (image->source.frame_id == source->second.record.frame_id && image->generated == generated && image->ordinal == ordinal) {
                failure("duplicate-callback-resource"); return;
            }
        if (pending.size() >= 64) { failure("capture-count-budget"); return; }
        auto image = std::make_unique<Image>();
        ID3D12Device* device = nullptr;
        if (FAILED(resource->GetDevice(IID_PPV_ARGS(&device)))) { failure("resource-device"); return; }
        UINT rows = 0; UINT64 row_bytes = 0;
        device->GetCopyableFootprints(&desc, 0, 1, 0, &image->footprint, &rows, &row_bytes, &image->bytes);
        if (image->bytes > 256ull * 1024 * 1024 - retained_bytes) { device->Release(); failure("capture-byte-budget"); return; }
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
        D3D12_RESOURCE_DESC buffer{}; buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        buffer.Width = image->bytes; buffer.Height = buffer.DepthOrArraySize = buffer.MipLevels = 1;
        buffer.SampleDesc.Count = 1; buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        const HRESULT created = device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &buffer,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&image->readback));
        device->Release();
        if (FAILED(created)) { failure("readback-allocation"); return; }
        image->source = source->second.record; image->generated_count = source->second.generated_count;
        image->actual_count = source->second.actual_count;
        image->token = token; image->kind = kind; image->ordinal = ordinal; image->generated = generated;
        image->source_resource = reinterpret_cast<uint64_t>(resource); image->command_list = reinterpret_cast<uint64_t>(commands);
        auto* retained = image.get(); retained_bytes += image->bytes;
        pending.push_back(std::move(image)); // Own the allocation before recording any GPU reference.
        transition(commands, resource, state, D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = resource; from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = retained->readback; to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        to.PlacedFootprint = retained->footprint;
        commands->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
        transition(commands, resource, D3D12_RESOURCE_STATE_COPY_SOURCE, state);
    } catch (...) { failure("callback-exception"); }
    static void input(void* user, uint64_t token, uint32_t kind,
        const xefg_swapchain_d3d12_resource_data_t* data, ID3D12GraphicsCommandList* commands) noexcept {
        if (user && data) {
            auto* capture = static_cast<Capture*>(user);
            if (capture->probe || kind == XEFG_SWAPCHAIN_RES_BACKBUFFER)
                capture->observe(token, kind, data->pResource, commands, 0, false, data->incomingState);
        }
    }
    static void output(void* user, uint64_t token, uint32_t kind,
        ID3D12Resource* resource, ID3D12GraphicsCommandList* commands, uint32_t ordinal) noexcept {
        if (user && kind == 0) static_cast<Capture*>(user)->observe(token, kind, resource, commands, ordinal, true, D3D12_RESOURCE_STATE_COMMON);
    }
    static void before_outputs(void* user, uint64_t token, ID3D12GraphicsCommandList*, uint32_t count) noexcept try {
        if (!user) return;
        auto& capture = *static_cast<Capture*>(user);
        std::lock_guard lock(capture.guard);
        const auto found = capture.sources.find(static_cast<uint32_t>(token));
        if (found != capture.sources.end()) found->second.actual_count = count <= 15 ? count : 0;
    } catch (...) {}
public:
    Capture() noexcept { try {
        wchar_t path[32768]{}, value[32]{};
        DWORD size = GetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", path, 32768);
        if (!size || size >= 32768) return;
        directory = path;
        if (GetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_FIRST", value, 32)) first = std::wcstoull(value, nullptr, 10);
        if (GetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_COUNT", value, 32)) count = (std::min)(uint64_t(16), std::wcstoull(value, nullptr, 10));
        probe = GetEnvironmentVariableW(L"RSF_XESS_DIAGNOSTIC_PROBE", value, 32) && std::wcscmp(value, L"1") == 0;
        std::filesystem::create_directories(directory);
    } catch (...) { directory.clear(); } }
    bool enabled() const { return !directory.empty() && count; }
    bool initialize(HMODULE module, xefg_swapchain_handle_t fg) {
        if (!enabled()) return true;
        if (!fingerprint(module)) { failure("diagnostic-runtime-fingerprint"); return false; }
        set_diagnostics = reinterpret_cast<SetDiagnostics>(GetProcAddress(module, "xefgSwapChainD3D12SetDiagnosticCallbacks"));
        if (!set_diagnostics) { failure("diagnostic-export"); return false; }
        DiagnosticsV2 diagnostics; diagnostics.user = this; diagnostics.input = input;
        diagnostics.before_outputs = before_outputs; diagnostics.output = output;
        int result = set_diagnostics(fg, &diagnostics);
        if (result < 0) { failure("diagnostic-registration"); return false; }
        context = fg;
        return true;
    }
    bool source(const rsf_frame_record& record, uint32_t generated_count) noexcept try {
        if (!enabled()) return true;
        std::lock_guard lock(guard);
        sources[static_cast<uint32_t>(record.frame_id)] = {record, generated_count, 0};
        while (sources.size() > 32) sources.erase(sources.begin());
        return true;
    } catch (...) { failure("source-binding"); return false; }
    uint64_t observed() const { return callbacks.load(); }
    // The bridge disables/drains SDK work and completes its exact DX12 fence
    // before this call. No readback map or allocation release precedes completion.
    bool flush() noexcept try {
        std::lock_guard lock(guard);
        bool okay = true;
        for (const auto& image : pending) {
            const auto& record = image->source; const auto& camera = record.camera;
            const char* role = image->generated ? "fg-xess-generated-before-present" : "fg-xess-sdk-backbuffer-input";
            const std::string stem = std::to_string(record.session_id) + '-' + std::to_string(record.frame_id) + '-' + role + '-' + std::to_string(image->ordinal);
            void* pixels = nullptr; const D3D12_RANGE range{0, static_cast<SIZE_T>(image->bytes)};
            if (FAILED(image->readback->Map(0, &range, &pixels))) { okay = false; failure("readback-map"); continue; }
            std::ofstream raw(directory / (stem + ".raw"), std::ios::binary);
            raw.write(static_cast<const char*>(pixels), static_cast<std::streamsize>(image->bytes));
            const D3D12_RANGE written{0, 0}; image->readback->Unmap(0, &written);
            std::ofstream json(directory / (stem + ".json"));
            json << std::setprecision(9) << "{\n\"role\":\"" << role << "\",\n\"frame\":" << record.frame_id
                << ",\n\"session\":" << record.session_id << ",\n\"view\":" << record.view_id << ",\n\"generation\":" << record.resource_generation
                << ",\n\"flags\":" << record.flags << ",\n\"frame_time_ms\":" << record.frame_time_ms
                << ",\n\"width\":" << image->footprint.Footprint.Width << ",\n\"height\":" << image->footprint.Footprint.Height
                << ",\n\"row_pitch\":" << image->footprint.Footprint.RowPitch << ",\n\"offset\":" << image->footprint.Offset
                << ",\n\"dxgi_format\":" << image->footprint.Footprint.Format << ",\n\"bytes\":" << image->bytes
                << ",\n\"fg_backend\":5,\n\"sdk_token\":" << image->token << ",\n\"sdk_output_kind\":" << image->kind
                << ",\n\"sdk_array_ordinal\":" << image->ordinal << ",\n\"fraction_established\":false"
                << ",\n\"requested_generated_count\":" << image->generated_count
                << ",\n\"observed_output_count\":" << image->actual_count
                << ",\n\"generated\":" << (image->generated ? "true" : "false")
                << ",\n\"capture_boundary\":\"" << (image->generated ? "sdk-interpolation-output-callback" : "sdk-input-backbuffer-callback") << '"'
                << ",\n\"provider_sha256\":\"ec5e0c65e075570c6ede72618bb666d0be0c2e10b2ea9762c0fe8cb8e375ab27\""
                << ",\n\"source_resource\":" << image->source_resource << ",\n\"command_list\":" << image->command_list
                << ",\n\"completion\":\"bridge-owned-dx12-queue-drained\",\n\"render_size\":[" << camera.render_width << ',' << camera.render_height
                << "],\n\"output_size\":[" << camera.output_width << ',' << camera.output_height << "],\n\"jitter_pixels\":[" << camera.jitter_pixels[0] << ',' << camera.jitter_pixels[1]
                << "],\n\"matrix_convention\":\"row_vectors\"";
            auto matrix = [&json](const char* name, const float* values) {
                json << ",\n\"" << name << "\":[";
                for (uint32_t i = 0; i < 16; ++i) { if (i) json << ','; json << values[i]; }
                json << ']';
            };
            matrix("view_to_clip", camera.view_to_clip); matrix("clip_to_view", camera.clip_to_view);
            matrix("view_to_world", camera.view_to_world); matrix("world_to_view", camera.world_to_view);
            matrix("clip_to_previous_clip", camera.clip_to_previous_clip);
            json << "\n}\n";
            okay &= bool(raw) && bool(json);
        }
        pending.clear(); retained_bytes = 0;
        if (context && set_diagnostics && set_diagnostics(context, nullptr) < 0) { failure("diagnostic-unregister"); return false; }
        context = nullptr;
        return okay;
    } catch (...) { failure("readback-flush-exception"); return false; }
};
}
