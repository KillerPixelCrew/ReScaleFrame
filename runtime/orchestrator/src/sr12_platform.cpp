// SPDX-License-Identifier: MIT
// D3D12, vendor, capture and sharpening operations for the Rust generic D3D12 SR owner in sr12.rs.
// Each leaf performs one platform call. Resource ownership, validation, history, dispatch
// constants, ordering and the frame generation handoff are decided in Rust. Returned COM
// interfaces carry one reference for the caller; resource arguments are borrowed for the call.
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/dlss_native12.h>
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/srv_format.h>
#include "../../backends/common/d3d12_helpers.h"
#include "../../backends/common/frame_capture12.h"
#include "../../graphics/src/sharpening/d3d12.h"
#include "fg12_platform.h"
#include "sharpening_pass_options.h"
#include <windows.h>
#include <d3d12.h>
#include <wrl/client.h>
#include <new>

using Microsoft::WRL::ComPtr;

// Values sr12.rs names directly.
static_assert(DXGI_FORMAT_R32G32B32A32_FLOAT == 2 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10 &&
              DXGI_FORMAT_R32G32_FLOAT == 16 && DXGI_FORMAT_R32G8X24_TYPELESS == 19 &&
              DXGI_FORMAT_D32_FLOAT_S8X24_UINT == 20 && DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS == 21 &&
              DXGI_FORMAT_R11G11B10_FLOAT == 26 && DXGI_FORMAT_R16G16_FLOAT == 34 &&
              DXGI_FORMAT_R32_TYPELESS == 39 && DXGI_FORMAT_D32_FLOAT == 40 && DXGI_FORMAT_R32_FLOAT == 41 &&
              DXGI_FORMAT_R24G8_TYPELESS == 44 && DXGI_FORMAT_D24_UNORM_S8_UINT == 45 &&
              DXGI_FORMAT_R24_UNORM_X8_TYPELESS == 46 && DXGI_FORMAT_R16_FLOAT == 54);
static_assert(D3D12_RESOURCE_STATE_RENDER_TARGET == 0x4 && D3D12_RESOURCE_STATE_UNORDERED_ACCESS == 0x8 &&
              D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40 && D3D12_RESOURCE_STATE_COPY_DEST == 0x400 &&
              D3D12_RESOURCE_STATE_COPY_SOURCE == 0x800 && D3D12_RESOURCE_DIMENSION_TEXTURE2D == 3);
static_assert(RSF_SR_DLSS == 1 && RSF_SR_XESS == 5 && RSF_API_D3D12 == 2);
static_assert(RSF_DUMP_VIEW_RAW == 0 && RSF_DUMP_VIEW_HDR == 3);
static_assert(D3D12_TEXTURE_DATA_PITCH_ALIGNMENT == 256);

namespace {
struct TextureDesc {
    uint64_t width;
    uint32_t height, dimension, array_size, samples, format, srv_format;
};

template<class T> void* transfer(HRESULT result, T* object)
{
    if (SUCCEEDED(result)) return object;
    if (object) object->Release();
    return nullptr;
}

// The kernel and its root layout are retained from the C++ owner byte for byte.
constexpr char normalize_shader[] = R"(
Texture2D<float4> scene : register(t0);
Texture2D<float> depth : register(t1);
Texture2D<float2> motion : register(t2);
Texture2D<float> exposureInput : register(t3);
RWTexture2D<float4> sceneOut : register(u0);
RWTexture2D<float> depthOut : register(u1);
RWTexture2D<float2> motionOut : register(u2);
RWTexture2D<float> exposureOut : register(u3);
cbuffer Constants : register(b0) { uint2 extent; float2 toPixels; uint inputsOnly; uint externalExposure; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= extent)) return;
    if (!inputsOnly) sceneOut[id.xy] = scene.Load(int3(id.xy,0));
    depthOut[id.xy] = depth.Load(int3(id.xy,0));
    motionOut[id.xy] = motion.Load(int3(id.xy,0)) * toPixels;
    if (externalExposure && all(id.xy == 0)) exposureOut[uint2(0,0)] = exposureInput.Load(int3(0,0,0));
})";

