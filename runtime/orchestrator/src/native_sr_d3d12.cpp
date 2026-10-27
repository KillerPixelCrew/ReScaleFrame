// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/dlss_native12.h>
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/srv_format.h>
#include "../../backends/common/d3d12_helpers.h"
#include "../../backends/common/frame_capture12.h"
#include <windows.h>
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <cmath>
#include <cstring>
#include <memory>
#include <DirectXPackedVector.h>
#include <cstdio>
#include <algorithm>
#include <vector>

using Microsoft::WRL::ComPtr;
struct rsf_sr12 {
    ComPtr<ID3D12Device> device;
    rsf_sr_session* alternate = nullptr;
    rsf_dlss_native12* dlss = nullptr;
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> pipeline;
    std::array<ComPtr<ID3D12DescriptorHeap>, 3> heaps;
    std::array<rsf_frame_record, 3> captured_records{};
    std::array<void*, 3> captured_lists{};
    std::array<ComPtr<ID3D12Resource>, 3> normalized;
    ComPtr<ID3D12Resource> output;
    ComPtr<ID3D12Resource> exposure, exposure_upload;
    bool exposure_initialized = false;
    bool external_exposure = false;
    ComPtr<ID3D12Resource> exposure_input;
    uint32_t width = 0, height = 0, output_width = 0, output_height = 0;
    uint32_t backend = 0, quality = 0;
    bool normalize_color = false;
    bool input_sdr = false;
    uint64_t last_frame = 0, last_view = 0, last_session = 0;
    uint32_t last_generation = 0;
    rsf_backend_log_fn diagnostic_log = nullptr;
    void* diagnostic_user = nullptr;
    std::array<ComPtr<ID3D12Resource>, 3> motion_readback;
    std::array<bool, 3> motion_pending{};
    uint32_t diagnostic_frames = 0;
    // Colour, depth, motion and engine output as last validated, with the generation they were
    // validated in and the SRV formats they read with. The references keep a freed resource's
    // address from passing for the cached one.
    std::array<ComPtr<ID3D12Resource>, 4> validated;
    std::array<DXGI_FORMAT, 3> view_formats{};
    uint32_t validated_generation = 0;
    ComPtr<ID3D12GraphicsCommandList> validated_list;
    rsf::FrameCapture12 capture;
};
namespace {
using rsf::transition;
void motion_diagnostic(rsf_sr12& self, ID3D12GraphicsCommandList* list, uint32_t slot)
{
    if (!self.diagnostic_log) return;
    auto& buffer = self.motion_readback[slot];
    if (self.motion_pending[slot]) {
        void* mapped = nullptr; D3D12_RANGE range{0, 8192};
        if (SUCCEEDED(buffer->Map(0, &range, &mapped))) {
            float maximum = 0, sum = 0; uint32_t invalid = 0;
            for (uint32_t tile = 0; tile < 4; ++tile) for (uint32_t y = 0; y < 8; ++y) for (uint32_t x = 0; x < 8; ++x) {
                const auto* pixel = reinterpret_cast<const uint16_t*>(static_cast<const unsigned char*>(mapped) + tile * 2048 + y * 256 + x * 4);
                float vx = DirectX::PackedVector::XMConvertHalfToFloat(pixel[0]);
                float vy = DirectX::PackedVector::XMConvertHalfToFloat(pixel[1]);
                if (!std::isfinite(vx) || !std::isfinite(vy)) { ++invalid; continue; }
                const float magnitude = std::sqrt(vx * vx + vy * vy);
                sum += magnitude; maximum = (std::max)(maximum, magnitude);
            }
            D3D12_RANGE written{0, 0}; buffer->Unmap(0, &written);
            char text[160]; std::snprintf(text, sizeof(text), "Unity normalized motion samples: mean=%.6f max=%.6f render pixels invalid=%u", sum / 256, maximum, invalid);
            self.diagnostic_log(self.diagnostic_user, text);
        }
        self.motion_pending[slot] = false;
    }
    if (self.diagnostic_frames >= 12 || self.width < 16 || self.height < 16) return;
    if (!buffer) {
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
        D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; desc.Width = 8192;
        desc.Height = desc.DepthOrArraySize = desc.MipLevels = 1; desc.SampleDesc.Count = 1; desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if (FAILED(self.device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&buffer)))) return;
    }
    transition(list, self.normalized[2].Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION source{}; source.pResource = self.normalized[2].Get(); source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    for (uint32_t tile = 0; tile < 4; ++tile) {
        D3D12_TEXTURE_COPY_LOCATION target{}; target.pResource = buffer.Get(); target.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        target.PlacedFootprint.Offset = tile * 2048;
        target.PlacedFootprint.Footprint = {DXGI_FORMAT_R16G16_FLOAT, 8, 8, 1, 256};
        const uint32_t x = self.width * (tile % 2 ? 3 : 1) / 4, y = self.height * (tile / 2 ? 3 : 1) / 4;
        const D3D12_BOX box{x, y, 0, x + 8, y + 8, 1};
        list->CopyTextureRegion(&target, 0, 0, 0, &source, &box);
    }
    transition(list, self.normalized[2].Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    self.motion_pending[slot] = true; ++self.diagnostic_frames;
}
constexpr char normalize_shader[] = R"(
Texture2D<float4> scene : register(t0);
Texture2D<float> depth : register(t1);
Texture2D<float2> motion : register(t2);
RWTexture2D<float4> sceneOut : register(u0);
RWTexture2D<float> depthOut : register(u1);
RWTexture2D<float2> motionOut : register(u2);
cbuffer Constants : register(b0) { uint2 extent; float2 toPixels; uint inputsOnly; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= extent)) return;
    if (!inputsOnly) sceneOut[id.xy] = scene.Load(int3(id.xy,0));
    depthOut[id.xy] = depth.Load(int3(id.xy,0));
    motionOut[id.xy] = motion.Load(int3(id.xy,0)) * toPixels;
})";
bool texture(rsf_sr12& self, uint32_t width, uint32_t height, DXGI_FORMAT format, ComPtr<ID3D12Resource>& out)
{
    return rsf::create_uav_texture(self.device.Get(), width, height, format, out.ReleaseAndGetAddressOf());
}
bool prepare_shader(rsf_sr12& self)
{
    D3D12_DESCRIPTOR_RANGE ranges[] = {
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 3, 0, 0, 0},
        {D3D12_DESCRIPTOR_RANGE_TYPE_UAV, 3, 0, 0, 3}};
    D3D12_ROOT_PARAMETER parameters[2]{};
    parameters[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    parameters[0].DescriptorTable = {2, ranges};
    parameters[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    parameters[1].Constants = {0, 0, 5};
    D3D12_ROOT_SIGNATURE_DESC desc{2, parameters, 0, nullptr, D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> serialized, errors;
    std::vector<uint8_t> shader;
    if (FAILED(D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &serialized, &errors)) ||
        FAILED(self.device->CreateRootSignature(0, serialized->GetBufferPointer(), serialized->GetBufferSize(), IID_PPV_ARGS(&self.root))) ||
        !rsf::compile_shader(normalize_shader, sizeof(normalize_shader) - 1, "RSF temporal input normalization", "main",
            "cs_5_0", shader, self.diagnostic_log, self.diagnostic_user,
            D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3)) return false;
    D3D12_COMPUTE_PIPELINE_STATE_DESC pipeline{}; pipeline.pRootSignature = self.root.Get();
    pipeline.CS = {shader.data(), shader.size()};
    if (FAILED(self.device->CreateComputePipelineState(&pipeline, IID_PPV_ARGS(&self.pipeline)))) return false;
    for (auto& heap : self.heaps) {
        D3D12_DESCRIPTOR_HEAP_DESC heap_desc{}; heap_desc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        heap_desc.NumDescriptors = 6; heap_desc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (FAILED(self.device->CreateDescriptorHeap(&heap_desc, IID_PPV_ARGS(&heap)))) return false;
    }
    return true;
}
// The SRV format for a depth input: the readable plane of a depth format, R32_FLOAT otherwise.
DXGI_FORMAT depth_view_format(DXGI_FORMAT format)
{
    const DXGI_FORMAT view = rsf::srv_format(format);
    return view == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS || view == DXGI_FORMAT_R24_UNORM_X8_TYPELESS ? view
        : DXGI_FORMAT_R32_FLOAT;
}
bool inputs_valid(rsf_sr12& self, const rsf_game_render_pass& pass)
{
    if (!pass.camera_valid || pass.camera.struct_size != sizeof(rsf_camera_frame) ||
        pass.camera.abi_version != RSF_GAME_FRAME_ABI_VERSION || !pass.native_frame || !pass.session_id ||
        !pass.history_key || pass.camera.render_width != self.width || pass.camera.render_height != self.height ||
        pass.camera.output_width != self.output_width || pass.camera.output_height != self.output_height)
        return false;
    if (!(std::isfinite(pass.motion_to_uv[0]) && std::isfinite(pass.motion_to_uv[1]) &&
          pass.motion_to_uv[0] != 0 && pass.motion_to_uv[1] != 0)) return false;
    if (self.external_exposure) {
        if (!pass.exposure) return false;
        auto* exposure = static_cast<ID3D12Resource*>(pass.exposure);
        const auto desc = exposure->GetDesc();
        ComPtr<ID3D12Device> owner;
        if (FAILED(exposure->GetDevice(IID_PPV_ARGS(&owner))) || owner.Get() != self.device.Get() ||
            desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.Width != 1 || desc.Height != 1 ||
            desc.Format != DXGI_FORMAT_R32_FLOAT || desc.SampleDesc.Count != 1 || desc.DepthOrArraySize != 1) return false;
        self.exposure_input = exposure;
    }
    void* pointers[] = {pass.color_input, pass.depth, pass.motion, pass.color_output};
    // The resources' descriptions cannot change, and the extents they are checked against are
    // fixed at create, so a set already accepted in this generation needs no second look.
    bool cached = self.validated_generation == pass.resource_generation;
    for (size_t i = 0; i < 4 && cached; ++i)
        cached = (i == 3 && !self.backend) || (pointers[i] && self.validated[i].Get() == pointers[i]);
    if (cached) return true;
    for (auto& resource : self.validated) resource.Reset();
    for (size_t i = 0; i < 4; ++i) {
        if (i == 3 && !self.backend) continue;
        if (!pointers[i]) return false;
        auto* resource = static_cast<ID3D12Resource*>(pointers[i]);
        const auto desc = resource->GetDesc();
        ComPtr<ID3D12Device> owner;
        if (FAILED(resource->GetDevice(IID_PPV_ARGS(&owner))) || owner.Get() != self.device.Get() ||
            desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count != 1 ||
            desc.DepthOrArraySize != 1 || desc.Width < (i == 3 ? self.output_width : self.width) ||
            desc.Height < (i == 3 ? self.output_height : self.height)) return false;
        const DXGI_FORMAT readable_format = rsf::srv_format(desc.Format);
        if (i == 0 && readable_format != DXGI_FORMAT_R16G16B16A16_FLOAT && readable_format != DXGI_FORMAT_R11G11B10_FLOAT)
            return false;
        if (i == 1 && desc.Format != DXGI_FORMAT_R32_FLOAT && desc.Format != DXGI_FORMAT_R32_TYPELESS &&
            desc.Format != DXGI_FORMAT_D32_FLOAT && desc.Format != DXGI_FORMAT_D32_FLOAT_S8X24_UINT &&
            desc.Format != DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format != DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS &&
            desc.Format != DXGI_FORMAT_D24_UNORM_S8_UINT && desc.Format != DXGI_FORMAT_R24G8_TYPELESS) return false;
        if (i == 2 && readable_format != DXGI_FORMAT_R16G16_FLOAT) return false;
        if (i == 3 && readable_format != DXGI_FORMAT_R16G16B16A16_FLOAT) return false;
        if (i < 3) self.view_formats[i] = i == 1 ? depth_view_format(desc.Format) : readable_format;
    }
    for (size_t i = 0; i < 4; ++i) self.validated[i] = static_cast<ID3D12Resource*>(pointers[i]);
    self.validated_generation = pass.resource_generation;
    return true;
}
}
extern "C" rsf_backend_result rsf_sr12_create(const rsf_sr12_setup* setup, rsf_sr12** out) try
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!setup || setup->struct_size < offsetof(rsf_sr12_setup, engine_input_extents) || !setup->device || !setup->output_width || !setup->output_height ||
        setup->backend > RSF_SR_XESS || setup->quality > 5) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != 1 && setup->abi_version != 2) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (setup->abi_version == 2 && setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const bool engine_extents = setup->abi_version == 2 && setup->engine_input_extents;
    if (engine_extents && (!setup->render_width || !setup->render_height ||
        setup->render_width > setup->output_width || setup->render_height > setup->output_height))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::unique_ptr<rsf_sr12, decltype(&rsf_sr12_destroy)> self(new rsf_sr12, rsf_sr12_destroy);
    self->device = static_cast<ID3D12Device*>(setup->device);
    self->diagnostic_log = setup->log; self->diagnostic_user = setup->user;
    self->backend = setup->backend; self->quality = setup->quality;
    self->external_exposure = setup->abi_version == 2 && setup->external_exposure;
    self->output_width = setup->output_width; self->output_height = setup->output_height;
    rsf_backend_result result = RSF_BACKEND_OK;
    if (!setup->backend) {
        self->width = setup->render_width ? setup->render_width : setup->output_width;
        self->height = setup->render_height ? setup->render_height : setup->output_height;
    } else if (setup->backend == RSF_SR_DLSS) {
        if (setup->abi_version == 2 && setup->dlss_registration) {
            result = rsf_dlss_native12_create_registered(setup->device, &setup->dlss,
                static_cast<rsf_dlss_native12_registration*>(setup->dlss_registration), &self->dlss);
        } else result = setup->streamline_host ? rsf_dlss_native12_create_shared(setup->device, &setup->dlss,
            static_cast<rsf_streamline_host*>(setup->streamline_host), &self->dlss) : rsf_dlss_native12_create(setup->device, &setup->dlss, &self->dlss);
        if (result == RSF_BACKEND_OK) result = rsf_dlss_native12_plan(self->dlss, self->output_width, self->output_height,
            self->quality, &self->width, &self->height);
        if (result == RSF_BACKEND_OK) rsf_dlss_native12_set_sdr(self->dlss, setup->input_sdr);
    } else {
        rsf_sr_session_setup alternate{}; alternate.struct_size = sizeof(alternate); alternate.abi_version = RSF_SR_SESSION_ABI_VERSION;
        alternate.open.struct_size = sizeof(alternate.open); alternate.open.abi_version = RSF_BACKEND_ABI_VERSION;
        alternate.open.api = RSF_API_D3D12; alternate.open.device = setup->device;
        alternate.open.output_width = self->output_width; alternate.open.output_height = self->output_height;
        alternate.open.dynamic_resolution = 1; alternate.open.hdr = !setup->input_sdr;
        alternate.open.auto_exposure = !setup->input_sdr && !self->external_exposure;
        alternate.open.inverted_depth = setup->inverted_depth; alternate.open.depth_infinite = 0;
        alternate.open.log = setup->log; alternate.open.log_user = setup->user;
        alternate.fsr2_directory_utf8 = setup->fsr2_directory_utf8; alternate.fsr3_directory_utf8 = setup->fsr3_directory_utf8;
        alternate.fsr4_directory_utf8 = setup->fsr4_directory_utf8; alternate.xess_directory_utf8 = setup->xess_directory_utf8;
        result = rsf_sr_session_create(&alternate, &self->alternate);
        if (result == RSF_BACKEND_OK) result = rsf_sr_session_select(self->alternate, setup->backend, setup->quality, 0);
        if (result == RSF_BACKEND_OK) {
            rsf_sr_session_status status{}; status.struct_size = sizeof(status);
            result = rsf_sr_session_get_status(self->alternate, &status);
            self->width = status.render_width; self->height = status.render_height;
        }
    }
    if (result != RSF_BACKEND_OK) return result;
    if (engine_extents) { self->width = setup->render_width; self->height = setup->render_height; }
    self->normalize_color = self->backend || self->capture.enabled();
    self->input_sdr = setup->input_sdr != 0;
    if (!prepare_shader(*self) || !texture(*self, self->normalize_color ? self->width : 1u, self->normalize_color ? self->height : 1u, DXGI_FORMAT_R16G16B16A16_FLOAT, self->normalized[0]) ||
        !texture(*self, self->width, self->height, DXGI_FORMAT_R32_FLOAT, self->normalized[1]) ||
        !texture(*self, self->width, self->height, DXGI_FORMAT_R16G16_FLOAT, self->normalized[2]) ||
        (self->backend && !texture(*self, self->output_width, self->output_height, DXGI_FORMAT_R16G16B16A16_FLOAT, self->output)))
        return RSF_BACKEND_ERROR_INIT_FAILED;
    if (setup->input_sdr && !self->external_exposure && self->backend && self->backend != RSF_SR_DLSS) {
        // SDR has no changing pre-exposure. XeSS requires an exposure texture when
        // auto exposure is disabled; feed the actual constant scale, not HDR adaptation.
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        desc.Width = 1; desc.Height = 1; desc.DepthOrArraySize = 1; desc.MipLevels = 1; desc.SampleDesc.Count = 1;
        desc.Format = DXGI_FORMAT_R32_FLOAT;
        if (FAILED(self->device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&self->exposure)))) return RSF_BACKEND_ERROR_INIT_FAILED;
        heap.Type = D3D12_HEAP_TYPE_UPLOAD; desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        desc.Width = 256; desc.Format = DXGI_FORMAT_UNKNOWN; desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if (FAILED(self->device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
            D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&self->exposure_upload)))) return RSF_BACKEND_ERROR_INIT_FAILED;
        void* mapped = nullptr; D3D12_RANGE read{0, 0};
        if (FAILED(self->exposure_upload->Map(0, &read, &mapped))) return RSF_BACKEND_ERROR_INIT_FAILED;
        *static_cast<float*>(mapped) = 1.0f; D3D12_RANGE written{0, sizeof(float)}; self->exposure_upload->Unmap(0, &written);
    }
    *out = self.release(); return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
