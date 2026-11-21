// SPDX-License-Identifier: MIT
#pragma once
#include "capture_api.h"
#include "session_abi.h"
#include "../../common/include/rescaleframe/sha256_file.h"
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <string>

/* Native leaves of the opt-in XeSS-FG diagnostic capture. The Rust owner in
   capture.rs decides configuration, source identity, admission, budgets,
   duplicates, the callback counter and completion-gated publication, and owns
   the pending readbacks. These leaves keep the typed SDK callback table, the
   DX12 readback, copy and map calls, and the on-disk formats. */

// Records shared with runtime/vulkan/xess-fg/capture.rs.
struct rsf_xess_capture_desc {
    uint64_t width;
    uint32_t height, dimension, format, depth_or_array_size, mip_levels, sample_count;
};
struct rsf_xess_capture_event {
    uint64_t frame, token, resource, commands, width;
    uint32_t kind, ordinal, generated, height, format, state;
};
// Owns one readback reference. The source and command list are identities only.
struct rsf_xess_capture_image {
    ID3D12Resource* readback;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint;
    uint64_t bytes, token, source_resource, command_list;
    uint32_t ordinal, kind, generated_count, actual_count, generated, reserved;
    rsf_frame_record source;
};
static_assert(sizeof(rsf_xess_capture_desc) == 32);
static_assert(sizeof(rsf_xess_capture_event) == 64 && offsetof(rsf_xess_capture_event, kind) == 40 &&
    offsetof(rsf_xess_capture_event, state) == 60);
static_assert(sizeof(D3D12_PLACED_SUBRESOURCE_FOOTPRINT) == 32 &&
    offsetof(D3D12_PLACED_SUBRESOURCE_FOOTPRINT, Footprint) == 8 && offsetof(D3D12_SUBRESOURCE_FOOTPRINT, RowPitch) == 16);
static_assert(sizeof(rsf_frame_record) == 480 && alignof(rsf_frame_record) == 8);
static_assert(offsetof(rsf_frame_record, frame_id) == 8 && offsetof(rsf_frame_record, output_width) == 52 &&
    offsetof(rsf_frame_record, output_height) == 56 && offsetof(rsf_frame_record, input_qpc) == 64);
static_assert(sizeof(rsf_xess_capture_image) == 576 && alignof(rsf_xess_capture_image) == 8);
static_assert(offsetof(rsf_xess_capture_image, footprint) == 8 && offsetof(rsf_xess_capture_image, bytes) == 40 &&
    offsetof(rsf_xess_capture_image, ordinal) == 72 && offsetof(rsf_xess_capture_image, generated) == 88 &&
    offsetof(rsf_xess_capture_image, source) == 96);

extern "C" {
// Zero, or -7012 when the source cannot be bound.
int32_t rsf_core_xess_capture_source(rsf_xess_capture_owner*, const rsf_frame_record*, uint32_t);
void rsf_core_xess_capture_input(rsf_xess_capture_owner*, uint64_t, uint32_t, uint32_t, void*, void*, uint32_t);
void rsf_core_xess_capture_output(rsf_xess_capture_owner*, uint64_t, uint32_t, void*, void*, uint32_t);
void rsf_core_xess_capture_before_outputs(rsf_xess_capture_owner*, uint64_t, uint32_t);
uint64_t rsf_core_xess_capture_callbacks(const rsf_xess_capture_owner*);
}

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

// Results of Publish, matched in capture.rs.
inline constexpr int32_t kPublished = 0, kMapFailed = -1, kWriteFailed = -2, kPublishException = -3;

// The Rust owner lends a NUL-terminated UTF-16 directory for the call.
inline std::filesystem::path Directory(const uint16_t* directory) {
    return std::filesystem::path(reinterpret_cast<const wchar_t*>(directory));
}

// The SDK user pointer is the Rust capture owner.
inline void Input(void* user, uint64_t token, uint32_t kind,
    const xefg_swapchain_d3d12_resource_data_t* data, ID3D12GraphicsCommandList* commands) noexcept {
    if (user && data)
        rsf_core_xess_capture_input(static_cast<rsf_xess_capture_owner*>(user), token, kind,
            kind == XEFG_SWAPCHAIN_RES_BACKBUFFER, data->pResource, commands,
            static_cast<uint32_t>(data->incomingState));
}

inline void Output(void* user, uint64_t token, uint32_t kind,
    ID3D12Resource* resource, ID3D12GraphicsCommandList* commands, uint32_t ordinal) noexcept {
    if (user && kind == 0)
        rsf_core_xess_capture_output(static_cast<rsf_xess_capture_owner*>(user), token, kind, resource, commands, ordinal);
}

inline void BeforeOutputs(void* user, uint64_t token, ID3D12GraphicsCommandList*, uint32_t count) noexcept {
    if (user) rsf_core_xess_capture_before_outputs(static_cast<rsf_xess_capture_owner*>(user), token, count);
}

inline uint32_t Fingerprint(void* module) {
    wchar_t path[32768]{};
    DWORD size = GetModuleFileNameW(static_cast<HMODULE>(module), path, 32768);
    if (!size || size >= 32768) return 0;
    unsigned char actual[32]{};
    constexpr unsigned char expected[32]{0xec,0x5e,0x0c,0x65,0xe0,0x75,0x57,0x0c,0x6e,0xde,0x72,0x61,0x8b,0xb6,0x66,0xd0,
        0xbe,0x0c,0x2e,0x10,0xb2,0xea,0x97,0x62,0xc0,0xfe,0x8c,0xb8,0xe3,0x75,0xab,0x27};
    return rsf::sha256_file(path, actual) && std::memcmp(actual, expected, sizeof(actual)) == 0;
}