int create_pipeline(void* device, rsf_backend_log_fn log, void* user, void** root_out, void** pipeline_out) try
{
    auto* owner = static_cast<ID3D12Device*>(device);
    D3D12_DESCRIPTOR_RANGE ranges[] = {
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 4, 0, 0, 0},
        {D3D12_DESCRIPTOR_RANGE_TYPE_UAV, 4, 0, 0, 4}};
    D3D12_ROOT_PARAMETER parameters[2]{};
    parameters[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    parameters[0].DescriptorTable = {2, ranges};
    parameters[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    parameters[1].Constants = {0, 0, 6};
    D3D12_ROOT_SIGNATURE_DESC desc{2, parameters, 0, nullptr, D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> serialized, errors;
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> pipeline;
    std::vector<uint8_t> shader;
    if (FAILED(D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &serialized, &errors)) ||
        FAILED(owner->CreateRootSignature(0, serialized->GetBufferPointer(), serialized->GetBufferSize(), IID_PPV_ARGS(&root))) ||
        !rsf::compile_shader(normalize_shader, sizeof(normalize_shader) - 1, "RSF temporal input normalization", "main",
            "cs_5_0", shader, log, user, D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3)) return 0;
    D3D12_COMPUTE_PIPELINE_STATE_DESC state{}; state.pRootSignature = root.Get();
    state.CS = {shader.data(), shader.size()};
    if (FAILED(owner->CreateComputePipelineState(&state, IID_PPV_ARGS(&pipeline)))) return 0;
    *root_out = root.Detach(); *pipeline_out = pipeline.Detach();
    return 1;
} catch (...) { return 0; }

void* create_heap(void* device)
{
    D3D12_DESCRIPTOR_HEAP_DESC desc{}; desc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    desc.NumDescriptors = 8; desc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    ID3D12DescriptorHeap* heap = nullptr;
    const HRESULT result = static_cast<ID3D12Device*>(device)->CreateDescriptorHeap(&desc, IID_PPV_ARGS(&heap));
    return transfer(result, heap);
}
// Created in UNORDERED_ACCESS.
void* create_texture(void* device, uint32_t width, uint32_t height, uint32_t format)
{
    ID3D12Resource* texture = nullptr;
    return rsf::create_uav_texture(static_cast<ID3D12Device*>(device), width, height,
        static_cast<DXGI_FORMAT>(format), &texture) ? texture : nullptr;
}
// The SDR exposure texture starts in COPY_DEST.
void* create_exposure_texture(void* device)
{
    auto* owner = static_cast<ID3D12Device*>(device);
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = 1; desc.Height = 1; desc.DepthOrArraySize = 1; desc.MipLevels = 1; desc.SampleDesc.Count = 1;
    desc.Format = DXGI_FORMAT_R32_FLOAT;
    ID3D12Resource* texture = nullptr;
    const auto result = owner->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&texture));
    return transfer(result, texture);
}

void* create_exposure_upload(void* device, uint64_t bytes)
{
    auto* owner = static_cast<ID3D12Device*>(device);
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_UPLOAD;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width = bytes; desc.Height = 1; desc.DepthOrArraySize = 1; desc.MipLevels = 1; desc.SampleDesc.Count = 1;
    desc.Format = DXGI_FORMAT_UNKNOWN; desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* upload = nullptr;
    const auto result = owner->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&upload));
    return transfer(result, upload);
}

int write_exposure_upload(void* resource, float value)
{
    auto* upload = static_cast<ID3D12Resource*>(resource);
    void* mapped = nullptr; D3D12_RANGE read{0, 0};
    if (FAILED(upload->Map(0, &read, &mapped))) return 0;
    if (!mapped) {
        upload->Unmap(0, &read);
        return 0;
    }
    *static_cast<float*>(mapped) = value;
    const D3D12_RANGE written{0, sizeof(float)};
    upload->Unmap(0, &written);
    return 1;
}
// Created in COPY_DEST.
void* create_readback(void* device, uint64_t bytes)
{
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; desc.Width = bytes;
    desc.Height = desc.DepthOrArraySize = desc.MipLevels = 1; desc.SampleDesc.Count = 1; desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* buffer = nullptr;
    const HRESULT result = static_cast<ID3D12Device*>(device)->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&buffer));
    return transfer(result, buffer);
}

