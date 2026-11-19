// SPDX-License-Identifier: MIT
// D3D11, D3D12, shared-surface, capture and SR session operations for the Rust SR bridge owner in
// sr_bridge.rs. Each leaf performs one platform call. Ownership, ordering, leases, retirement and
// failure policy are decided in Rust. Returned COM interfaces carry one reference for the caller.
#include <rescaleframe/sr_bridge.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/shared_surface.h>
#include "../../backends/common/sr_helpers.h"
#include "../../backends/common/d3d12_helpers.h"
#include <d3d11_4.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
using Microsoft::WRL::ComPtr;

// Values sr_bridge.rs names directly.
static_assert(DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && DXGI_FORMAT_R16G16_FLOAT == 34 && DXGI_FORMAT_R32_FLOAT == 41);
static_assert(D3D12_RESOURCE_STATE_COMMON == 0 && D3D12_RESOURCE_STATE_UNORDERED_ACCESS == 0x8 &&
              D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40);
static_assert(RSF_API_D3D11 == 1 && RSF_API_D3D12 == 2 && RSF_SR_NONE == 0);

namespace {
struct TextureDesc {
    uint32_t width, height, format, samples, array_size;
};

template<class T> void* transfer(HRESULT result, T* object)
{
    if (SUCCEEDED(result)) return object;
    if (object) object->Release();
    return nullptr;
}

void* immediate_context(void* device11)
{
    ID3D11DeviceContext* context = nullptr;
    static_cast<ID3D11Device*>(device11)->GetImmediateContext(&context);
    return context;
}
void* query_context4(void* context)
{
    ID3D11DeviceContext4* queried = nullptr;
    const HRESULT result = static_cast<ID3D11DeviceContext*>(context)->QueryInterface(IID_PPV_ARGS(&queried));
    return transfer(result, queried);
}
// D3D12 on the adapter that owns the D3D11 device.
void* create_device12(void* device11)
{
    ComPtr<IDXGIDevice> dxgi;
    ComPtr<IDXGIAdapter> adapter;
    if (FAILED(static_cast<ID3D11Device*>(device11)->QueryInterface(IID_PPV_ARGS(&dxgi))) ||
        FAILED(dxgi->GetAdapter(&adapter))) return nullptr;
    ID3D12Device* device = nullptr;
    const HRESULT result = D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device));
    return transfer(result, device);
}
void* create_queue(void* device12)
{
    D3D12_COMMAND_QUEUE_DESC desc{};
    desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ID3D12CommandQueue* queue = nullptr;
    const HRESULT result = static_cast<ID3D12Device*>(device12)->CreateCommandQueue(&desc, IID_PPV_ARGS(&queue));
    return transfer(result, queue);
}
int32_t create_commands(void* device12, void** allocator, void** list)
{
    ID3D12CommandAllocator* created_allocator = nullptr;
    ID3D12GraphicsCommandList* created_list = nullptr;
    if (!rsf::create_command_list(static_cast<ID3D12Device*>(device12), D3D12_COMMAND_LIST_TYPE_DIRECT,
                                  &created_allocator, &created_list)) return 0;
    *allocator = created_allocator;
    *list = created_list;
    return 1;
}
void* create_output(void* device12, uint32_t width, uint32_t height, uint32_t format)
{
    ID3D12Resource* output = nullptr;
    return rsf::create_uav_texture(static_cast<ID3D12Device*>(device12), width, height,
                                   static_cast<DXGI_FORMAT>(format), &output) ? output : nullptr;
}
// A default-heap texture in the common state.
void* create_texture(void* device12, uint32_t width, uint32_t height, uint32_t format)
{
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width; desc.Height = height; desc.DepthOrArraySize = 1; desc.MipLevels = 1;
    desc.Format = static_cast<DXGI_FORMAT>(format); desc.SampleDesc.Count = 1;
    ID3D12Resource* texture = nullptr;
    const HRESULT result = static_cast<ID3D12Device*>(device12)->CreateCommittedResource(
        &heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&texture));
    return transfer(result, texture);
}
void* create_fence(void* device12)
{
    ID3D12Fence* fence = nullptr;
    const HRESULT result = static_cast<ID3D12Device*>(device12)->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence));
    return transfer(result, fence);
}
void* create_event() { return CreateEventW(nullptr, FALSE, FALSE, nullptr); }
void close_event(void* event) { CloseHandle(event); }
// The description of a texture created by `device11`; 0 for any other owner.
int32_t describe(void* texture, void* device11, TextureDesc* out)
{
    auto* typed = static_cast<ID3D11Texture2D*>(texture);
    ComPtr<ID3D11Device> owner;
    typed->GetDevice(&owner);
    if (owner.Get() != device11) return 0;
    D3D11_TEXTURE2D_DESC desc{};
    typed->GetDesc(&desc);
    *out = {desc.Width, desc.Height, static_cast<uint32_t>(desc.Format), desc.SampleDesc.Count, desc.ArraySize};
    return 1;
}
int32_t same_context(void* pointer, void* context4)
{
    ComPtr<ID3D11DeviceContext4> queried;
    return SUCCEEDED(static_cast<ID3D11DeviceContext*>(pointer)->QueryInterface(IID_PPV_ARGS(&queried))) &&
        queried.Get() == context4;
}
int32_t wait_fence(void* fence12, uint64_t value, void* event, uint32_t timeout_ms)
{
    return SUCCEEDED(rsf::wait_fence(static_cast<ID3D12Fence*>(fence12), value, event, timeout_ms));
}
void flush(void* context4) { static_cast<ID3D11DeviceContext4*>(context4)->Flush(); }
// The origin-zero rectangle of subresource 0.
void copy_region(void* context4, void* destination, void* source, uint32_t width, uint32_t height)
{
    const D3D11_BOX box{0, 0, 0, width, height, 1};
    static_cast<ID3D11DeviceContext4*>(context4)->CopySubresourceRegion(static_cast<ID3D11Resource*>(destination),
        0, 0, 0, 0, static_cast<ID3D11Resource*>(source), 0, &box);
}
void copy_resource(void* context4, void* destination, void* source)
{
    static_cast<ID3D11DeviceContext4*>(context4)->CopyResource(static_cast<ID3D11Resource*>(destination),
                                                              static_cast<ID3D11Resource*>(source));
}
int32_t context_signal(void* context4, void* fence11, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D11DeviceContext4*>(context4)->Signal(static_cast<ID3D11Fence*>(fence11), value));
}
int32_t context_wait(void* context4, void* fence11, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D11DeviceContext4*>(context4)->Wait(static_cast<ID3D11Fence*>(fence11), value));
}
int32_t reset_commands(void* allocator, void* list)
{
    auto* typed = static_cast<ID3D12CommandAllocator*>(allocator);
    return SUCCEEDED(typed->Reset()) &&
        SUCCEEDED(static_cast<ID3D12GraphicsCommandList*>(list)->Reset(typed, nullptr));
}
void transition(void* list, void* resource, uint32_t before, uint32_t after)
{
    rsf::transition(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(resource),
                    static_cast<D3D12_RESOURCE_STATES>(before), static_cast<D3D12_RESOURCE_STATES>(after));
}
void copy_transitioned(void* list, void* destination, uint32_t destination_before, uint32_t destination_after,
                       void* source, uint32_t source_before, uint32_t source_after)
{
    rsf::copy_transitioned(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(destination),
        static_cast<D3D12_RESOURCE_STATES>(destination_before), static_cast<D3D12_RESOURCE_STATES>(destination_after),
        static_cast<ID3D12Resource*>(source), static_cast<D3D12_RESOURCE_STATES>(source_before),
        static_cast<D3D12_RESOURCE_STATES>(source_after));
}
// Closes and executes the list. 0 when it could not be closed, in which case nothing was submitted.
int32_t submit(void* queue, void* list)
{
    auto* typed = static_cast<ID3D12GraphicsCommandList*>(list);
    if (FAILED(typed->Close())) return 0;
    ID3D12CommandList* lists[] = {typed};
    static_cast<ID3D12CommandQueue*>(queue)->ExecuteCommandLists(1, lists);
    return 1;
}
int32_t queue_wait(void* queue, void* fence12, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->Wait(static_cast<ID3D12Fence*>(fence12), value));
}
int32_t queue_signal(void* queue, void* fence12, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->Signal(static_cast<ID3D12Fence*>(fence12), value));
}
int32_t device_ready(void* device12)
{
    return SUCCEEDED(static_cast<ID3D12Device*>(device12)->GetDeviceRemovedReason());
}
void* surface_create(void* device11, void* device12, uint32_t width, uint32_t height, uint32_t format,
                     rsf_backend_log_fn log, void* log_user)
{
    rsf_shared_surface_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
    setup.width = width; setup.height = height; setup.format = format;
    setup.log = log; setup.log_user = log_user;
    rsf_shared_surface* surface = nullptr;
    return rsf_shared_surface_create(device11, device12, &setup, &surface) == RSF_SHARED_OK ? surface : nullptr;
}
void surface_destroy(void* surface) { rsf_shared_surface_destroy(static_cast<rsf_shared_surface*>(surface)); }
void* surface_d3d11(void* surface) { return rsf_shared_surface_d3d11(static_cast<rsf_shared_surface*>(surface)); }
void* surface_d3d12(void* surface) { return rsf_shared_surface_d3d12(static_cast<rsf_shared_surface*>(surface)); }
void* fence_create(void* device11, void* device12, rsf_backend_log_fn log, void* log_user)
{
    rsf_shared_fence* fence = nullptr;
    return rsf_shared_fence_create(device11, device12, log, log_user, &fence) == RSF_SHARED_OK ? fence : nullptr;
}
void fence_destroy(void* fence) { rsf_shared_fence_destroy(static_cast<rsf_shared_fence*>(fence)); }
void* fence_d3d11(void* fence) { return rsf_shared_fence_d3d11(static_cast<rsf_shared_fence*>(fence)); }
void* fence_d3d12(void* fence) { return rsf_shared_fence_d3d12(static_cast<rsf_shared_fence*>(fence)); }
int32_t dump_poll_requested()
{
    rsf_input_dump_poll();
    return rsf_input_dump_requested();
}
void dump_inputs(void* context, const rsf_sr_frame* frame, uint32_t backend, uint32_t hdr)
{
    if (!rsf_input_dump_begin_record(frame->record, backend)) return;
    const auto colour = hdr ? RSF_DUMP_VIEW_HDR : RSF_DUMP_VIEW_RAW;
    rsf_input_dump_sr_frame(frame, backend, hdr ? 0u : 1u);
    rsf_input_dump_texture_record(context, frame->record, "sr_source_jittered", "D3D11 SR source colour", frame->color.resource, colour, 1);
    rsf_input_dump_texture_record(context, frame->record, "sr_color", "D3D11 SR backend colour", frame->color.resource, colour, 1);
    rsf_input_dump_texture_record(context, frame->record, "sr_depth", "D3D11 SR device depth", frame->depth.resource, RSF_DUMP_VIEW_DEPTH, 1);
    rsf_input_dump_texture_record(context, frame->record, "sr_motion", "D3D11 dense pixel displacement", frame->motion.resource, RSF_DUMP_VIEW_DENSE_MOTION, 1.0f / 32);
    rsf_input_dump_texture_record(context, frame->record, "sr_exposure", "D3D11 SR exposure", frame->exposure.resource, RSF_DUMP_VIEW_SCALAR, 1);
}
int32_t dump_wants(const rsf_frame_record* record) { return rsf_input_dump_wants(record); }
void dump_result(void* context, const rsf_sr_frame* frame, uint32_t backend, int32_t result, uint32_t hdr)
{
    rsf_input_dump_result(frame->record, "sr", backend, result);
    if (result == RSF_BACKEND_OK) rsf_input_dump_texture_record(context, frame->record, "sr_output",
        "Reconstructed D3D11 scene before tonemapping", frame->output.resource, hdr ? RSF_DUMP_VIEW_HDR : RSF_DUMP_VIEW_RAW, 1);
}
rsf_backend_result validate_frame(const rsf_sr_frame* frame) { return rsf::validate_frame(frame); }

