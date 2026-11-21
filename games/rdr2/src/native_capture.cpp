// SPDX-License-Identifier: MIT
// Bounded copies after actual swapchain draws, preserving the game's command list and state.
#include "native_capture.h"
#include "capture_state.h"
#include "camera_probe.h"
#include <rescaleframe/census_d3d12_observer.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <intrin.h>
#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cstddef>
#include <memory>
#include <iterator>
#include <mutex>
#include <optional>
#include <vector>
#include <string>

namespace {
using Microsoft::WRL::ComPtr;
using Pipeline = rsf_rdr2_capture_pipeline;
constexpr rsf_rdr2_capture_gpu_ops gpu_ops{
    [](void* object) noexcept { static_cast<IUnknown*>(object)->AddRef(); },
    [](void* object) noexcept { static_cast<IUnknown*>(object)->Release(); },
    [](void* pointer) noexcept -> void* {
        auto* queue = static_cast<ID3D12CommandQueue*>(pointer);
        ComPtr<ID3D12Device> device;
        ComPtr<ID3D12Fence> fence;
        if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device))) ||
            FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return nullptr;
        return fence.Detach();
    },
    [](void* queue, void* fence, uint64_t value) noexcept {
        return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->Signal(static_cast<ID3D12Fence*>(fence), value));
    },
    [](void* swapchain, uint32_t index) noexcept -> void* {
        ComPtr<ID3D12Resource> buffer;
        if (FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetBuffer(index, IID_PPV_ARGS(&buffer)))) return nullptr;
        return buffer.Detach();
    },
};
Pipeline blend_facts(const D3D12_RENDER_TARGET_BLEND_DESC& b) noexcept
{
    return {0, 0, b.BlendEnable, static_cast<uint32_t>(b.SrcBlend), static_cast<uint32_t>(b.DestBlend),
        static_cast<uint32_t>(b.BlendOp), static_cast<uint32_t>(b.SrcBlendAlpha),
        static_cast<uint32_t>(b.DestBlendAlpha), static_cast<uint32_t>(b.BlendOpAlpha), b.RenderTargetWriteMask};
}
constexpr auto unknown_state = static_cast<D3D12_RESOURCE_STATES>(~0u);
struct Shot : rsf_rdr2_capture_readback {
    rsf_rdr2_capture_gpu_leases gpu{};
    uint64_t resource{}, tick{}, caller{};
    UINT width{}, height{}, pitch{}, format{}, vertices{}, instances{}, frame{}, sequence{}, ordinal{};
    Pipeline pipeline{};
    std::string role{"draw"};
    UINT footprint_format{}, subresource{}, state_before{}, input_index{};

    Shot() : rsf_rdr2_capture_readback{} {}
    Shot(const Shot&) = delete;
    Shot& operator=(const Shot&) = delete;
    ~Shot() { rsf_rdr2_capture_gpu_drop(&gpu); }
    ID3D12Resource* buffer() const noexcept { return static_cast<ID3D12Resource*>(gpu.buffer); }
    uint64_t completed() const noexcept { return gpu.fence ? static_cast<ID3D12Fence*>(gpu.fence)->GetCompletedValue() : 0; }
};
struct Submission {
    ID3D12CommandQueue* queue{};
    std::unique_ptr<rsf_rdr2_capture_submission, decltype(&rsf_rdr2_capture_discard_submission)>
        facts{nullptr, rsf_rdr2_capture_discard_submission};
    std::vector<std::shared_ptr<Shot>> shots;
};
using DrawEvent = rsf_rdr2_capture_draw;
thread_local std::vector<Submission> submissions;
thread_local bool recording_copy{};
thread_local bool drawing{};
struct CopyScope {
    bool previous{recording_copy};
    CopyScope() { recording_copy = true; }
    ~CopyScope() { recording_copy = previous; }
};
struct DrawScope {
    bool outer{!drawing};
    DrawScope() { drawing = true; }
    ~DrawScope() { if (outer) drawing = false; }
};
struct State {
    std::mutex mutex;
    rsf_rdr2_capture_paths paths{};
    uintptr_t base{};
    rsf_rdr2_capture_state* capture{};
    std::vector<std::shared_ptr<Shot>> shots;
    std::shared_ptr<Shot> tail;
#ifdef RSF_RDR2_CAPTURE_TEST
    void* fixture_swapchain{};
#endif
};
State* g{}; // Pinned with the diagnostic ASI until process exit.
rsf_rdr2_capture_progress capture_progress() noexcept
{
    rsf_rdr2_capture_progress status{};
    rsf_rdr2_capture_progress_get(g->capture, &status);
    return status;
}
bool read_local(uint64_t address, void* out, size_t bytes);
void log(const char* format, ...) noexcept
{
    FILE* f = _wfopen(g->paths.log, L"ab");
    if (!f) return;
    fprintf(f, "[%llu] ", static_cast<unsigned long long>(GetTickCount64()));
    va_list args; va_start(args, format); vfprintf(f, format, args); va_end(args);
    fputc('\n', f); fclose(f);
}
std::optional<Pipeline> hash_pipeline(const D3D12_SHADER_BYTECODE& ps, const D3D12_SHADER_BYTECODE& vs,
    Pipeline pipeline) noexcept
{
    if (!rsf_rdr2_capture_hash_shader(static_cast<const uint8_t*>(ps.pShaderBytecode), ps.BytecodeLength, &pipeline.ps) ||
        !rsf_rdr2_capture_hash_shader(static_cast<const uint8_t*>(vs.pShaderBytecode), vs.BytecodeLength, &pipeline.vs))
        return std::nullopt;
    return pipeline;
}
void remember_pipeline(void* result, const std::optional<Pipeline>& facts)
{
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_record_pipeline(g->capture, result, facts ? &*facts : nullptr);
}
static_assert(MH_OK == 0 && MH_ERROR_ENABLED == 5 && MH_UNKNOWN == -1);
constexpr rsf_rdr2_capture_hook_ops hook_ops{
    [](void* object, uint32_t slot) noexcept { return (*static_cast<void***>(object))[slot]; },
    [](void* target, const void* detour, void** original) noexcept {
        return static_cast<int32_t>(MH_CreateHook(target, const_cast<void*>(detour), original));
    },
    [](void* target) noexcept { return static_cast<int32_t>(MH_EnableHook(target)); },
    [](uint32_t slot, void* target, int32_t result) noexcept {
        log("hook slot=%zu target=%p result=%s", static_cast<size_t>(slot), target, MH_StatusToString(static_cast<MH_STATUS>(result)));
    },
};

