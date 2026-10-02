// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/sr_bridge.h>
#include <rescaleframe/shared_surface.h>
#include "../../backends/common/sr_helpers.h"
#include <d3d11_4.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <new>
using Microsoft::WRL::ComPtr;

struct rsf_sr_bridge {
    ComPtr<ID3D11Device> device11;
    ComPtr<ID3D11DeviceContext4> context11;
    ComPtr<ID3D12Device> device12;
    ComPtr<ID3D12CommandQueue> queue;
    struct Commands {
        ComPtr<ID3D12CommandAllocator> allocator;
        ComPtr<ID3D12GraphicsCommandList> list;
        uint64_t complete = 0;
    } commands[3];
    uint32_t next_commands = 0;
    ComPtr<ID3D12Resource> output;
    rsf_shared_fence* fence = nullptr;
    rsf_shared_surface* surfaces[5]{};
    rsf_sr_session* session = nullptr;
    uint64_t tick = 0;
    HANDLE event = nullptr;
    uint32_t width = 0, height = 0;
    DXGI_FORMAT color_format = DXGI_FORMAT_UNKNOWN;
    rsf_backend_log_fn log = nullptr;
    void* log_user = nullptr;
    bool faulted = false;
    uint32_t auto_exposure = 0;
};
namespace {
rsf_backend_result gpu_failure(rsf_sr_bridge* bridge)
{
    bridge->faulted = true;
    return RSF_BACKEND_ERROR_FEATURE_FAILED;
}
rsf_backend_result wait_value(rsf_sr_bridge* bridge, uint64_t value)
{
    if (!value) return RSF_BACKEND_OK;
    auto* fence = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(bridge->fence));
    if (fence->GetCompletedValue() == UINT64_MAX) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (fence->GetCompletedValue() >= value) return RSF_BACKEND_OK;
    if (FAILED(fence->SetEventOnCompletion(value, bridge->event))) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const DWORD result = WaitForSingleObject(bridge->event, 10000);
    return result == WAIT_OBJECT_0 ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
rsf_backend_result wait(rsf_sr_bridge* bridge) { return wait_value(bridge, bridge->tick); }
void release_surfaces(rsf_sr_bridge* bridge)
{
    for (auto*& surface : bridge->surfaces) {
        rsf_shared_surface_destroy(surface); surface = nullptr;
    }
    bridge->output.Reset();
    bridge->width = 0;
}
void transition(ID3D12GraphicsCommandList* list, ID3D12Resource* resource,
                D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after};
    list->ResourceBarrier(1, &barrier);
}
rsf_backend_result prepare(rsf_sr_bridge* bridge, const rsf_sr_frame& frame)
{
    const auto& record = *frame.record;
    auto* source = static_cast<ID3D11Texture2D*>(frame.color.resource);
    D3D11_TEXTURE2D_DESC color{}; source->GetDesc(&color);
    if (bridge->width == record.render_width && bridge->height == record.render_height &&
        bridge->color_format == color.Format) return RSF_BACKEND_OK;
    if (wait(bridge) != RSF_BACKEND_OK) return gpu_failure(bridge);
    release_surfaces(bridge);
    for (uint32_t i = 0; i < 5; ++i) {
        rsf_shared_surface_setup setup{};
        setup.struct_size = sizeof(setup);
        setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
        setup.width = i == 4 ? record.output_width : i == 3 ? 1 : record.render_width;
        setup.height = i == 4 ? record.output_height : i == 3 ? 1 : record.render_height;
        setup.format = i == 1 || i == 3 ? DXGI_FORMAT_R32_FLOAT : i == 2 ? DXGI_FORMAT_R16G16_FLOAT : color.Format;
        setup.log = bridge->log; setup.log_user = bridge->log_user;
        if (rsf_shared_surface_create(bridge->device11.Get(), bridge->device12.Get(), &setup,
                                     &bridge->surfaces[i]) != RSF_SHARED_OK) {
            release_surfaces(bridge); return RSF_BACKEND_ERROR_INIT_FAILED;
        }
    }
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = record.output_width; desc.Height = record.output_height;
    desc.DepthOrArraySize = 1; desc.MipLevels = 1; desc.SampleDesc.Count = 1;
    desc.Format = color.Format; desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    if (FAILED(bridge->device12->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS, nullptr, IID_PPV_ARGS(&bridge->output)))) {
        release_surfaces(bridge); return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    bridge->width = record.render_width; bridge->height = record.render_height;
    bridge->color_format = color.Format;
    return RSF_BACKEND_OK;
}
}
extern "C" rsf_backend_result rsf_sr_bridge_create(const rsf_sr_session_setup* setup, rsf_sr_bridge** out)
{
    if (!setup || !out || setup->struct_size < sizeof(*setup) ||
        setup->open.struct_size < sizeof(rsf_sr_open_desc) || !setup->open.device) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (setup->abi_version != RSF_SR_SESSION_ABI_VERSION || setup->open.abi_version != RSF_BACKEND_ABI_VERSION)
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (setup->open.api != RSF_API_D3D11) return RSF_BACKEND_ERROR_WRONG_API;
    auto* bridge = new (std::nothrow) rsf_sr_bridge;
    if (!bridge) return RSF_BACKEND_ERROR_INIT_FAILED;
    bridge->device11 = static_cast<ID3D11Device*>(setup->open.device);
    bridge->log = setup->open.log; bridge->log_user = setup->open.log_user;
    bridge->auto_exposure = setup->open.auto_exposure;
    ComPtr<ID3D11DeviceContext> context;
    bridge->device11->GetImmediateContext(&context);
    ComPtr<IDXGIDevice> dxgi;
    ComPtr<IDXGIAdapter> adapter;
    if (FAILED(context.As(&bridge->context11)) || FAILED(bridge->device11.As(&dxgi)) ||
        FAILED(dxgi->GetAdapter(&adapter)) || FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0,
                                                        IID_PPV_ARGS(&bridge->device12)))) {
        rsf_sr_bridge_destroy(bridge); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    D3D12_COMMAND_QUEUE_DESC queue{}; queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    if (FAILED(bridge->device12->CreateCommandQueue(&queue, IID_PPV_ARGS(&bridge->queue))) ||
        rsf_shared_fence_create(bridge->device11.Get(), bridge->device12.Get(), bridge->log,
                               bridge->log_user, &bridge->fence) != RSF_SHARED_OK) {
        rsf_sr_bridge_destroy(bridge); return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    bridge->event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!bridge->event) { rsf_sr_bridge_destroy(bridge); return RSF_BACKEND_ERROR_INIT_FAILED; }
    for (auto& commands : bridge->commands) {
        if (FAILED(bridge->device12->CreateCommandAllocator(queue.Type, IID_PPV_ARGS(&commands.allocator))) ||
            FAILED(bridge->device12->CreateCommandList(0, queue.Type, commands.allocator.Get(), nullptr,
                                                      IID_PPV_ARGS(&commands.list))) || FAILED(commands.list->Close())) {
            rsf_sr_bridge_destroy(bridge); return RSF_BACKEND_ERROR_INIT_FAILED;
        }
    }
    rsf_sr_session_setup translated = *setup;
    translated.open.api = RSF_API_D3D12;
    translated.open.device = bridge->device12.Get();
    const auto result = rsf_sr_session_create(&translated, &bridge->session);
    if (result != RSF_BACKEND_OK) { rsf_sr_bridge_destroy(bridge); return result; }
    *out = bridge;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_sr_bridge_select(rsf_sr_bridge* bridge,
    rsf_sr_backend backend, rsf_quality quality, uint64_t version_id)
{
    if (!bridge) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (bridge->faulted && backend != RSF_SR_NONE) return RSF_BACKEND_ERROR_NOT_READY;
    const auto result = wait(bridge);
    return result == RSF_BACKEND_OK ? rsf_sr_session_select(bridge->session, backend, quality, version_id) : gpu_failure(bridge);
}
extern "C" rsf_backend_result rsf_sr_bridge_set_auto_exposure(rsf_sr_bridge* bridge, uint32_t enabled)
{
    if (!bridge || enabled > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (bridge->faulted) return RSF_BACKEND_ERROR_NOT_READY;
    if (bridge->auto_exposure == enabled) return RSF_BACKEND_OK;
    if (wait(bridge) != RSF_BACKEND_OK) return gpu_failure(bridge);
    const auto result = rsf_sr_session_set_auto_exposure(bridge->session, enabled);
    if (result == RSF_BACKEND_OK) bridge->auto_exposure = enabled;
    return result;
}
extern "C" rsf_backend_result rsf_sr_bridge_evaluate(rsf_sr_bridge* bridge,
    void* context_pointer, const rsf_sr_frame* frame)
{
    if (!bridge || !context_pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (bridge->faulted) return RSF_BACKEND_ERROR_NOT_READY;
    auto result = rsf::validate_frame(frame);
    if (result != RSF_BACKEND_OK) return result;
    ComPtr<ID3D11DeviceContext4> context;
    if (FAILED(static_cast<ID3D11DeviceContext*>(context_pointer)->QueryInterface(IID_PPV_ARGS(&context))) ||
        context.Get() != bridge->context11.Get()) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    // Check all source owners, extents and formats before issuing any copy.
    const rsf_backend_resource* inputs[] = {&frame->color, &frame->depth, &frame->motion, &frame->exposure, &frame->output};
    for (uint32_t i = 0; i < 5; ++i) {
        if (i == 3 && !inputs[i]->resource) continue;
        auto* texture = static_cast<ID3D11Texture2D*>(inputs[i]->resource);
        ComPtr<ID3D11Device> owner; texture->GetDevice(&owner);
        D3D11_TEXTURE2D_DESC desc{}; texture->GetDesc(&desc);
        uint32_t width = i == 4 ? frame->record->output_width : i == 3 ? 1 : frame->record->render_width;
        uint32_t height = i == 4 ? frame->record->output_height : i == 3 ? 1 : frame->record->render_height;
        if (owner.Get() != bridge->device11.Get() || desc.Width < width || desc.Height < height ||
            desc.SampleDesc.Count != 1 || desc.ArraySize != 1 ||
            (i == 4 && (desc.Width != width || desc.Height != height)) ||
            ((i == 1 || i == 3) && desc.Format != DXGI_FORMAT_R32_FLOAT) ||
            (i == 2 && desc.Format != DXGI_FORMAT_R16G16_FLOAT) ||
            ((i == 0 || i == 4) && desc.Format != DXGI_FORMAT_R16G16B16A16_FLOAT)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    result = prepare(bridge, *frame);
    if (result != RSF_BACKEND_OK) return result;
    // CPU waits protect allocator reuse only. Cross-API resource ownership stays ordered on
    // the GPU: D3D11 inputs -> D3D12 SR -> D3D11 output copy -> the next frame's inputs.
    auto& commands = bridge->commands[bridge->next_commands];
    if (wait_value(bridge,commands.complete) != RSF_BACKEND_OK) return gpu_failure(bridge);
    if (FAILED(commands.allocator->Reset()) || FAILED(commands.list->Reset(commands.allocator.Get(),nullptr)))
        return gpu_failure(bridge);
    auto* list = commands.list.Get();
    for (uint32_t i = 0; i < 4; ++i) {
        if (!inputs[i]->resource) continue;
        D3D11_BOX box{0, 0, 0, i == 3 ? 1u : frame->record->render_width,
                                 i == 3 ? 1u : frame->record->render_height, 1};
        context->CopySubresourceRegion(static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(bridge->surfaces[i])),
            0, 0, 0, 0, static_cast<ID3D11Resource*>(inputs[i]->resource), 0, &box);
    }
    auto* fence11 = static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(bridge->fence));
    auto* fence12 = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(bridge->fence));
    const uint64_t ready = ++bridge->tick;
    if (FAILED(context->Signal(fence11, ready))) return gpu_failure(bridge);
    context->Flush();
    if (FAILED(bridge->queue->Wait(fence12, ready))) return gpu_failure(bridge);
    rsf_sr_frame translated = *frame;
    rsf_backend_resource* targets[] = {&translated.color, &translated.depth, &translated.motion, &translated.exposure};
    for (uint32_t i = 0; i < 4; ++i) {
        if (!inputs[i]->resource) continue;
        targets[i]->resource = rsf_shared_surface_d3d12(bridge->surfaces[i]);
        targets[i]->state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        transition(list, static_cast<ID3D12Resource*>(targets[i]->resource),
                   D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    translated.output.resource = bridge->output.Get();
    translated.output.state = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    result = rsf_sr_session_evaluate(bridge->session, list, &translated);
    for (uint32_t i = 0; i < 4; ++i) {
        if (inputs[i]->resource) transition(list, static_cast<ID3D12Resource*>(targets[i]->resource),
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
    }
    auto* shared_output = static_cast<ID3D12Resource*>(rsf_shared_surface_d3d12(bridge->surfaces[4]));
    if (result == RSF_BACKEND_OK) {
        transition(list, bridge->output.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
        transition(list, shared_output, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST);
        list->CopyResource(shared_output, bridge->output.Get());
        transition(list, shared_output, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COMMON);
        transition(list, bridge->output.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    }
    if (FAILED(list->Close())) return gpu_failure(bridge);
    ID3D12CommandList* lists[] = {list}; bridge->queue->ExecuteCommandLists(1, lists);
    const uint64_t complete = ++bridge->tick;
    if (FAILED(bridge->queue->Signal(fence12, complete)))
        return gpu_failure(bridge);
    commands.complete = complete;
    bridge->next_commands = (bridge->next_commands+1) % 3;
    // Even a refused evaluation may have recorded commands that reference shared inputs.
    if (FAILED(context->Wait(fence11, complete))) return gpu_failure(bridge);
    if (result == RSF_BACKEND_OK) {
        context->CopyResource(static_cast<ID3D11Resource*>(frame->output.resource),
                             static_cast<ID3D11Resource*>(rsf_shared_surface_d3d11(bridge->surfaces[4])));
        const uint64_t copied = ++bridge->tick;
        if (FAILED(context->Signal(fence11, copied))) return gpu_failure(bridge);
    }
    context->Flush();
    return result;
}
extern "C" rsf_backend_result rsf_sr_bridge_get_status(const rsf_sr_bridge* bridge,
    rsf_sr_session_status* status)
{
    return bridge ? rsf_sr_session_get_status(bridge->session, status) : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
extern "C" void rsf_sr_bridge_destroy(rsf_sr_bridge* bridge)
{
    if (!bridge) return;
    if (bridge->fence && bridge->event && wait(bridge) != RSF_BACKEND_OK &&
        SUCCEEDED(bridge->device12->GetDeviceRemovedReason())) {
        // A timed-out queue may still reference SDK descriptors and shared surfaces. Keep the
        // failed bridge alive until process exit rather than freeing memory under pending work.
        if (bridge->log) bridge->log(bridge->log_user, "SR GPU completion failed; retaining the failed bridge resources until process exit");
        return;
    }
    rsf_sr_session_destroy(bridge->session);
    release_surfaces(bridge);
    rsf_shared_fence_destroy(bridge->fence);
    if (bridge->event) CloseHandle(bridge->event);
    delete bridge;
}