void describe(void* resource, TextureDesc* out)
{
    const auto desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    *out = {desc.Width, desc.Height, uint32_t(desc.Dimension), desc.DepthOrArraySize, desc.SampleDesc.Count,
        uint32_t(desc.Format), uint32_t(rsf::srv_format(desc.Format))};
}
// Resources and command lists are both device children.
int owned_by(void* child, void* device)
{
    ComPtr<ID3D12Device> owner;
    return SUCCEEDED(static_cast<ID3D12DeviceChild*>(child)->GetDevice(IID_PPV_ARGS(&owner))) &&
        owner.Get() == static_cast<ID3D12Device*>(device);
}

void write_srv(void* device, void* heap, uint32_t index, void* resource, uint32_t format)
{
    auto* owner = static_cast<ID3D12Device*>(device);
    auto cpu = static_cast<ID3D12DescriptorHeap*>(heap)->GetCPUDescriptorHandleForHeapStart();
    cpu.ptr += size_t(index) * owner->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    D3D12_SHADER_RESOURCE_VIEW_DESC view{}; view.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    view.Format = static_cast<DXGI_FORMAT>(format);
    view.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING; view.Texture2D.MipLevels = 1;
    owner->CreateShaderResourceView(static_cast<ID3D12Resource*>(resource), &view, cpu);
}
void write_uav(void* device, void* heap, uint32_t index, void* resource, uint32_t format)
{
    auto* owner = static_cast<ID3D12Device*>(device);
    auto cpu = static_cast<ID3D12DescriptorHeap*>(heap)->GetCPUDescriptorHandleForHeapStart();
    cpu.ptr += size_t(index) * owner->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    D3D12_UNORDERED_ACCESS_VIEW_DESC view{}; view.ViewDimension = D3D12_UAV_DIMENSION_TEXTURE2D;
    view.Format = static_cast<DXGI_FORMAT>(format);
    owner->CreateUnorderedAccessView(static_cast<ID3D12Resource*>(resource), nullptr, &view, cpu);
}
void dispatch(void* list, void* heap, void* root, void* pipeline, const uint32_t* constants,
    uint32_t groups_x, uint32_t groups_y)
{
    auto* commands = static_cast<ID3D12GraphicsCommandList*>(list);
    auto* descriptors = static_cast<ID3D12DescriptorHeap*>(heap);
    commands->SetDescriptorHeaps(1, &descriptors);
    commands->SetComputeRootSignature(static_cast<ID3D12RootSignature*>(root));
    commands->SetComputeRootDescriptorTable(0, descriptors->GetGPUDescriptorHandleForHeapStart());
    commands->SetPipelineState(static_cast<ID3D12PipelineState*>(pipeline));
    commands->SetComputeRoot32BitConstants(1, 6, constants, 0);
    commands->Dispatch(groups_x, groups_y, 1);
}
void transition(void* list, void* resource, uint32_t before, uint32_t after)
{
    rsf::transition(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(resource),
        static_cast<D3D12_RESOURCE_STATES>(before), static_cast<D3D12_RESOURCE_STATES>(after));
}
// Subresource 0 to subresource 0 at the origin, limited to width x height.
void copy_region(void* list, void* target, void* source, uint32_t width, uint32_t height)
{
    D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = static_cast<ID3D12Resource*>(source); from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = static_cast<ID3D12Resource*>(target); to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    const D3D12_BOX rectangle{0, 0, 0, width, height, 1};
    static_cast<ID3D12GraphicsCommandList*>(list)->CopyTextureRegion(&to, 0, 0, 0, &from, &rectangle);
}
void copy_unit_exposure(void* list, void* target, void* upload)
{
    D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = static_cast<ID3D12Resource*>(upload); from.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    from.PlacedFootprint.Footprint = {DXGI_FORMAT_R32_FLOAT, 1, 1, 1, D3D12_TEXTURE_DATA_PITCH_ALIGNMENT};
    D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = static_cast<ID3D12Resource*>(target); to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    static_cast<ID3D12GraphicsCommandList*>(list)->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
}
// One 8x8 R16G16_FLOAT tile into a 256-byte-pitch readback footprint at offset.
void copy_motion_tile(void* list, void* buffer, uint64_t offset, void* source, uint32_t x, uint32_t y)
{
    D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = static_cast<ID3D12Resource*>(source); from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = static_cast<ID3D12Resource*>(buffer); to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint.Offset = offset;
    to.PlacedFootprint.Footprint = {DXGI_FORMAT_R16G16_FLOAT, 8, 8, 1, 256};
    const D3D12_BOX box{x, y, 0, x + 8, y + 8, 1};
    static_cast<ID3D12GraphicsCommandList*>(list)->CopyTextureRegion(&to, 0, 0, 0, &from, &box);
}
const uint8_t* map(void* buffer, uint64_t bytes)
{
    void* mapped = nullptr; D3D12_RANGE range{0, static_cast<size_t>(bytes)};
    return SUCCEEDED(static_cast<ID3D12Resource*>(buffer)->Map(0, &range, &mapped)) ? static_cast<const uint8_t*>(mapped) : nullptr;
}
void unmap(void* buffer)
{
    D3D12_RANGE written{0, 0}; static_cast<ID3D12Resource*>(buffer)->Unmap(0, &written);
}