inline void Failure(const uint16_t* directory, const char* reason) noexcept try {
    std::ofstream(Directory(directory) / "xess-capture-errors.log", std::ios::app) << reason << '\n';
} catch (...) {}

inline void Describe(void* resource, rsf_xess_capture_desc* output) {
    const D3D12_RESOURCE_DESC desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    output->width = desc.Width;
    output->height = desc.Height;
    output->dimension = static_cast<uint32_t>(desc.Dimension);
    output->format = static_cast<uint32_t>(desc.Format);
    output->depth_or_array_size = desc.DepthOrArraySize;
    output->mip_levels = desc.MipLevels;
    output->sample_count = desc.SampleDesc.Count;
}

inline int32_t WriteEvent(const uint16_t* directory, const rsf_xess_capture_event* event) noexcept try {
    std::ofstream events(Directory(directory) / "xess-diagnostic-callbacks.jsonl", std::ios::app);
    events << "{\"frame\":" << event->frame << ",\"token\":" << event->token
        << ",\"kind\":" << event->kind << ",\"ordinal\":" << event->ordinal << ",\"generated\":" << (event->generated ? "true" : "false")
        << ",\"resource\":" << event->resource << ",\"commands\":" << event->commands
        << ",\"width\":" << event->width << ",\"height\":" << event->height << ",\"format\":" << static_cast<DXGI_FORMAT>(event->format)
        << ",\"state\":" << static_cast<D3D12_RESOURCE_STATES>(event->state) << ",\"fraction_established\":false}\n";
    return 0;
} catch (...) {
    return -1;
}

// Returns a device reference for the readback allocation, or -1 without one.
inline int32_t QueryFootprint(void* resource, void* footprint, uint64_t* bytes, void** device) {
    auto* source = static_cast<ID3D12Resource*>(resource);
    ID3D12Device* owner = nullptr;
    if (FAILED(source->GetDevice(IID_PPV_ARGS(&owner)))) return -1;
    const D3D12_RESOURCE_DESC desc = source->GetDesc();
    UINT rows = 0; UINT64 row_bytes = 0;
    owner->GetCopyableFootprints(&desc, 0, 1, 0, static_cast<D3D12_PLACED_SUBRESOURCE_FOOTPRINT*>(footprint),
        &rows, &row_bytes, bytes);
    *device = owner;
    return 0;
}

inline void* CreateReadback(void* device, uint64_t bytes) {
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer{}; buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = bytes; buffer.Height = buffer.DepthOrArraySize = buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1; buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* readback = nullptr;
    const HRESULT created = static_cast<ID3D12Device*>(device)->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE,
        &buffer, D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&readback));
    if (SUCCEEDED(created)) return readback;
    if (readback) readback->Release();
    return nullptr;
}

inline void ReleaseObject(void* object) {
    if (object) static_cast<IUnknown*>(object)->Release();
}

inline void Transition(ID3D12GraphicsCommandList* commands, ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    if (before == after) return;
    D3D12_RESOURCE_BARRIER barrier{}; barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = resource; barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = before; barrier.Transition.StateAfter = after;
    commands->ResourceBarrier(1, &barrier);
}

// Records into the borrowed SDK command list and restores the callback state.
inline void RecordCopy(void* commands, void* resource, void* readback, const void* footprint, uint32_t state) {
    auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
    auto* source = static_cast<ID3D12Resource*>(resource);
    const auto incoming = static_cast<D3D12_RESOURCE_STATES>(state);
    Transition(list, source, incoming, D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = source; from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = static_cast<ID3D12Resource*>(readback);
    to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint = *static_cast<const D3D12_PLACED_SUBRESOURCE_FOOTPRINT*>(footprint);
    list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    Transition(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, incoming);
}

// Called only after the bridge drained its DX12 queue and completed its fence.
inline int32_t Publish(const uint16_t* directory, const rsf_xess_capture_image* image) noexcept try {
    const auto& record = image->source; const auto& camera = record.camera;
    const char* role = image->generated ? "fg-xess-generated-before-present" : "fg-xess-sdk-backbuffer-input";
    const std::string stem = std::to_string(record.session_id) + '-' + std::to_string(record.frame_id) + '-' + role + '-' + std::to_string(image->ordinal);
    const std::filesystem::path base = Directory(directory);
    void* pixels = nullptr; const D3D12_RANGE range{0, static_cast<SIZE_T>(image->bytes)};
    if (FAILED(image->readback->Map(0, &range, &pixels))) return kMapFailed;
    std::ofstream raw(base / (stem + ".raw"), std::ios::binary);
    raw.write(static_cast<const char*>(pixels), static_cast<std::streamsize>(image->bytes));
    const D3D12_RANGE written{0, 0}; image->readback->Unmap(0, &written);
    std::ofstream json(base / (stem + ".json"));
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
    return raw && json ? kPublished : kWriteFailed;
} catch (...) {
    return kPublishException;
}
}