extern "C" rsf_backend_result rsf_sr12_plan(const rsf_sr12* self, uint32_t* width, uint32_t* height)
{
    if (!self || !width || !height) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *width = self->width; *height = self->height; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_sr12_evaluate(rsf_sr12* self, void* commands,
    const rsf_game_render_pass* pass, uint32_t slot)
{
    if (!self || !commands || !pass || pass->struct_size < sizeof(*pass) || slot >= 3 || !inputs_valid(*self, *pass))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
    (void)self->capture.flush(slot);
    if (list != self->validated_list.Get()) {
        ComPtr<ID3D12Device> list_device;
        if (FAILED(list->GetDevice(IID_PPV_ARGS(&list_device))) || list_device.Get() != self->device.Get())
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        self->validated_list = list;
    }
    auto* heap = self->heaps[slot].Get();
    auto cpu = heap->GetCPUDescriptorHandleForHeapStart();
    const uint32_t stride = self->device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    void* inputs[] = {pass->color_input, pass->depth, pass->motion};
    for (size_t i = 0; i < 3; ++i) {
        auto* resource = static_cast<ID3D12Resource*>(inputs[i]);
        D3D12_SHADER_RESOURCE_VIEW_DESC view{}; view.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
        view.Format = self->view_formats[i];
        view.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING; view.Texture2D.MipLevels = 1;
        self->device->CreateShaderResourceView(resource, &view, cpu); cpu.ptr += stride;
    }
    const DXGI_FORMAT normalized_formats[] = {DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R16G16_FLOAT};
    for (size_t i = 0; i < self->normalized.size(); ++i) {
        D3D12_UNORDERED_ACCESS_VIEW_DESC view{}; view.ViewDimension = D3D12_UAV_DIMENSION_TEXTURE2D;
        view.Format = normalized_formats[i];
        self->device->CreateUnorderedAccessView(self->normalized[i].Get(), nullptr, &view, cpu); cpu.ptr += stride;
    }
    list->SetDescriptorHeaps(1, &heap); list->SetComputeRootSignature(self->root.Get());
    list->SetComputeRootDescriptorTable(0, heap->GetGPUDescriptorHandleForHeapStart()); list->SetPipelineState(self->pipeline.Get());
    struct Constants { uint32_t width, height; float x, y; uint32_t inputs_only; } constants{self->width, self->height,
        -static_cast<float>(self->width) * pass->motion_to_uv[0], -static_cast<float>(self->height) * pass->motion_to_uv[1], !self->normalize_color};
    list->SetComputeRoot32BitConstants(1, 5, &constants, 0);
    list->Dispatch((self->width + 7) / 8, (self->height + 7) / 8, 1);
    for (auto& resource : self->normalized) transition(list, resource.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    // The caller joins a slot's previous GPU completion before entering this evaluation.
    motion_diagnostic(*self, list, slot);
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = pass->session_id; record.frame_id = pass->source_frame_id ? pass->source_frame_id : pass->native_frame; record.view_id = static_cast<uint32_t>(pass->view_key);
    record.resource_generation = pass->resource_generation; record.camera = pass->camera; record.screen = pass->screen;
    record.render_width = self->width; record.render_height = self->height;
    record.output_width = self->output_width; record.output_height = self->output_height;
    record.frame_time_ms = pass->camera.frame_time_seconds * 1000;
    const bool reset = (pass->flags & RSF_GAME_RENDER_RESET) || self->last_frame + 1 != pass->native_frame ||
        self->last_view != pass->history_key || self->last_session != pass->session_id || self->last_generation != pass->resource_generation;
    record.flags = (reset ? RSF_FRAME_FLAG_RESET : 0u) |
        ((pass->flags & RSF_GAME_RENDER_NO_FG) ? RSF_FRAME_FLAG_NO_FG : 0u);
    auto capture_identity = record;
    if (!pass->source_frame_id) capture_identity.flags |= RSF_FRAME_FLAG_AMBIGUOUS_ID;
    if (rsf_input_dump_begin_record(&capture_identity, self->backend)) {
        rsf_input_dump_pass_metadata(pass);
        (void)self->capture.record(list, static_cast<ID3D12Resource*>(pass->color_input), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "sr-source-jittered", slot, 0, UINT32_MAX,
            self->input_sdr ? RSF_DUMP_VIEW_RAW : RSF_DUMP_VIEW_HDR);
        (void)self->capture.record(list, static_cast<ID3D12Resource*>(pass->depth), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "sr-scene-depth", slot);
        (void)self->capture.record(list, static_cast<ID3D12Resource*>(pass->motion), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "sr-game-motion-uv", slot);
        (void)self->capture.record(list, self->normalized[1].Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "sr-depth", slot);
        (void)self->capture.record(list, self->normalized[2].Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "sr-motion-pixels", slot);
    }
    if (self->normalize_color) (void)self->capture.record(list, self->normalized[0].Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "sr-linear-color", slot, 0, UINT32_MAX,
        self->input_sdr ? RSF_DUMP_VIEW_RAW : RSF_DUMP_VIEW_HDR);
    (void)self->capture.record(list, self->normalized[1].Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "fg-depth", slot);
    (void)self->capture.record(list, self->normalized[2].Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "fg-motion-pixels", slot);
    rsf_sr_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record;
    if (self->external_exposure) {
        frame.exposure.struct_size = sizeof(frame.exposure); frame.exposure.resource = self->exposure_input.Get();
        frame.exposure.width = frame.exposure.height = 1; frame.exposure.generation = record.resource_generation;
        frame.exposure.state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    }
    if (self->exposure) {
        if (!self->exposure_initialized) {
            D3D12_TEXTURE_COPY_LOCATION source{}; source.pResource = self->exposure_upload.Get(); source.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
            source.PlacedFootprint.Footprint = {DXGI_FORMAT_R32_FLOAT, 1, 1, 1, 256};
            D3D12_TEXTURE_COPY_LOCATION target{}; target.pResource = self->exposure.Get(); target.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
            list->CopyTextureRegion(&target, 0, 0, 0, &source, nullptr);
            transition(list, self->exposure.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            self->exposure_initialized = true;
        }
        frame.exposure.struct_size = sizeof(frame.exposure); frame.exposure.resource = self->exposure.Get();
        frame.exposure.width = frame.exposure.height = 1; frame.exposure.generation = record.resource_generation;
        frame.exposure.state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    }
    rsf_backend_resource* resources[] = {&frame.color, &frame.depth, &frame.motion, &frame.output};
    void* textures[] = {self->normalized[0].Get(), self->normalized[1].Get(), self->normalized[2].Get(), self->output.Get()};
    for (size_t i = 0; i < 4; ++i) {
        resources[i]->struct_size = sizeof(*resources[i]); resources[i]->resource = textures[i];
        resources[i]->width = i == 3 ? self->output_width : self->width;
        resources[i]->height = i == 3 ? self->output_height : self->height;
        resources[i]->generation = record.resource_generation;
        resources[i]->state = i == 3 ? D3D12_RESOURCE_STATE_UNORDERED_ACCESS : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    }
    frame.jitter_x = pass->camera.jitter_pixels[0]; frame.jitter_y = pass->camera.jitter_pixels[1];
    // The producer measures pixel jitter from the actual GPU projection, including its Y conversion.
    frame.motion_scale_x = frame.motion_scale_y = 1; frame.pre_exposure = frame.view_space_to_meters = 1;
    frame.reset = reset || self->capture.force_sr_history_reset(record.frame_id);
    rsf_input_dump_sr_frame(&frame, self->backend, self->input_sdr ? 1u : 0u);
    if (frame.exposure.resource) (void)self->capture.record(list, static_cast<ID3D12Resource*>(frame.exposure.resource),
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, record, "sr-exposure", slot);
    const auto result = !self->backend ? RSF_BACKEND_OK : self->dlss ? rsf_dlss_native12_evaluate(self->dlss, list, &frame) :
        rsf_sr_session_evaluate(self->alternate, list, &frame);
    rsf_input_dump_result(&record, "sr", self->backend, result);
    (void)self->capture.event(record, "sr-evaluate", record.frame_id, frame.reset != 0, result);
    if (result == RSF_BACKEND_OK) rsf_fg12_capture(list, &record, self->normalized[1].Get(), self->normalized[2].Get());
    for (auto& resource : self->normalized) transition(list, resource.Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    if (result != RSF_BACKEND_OK) { self->last_frame = 0; return result; }
    if (self->backend) {
        (void)self->capture.record(list, self->output.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, record, "sr-output-linear", slot, 0, frame.reset ? 1u : 0u,
            self->input_sdr ? RSF_DUMP_VIEW_RAW : RSF_DUMP_VIEW_HDR);
        transition(list, self->output.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION source{}; source.pResource = self->output.Get(); source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        D3D12_TEXTURE_COPY_LOCATION target{}; target.pResource = static_cast<ID3D12Resource*>(pass->color_output); target.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        const D3D12_BOX rectangle{0, 0, 0, self->output_width, self->output_height, 1};
        list->CopyTextureRegion(&target, 0, 0, 0, &source, &rectangle);
        transition(list, self->output.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    }
    self->captured_records[slot] = record; self->captured_lists[slot] = commands;
    self->last_frame = pass->native_frame; self->last_view = pass->history_key;
    self->last_session = pass->session_id; self->last_generation = pass->resource_generation;
    return result;
}
extern "C" void rsf_sr12_destroy(rsf_sr12* self)
{
    if (!self) return;
    (void)self->capture.flush();
    rsf_sr_session_destroy(self->alternate); rsf_dlss_native12_destroy(self->dlss); delete self;
}
extern "C" rsf_backend_result rsf_sr12_present(rsf_sr12* self, void* swapchain,
    uint32_t sync_interval, uint32_t flags, uint32_t begin)
{
    if (!self || !swapchain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return self->dlss ? rsf_dlss_native12_present(self->dlss, swapchain, sync_interval, flags, begin) : RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_sr12_capture(rsf_sr12* self, const char* directory,
    uint64_t first_frame, uint32_t frames)
{
    return self && self->capture.configure(directory, first_frame, frames) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
extern "C" rsf_backend_result rsf_sr12_capture_flush(rsf_sr12* self, uint32_t slot)
{
    if (!self || slot >= 3) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return self->capture.flush(slot) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
extern "C" rsf_backend_result rsf_sr12_capture_scene(rsf_sr12* self, void* commands,
    const rsf_game_render_pass* pass, uint32_t slot)
{
    if (!self || !commands || !pass || pass->struct_size < sizeof(*pass) || slot >= 3 || !pass->color_input)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto& record = self->captured_records[slot];
    if (record.frame_id != (pass->source_frame_id ? pass->source_frame_id : pass->native_frame) || record.session_id != pass->session_id ||
        self->captured_lists[slot] != commands) return RSF_BACKEND_ERROR_STALE_RESOURCES;
    auto* scene = static_cast<ID3D12Resource*>(pass->color_input);
    const auto desc = scene->GetDesc();
    ComPtr<ID3D12Device> owner;
    if (FAILED(scene->GetDevice(IID_PPV_ARGS(&owner))) || owner.Get() != self->device.Get())
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (desc.Width != record.output_width || desc.Height != record.output_height || desc.SampleDesc.Count != 1)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!self->capture.wants(record.frame_id)) return RSF_BACKEND_ERROR_NOT_READY;
    return self->capture.record(static_cast<ID3D12GraphicsCommandList*>(commands), scene,
        D3D12_RESOURCE_STATE_RENDER_TARGET, record, "fg-scene-output", slot,
        static_cast<uint32_t>(pass->pass_key)) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