uint32_t depth_domains(const rsf_game_render_pass* pass) { return rsf_game_depth_domains_requested(&pass->depth_domains); }

// The bounded original-pixel capture owner. Construction reads the process environment.
void* capture_create() { return new (std::nothrow) rsf::FrameCapture12(); }
void capture_destroy(void* capture) { delete static_cast<rsf::FrameCapture12*>(capture); }
int capture_enabled(void* capture) { return static_cast<rsf::FrameCapture12*>(capture)->enabled(); }
int capture_configure(void* capture, const char* directory, uint64_t first, uint32_t frames)
{
    return static_cast<rsf::FrameCapture12*>(capture)->configure(directory, first, frames);
}
int capture_flush(void* capture, uint32_t slot) { return static_cast<rsf::FrameCapture12*>(capture)->flush(slot); }
int capture_record(void* capture, void* list, void* source, uint32_t state, const rsf_frame_record* record,
    const char* role, uint32_t slot, uint32_t ordinal, uint32_t dispatch_reset, uint32_t preview)
{
    return static_cast<rsf::FrameCapture12*>(capture)->record(static_cast<ID3D12GraphicsCommandList*>(list),
        static_cast<ID3D12Resource*>(source), static_cast<D3D12_RESOURCE_STATES>(state), *record, role, slot,
        ordinal, dispatch_reset, preview);
}
int capture_event(void* capture, const rsf_frame_record* record, const char* name, uint64_t sequence,
    uint32_t reset, int32_t result)
{
    return static_cast<rsf::FrameCapture12*>(capture)->event(*record, name, sequence, reset != 0, result);
}
int capture_wants(void* capture, uint64_t frame) { return static_cast<rsf::FrameCapture12*>(capture)->wants(frame); }
int capture_force_reset(void* capture, uint64_t frame)
{
    return static_cast<rsf::FrameCapture12*>(capture)->force_sr_history_reset(frame);
}

void* sharpen_create() { return new (std::nothrow) rsf::sharpening::D3D12(); }
void sharpen_destroy(void* pass) { delete static_cast<rsf::sharpening::D3D12*>(pass); }
int sharpening_valid(const rsf_sharpening_options* options) { return rsf::sharpening_settings::valid(*options); }
void sharpening_defaults(rsf_sharpening_options* options) { *options = rsf_sharpening_defaults(); }
float vendor_sharpness(const rsf_sharpening_options* options, uint32_t backend)
{
    return rsf::sharpening_settings::vendor_sharpness(*options, backend);
}
// One when the options are inactive or the pass was recorded, zero when an active pass refused.
int sharpen_run(void* pass, void* list, void* output, void* motion, void* depth, void* exposure,
    const rsf_sharpening_options* sharpen, uint32_t hdr, uint32_t depth_inverted, float near_plane, float far_plane,
    uint32_t slot) try
{
    const auto options = rsf::sharpening::options_from_request(*sharpen);
    if (!rsf::sharpening::active(options)) return 1;
    rsf::sharpening::Frame mapping{};
    mapping.hdr = hdr != 0; mapping.depth_inverted = depth_inverted;
    mapping.near_plane = near_plane; mapping.far_plane = far_plane;
    return static_cast<rsf::sharpening::D3D12*>(pass)->run(static_cast<ID3D12GraphicsCommandList*>(list),
        static_cast<ID3D12Resource*>(output), static_cast<ID3D12Resource*>(motion), static_cast<ID3D12Resource*>(depth),
        static_cast<ID3D12Resource*>(exposure), options, mapping, slot);
} catch (...) { return 0; }