// Field order matches `Platform` in sr_bridge.rs.
struct Platform {
    decltype(&rsf_sr_session_create) session_create;
    decltype(&rsf_sr_session_select) session_select;
    decltype(&rsf_sr_session_set_auto_exposure) session_set_auto_exposure;
    decltype(&rsf_sr_session_evaluate) session_evaluate;
    decltype(&rsf_sr_session_warm_up) session_warm_up;
    decltype(&rsf_sr_session_get_status) session_get_status;
    decltype(&rsf_sr_session_destroy) session_destroy;
    decltype(&validate_frame) validate_frame;
    decltype(&immediate_context) immediate_context;
    decltype(&query_context4) query_context4;
    decltype(&create_device12) create_device12;
    decltype(&create_queue) create_queue;
    decltype(&create_commands) create_commands;
    decltype(&create_output) create_output;
    decltype(&create_texture) create_texture;
    decltype(&create_fence) create_fence;
    decltype(&create_event) create_event;
    decltype(&close_event) close_event;
    decltype(&describe) describe;
    decltype(&same_context) same_context;
    decltype(&wait_fence) wait_fence;
    decltype(&flush) flush;
    decltype(&copy_region) copy_region;
    decltype(&copy_resource) copy_resource;
    decltype(&context_signal) context_signal;
    decltype(&context_wait) context_wait;
    decltype(&reset_commands) reset_commands;
    decltype(&transition) transition;
    decltype(&copy_transitioned) copy_transitioned;
    decltype(&submit) submit;
    decltype(&queue_wait) queue_wait;
    decltype(&queue_signal) queue_signal;
    decltype(&device_ready) device_ready;
    decltype(&surface_create) surface_create;
    decltype(&surface_destroy) surface_destroy;
    decltype(&surface_d3d11) surface_d3d11;
    decltype(&surface_d3d12) surface_d3d12;
    decltype(&fence_create) fence_create;
    decltype(&fence_destroy) fence_destroy;
    decltype(&fence_d3d11) fence_d3d11;
    decltype(&fence_d3d12) fence_d3d12;
    decltype(&dump_poll_requested) dump_poll_requested;
    decltype(&dump_inputs) dump_inputs;
    decltype(&dump_wants) dump_wants;
    decltype(&dump_result) dump_result;
};

constexpr Platform platform{
    rsf_sr_session_create, rsf_sr_session_select, rsf_sr_session_set_auto_exposure, rsf_sr_session_evaluate,
    rsf_sr_session_warm_up, rsf_sr_session_get_status, rsf_sr_session_destroy, validate_frame,
    immediate_context, query_context4, create_device12, create_queue, create_commands, create_output,
    create_texture, create_fence, create_event, close_event, describe, same_context, wait_fence, flush,
    copy_region, copy_resource, context_signal, context_wait, reset_commands, transition, copy_transitioned,
    submit, queue_wait, queue_signal, device_ready, surface_create, surface_destroy, surface_d3d11, surface_d3d12,
    fence_create, fence_destroy, fence_d3d11, fence_d3d12, dump_poll_requested, dump_inputs, dump_wants, dump_result,
};
}

extern "C" rsf_backend_result rsf_core_sr_bridge_create(const rsf_sr_session_setup*, rsf_sr_bridge**, const Platform*);

extern "C" rsf_backend_result rsf_sr_bridge_create(const rsf_sr_session_setup* setup, rsf_sr_bridge** out)
{
    return rsf_core_sr_bridge_create(setup, out, &platform);
}