template<class Detour> void bind(rsf_rdr2_capture_hook role, void* object, Detour detour) noexcept
{
    rsf_rdr2_capture_bind_hook(&hook_ops, static_cast<uint32_t>(role), object, reinterpret_cast<const void*>(detour));
}

template<class Original> Original original(rsf_rdr2_capture_hook role) noexcept
{
    return reinterpret_cast<Original>(rsf_rdr2_capture_hook_original(static_cast<uint32_t>(role)));
}

using CreatePso = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, const D3D12_GRAPHICS_PIPELINE_STATE_DESC*, REFIID, void**);
using CreateList = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, UINT, D3D12_COMMAND_LIST_TYPE, ID3D12CommandAllocator*, ID3D12PipelineState*, REFIID, void**);
using CreateRtv = void (STDMETHODCALLTYPE*)(ID3D12Device*, ID3D12Resource*, const D3D12_RENDER_TARGET_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);
using CreateLibrary = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device1*, const void*, SIZE_T, REFIID, void**);
using LoadPso = HRESULT (STDMETHODCALLTYPE*)(ID3D12PipelineLibrary*, LPCWSTR, const D3D12_GRAPHICS_PIPELINE_STATE_DESC*, REFIID, void**);
using CreateStreamPso = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device2*, const D3D12_PIPELINE_STATE_STREAM_DESC*, REFIID, void**);
using LoadStreamPso = HRESULT (STDMETHODCALLTYPE*)(ID3D12PipelineLibrary1*, LPCWSTR, const D3D12_PIPELINE_STATE_STREAM_DESC*, REFIID, void**);
using Reset = HRESULT (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, ID3D12CommandAllocator*, ID3D12PipelineState*);
using SetPso = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, ID3D12PipelineState*);
using SetRtv = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, const D3D12_CPU_DESCRIPTOR_HANDLE*, BOOL, const D3D12_CPU_DESCRIPTOR_HANDLE*);
using Draw = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, UINT, UINT, UINT);
using DrawIndexed = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, UINT, UINT, INT, UINT);
using Barrier = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, const D3D12_RESOURCE_BARRIER*);
using CreateCommitted = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, const D3D12_HEAP_PROPERTIES*, D3D12_HEAP_FLAGS,
    const D3D12_RESOURCE_DESC*, D3D12_RESOURCE_STATES, const D3D12_CLEAR_VALUE*, REFIID, void**);
using CreatePlaced = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, ID3D12Heap*, UINT64,
    const D3D12_RESOURCE_DESC*, D3D12_RESOURCE_STATES, const D3D12_CLEAR_VALUE*, REFIID, void**);