// The shared frame generation owner binds its platform table before the Rust handoff.
void fg_bind() { rsf_fg12_platform_bind(); }

// Field order matches `Platform` in sr12.rs.
struct Platform {
    uint32_t struct_size;
    decltype(&rsf_dlss_native12_create) dlss_create;
    decltype(&rsf_dlss_native12_create_shared) dlss_create_shared;
    decltype(&rsf_dlss_native12_create_registered) dlss_create_registered;
    decltype(&rsf_dlss_native12_plan) dlss_plan;
    decltype(&rsf_dlss_native12_set_sdr) dlss_set_sdr;
    decltype(&rsf_dlss_native12_evaluate) dlss_evaluate;
    decltype(&rsf_dlss_native12_evaluate_borrowed) dlss_evaluate_borrowed;
    decltype(&rsf_dlss_native12_present) dlss_present;
    decltype(&rsf_dlss_native12_destroy) dlss_destroy;
    decltype(&rsf_sr_session_create) session_create;
    decltype(&rsf_sr_session_select) session_select;
    decltype(&rsf_sr_session_get_status) session_get_status;
    decltype(&rsf_sr_session_evaluate) session_evaluate;
    decltype(&rsf_sr_session_destroy) session_destroy;
    decltype(&create_pipeline) create_pipeline;
    decltype(&create_heap) create_heap;
    decltype(&create_texture) create_texture;
    decltype(&create_exposure_texture) create_exposure_texture;
    decltype(&create_exposure_upload) create_exposure_upload;
    decltype(&write_exposure_upload) write_exposure_upload;
    decltype(&create_readback) create_readback;
    decltype(&describe) describe;
    decltype(&owned_by) owned_by;
    decltype(&write_srv) write_srv;
    decltype(&write_uav) write_uav;
    decltype(&dispatch) dispatch;
    decltype(&transition) transition;
    decltype(&copy_region) copy_region;
    decltype(&copy_unit_exposure) copy_unit_exposure;
    decltype(&copy_motion_tile) copy_motion_tile;
    decltype(&map) map;
    decltype(&unmap) unmap;
    decltype(&depth_domains) depth_domains;
    decltype(&capture_create) capture_create;
    decltype(&capture_destroy) capture_destroy;
    decltype(&capture_enabled) capture_enabled;
    decltype(&capture_configure) capture_configure;
    decltype(&capture_flush) capture_flush;
    decltype(&capture_record) capture_record;
    decltype(&capture_event) capture_event;
    decltype(&capture_wants) capture_wants;
    decltype(&capture_force_reset) capture_force_reset;
    decltype(&sharpen_create) sharpen_create;
    decltype(&sharpen_destroy) sharpen_destroy;
    decltype(&sharpening_valid) sharpening_valid;
    decltype(&sharpening_defaults) sharpening_defaults;
    decltype(&vendor_sharpness) vendor_sharpness;
    decltype(&sharpen_run) sharpen_run;
    decltype(&rsf_input_dump_begin_record) dump_begin_record;
    decltype(&rsf_input_dump_pass_metadata) dump_pass_metadata;
    decltype(&rsf_input_dump_sr_frame) dump_sr_frame;
    decltype(&rsf_input_dump_result) dump_result;
    decltype(&fg_bind) fg_bind;
};

constexpr Platform platform{
    sizeof(Platform),
    rsf_dlss_native12_create, rsf_dlss_native12_create_shared, rsf_dlss_native12_create_registered,
    rsf_dlss_native12_plan, rsf_dlss_native12_set_sdr, rsf_dlss_native12_evaluate,
    rsf_dlss_native12_evaluate_borrowed, rsf_dlss_native12_present, rsf_dlss_native12_destroy,
    rsf_sr_session_create, rsf_sr_session_select, rsf_sr_session_get_status, rsf_sr_session_evaluate,
    rsf_sr_session_destroy, create_pipeline, create_heap, create_texture, create_exposure_texture,
    create_exposure_upload, write_exposure_upload, create_readback,
    describe, owned_by, write_srv, write_uav, dispatch, transition, copy_region, copy_unit_exposure,
    copy_motion_tile, map, unmap, depth_domains, capture_create, capture_destroy, capture_enabled,
    capture_configure, capture_flush, capture_record, capture_event, capture_wants, capture_force_reset,
    sharpen_create, sharpen_destroy, sharpening_valid, sharpening_defaults, vendor_sharpness, sharpen_run,
    rsf_input_dump_begin_record, rsf_input_dump_pass_metadata, rsf_input_dump_sr_frame, rsf_input_dump_result,
    fg_bind,
};
}

extern "C" {
rsf_backend_result rsf_core_sr12_create(const rsf_sr12_setup*, rsf_sr12**, const Platform*);
rsf_backend_result rsf_core_sr12_plan(const rsf_sr12*, uint32_t*, uint32_t*);
rsf_backend_result rsf_core_sr12_evaluate(rsf_sr12*, void*, const rsf_game_render_pass*, uint32_t);
rsf_backend_result rsf_core_sr12_set_sharpening(rsf_sr12*, const rsf_sharpening_options*);
void rsf_core_sr12_destroy(rsf_sr12*);
rsf_backend_result rsf_core_sr12_present(rsf_sr12*, void*, uint32_t, uint32_t, uint32_t);
rsf_backend_result rsf_core_sr12_capture(rsf_sr12*, const char*, uint64_t, uint32_t);
rsf_backend_result rsf_core_sr12_capture_flush(rsf_sr12*, uint32_t);
rsf_backend_result rsf_core_sr12_capture_scene(rsf_sr12*, void*, const rsf_game_render_pass*, uint32_t);
}

// The public entries stay native so the Runtime DLL and every embedded consumer keep the same
// exported names and decorations.
extern "C" rsf_backend_result rsf_sr12_create(const rsf_sr12_setup* setup, rsf_sr12** out)
{
    return rsf_core_sr12_create(setup, out, &platform);
}
extern "C" rsf_backend_result rsf_sr12_plan(const rsf_sr12* self, uint32_t* width, uint32_t* height)
{
    return rsf_core_sr12_plan(self, width, height);
}
extern "C" rsf_backend_result rsf_sr12_evaluate(rsf_sr12* self, void* commands, const rsf_game_render_pass* pass, uint32_t slot)
{
    return rsf_core_sr12_evaluate(self, commands, pass, slot);
}
extern "C" rsf_backend_result rsf_sr12_set_sharpening(rsf_sr12* self, const rsf_sharpening_options* options)
{
    return rsf_core_sr12_set_sharpening(self, options);
}
extern "C" void rsf_sr12_destroy(rsf_sr12* self) { rsf_core_sr12_destroy(self); }
extern "C" rsf_backend_result rsf_sr12_present(rsf_sr12* self, void* swapchain, uint32_t sync_interval,
    uint32_t flags, uint32_t begin)
{
    return rsf_core_sr12_present(self, swapchain, sync_interval, flags, begin);
}
extern "C" rsf_backend_result rsf_sr12_capture(rsf_sr12* self, const char* directory, uint64_t first_frame, uint32_t frames)
{
    return rsf_core_sr12_capture(self, directory, first_frame, frames);
}
extern "C" rsf_backend_result rsf_sr12_capture_flush(rsf_sr12* self, uint32_t slot)
{
    return rsf_core_sr12_capture_flush(self, slot);
}
extern "C" rsf_backend_result rsf_sr12_capture_scene(rsf_sr12* self, void* commands, const rsf_game_render_pass* pass,
    uint32_t slot)
{
    return rsf_core_sr12_capture_scene(self, commands, pass, slot);
}