using SetHdr = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain4*, DXGI_HDR_METADATA_TYPE, UINT, void*);
// D3D12 streams align each tagged subobject to a pointer, with natural payload alignment.
template<class T> struct StreamPayload { D3D12_PIPELINE_STATE_SUBOBJECT_TYPE type; T value; };
template<class T> union StreamItem { StreamPayload<T> payload; void* alignment; };
template<class T> bool stream_value(const unsigned char* data, size_t remaining, T& value, size_t& size)
{
    size = sizeof(StreamItem<T>);
    if (remaining < size) return false;
    std::memcpy(&value, data + offsetof(StreamPayload<T>, value), sizeof(T));
    return true;
}
std::optional<Pipeline> stream_facts(const D3D12_PIPELINE_STATE_STREAM_DESC* desc) noexcept
{
    if (!desc || !desc->pPipelineStateSubobjectStream) return std::nullopt;
    D3D12_SHADER_BYTECODE ps{}, vs{};
    Pipeline pipeline{};
    const auto* data = static_cast<const unsigned char*>(desc->pPipelineStateSubobjectStream);
    for (size_t offset = 0; offset < desc->SizeInBytes;) {
        const auto remaining = desc->SizeInBytes - offset;
        if (remaining < sizeof(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE)) return std::nullopt;
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE type{};
        std::memcpy(&type, data + offset, sizeof(type));
        size_t size{};
        switch (type) {
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS: {
            auto& shader = type == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS ? vs : ps;
            if (!stream_value(data + offset, remaining, shader, size)) return std::nullopt;
            break;
        }
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND: {
            D3D12_BLEND_DESC blend{};
            if (!stream_value(data + offset, remaining, blend, size)) return std::nullopt;
            pipeline = blend_facts(blend.RenderTarget[0]);
            break;
        }
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS: size = sizeof(StreamItem<D3D12_SHADER_BYTECODE>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE: size = sizeof(StreamItem<ID3D12RootSignature*>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT: size = sizeof(StreamItem<D3D12_STREAM_OUTPUT_DESC>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER: size = sizeof(StreamItem<D3D12_RASTERIZER_DESC>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL: size = sizeof(StreamItem<D3D12_DEPTH_STENCIL_DESC>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1: size = sizeof(StreamItem<D3D12_DEPTH_STENCIL_DESC1>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT: size = sizeof(StreamItem<D3D12_INPUT_LAYOUT_DESC>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS: size = sizeof(StreamItem<D3D12_RT_FORMAT_ARRAY>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC: size = sizeof(StreamItem<DXGI_SAMPLE_DESC>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO: size = sizeof(StreamItem<D3D12_CACHED_PIPELINE_STATE>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING: size = sizeof(StreamItem<D3D12_VIEW_INSTANCING_DESC>); break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS: size = sizeof(StreamItem<UINT>); break;
        default: return std::nullopt; // Unknown stream layouts remain explicitly unidentified.
        }
        if (size > remaining) return std::nullopt;
        offset += size;
    }
    return hash_pipeline(ps, vs, pipeline);
}
HRESULT STDMETHODCALLTYPE create_stream_pso(ID3D12Device2* device,
    const D3D12_PIPELINE_STATE_STREAM_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original<CreateStreamPso>(RSF_RDR2_CAPTURE_CREATE_STREAM_PSO)(device, desc, iid, out);
    if (SUCCEEDED(hr) && out && *out) remember_pipeline(*out, stream_facts(desc));
    return hr;
}
HRESULT STDMETHODCALLTYPE load_stream_pso(ID3D12PipelineLibrary1* library, LPCWSTR name,
    const D3D12_PIPELINE_STATE_STREAM_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original<LoadStreamPso>(RSF_RDR2_CAPTURE_LOAD_STREAM_PSO)(library, name, desc, iid, out);
    if (SUCCEEDED(hr) && out && *out) remember_pipeline(*out, stream_facts(desc));
    return hr;
}

std::optional<Pipeline> graphics_pipeline_facts(const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc) noexcept
{
    if (!desc) return std::nullopt;
    return hash_pipeline(desc->PS, desc->VS, blend_facts(desc->BlendState.RenderTarget[0]));
}
HRESULT STDMETHODCALLTYPE create_pso(ID3D12Device* d, const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original<CreatePso>(RSF_RDR2_CAPTURE_CREATE_PSO)(d, desc, iid, out);
    if (SUCCEEDED(hr) && out && *out) remember_pipeline(*out, graphics_pipeline_facts(desc));
    return hr;
}
HRESULT STDMETHODCALLTYPE load_pso(ID3D12PipelineLibrary* library, LPCWSTR name,
    const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original<LoadPso>(RSF_RDR2_CAPTURE_LOAD_PSO)(library, name, desc, iid, out);
    if (SUCCEEDED(hr) && out && *out) remember_pipeline(*out, graphics_pipeline_facts(desc));
    return hr;
}
HRESULT STDMETHODCALLTYPE create_library(ID3D12Device1* d, const void* blob, SIZE_T size, REFIID iid, void** out)
{
    const auto hr = original<CreateLibrary>(RSF_RDR2_CAPTURE_CREATE_LIBRARY)(d, blob, size, iid, out);
    if (SUCCEEDED(hr) && out && *out) {
        bind(RSF_RDR2_CAPTURE_LOAD_PSO, *out, &load_pso);
        ComPtr<ID3D12PipelineLibrary1> library1;
        if (SUCCEEDED(static_cast<IUnknown*>(*out)->QueryInterface(IID_PPV_ARGS(&library1))))
            bind(RSF_RDR2_CAPTURE_LOAD_STREAM_PSO, library1.Get(), &load_stream_pso);
    }
    return hr;
}
void STDMETHODCALLTYPE create_rtv(ID3D12Device* d, ID3D12Resource* resource,
    const D3D12_RENDER_TARGET_VIEW_DESC* desc, D3D12_CPU_DESCRIPTOR_HANDLE handle)
{
    original<CreateRtv>(RSF_RDR2_CAPTURE_CREATE_RTV)(d, resource, desc, handle);
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_record_rtv(g->capture, handle.ptr, resource);
}
HRESULT STDMETHODCALLTYPE create_committed(ID3D12Device* d, const D3D12_HEAP_PROPERTIES* heap,
    D3D12_HEAP_FLAGS flags, const D3D12_RESOURCE_DESC* desc, D3D12_RESOURCE_STATES state,
    const D3D12_CLEAR_VALUE* clear, REFIID iid, void** out)
{
    const auto hr = original<CreateCommitted>(RSF_RDR2_CAPTURE_CREATE_COMMITTED)(d, heap, flags, desc, state, clear, iid, out);
    if (!recording_copy && SUCCEEDED(hr) && out && *out) {
        std::lock_guard<std::mutex> lock(g->mutex);
        rsf_rdr2_capture_initial_state(g->capture, *out, static_cast<uint32_t>(state));
    }
    return hr;
}
HRESULT STDMETHODCALLTYPE create_placed(ID3D12Device* d, ID3D12Heap* heap, UINT64 offset,
    const D3D12_RESOURCE_DESC* desc, D3D12_RESOURCE_STATES state, const D3D12_CLEAR_VALUE* clear,
    REFIID iid, void** out)
{
    const auto hr = original<CreatePlaced>(RSF_RDR2_CAPTURE_CREATE_PLACED)(d, heap, offset, desc, state, clear, iid, out);
    if (!recording_copy && SUCCEEDED(hr) && out && *out) {
        std::lock_guard<std::mutex> lock(g->mutex);
        rsf_rdr2_capture_initial_state(g->capture, *out, static_cast<uint32_t>(state));
    }
    return hr;
}
void STDMETHODCALLTYPE barriers(ID3D12GraphicsCommandList* list, UINT count, const D3D12_RESOURCE_BARRIER* items)
{
    original<Barrier>(RSF_RDR2_CAPTURE_BARRIER)(list, count, items);
    if (recording_copy) return;
    std::lock_guard<std::mutex> lock(g->mutex);
    for (UINT i = 0; i < count; ++i) {
        const auto& b = items[i];
        if (b.Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION) {
            const auto& t = b.Transition;
            rsf_rdr2_capture_transition(g->capture, list, t.pResource, t.Subresource,
                static_cast<uint32_t>(t.StateAfter), b.Flags == D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY);
        } else if (b.Type == D3D12_RESOURCE_BARRIER_TYPE_ALIASING) {
            rsf_rdr2_capture_alias(g->capture, list, b.Aliasing.pResourceAfter);
        }
    }
}
HRESULT STDMETHODCALLTYPE reset(ID3D12GraphicsCommandList* list, ID3D12CommandAllocator* allocator, ID3D12PipelineState* pso)
{
    const auto hr = original<Reset>(RSF_RDR2_CAPTURE_RESET)(list, allocator, pso);
    if (SUCCEEDED(hr)) {
        std::lock_guard<std::mutex> lock(g->mutex);
        rsf_rdr2_capture_reset_list(g->capture, list, pso);
    }
    return hr;
}
void STDMETHODCALLTYPE set_pso(ID3D12GraphicsCommandList* list, ID3D12PipelineState* pso)
{
    original<SetPso>(RSF_RDR2_CAPTURE_SET_PSO)(list, pso);
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_bind_pipeline(g->capture, list, pso);
}
void STDMETHODCALLTYPE set_rtv(ID3D12GraphicsCommandList* list, UINT count,
    const D3D12_CPU_DESCRIPTOR_HANDLE* handles, BOOL contiguous, const D3D12_CPU_DESCRIPTOR_HANDLE* depth)
{
    original<SetRtv>(RSF_RDR2_CAPTURE_SET_RTV)(list, count, handles, contiguous, depth);
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_bind_rtv(g->capture, list, count && handles ? &handles[0].ptr : nullptr);
}
bool copy_resource(ID3D12GraphicsCommandList* list, ID3D12Resource* target,
    D3D12_RESOURCE_STATES before, const std::shared_ptr<Shot>& shot)
{
    CopyScope copying;
    const auto desc = target->GetDesc();
    if (desc.SampleDesc.Count != 1 || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D ||
        desc.DepthOrArraySize != 1 || before == unknown_state) return false;
    ComPtr<ID3D12Device> device;
    if (FAILED(target->GetDevice(IID_PPV_ARGS(&device)))) return false;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
    UINT64 bytes{};
    device->GetCopyableFootprints(&desc, 0, 1, 0, &footprint, nullptr, nullptr, &bytes);
    // GetCopyableFootprints need not include the final row's padding. Our raw file format does.
    const auto padded_bytes = footprint.Offset + static_cast<UINT64>(footprint.Footprint.RowPitch) * footprint.Footprint.Height;
    if (bytes < padded_bytes) bytes = padded_bytes;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC destination{};
    destination.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; destination.Width = bytes;
    destination.Height = 1; destination.DepthOrArraySize = 1; destination.MipLevels = 1;
    destination.SampleDesc.Count = 1; destination.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (!shot->buffer()) {
        ComPtr<ID3D12Resource> buffer;
        const auto hr = device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &destination,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&buffer));
        if (FAILED(hr)) { log("readback allocation failed hr=%08lx", static_cast<unsigned long>(hr)); return false; }
        if (!rsf_rdr2_capture_gpu_adopt(&shot->gpu, buffer.Detach(), &gpu_ops)) return false;
    }
    rsf_rdr2_capture_gpu_bind_fence(&shot->gpu, nullptr);
    // Preserve the observed input state; copying does not alter the producer/consumer contract.
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition = {target, 0, before, D3D12_RESOURCE_STATE_COPY_SOURCE};
    if (before != D3D12_RESOURCE_STATE_COPY_SOURCE) list->ResourceBarrier(1, &barrier);
    D3D12_TEXTURE_COPY_LOCATION source{}; source.pResource = target;
    source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION dest{}; dest.pResource = shot->buffer();
    dest.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; dest.PlacedFootprint = footprint;
    list->CopyTextureRegion(&dest, 0, 0, 0, &source, nullptr);
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barrier.Transition.StateAfter = before;
    if (before != D3D12_RESOURCE_STATE_COPY_SOURCE) list->ResourceBarrier(1, &barrier);
    shot->width = static_cast<UINT>(desc.Width); shot->height = desc.Height;
    shot->format = desc.Format; shot->pitch = footprint.Footprint.RowPitch;
    shot->footprint_format = footprint.Footprint.Format; shot->state_before = before;
    rsf_rdr2_capture_stamp_copy(g->capture, list, shot.get());
    shot->resource = reinterpret_cast<uint64_t>(target); shot->tick = GetTickCount64();
    read_local(g->base + 0x542904c, &shot->frame, sizeof(shot->frame));
    return true;
}
template<class T> bool ngx_get(const void* params, size_t slot, const char* key, T& value)
{
    if (!params) return false;
    using Get = int32_t (*)(const void*, const char*, T*);
    const auto table = *static_cast<void* const* const*>(params);
    return reinterpret_cast<Get>(table[slot])(params, key, &value) == 1;
}
bool read_local(uint64_t address, void* out, size_t bytes)
{
    SIZE_T copied{};
    return address && ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<void*>(address), out, bytes, &copied) && copied == bytes;
}
void input_camera(const rsf_rdr2_capture_input_id& id)
{
    rsf_rdr2_capture_camera_facts facts{};
    uint8_t camera[RSF_RDR2_CAPTURE_CAMERA_BYTES]{};
    bool valid{};
    uint32_t index{}; uint64_t block{};
    read_local(g->base + 0x542904c, &facts.frame, sizeof(facts.frame));
    LARGE_INTEGER qpc{}; QueryPerformanceCounter(&qpc); facts.qpc = qpc.QuadPart;
    facts.tick = GetTickCount64(); facts.thread = GetCurrentThreadId();
    if (read_local(g->base + 0x5b403e0, &index, sizeof(index)) && index < 4096 &&
        read_local(__readgsqword(0x58) + index * 8, &block, sizeof(block))) {
        read_local(block + 0x14d8, &facts.view, sizeof(facts.view));
        read_local(block + 0x14f0, &facts.draw_context, sizeof(facts.draw_context));
        read_local(block + 0x1ce8, &facts.job, sizeof(facts.job));
        valid = read_local(facts.view, camera, sizeof(camera));
    }
    rsf_rdr2_capture_input_camera(g->capture, &id, &facts, valid ? camera : nullptr);
}
void input_resource(ID3D12GraphicsCommandList* list, const void* parameters,
    const rsf_rdr2_capture_input_id& id, rsf_rdr2_capture_input_role input_role,
    const char* key, const char* role, bool required = true)
{
    ID3D12Resource* resource{};
    if ((!ngx_get(parameters, 9, key, resource) && !ngx_get(parameters, 8, key, resource)) || !resource) {
        if (required) rsf_rdr2_capture_input_resource_refused(g->capture, &id, input_role, true);
        return;
    }
    const auto before = static_cast<D3D12_RESOURCE_STATES>(rsf_rdr2_capture_resource_state(g->capture, list, resource));
    auto shot = std::make_shared<Shot>();
    shot->role = role; shot->sequence = id.sequence; shot->input_index = id.index;
    g->shots.push_back(shot);
    if (!copy_resource(list, resource, before, shot)) {
        g->shots.pop_back();
        rsf_rdr2_capture_input_resource_refused(g->capture, &id, input_role, false);
    }
}
void ngx_created(void* handle, const void* parameters, uint32_t feature) noexcept try
{
    uint32_t flags{};
    bool observed{};
    if (feature == 1) {
        int32_t signed_flags{};
        if (ngx_get(parameters, 11, "DLSS.Feature.Create.Flags", signed_flags)) {
            flags = static_cast<uint32_t>(signed_flags);
            observed = true;
        } else {
            observed = ngx_get(parameters, 12, "DLSS.Feature.Create.Flags", flags);
        }
    }
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_record_feature(g->capture, handle, observed ? &flags : nullptr);
}
catch (...) { log("NGX feature observation failed"); }
void ngx_released(void* handle) noexcept
{
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_record_feature(g->capture, handle, nullptr);
}
struct FinishInput {
    bool pending{true};
    ~FinishInput() { if (pending) rsf_rdr2_capture_finish_input(g->capture); }
};
void ngx_evaluate(void* pointer, void* handle, const void* parameters, uint32_t phase, uint64_t result) noexcept try
{
    if (!rsf_rdr2_capture_track_evaluation(phase != 0)) return;
    if (!phase && (!pointer || !rsf_rdr2_capture_has_inputs(g->capture))) return;
    auto* list = static_cast<ID3D12GraphicsCommandList*>(pointer);
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_input_id id{};
    if (phase) {
        FinishInput finish;
        if (rsf_rdr2_capture_input_result(g->capture, list, result, &id))
            input_resource(list, parameters, id, RSF_RDR2_CAPTURE_OUTPUT, "Output", "output");
        return;
    }
    if (!rsf_rdr2_capture_begin_input(g->capture, list, handle, &id)) return;
    FinishInput finish;
    input_camera(id);
    for (uint32_t index = 0;; ++index) {
        bool floating{};
        const auto* key = rsf_rdr2_capture_input_scalar_key(index, &floating);
        if (!key) break;
        if (floating) {
            float value{};
            const bool available = ngx_get(parameters, 14, key, value);
            rsf_rdr2_capture_input_scalar(g->capture, &id, index, value, available);
        } else {
            uint32_t value{}; int32_t alternate{};
            if (ngx_get(parameters, 12, key, value))
                rsf_rdr2_capture_input_scalar(g->capture, &id, index, value, true);
            else {
                const bool available = ngx_get(parameters, 11, key, alternate);
                rsf_rdr2_capture_input_scalar(g->capture, &id, index, alternate, available);
            }
        }
    }
    input_resource(list, parameters, id, RSF_RDR2_CAPTURE_COLOR, "Color", "color");
    input_resource(list, parameters, id, RSF_RDR2_CAPTURE_DEPTH, "Depth", "depth");
    input_resource(list, parameters, id, RSF_RDR2_CAPTURE_MOTION, "MotionVectors", "motion");
    input_resource(list, parameters, id, RSF_RDR2_CAPTURE_EXPOSURE, "ExposureTexture", "exposure", false);
    finish.pending = !rsf_rdr2_capture_commit_input(g->capture);
}
catch (...) { log("NGX input observation failed"); }
void snapshot(ID3D12GraphicsCommandList* list, UINT vertices, UINT instances, void* caller) noexcept try
{
    if (!rsf_rdr2_capture_active(g->capture)) return;
    std::unique_lock<std::mutex> lock(g->mutex);
    if (!rsf_rdr2_capture_active(g->capture)) return;
    rsf_rdr2_capture_draw_binding binding{};
    if (!rsf_rdr2_capture_get_draw_binding(g->capture, list, &binding)) return;
    auto* target = static_cast<ID3D12Resource*>(binding.target);
    if (!rsf_rdr2_capture_gpu_accepts_target(g->capture, target)) return;
    const auto progress = capture_progress();
    const bool reuse_tail = progress.reuse_tail != 0;
    auto shot = reuse_tail ? g->tail : std::make_shared<Shot>();
    if (!shot || (reuse_tail && !rsf_rdr2_capture_reuse_readback(g->capture, shot.get(), list, shot->completed())))
        return;
    // Keep the lease before issuing the GPU copy. Later diagnostic allocations may fail.
    if (!reuse_tail) g->shots.push_back(shot);
    if (!copy_resource(list, target, D3D12_RESOURCE_STATE_RENDER_TARGET, shot)) {
        if (!reuse_tail) g->shots.pop_back();
        return;
    }
    shot->sequence = progress.sequence; shot->ordinal = progress.draws;
    shot->vertices = vertices; shot->instances = instances;
    shot->caller = reinterpret_cast<uint64_t>(caller);
    shot->pipeline = binding.pipeline;
    rsf_rdr2_capture_gpu_select_target(g->capture, target);
    const auto action = rsf_rdr2_capture_commit_draw(g->capture);
    if (action & RSF_RDR2_CAPTURE_KEEP_TAIL) g->tail = shot;
    if (action & RSF_RDR2_CAPTURE_SAVE_DRAW) {
        DrawEvent event{shot->ordinal, shot->frame, vertices, instances, reinterpret_cast<uint64_t>(list),
            shot->generation, shot->caller, binding.pso, shot->pipeline, {}, 0};
        event.stack_size = CaptureStackBackTrace(1, static_cast<DWORD>(std::size(event.stack)), event.stack, nullptr);
        if (!rsf_rdr2_capture_record_draw(g->capture, &event)) {
            lock.unlock();
            log("snapshot failed");
        }
    }
}
catch (...) { log("snapshot failed"); }
void STDMETHODCALLTYPE draw(ID3D12GraphicsCommandList* list, UINT vertices, UINT instances, UINT first, UINT instance)
{
    DrawScope scope;
    original<Draw>(RSF_RDR2_CAPTURE_DRAW)(list, vertices, instances, first, instance);
    if (scope.outer) snapshot(list, vertices, instances, _ReturnAddress());
}
void STDMETHODCALLTYPE draw_indexed(ID3D12GraphicsCommandList* list, UINT indices, UINT instances, UINT first, INT base, UINT instance)
{
    DrawScope scope;
    original<DrawIndexed>(RSF_RDR2_CAPTURE_DRAW_INDEXED)(list, indices, instances, first, base, instance);
    if (scope.outer) snapshot(list, indices, instances, _ReturnAddress());
}
HRESULT STDMETHODCALLTYPE create_list(ID3D12Device* d, UINT node, D3D12_COMMAND_LIST_TYPE type,
    ID3D12CommandAllocator* allocator, ID3D12PipelineState* pso, REFIID iid, void** out)
{
    const auto hr = original<CreateList>(RSF_RDR2_CAPTURE_CREATE_LIST)(d, node, type, allocator, pso, iid, out);
    if (SUCCEEDED(hr) && out && *out && type == D3D12_COMMAND_LIST_TYPE_DIRECT) {
        bind(RSF_RDR2_CAPTURE_RESET, *out, &reset);
        bind(RSF_RDR2_CAPTURE_DRAW, *out, &draw);
        bind(RSF_RDR2_CAPTURE_DRAW_INDEXED, *out, &draw_indexed);
        bind(RSF_RDR2_CAPTURE_SET_PSO, *out, &set_pso);
        bind(RSF_RDR2_CAPTURE_BARRIER, *out, &barriers);
        bind(RSF_RDR2_CAPTURE_SET_RTV, *out, &set_rtv);
        std::lock_guard<std::mutex> lock(g->mutex);
        rsf_rdr2_capture_reset_list(g->capture, *out, pso);
    }
    return hr;
}
void device_created(void* pointer) noexcept try
{
    auto* device = static_cast<ID3D12Device*>(pointer);
    bind(RSF_RDR2_CAPTURE_CREATE_PSO, device, &create_pso);
    bind(RSF_RDR2_CAPTURE_CREATE_LIST, device, &create_list);
    bind(RSF_RDR2_CAPTURE_CREATE_RTV, device, &create_rtv);
    bind(RSF_RDR2_CAPTURE_CREATE_COMMITTED, device, &create_committed);
    bind(RSF_RDR2_CAPTURE_CREATE_PLACED, device, &create_placed);
    ComPtr<ID3D12Device1> device1;
    if (SUCCEEDED(device->QueryInterface(IID_PPV_ARGS(&device1))))
        bind(RSF_RDR2_CAPTURE_CREATE_LIBRARY, device1.Get(), &create_library);
    ComPtr<ID3D12Device2> device2;
    if (SUCCEEDED(device->QueryInterface(IID_PPV_ARGS(&device2))))
        bind(RSF_RDR2_CAPTURE_CREATE_STREAM_PSO, device2.Get(), &create_stream_pso);
}
catch (...) { log("device observation failed"); }
void* identity(void* pointer)
{
    ComPtr<IUnknown> object;
    return pointer && SUCCEEDED(static_cast<IUnknown*>(pointer)->QueryInterface(IID_PPV_ARGS(&object))) ? object.Get() : pointer;
}
void color_space_set(void* pointer, uint32_t space) noexcept try
{
    auto* key = identity(pointer);
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_record_color_space(g->capture, key, space);
}
catch (...) { log("color space observation failed"); }
HRESULT STDMETHODCALLTYPE set_hdr(IDXGISwapChain4* swapchain, DXGI_HDR_METADATA_TYPE type, UINT size, void* metadata)
{
    const auto result = original<SetHdr>(RSF_RDR2_CAPTURE_SET_HDR)(swapchain, type, size, metadata);
    if (SUCCEEDED(result)) {
        auto* key = identity(swapchain);
        std::lock_guard<std::mutex> lock(g->mutex);
        rsf_rdr2_capture_record_hdr(g->capture, key, static_cast<uint32_t>(type),
            static_cast<const uint8_t*>(metadata), size);
    }
    return result;
}
void swapchain_created(void* pointer) noexcept try
{
    auto* swapchain = static_cast<IDXGISwapChain*>(pointer);
    ComPtr<IDXGISwapChain4> sc4;
    if (SUCCEEDED(swapchain->QueryInterface(IID_PPV_ARGS(&sc4))))
        bind(RSF_RDR2_CAPTURE_SET_HDR, sc4.Get(), &set_hdr);
    DXGI_SWAP_CHAIN_DESC desc{};
    if (SUCCEEDED(swapchain->GetDesc(&desc))) {
        auto* key = identity(swapchain);
        std::lock_guard<std::mutex> lock(g->mutex);
        rsf_rdr2_capture_reset_presentation(g->capture, key);
    }
}
catch (...) { log("swapchain observation failed"); }
void before_submit(void* pointer, uint32_t count, void* const* lists) noexcept try
{
    std::lock_guard<std::mutex> lock(g->mutex);
    auto& submission = submissions.emplace_back();
    submission.queue = static_cast<ID3D12CommandQueue*>(pointer);
    submission.facts.reset(rsf_rdr2_capture_before_submit(g->capture, count, lists));
    if (submission.facts) {
        submission.shots.reserve(g->shots.size());
        for (const auto& shot : g->shots) {
            if (rsf_rdr2_capture_claim_readback(submission.facts.get(), shot.get()))
                submission.shots.push_back(shot);
        }
    }
}
catch (...) { log("submission snapshot failed"); }
void after_submit(void* pointer, uint32_t, void* const*) noexcept try
{
    if (submissions.empty() || submissions.back().queue != pointer) {
        log("submission pairing failed"); return;
    }
    auto submitted = std::move(submissions.back()); submissions.pop_back();
    auto* queue = submitted.queue;
    std::lock_guard<std::mutex> lock(g->mutex);
    const auto serial = rsf_rdr2_capture_after_submit(g->capture, submitted.facts.get());
    for (const auto& shot : submitted.shots) {
        rsf_rdr2_capture_submit_readback(shot.get(), queue, serial);
        rsf_rdr2_capture_gpu_bind_queue(&shot->gpu, queue);
    }
    const auto awaits_fence = [queue](const std::shared_ptr<Shot>& shot) {
        return rsf_rdr2_capture_readback_awaits_fence(shot.get(), queue);
    };
    if (std::none_of(g->shots.begin(), g->shots.end(), awaits_fence)) return;
    rsf_rdr2_capture_gpu_proof proof{};
    switch (rsf_rdr2_capture_gpu_signal(g->capture, queue, &gpu_ops, &proof)) {
    case RSF_RDR2_GPU_SIGNAL_ACCEPTED: break;
    case RSF_RDR2_GPU_CREATE_REFUSED: log("cannot create readback fence; retaining submitted readbacks"); return;
    case RSF_RDR2_GPU_SIGNAL_REFUSED: log("readback fence Signal failed"); return;
    default: log("readback fence owner unavailable; retaining submitted readbacks"); return;
    }
    for (const auto& shot : g->shots)
        if (rsf_rdr2_capture_fence_readback(shot.get(), queue, proof.value))
            rsf_rdr2_capture_gpu_bind_fence(&shot->gpu, proof.fence);
}
catch (...) { log("submission observation failed"); }
void before_present(void* pointer) noexcept try
{
    void* game_swapchain{};
    read_local(g->base + 0x571c7e8, &game_swapchain, sizeof(game_swapchain));
#ifdef RSF_RDR2_CAPTURE_TEST
    game_swapchain = g->fixture_swapchain;
#endif
    if (pointer != game_swapchain) return;
    auto* swapchain = static_cast<IDXGISwapChain*>(pointer);
    std::lock_guard<std::mutex> lock(g->mutex);
    auto* selected = rsf_rdr2_capture_gpu_selected(g->capture, swapchain);
    if (rsf_rdr2_capture_active(g->capture) && selected) {
        ComPtr<IDXGISwapChain3> sc3; ComPtr<ID3D12Resource> current;
        if (SUCCEEDED(swapchain->QueryInterface(IID_PPV_ARGS(&sc3))) &&
            SUCCEEDED(sc3->GetBuffer(sc3->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&current))) &&
            current.Get() == selected) {
            rsf_rdr2_capture_end_draws(g->capture);
            const auto progress = capture_progress();
            log("capture %u recording complete: %u draws, %zu readbacks resource=%p", progress.sequence, progress.draws, g->shots.size(), selected);
        }
    }
    if (rsf_rdr2_capture_can_arm(g->capture, g->shots.empty())) {
        DXGI_SWAP_CHAIN_DESC desc{};
        if (FAILED(swapchain->GetDesc(&desc))) return;
        auto* key = identity(swapchain);
        if (!rsf_rdr2_capture_gpu_arm_backbuffers(g->capture, swapchain, desc.BufferCount, &gpu_ops)) return;
        const rsf_rdr2_capture_presentation_desc descriptor{
            static_cast<uint32_t>(desc.BufferDesc.Format), desc.BufferDesc.Width, desc.BufferDesc.Height,
        };
        rsf_rdr2_capture_arm(g->capture, GetTickCount64(), key, &descriptor);
        g->tail.reset();
        rsf_rdr2_request_camera_capture();
        log("capture %u armed", capture_progress().sequence);
    }
}
catch (...) { log("present observation failed"); }
bool write_shot(const Shot& s) noexcept
{
    const rsf_rdr2_capture_image image{
        reinterpret_cast<uint64_t>(s.list), s.generation, s.submission, reinterpret_cast<uint64_t>(s.queue), s.fence_value,
        s.tick, s.resource, s.caller, s.sequence, s.ordinal, s.width, s.height, s.pitch, s.format,
        s.input_index, s.footprint_format, s.state_before, s.subresource, s.frame,
        s.vertices, s.instances, s.pipeline,
    };
    void* data{};
    const SIZE_T bytes = static_cast<SIZE_T>(s.pitch) * s.height;
    const D3D12_RANGE range{0, bytes};
    if (!s.buffer()) return false;
    const auto mapped = s.buffer()->Map(0, &range, &data);
    if (FAILED(mapped)) {
        log("readback Map failed hr=%08lx role=%s", static_cast<unsigned long>(mapped), s.role.c_str());
        return false;
    }
    const bool ok = rsf_rdr2_capture_write_image(&image,
        reinterpret_cast<const uint16_t*>(g->paths.directory), wcslen(g->paths.directory), GetCurrentProcessId(),
        s.role.c_str(), static_cast<const uint8_t*>(data), bytes);
    const D3D12_RANGE empty{0, 0};
    s.buffer()->Unmap(0, &empty);
    return ok;
}
DWORD WINAPI poll(void*) noexcept
{
    for (;;) {
        Sleep(100);
#ifdef RSF_RDR2_CAPTURE_TEST
        if (rsf_rdr2_capture_fixture_stopping()) return 0;
#endif
        rsf_rdr2_capture_poll_request(g->capture, reinterpret_cast<const uint16_t*>(g->paths.request));
        std::vector<std::shared_ptr<Shot>> ready;
        rsf_rdr2_capture_gpu_retired* retired{};
        {
            std::lock_guard<std::mutex> lock(g->mutex);
            if (rsf_rdr2_capture_timeout(g->capture, GetTickCount64())) {
                log("capture %u timed out waiting for scene or DLSS input; retaining explicit incomplete result",capture_progress().sequence);
            }
            if (!rsf_rdr2_capture_active(g->capture)) {
                auto it = g->shots.begin();
                while (it != g->shots.end()) {
                    const auto& shot = *it;
                    if (rsf_rdr2_capture_readback_complete(shot.get(), shot->completed())) {
                        ready.push_back(shot); it = g->shots.erase(it);
                    } else ++it;
                }
                // ResizeBuffers must not be blocked by diagnostic references after retirement.
                if (g->shots.empty()) retired = rsf_rdr2_capture_gpu_take_idle(g->capture);
            }
        }
        rsf_rdr2_capture_gpu_drop_idle(retired);
        for (const auto& shot : ready) {
            const bool ok = write_shot(*shot);
            std::lock_guard<std::mutex> lock(g->mutex);
            rsf_rdr2_capture_readback_written(g->capture, ok);
        }
        {
            std::lock_guard<std::mutex> lock(g->mutex);
            if (rsf_rdr2_capture_can_publish(g->capture, g->shots.empty())) {
                const rsf_rdr2_capture_bundle bundle{
                    reinterpret_cast<const uint16_t*>(g->paths.directory), wcslen(g->paths.directory), g->base, GetCurrentProcessId(),
                };
                size_t draw_count{}, temporal_count{};
                const bool ok = rsf_rdr2_capture_write_bundle(g->capture, &bundle,
                    [](double value, char* buffer, size_t size) noexcept {
                        return std::snprintf(buffer, size, "%.12g", value);
                    }, &draw_count, &temporal_count);
                const auto progress = capture_progress();
                log("capture %u bundle saved=%d input_frames=%zu draws=%zu readbacks=%u",progress.sequence,ok,temporal_count,draw_count,progress.written);
                rsf_rdr2_capture_published(g->capture);
                g->tail.reset();
            }
        }
    }
}
bool create_state(rsf_rdr2_capture_state* capture, const rsf_rdr2_capture_paths* paths) noexcept try
{
    auto state = std::make_unique<State>();
    state->capture = capture;
    state->base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    state->paths = *paths;
    // The worker can run as soon as CreateThread succeeds.
    g = state.release();
    return true;
}
catch (...) { return false; }

constexpr rsf_rdr2_capture_install_ops install_ops{
    sizeof(rsf_rdr2_capture_install_ops), 1,
    create_state,
    []() noexcept { delete g; g = nullptr; },
    []() noexcept { return CreateThread(nullptr, 0, poll, nullptr, 0, nullptr); },
    []() noexcept {
        // The census initializes MinHook before its device callback can run.
        rsf_census_observe_d3d12(device_created, before_submit, after_submit, before_present, swapchain_created, color_space_set);
    },
    []() noexcept { rsf_census_observe_ngx_d3d12(ngx_created, ngx_evaluate, ngx_released); },
    [](void* thread, uint32_t timeout_ms) noexcept { WaitForSingleObject(thread, timeout_ms); },
    [](void* thread) noexcept { CloseHandle(thread); },
    [](const char* message) noexcept { log("%s", message); },
};
}

bool rsf_rdr2_install_native_capture(const wchar_t* directory) noexcept
{
#ifdef RSF_RDR2_CAPTURE_TEST
    constexpr bool fixture = true;
#else
    constexpr bool fixture = false;
#endif
    return rsf_rdr2_capture_install(directory, &install_ops, fixture);
}

#ifdef RSF_RDR2_CAPTURE_TEST
// The standalone GPU fixture owns its swapchain; production only accepts the guarded game owner.
void rsf_rdr2_capture_test_arm(void* swapchain) noexcept
{
    std::lock_guard<std::mutex> lock(g->mutex);
    g->fixture_swapchain = swapchain; rsf_rdr2_capture_request(g->capture);
}
void rsf_rdr2_capture_test_stop() noexcept
{
    rsf_rdr2_capture_stop_fixture();
}
#endif
