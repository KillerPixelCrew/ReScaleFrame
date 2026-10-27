// SPDX-License-Identifier: GPL-3.0-only
// Bounded copies after actual swapchain draws, preserving the game's command list and state.
#include "native_capture.h"
#include "camera_probe.h"
#include <rescaleframe/census_d3d12_observer.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cstddef>
#include <memory>
#include <mutex>
#include <unordered_map>
#include <vector>
#include <string>
#include <cmath>

namespace {
using Microsoft::WRL::ComPtr;
struct Pipeline { uint32_t ps{}, vs{}; D3D12_RENDER_TARGET_BLEND_DESC blend{}; };
using ResourceStates = std::unordered_map<ID3D12Resource*, D3D12_RESOURCE_STATES>;
constexpr auto unknown_state = static_cast<D3D12_RESOURCE_STATES>(~0u);
struct List {
    ID3D12Resource* target{};
    ID3D12PipelineState* pso{};
    ResourceStates states;
    uint64_t generation{};
};
struct Shot {
    ComPtr<ID3D12Resource> readback;
    ComPtr<ID3D12Fence> fence;
    ID3D12GraphicsCommandList* list{};
    uint64_t fence_value{}, resource{}, tick{}, caller{}, generation{}, submission{}, queue{}, write_serial{};
    UINT width{}, height{}, pitch{}, format{}, vertices{}, instances{}, frame{}, sequence{}, ordinal{};
    Pipeline pipeline{};
    std::string role{"draw"};
    UINT footprint_format{}, subresource{}, state_before{}, input_index{};
};
struct Queue { ComPtr<ID3D12Fence> fence; uint64_t next{1}; };
struct Presentation {
    UINT format{}, width{}, height{}, color_space{}, metadata_type{};
    bool color_space_set{};
    std::vector<unsigned char> metadata;
};
struct Submission {
    ID3D12CommandQueue* queue{};
    uint64_t serial{};
    ResourceStates states;
    std::vector<std::pair<std::shared_ptr<Shot>, uint64_t>> shots;
};
struct Temporal {
    UINT sequence{}, index{}, frame{}, thread{}, feature_flags{};
    uint64_t tick{}, qpc{}, list{}, generation{}, view{}, draw_context{}, job{}, result{};
    bool camera_valid{};
    std::array<unsigned char, 0x800> camera{};
    std::vector<std::pair<std::string, double>> scalars;
    std::vector<std::string> refusals;
};
struct DrawEvent {
    UINT ordinal{}, frame{}, vertices{}, instances{};
    uint64_t list{}, generation{}, caller{}, pso{};
    Pipeline pipeline{};
    std::array<void*, 12> stack{};
    USHORT stack_size{};
};
thread_local std::vector<Submission> submissions;
thread_local bool recording_copy{};
thread_local bool drawing{};
thread_local std::shared_ptr<Temporal> evaluating;
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
    wchar_t directory[MAX_PATH]{}, request[MAX_PATH]{}, log[MAX_PATH]{};
    uintptr_t base{};
    std::atomic<bool> pending{};
    std::atomic<bool> active{};
    UINT sequence{}, ordinal{}, input_index{};
    std::atomic<UINT> inputs_remaining{};
    uint64_t submission_serial{}, write_serial{};
    ID3D12Resource* selected{};
    std::array<ComPtr<ID3D12Resource>, 4> buffers;
    std::unordered_map<SIZE_T, ID3D12Resource*> rtvs;
    std::unordered_map<ID3D12GraphicsCommandList*, List> lists;
    std::unordered_map<ID3D12PipelineState*, Pipeline> pipelines;
    std::unordered_map<ID3D12CommandQueue*, Queue> queues;
    std::vector<std::shared_ptr<Shot>> shots;
    std::shared_ptr<Shot> tail;
    ResourceStates submitted_states;
    std::unordered_map<void*, uint32_t> sr_features;
    std::vector<DrawEvent> draws;
    std::vector<std::shared_ptr<Temporal>> temporal;
    bool draw_overflow{};
    bool capture_dirty{}, capture_timeout{}, write_failed{};
    UINT inputs_in_flight{}, written{};
    uint64_t capture_started{};
    std::unordered_map<void*, Presentation> presentation;
    Presentation captured_presentation;
#ifdef RSF_RDR2_CAPTURE_TEST
    void* fixture_swapchain{};
    std::atomic<bool> fixture_stop{};
    HANDLE fixture_thread{};
#endif
};
State* g{}; // Pinned with the diagnostic ASI until process exit.
bool read_local(uint64_t address, void* out, size_t bytes);
std::mutex bind_mutex;
void log(const char* format, ...) noexcept
{
    FILE* f = _wfopen(g->log, L"ab");
    if (!f) return;
    fprintf(f, "[%llu] ", static_cast<unsigned long long>(GetTickCount64()));
    va_list args; va_start(args, format); vfprintf(f, format, args); va_end(args);
    fputc('\n', f); fclose(f);
}
uint32_t crc(const D3D12_SHADER_BYTECODE& code) noexcept
{
    uint32_t value = ~0u;
    const auto* bytes = static_cast<const unsigned char*>(code.pShaderBytecode);
    for (SIZE_T i = 0; i < code.BytecodeLength; ++i) {
        value ^= bytes[i];
        for (unsigned bit = 0; bit < 8; ++bit) value = (value >> 1) ^ (0xedb88320u & (0u - (value & 1)));
    }
    return ~value;
}
template<class F> void bind(void* object, size_t slot, void* replacement, F& original)
{
    std::lock_guard<std::mutex> lock(bind_mutex);
    if (original) return;
    void* address = (*static_cast<void***>(object))[slot];
    const auto made = MH_CreateHook(address, replacement, reinterpret_cast<void**>(&original));
    const auto enabled = made == MH_OK ? MH_EnableHook(address) : made;
    log("hook slot=%zu target=%p result=%s", slot, address, MH_StatusToString(enabled));
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
CreatePso original_create_pso{}; CreateList original_create_list{}; CreateRtv original_create_rtv{};
CreateLibrary original_create_library{}; LoadPso original_load_pso{};
CreateStreamPso original_create_stream_pso{}; LoadStreamPso original_load_stream_pso{};
Reset original_reset{}; SetPso original_set_pso{}; SetRtv original_set_rtv{};
Draw original_draw{}; DrawIndexed original_draw_indexed{};
Barrier original_barrier{};
CreateCommitted original_create_committed{}; CreatePlaced original_create_placed{};
SetHdr original_set_hdr{};

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
void remember_stream(const D3D12_PIPELINE_STATE_STREAM_DESC* desc, void* result)
{
    if (!desc || !result || !desc->pPipelineStateSubobjectStream) return;
    Pipeline pipeline{};
    const auto* data = static_cast<const unsigned char*>(desc->pPipelineStateSubobjectStream);
    for (size_t offset = 0; offset < desc->SizeInBytes;) {
        const auto remaining = desc->SizeInBytes - offset;
        if (remaining < sizeof(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE)) return;
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE type{};
        std::memcpy(&type, data + offset, sizeof(type));
        size_t size{};
        switch (type) {
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS: {
            D3D12_SHADER_BYTECODE shader{};
            if (!stream_value(data + offset, remaining, shader, size)) return;
            if (type == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS) pipeline.vs = crc(shader);
            else pipeline.ps = crc(shader);
            break;
        }
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND: {
            D3D12_BLEND_DESC blend{};
            if (!stream_value(data + offset, remaining, blend, size)) return;
            pipeline.blend = blend.RenderTarget[0];
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
        default: return; // Unknown stream layouts remain explicitly unidentified.
        }
        if (size > remaining) return;
        offset += size;
    }
    std::lock_guard<std::mutex> lock(g->mutex);
    g->pipelines[static_cast<ID3D12PipelineState*>(result)] = pipeline;
}
HRESULT STDMETHODCALLTYPE create_stream_pso(ID3D12Device2* device,
    const D3D12_PIPELINE_STATE_STREAM_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original_create_stream_pso(device, desc, iid, out);
    if (SUCCEEDED(hr) && out) remember_stream(desc, *out);
    return hr;
}
HRESULT STDMETHODCALLTYPE load_stream_pso(ID3D12PipelineLibrary1* library, LPCWSTR name,
    const D3D12_PIPELINE_STATE_STREAM_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original_load_stream_pso(library, name, desc, iid, out);
    if (SUCCEEDED(hr) && out) remember_stream(desc, *out);
    return hr;
}

void remember_pso(const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc, void* result)
{
    if (!desc || !result) return;
    const Pipeline pipeline{crc(desc->PS), crc(desc->VS), desc->BlendState.RenderTarget[0]};
    std::lock_guard<std::mutex> lock(g->mutex);
    g->pipelines[static_cast<ID3D12PipelineState*>(result)] = pipeline;
}
HRESULT STDMETHODCALLTYPE create_pso(ID3D12Device* d, const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original_create_pso(d, desc, iid, out);
    if (SUCCEEDED(hr) && out) remember_pso(desc, *out);
    return hr;
}
HRESULT STDMETHODCALLTYPE load_pso(ID3D12PipelineLibrary* library, LPCWSTR name,
    const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original_load_pso(library, name, desc, iid, out);
    if (SUCCEEDED(hr) && out) remember_pso(desc, *out);
    return hr;
}
HRESULT STDMETHODCALLTYPE create_library(ID3D12Device1* d, const void* blob, SIZE_T size, REFIID iid, void** out)
{
    const auto hr = original_create_library(d, blob, size, iid, out);
    if (SUCCEEDED(hr) && out && *out) {
        bind(*out, 9, reinterpret_cast<void*>(&load_pso), original_load_pso);
        ComPtr<ID3D12PipelineLibrary1> library1;
        if (SUCCEEDED(static_cast<IUnknown*>(*out)->QueryInterface(IID_PPV_ARGS(&library1))))
            bind(library1.Get(), 13, reinterpret_cast<void*>(&load_stream_pso), original_load_stream_pso);
    }
    return hr;
}
void STDMETHODCALLTYPE create_rtv(ID3D12Device* d, ID3D12Resource* resource,
    const D3D12_RENDER_TARGET_VIEW_DESC* desc, D3D12_CPU_DESCRIPTOR_HANDLE handle)
{
    original_create_rtv(d, resource, desc, handle);
    std::lock_guard<std::mutex> lock(g->mutex);
    g->rtvs[handle.ptr] = resource;
}
HRESULT STDMETHODCALLTYPE create_committed(ID3D12Device* d, const D3D12_HEAP_PROPERTIES* heap,
    D3D12_HEAP_FLAGS flags, const D3D12_RESOURCE_DESC* desc, D3D12_RESOURCE_STATES state,
    const D3D12_CLEAR_VALUE* clear, REFIID iid, void** out)
{
    const auto hr = original_create_committed(d, heap, flags, desc, state, clear, iid, out);
    if (!recording_copy && SUCCEEDED(hr) && out && *out) {
        std::lock_guard<std::mutex> lock(g->mutex);
        g->submitted_states[static_cast<ID3D12Resource*>(*out)] = state;
    }
    return hr;
}
HRESULT STDMETHODCALLTYPE create_placed(ID3D12Device* d, ID3D12Heap* heap, UINT64 offset,
    const D3D12_RESOURCE_DESC* desc, D3D12_RESOURCE_STATES state, const D3D12_CLEAR_VALUE* clear,
    REFIID iid, void** out)
{
    const auto hr = original_create_placed(d, heap, offset, desc, state, clear, iid, out);
    if (!recording_copy && SUCCEEDED(hr) && out && *out) {
        std::lock_guard<std::mutex> lock(g->mutex);
        g->submitted_states[static_cast<ID3D12Resource*>(*out)] = state;
    }
    return hr;
}
void STDMETHODCALLTYPE barriers(ID3D12GraphicsCommandList* list, UINT count, const D3D12_RESOURCE_BARRIER* items)
{
    original_barrier(list, count, items);
    if (recording_copy) return;
    std::lock_guard<std::mutex> lock(g->mutex);
    auto& states = g->lists[list].states;
    for (UINT i = 0; i < count; ++i) {
        const auto& b = items[i];
        if (b.Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION) {
            const auto& t = b.Transition;
            // Inputs below copy subresource zero. A transition of another plane/mip does not
            // establish its state. Split begins are unusable until their matching end.
            if (t.Subresource == 0 || t.Subresource == D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES)
                states[t.pResource] = b.Flags == D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY ? unknown_state : t.StateAfter;
        } else if (b.Type == D3D12_RESOURCE_BARRIER_TYPE_ALIASING) {
            if (b.Aliasing.pResourceAfter) states[b.Aliasing.pResourceAfter] = unknown_state;
        }
    }
}
HRESULT STDMETHODCALLTYPE reset(ID3D12GraphicsCommandList* list, ID3D12CommandAllocator* allocator, ID3D12PipelineState* pso)
{
    const auto hr = original_reset(list, allocator, pso);
    if (SUCCEEDED(hr)) {
        std::lock_guard<std::mutex> lock(g->mutex);
        auto& state = g->lists[list];
        ++state.generation; state.target = nullptr; state.pso = pso; state.states.clear();
    }
    return hr;
}
void STDMETHODCALLTYPE set_pso(ID3D12GraphicsCommandList* list, ID3D12PipelineState* pso)
{
    original_set_pso(list, pso);
    std::lock_guard<std::mutex> lock(g->mutex); g->lists[list].pso = pso;
}
void STDMETHODCALLTYPE set_rtv(ID3D12GraphicsCommandList* list, UINT count,
    const D3D12_CPU_DESCRIPTOR_HANDLE* handles, BOOL contiguous, const D3D12_CPU_DESCRIPTOR_HANDLE* depth)
{
    original_set_rtv(list, count, handles, contiguous, depth);
    std::lock_guard<std::mutex> lock(g->mutex);
    auto found = count && handles ? g->rtvs.find(handles[0].ptr) : g->rtvs.end();
    g->lists[list].target = found != g->rtvs.end() ? found->second : nullptr;
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
    if (!shot->readback) {
        const auto hr = device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &destination,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&shot->readback));
        if (FAILED(hr)) { log("readback allocation failed hr=%08lx", static_cast<unsigned long>(hr)); return false; }
    }
    shot->fence.Reset(); shot->fence_value = 0;
    // Preserve the observed input state; copying does not alter the producer/consumer contract.
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition = {target, 0, before, D3D12_RESOURCE_STATE_COPY_SOURCE};
    if (before != D3D12_RESOURCE_STATE_COPY_SOURCE) list->ResourceBarrier(1, &barrier);
    D3D12_TEXTURE_COPY_LOCATION source{}; source.pResource = target;
    source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION dest{}; dest.pResource = shot->readback.Get();
    dest.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; dest.PlacedFootprint = footprint;
    list->CopyTextureRegion(&dest, 0, 0, 0, &source, nullptr);
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barrier.Transition.StateAfter = before;
    if (before != D3D12_RESOURCE_STATE_COPY_SOURCE) list->ResourceBarrier(1, &barrier);
    shot->list = list; shot->width = static_cast<UINT>(desc.Width); shot->height = desc.Height;
    shot->format = desc.Format; shot->pitch = footprint.Footprint.RowPitch;
    shot->footprint_format = footprint.Footprint.Format; shot->state_before = before;
    shot->generation = g->lists[list].generation;
    shot->write_serial = ++g->write_serial;
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
void input_camera(Temporal& t)
{
    uint32_t index{}; uint64_t block{};
    read_local(g->base + 0x542904c, &t.frame, sizeof(t.frame));
    LARGE_INTEGER qpc{}; QueryPerformanceCounter(&qpc); t.qpc = qpc.QuadPart;
    t.tick = GetTickCount64(); t.thread = GetCurrentThreadId();
    if (read_local(g->base + 0x5b403e0, &index, sizeof(index)) && index < 4096 &&
        read_local(__readgsqword(0x58) + index * 8, &block, sizeof(block))) {
        read_local(block + 0x14d8, &t.view, sizeof(t.view));
        read_local(block + 0x14f0, &t.draw_context, sizeof(t.draw_context));
        read_local(block + 0x1ce8, &t.job, sizeof(t.job));
        t.camera_valid = read_local(t.view, t.camera.data(), t.camera.size());
    }
    if (!t.camera_valid) t.refusals.emplace_back("camera_unreadable");
}
void input_resource(ID3D12GraphicsCommandList* list, const void* parameters,
    const char* key, const char* role, Temporal& t, bool required)
{
    ID3D12Resource* resource{};
    if ((!ngx_get(parameters, 9, key, resource) && !ngx_get(parameters, 8, key, resource)) || !resource) {
        if (required) t.refusals.emplace_back(std::string(role) + "_missing");
        return;
    }
    auto before = unknown_state;
    const auto& local = g->lists[list].states;
    auto state = local.find(resource);
    if (state != local.end()) before = state->second;
    else {
        const auto submitted = g->submitted_states.find(resource);
        if (submitted != g->submitted_states.end()) before = submitted->second;
    }
    auto shot = std::make_shared<Shot>();
    shot->role = role; shot->sequence = t.sequence; shot->input_index = t.index;
    if (!copy_resource(list, resource, before, shot)) {
        t.refusals.emplace_back(std::string(role) + "_state_or_copy_unavailable");
        return;
    }
    g->shots.push_back(std::move(shot));
}
void ngx_created(void* handle, const void* parameters, uint32_t feature) noexcept try
{
    if (feature != 1) return;
    int32_t flags{};
    if (!ngx_get(parameters, 11, "DLSS.Feature.Create.Flags", flags)) {
        uint32_t alternate{};
        if (!ngx_get(parameters, 12, "DLSS.Feature.Create.Flags", alternate)) return;
        flags = static_cast<int32_t>(alternate);
    }
    std::lock_guard<std::mutex> lock(g->mutex);
    g->sr_features[handle] = static_cast<uint32_t>(flags);
}
catch (...) { log("NGX feature observation failed"); }
void ngx_released(void* handle) noexcept
{
    std::lock_guard<std::mutex> lock(g->mutex); g->sr_features.erase(handle);
}
void ngx_evaluate(void* pointer, void* handle, const void* parameters, uint32_t phase, uint64_t result) noexcept try
{
    if (!pointer || (!phase && !g->inputs_remaining.load(std::memory_order_relaxed))) return;
    auto* list = static_cast<ID3D12GraphicsCommandList*>(pointer);
    std::lock_guard<std::mutex> lock(g->mutex);
    if (phase) {
        if (!evaluating || evaluating->list != reinterpret_cast<uint64_t>(list)) return;
        evaluating->result = result;
        if (static_cast<uint32_t>(result) == 1) input_resource(list, parameters, "Output", "output", *evaluating, true);
        else evaluating->refusals.emplace_back("native_evaluation_failed");
        evaluating.reset(); --g->inputs_in_flight;
        return;
    }
    if (evaluating || !g->inputs_remaining) return;
    const auto feature = g->sr_features.find(handle);
    if (feature == g->sr_features.end()) return;
    auto t = std::make_shared<Temporal>();
    t->sequence = g->sequence; t->index = g->input_index++; t->feature_flags = feature->second;
    t->list = reinterpret_cast<uint64_t>(list); t->generation = g->lists[list].generation;
    input_camera(*t);
    static const char* const floats[]{"Jitter.Offset.X", "Jitter.Offset.Y", "MV.Scale.X", "MV.Scale.Y",
        "DLSS.Pre.Exposure", "DLSS.Exposure.Scale", "FrameTimeDeltaInMsec"};
    for (const auto* key : floats) {
        float value{};
        if (ngx_get(parameters, 14, key, value) && std::isfinite(value)) t->scalars.emplace_back(key, value);
        else t->refusals.emplace_back(std::string("scalar_unavailable:") + key);
    }
    static const char* const integers[]{"Reset", "DLSS.Render.Subrect.Dimensions.Width", "DLSS.Render.Subrect.Dimensions.Height",
        "DLSS.Input.Color.Subrect.Base.X", "DLSS.Input.Color.Subrect.Base.Y", "DLSS.Input.Depth.Subrect.Base.X",
        "DLSS.Input.Depth.Subrect.Base.Y", "DLSS.Input.MV.Subrect.Base.X", "DLSS.Input.MV.Subrect.Base.Y",
        "DLSS.Output.Subrect.Base.X", "DLSS.Output.Subrect.Base.Y"};
    for (const auto* key : integers) {
        uint32_t value{}; int32_t alternate{};
        if (ngx_get(parameters, 12, key, value)) t->scalars.emplace_back(key, value);
        else if (ngx_get(parameters, 11, key, alternate)) t->scalars.emplace_back(key, alternate);
        else t->refusals.emplace_back(std::string("scalar_unavailable:") + key);
    }
    input_resource(list, parameters, "Color", "color", *t, true);
    input_resource(list, parameters, "Depth", "depth", *t, true);
    input_resource(list, parameters, "MotionVectors", "motion", *t, true);
    input_resource(list, parameters, "ExposureTexture", "exposure", *t, false);
    g->temporal.push_back(t); evaluating = std::move(t); --g->inputs_remaining; ++g->inputs_in_flight;
}
catch (...) { log("NGX input observation failed"); }
void snapshot(ID3D12GraphicsCommandList* list, UINT vertices, UINT instances, void* caller) noexcept try
{
    if (!g->active.load(std::memory_order_relaxed)) return;
    std::lock_guard<std::mutex> lock(g->mutex);
    if (!g->active) return;
    const auto state = g->lists.find(list);
    if (state == g->lists.end() || !state->second.target) return;
    auto* target = state->second.target;
    bool backbuffer = false;
    for (const auto& buffer : g->buffers) if (buffer.Get() == target) backbuffer = true;
    if (!backbuffer || (g->selected && g->selected != target)) return;
    const bool reuse_tail = g->ordinal >= 64;
    auto shot = reuse_tail ? g->tail : std::make_shared<Shot>();
    if (!shot || !copy_resource(list, target, D3D12_RESOURCE_STATE_RENDER_TARGET, shot)) return;
    shot->sequence = g->sequence; shot->ordinal = g->ordinal++;
    shot->vertices = vertices; shot->instances = instances;
    shot->caller = reinterpret_cast<uint64_t>(caller);
    const auto pipeline = g->pipelines.find(state->second.pso);
    shot->pipeline = {};
    if (pipeline != g->pipelines.end()) shot->pipeline = pipeline->second;
    g->selected = target;
    if (g->draws.size() < 4096) {
        DrawEvent event{shot->ordinal, shot->frame, vertices, instances, reinterpret_cast<uint64_t>(list),
            shot->generation, shot->caller, reinterpret_cast<uint64_t>(state->second.pso), shot->pipeline};
        event.stack_size = CaptureStackBackTrace(1, static_cast<DWORD>(event.stack.size()), event.stack.data(), nullptr);
        g->draws.push_back(event);
    }
    else g->draw_overflow = true;
    if (shot->ordinal == 63) g->tail = shot;
    if (!reuse_tail) g->shots.push_back(std::move(shot));
}
catch (...) { log("snapshot failed"); }
void STDMETHODCALLTYPE draw(ID3D12GraphicsCommandList* list, UINT vertices, UINT instances, UINT first, UINT instance)
{
    DrawScope scope;
    original_draw(list, vertices, instances, first, instance);
    if (scope.outer) snapshot(list, vertices, instances, _ReturnAddress());
}
void STDMETHODCALLTYPE draw_indexed(ID3D12GraphicsCommandList* list, UINT indices, UINT instances, UINT first, INT base, UINT instance)
{
    DrawScope scope;
    original_draw_indexed(list, indices, instances, first, base, instance);
    if (scope.outer) snapshot(list, indices, instances, _ReturnAddress());
}
HRESULT STDMETHODCALLTYPE create_list(ID3D12Device* d, UINT node, D3D12_COMMAND_LIST_TYPE type,
    ID3D12CommandAllocator* allocator, ID3D12PipelineState* pso, REFIID iid, void** out)
{
    const auto hr = original_create_list(d, node, type, allocator, pso, iid, out);
    if (SUCCEEDED(hr) && out && *out && type == D3D12_COMMAND_LIST_TYPE_DIRECT) {
        bind(*out, 10, reinterpret_cast<void*>(&reset), original_reset);
        bind(*out, 12, reinterpret_cast<void*>(&draw), original_draw);
        bind(*out, 13, reinterpret_cast<void*>(&draw_indexed), original_draw_indexed);
        bind(*out, 25, reinterpret_cast<void*>(&set_pso), original_set_pso);
        bind(*out, 26, reinterpret_cast<void*>(&barriers), original_barrier);
        bind(*out, 46, reinterpret_cast<void*>(&set_rtv), original_set_rtv);
        std::lock_guard<std::mutex> lock(g->mutex);
        auto& state = g->lists[static_cast<ID3D12GraphicsCommandList*>(*out)];
        state.target = nullptr; state.pso = pso; ++state.generation; state.states.clear();
    }
    return hr;
}
void device_created(void* pointer) noexcept try
{
    auto* device = static_cast<ID3D12Device*>(pointer);
    bind(device, 10, reinterpret_cast<void*>(&create_pso), original_create_pso);
    bind(device, 12, reinterpret_cast<void*>(&create_list), original_create_list);
    bind(device, 20, reinterpret_cast<void*>(&create_rtv), original_create_rtv);
    bind(device, 27, reinterpret_cast<void*>(&create_committed), original_create_committed);
    bind(device, 29, reinterpret_cast<void*>(&create_placed), original_create_placed);
    ComPtr<ID3D12Device1> device1;
    if (SUCCEEDED(device->QueryInterface(IID_PPV_ARGS(&device1))))
        bind(device1.Get(), 44, reinterpret_cast<void*>(&create_library), original_create_library);
    ComPtr<ID3D12Device2> device2;
    if (SUCCEEDED(device->QueryInterface(IID_PPV_ARGS(&device2))))
        bind(device2.Get(), 47, reinterpret_cast<void*>(&create_stream_pso), original_create_stream_pso);
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
    auto& state = g->presentation[key]; state.color_space = space; state.color_space_set = true;
}
catch (...) { log("color space observation failed"); }
HRESULT STDMETHODCALLTYPE set_hdr(IDXGISwapChain4* swapchain, DXGI_HDR_METADATA_TYPE type, UINT size, void* metadata)
{
    const auto result = original_set_hdr(swapchain, type, size, metadata);
    if (SUCCEEDED(result)) {
        auto* key = identity(swapchain);
        std::lock_guard<std::mutex> lock(g->mutex);
        auto& state = g->presentation[key]; state.metadata_type = type; state.metadata.clear();
        if (metadata && size <= 1024) {
            const auto* bytes = static_cast<const unsigned char*>(metadata);
            state.metadata.assign(bytes, bytes + size);
        }
    }
    return result;
}
void swapchain_created(void* pointer) noexcept try
{
    auto* swapchain = static_cast<IDXGISwapChain*>(pointer);
    ComPtr<IDXGISwapChain4> sc4;
    if (SUCCEEDED(swapchain->QueryInterface(IID_PPV_ARGS(&sc4))))
        bind(sc4.Get(), 40, reinterpret_cast<void*>(&set_hdr), original_set_hdr);
    DXGI_SWAP_CHAIN_DESC desc{};
    if (SUCCEEDED(swapchain->GetDesc(&desc))) {
        auto* key = identity(swapchain);
        std::lock_guard<std::mutex> lock(g->mutex);
        auto& state = g->presentation[key]; state = {};
        state.format = desc.BufferDesc.Format; state.width = desc.BufferDesc.Width; state.height = desc.BufferDesc.Height;
    }
}
catch (...) { log("swapchain observation failed"); }
void before_submit(void* pointer, uint32_t count, void* const* lists) noexcept try
{
    std::lock_guard<std::mutex> lock(g->mutex);
    Submission submission;
    submission.queue = static_cast<ID3D12CommandQueue*>(pointer);
    submission.serial = ++g->submission_serial;
    for (uint32_t i = 0; i < count; ++i) {
        const auto state = g->lists.find(static_cast<ID3D12GraphicsCommandList*>(lists[i]));
        if (state == g->lists.end()) continue;
        for (const auto& entry : state->second.states) submission.states[entry.first] = entry.second;
        for (const auto& shot : g->shots)
            if (!shot->fence && shot->list == lists[i] && shot->generation == state->second.generation)
                submission.shots.emplace_back(shot, shot->write_serial);
    }
    submissions.push_back(std::move(submission));
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
    for (const auto& entry : submitted.states) g->submitted_states[entry.first] = entry.second;
    if (submitted.shots.empty()) return;
    auto& state = g->queues[queue];
    if (!state.fence) {
        ComPtr<ID3D12Device> device;
        if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device))) ||
            FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&state.fence)))) {
            log("cannot fence submitted readbacks; retained until exit"); return;
        }
    }
    const auto value = state.next++;
    if (FAILED(queue->Signal(state.fence.Get(), value))) { log("readback fence Signal failed"); return; }
    for (auto& entry : submitted.shots) {
        auto& shot = entry.first;
        if (shot->write_serial != entry.second) continue;
        shot->fence = state.fence; shot->fence_value = value;
        shot->submission = submitted.serial; shot->queue = reinterpret_cast<uint64_t>(queue);
    }
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
    if (g->active && g->selected) {
        ComPtr<IDXGISwapChain3> sc3; ComPtr<ID3D12Resource> current;
        if (SUCCEEDED(swapchain->QueryInterface(IID_PPV_ARGS(&sc3))) &&
            SUCCEEDED(sc3->GetBuffer(sc3->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&current))) &&
            current.Get() == g->selected) {
            g->active = false;
            log("capture %u recording complete: %u draws, %zu readbacks resource=%p", g->sequence, g->ordinal, g->shots.size(), g->selected);
        }
    }
    if (g->pending.load() && !g->active && g->shots.empty() && !g->inputs_remaining &&
        !g->inputs_in_flight && !g->capture_dirty) {
        DXGI_SWAP_CHAIN_DESC desc{};
        if (FAILED(swapchain->GetDesc(&desc)) || desc.BufferCount > g->buffers.size()) return;
        for (auto& buffer : g->buffers) buffer.Reset();
        for (UINT i = 0; i < desc.BufferCount; ++i)
            if (FAILED(swapchain->GetBuffer(i, IID_PPV_ARGS(&g->buffers[i])))) return;
        ++g->sequence; g->ordinal = 0; g->selected = nullptr; g->active = true; g->pending = false;
        g->tail.reset(); g->draws.clear(); g->draw_overflow = false; g->temporal.clear();
        g->input_index = 0; g->inputs_remaining = 4;
        g->capture_dirty = true; g->capture_timeout = false; g->write_failed = false; g->written = 0;
        g->capture_started = GetTickCount64();
        g->captured_presentation = g->presentation[identity(swapchain)];
        g->captured_presentation.format = desc.BufferDesc.Format;
        g->captured_presentation.width = desc.BufferDesc.Width; g->captured_presentation.height = desc.BufferDesc.Height;
        rsf_rdr2_request_camera_capture();
        log("capture %u armed", g->sequence);
    }
}
catch (...) { log("present observation failed"); }
bool write_shot(const Shot& s) noexcept
{
    void* data{}; const SIZE_T bytes = static_cast<SIZE_T>(s.pitch) * s.height;
    D3D12_RANGE range{0, bytes};
    const auto mapped = s.readback->Map(0, &range, &data);
    if (FAILED(mapped)) { log("readback Map failed hr=%08lx role=%s",static_cast<unsigned long>(mapped),s.role.c_str()); return false; }
    wchar_t path[MAX_PATH]{};
    wchar_t stem[MAX_PATH]{};
    if (s.role == "draw") swprintf_s(stem, L"%s\\native-%lu-%03u-%03u", g->directory, GetCurrentProcessId(), s.sequence, s.ordinal);
    else swprintf_s(stem, L"%s\\input-%lu-%03u-%03u-%hs", g->directory, GetCurrentProcessId(), s.sequence, s.input_index, s.role.c_str());
    swprintf_s(path, L"%s.bin", stem);
    FILE* f = _wfopen(path, L"wb");
    bool ok = false;
    if (f) { ok = fwrite(data, 1, bytes, f) == bytes; ok = fclose(f) == 0 && ok; }
    D3D12_RANGE empty{0,0}; s.readback->Unmap(0, &empty);
    swprintf_s(path, L"%s.json", stem);
    f = _wfopen(path, L"wb");
    if (f) {
        fprintf(f, "{\"sequence\":%u,\"draw\":%u,\"width\":%u,\"height\":%u,\"row_pitch\":%u,\"format\":%u,"
            "\"role\":\"%s\",\"input_index\":%u,\"footprint_format\":%u,\"state_before\":%u,\"subresource\":%u,"
            "\"command_list\":\"0x%llx\",\"generation\":%llu,\"submission\":%llu,\"queue\":\"0x%llx\",\"fence_value\":%llu,"
            "\"frame_counter_observation\":%u,\"tick\":%llu,\"resource\":\"0x%llx\",\"caller\":\"0x%llx\","
            "\"vertices\":%u,\"instances\":%u,\"ps_crc32\":\"%08x\",\"vs_crc32\":\"%08x\","
            "\"blend\":%d,\"src\":%u,\"dst\":%u,\"op\":%u,\"src_alpha\":%u,\"dst_alpha\":%u,\"op_alpha\":%u,\"write_mask\":%u}\n",
            s.sequence,s.ordinal,s.width,s.height,s.pitch,s.format,
            s.role.c_str(),s.input_index,s.footprint_format,s.state_before,s.subresource,
            static_cast<unsigned long long>(reinterpret_cast<uint64_t>(s.list)),static_cast<unsigned long long>(s.generation),
            static_cast<unsigned long long>(s.submission),static_cast<unsigned long long>(s.queue),static_cast<unsigned long long>(s.fence_value),
            s.frame,static_cast<unsigned long long>(s.tick),static_cast<unsigned long long>(s.resource),
            static_cast<unsigned long long>(s.caller),s.vertices,s.instances,s.pipeline.ps,s.pipeline.vs,
            s.pipeline.blend.BlendEnable,s.pipeline.blend.SrcBlend,s.pipeline.blend.DestBlend,s.pipeline.blend.BlendOp,
            s.pipeline.blend.SrcBlendAlpha,s.pipeline.blend.DestBlendAlpha,s.pipeline.blend.BlendOpAlpha,
            s.pipeline.blend.RenderTargetWriteMask);
        ok = !ferror(f) && ok; ok = fclose(f) == 0 && ok;
    } else ok = false;
    return ok;
}
bool write_bundle() noexcept
{
    wchar_t path[MAX_PATH]{};
    for (const auto& t : g->temporal) {
        if (!t->camera_valid) continue;
        swprintf_s(path, L"%s\\input-%lu-%03u-%03u-camera.bin", g->directory, GetCurrentProcessId(), t->sequence, t->index);
        FILE* camera = _wfopen(path, L"wb");
        if (!camera) { g->write_failed = true; continue; }
        if (fwrite(t->camera.data(), 1, t->camera.size(), camera) != t->camera.size()) g->write_failed = true;
        if (fclose(camera)) g->write_failed = true;
    }
    swprintf_s(path, L"%s\\bundle-%lu-%03u.json", g->directory, GetCurrentProcessId(), g->sequence);
    FILE* f = _wfopen(path, L"wb");
    if (!f) return false;
    const auto& presentation = g->captured_presentation;
    fprintf(f, "{\"version\":1,\"sequence\":%u,\"module_base\":\"0x%llx\",\"draw_count\":%u,"
        "\"draw_overflow\":%s,\"capture_timeout\":%s,\"write_failed\":%s,\"readbacks_written\":%u,"
        "\"presentation\":{\"format\":%u,\"width\":%u,\"height\":%u,\"color_space\":%u,\"color_space_set\":%s,"
        "\"hdr_metadata_type\":%u,\"hdr_metadata_hex\":\"", g->sequence,static_cast<unsigned long long>(g->base),g->ordinal,
        g->draw_overflow?"true":"false",g->capture_timeout?"true":"false",g->write_failed?"true":"false",g->written,
        presentation.format,presentation.width,presentation.height,presentation.color_space,presentation.color_space_set?"true":"false",
        presentation.metadata_type);
    for (auto byte : presentation.metadata) fprintf(f, "%02x", byte);
    fputs("\"},\"requested_input_frames\":4,\"temporal\":[", f);
    bool comma = false;
    for (const auto& t : g->temporal) {
        fprintf(f, "%s{\"index\":%u,\"frame_counter_observation\":%u,\"thread\":%u,\"feature_flags\":%u,"
            "\"tick\":%llu,\"qpc\":%llu,\"command_list\":\"0x%llx\",\"generation\":%llu,\"view\":\"0x%llx\","
            "\"draw_context\":\"0x%llx\",\"job\":\"0x%llx\",\"camera_valid\":%s,\"evaluation_result\":%llu,\"scalars\":{",
            comma?",":"",t->index,t->frame,t->thread,t->feature_flags,static_cast<unsigned long long>(t->tick),
            static_cast<unsigned long long>(t->qpc),static_cast<unsigned long long>(t->list),static_cast<unsigned long long>(t->generation),
            static_cast<unsigned long long>(t->view),static_cast<unsigned long long>(t->draw_context),static_cast<unsigned long long>(t->job),
            t->camera_valid?"true":"false",static_cast<unsigned long long>(t->result));
        bool scalar_comma = false;
        for (const auto& scalar : t->scalars) {
            fprintf(f, "%s\"%s\":%.12g",scalar_comma?",":"",scalar.first.c_str(),scalar.second); scalar_comma = true;
        }
        fputs("},\"refusals\":[", f);
        bool refusal_comma = false;
        for (const auto& refusal : t->refusals) {
            fprintf(f, "%s\"%s\"",refusal_comma?",":"",refusal.c_str()); refusal_comma = true;
        }
        fputs("]}", f); comma = true;
    }
    fputs("],\"draws\":[", f); comma = false;
    for (const auto& d : g->draws) {
        const auto& b = d.pipeline.blend;
        fprintf(f, "%s{\"draw\":%u,\"frame_counter_observation\":%u,\"vertices\":%u,\"instances\":%u,"
            "\"command_list\":\"0x%llx\",\"generation\":%llu,\"caller\":\"0x%llx\",\"pso\":\"0x%llx\","
            "\"ps_crc32\":\"%08x\",\"vs_crc32\":\"%08x\",\"blend\":%d,\"src\":%u,\"dst\":%u,\"op\":%u,"
            "\"src_alpha\":%u,\"dst_alpha\":%u,\"op_alpha\":%u,\"write_mask\":%u,\"stack\":[",comma?",":"",
            d.ordinal,d.frame,d.vertices,d.instances,static_cast<unsigned long long>(d.list),static_cast<unsigned long long>(d.generation),
            static_cast<unsigned long long>(d.caller),static_cast<unsigned long long>(d.pso),d.pipeline.ps,d.pipeline.vs,
            b.BlendEnable,b.SrcBlend,b.DestBlend,b.BlendOp,b.SrcBlendAlpha,b.DestBlendAlpha,b.BlendOpAlpha,b.RenderTargetWriteMask);
        for (USHORT i=0;i<d.stack_size;++i)
            fprintf(f, "%s\"0x%llx\"",i?",":"",static_cast<unsigned long long>(reinterpret_cast<uint64_t>(d.stack[i])));
        fputs("]}", f); comma = true;
    }
    fputs("]}\n", f);
    bool ok = !ferror(f); ok = fclose(f) == 0 && ok;
    return ok;
}
DWORD WINAPI poll(void*) noexcept
{
    bool was_down = false;
    for (;;) {
        Sleep(100);
#ifdef RSF_RDR2_CAPTURE_TEST
        if (g->fixture_stop) return 0;
#endif
        const bool down = (GetAsyncKeyState(VK_F10) & 0x8000) != 0;
        if (down && !was_down) {
            DWORD foreground{};
            GetWindowThreadProcessId(GetForegroundWindow(), &foreground);
            if (foreground == GetCurrentProcessId()) g->pending = true;
        }
        was_down = down;
        if (GetFileAttributesW(g->request) != INVALID_FILE_ATTRIBUTES && DeleteFileW(g->request)) g->pending = true;
        std::vector<std::shared_ptr<Shot>> ready;
        {
            std::lock_guard<std::mutex> lock(g->mutex);
            if (g->capture_dirty && GetTickCount64() - g->capture_started > 5000 &&
                (g->active || g->inputs_remaining)) {
                g->active = false; g->inputs_remaining = 0; g->capture_timeout = true;
                log("capture %u timed out waiting for scene or DLSS input; retaining explicit incomplete result",g->sequence);
            }
            if (!g->active) {
                auto it = g->shots.begin();
                while (it != g->shots.end()) {
                    const auto& shot = *it;
                    const auto completed = shot->fence ? shot->fence->GetCompletedValue() : 0;
                    if (shot->fence && completed != UINT64_MAX && completed >= shot->fence_value) {
                        ready.push_back(shot); it = g->shots.erase(it);
                    } else ++it;
                }
                // ResizeBuffers must not be blocked by diagnostic references after retirement.
                if (g->shots.empty()) {
                    for (auto& buffer : g->buffers) buffer.Reset();
                    g->selected = nullptr;
                }
            }
        }
        for (const auto& shot : ready) {
            const bool ok = write_shot(*shot);
            std::lock_guard<std::mutex> lock(g->mutex);
            if (ok) ++g->written; else g->write_failed = true;
        }
        {
            std::lock_guard<std::mutex> lock(g->mutex);
            if (g->capture_dirty && !g->active && !g->inputs_remaining && !g->inputs_in_flight && g->shots.empty()) {
                const bool ok = write_bundle();
                log("capture %u bundle saved=%d input_frames=%zu draws=%zu readbacks=%u",g->sequence,ok,g->temporal.size(),g->draws.size(),g->written);
                g->capture_dirty = false; g->tail.reset();
            }
        }
    }
}
}
bool rsf_rdr2_install_native_capture(const wchar_t* directory) noexcept try
{
    if (g || !directory || wcslen(directory) > MAX_PATH - 64) return false;
    g = new State;
    g->base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    wcscpy_s(g->directory, directory);
    swprintf_s(g->request, L"%s\\native.request", directory);
    swprintf_s(g->log, L"%s\\native-probe.log", directory);
    // The census owns MinHook initialization. Its device callback runs only after installation.
    rsf_census_observe_d3d12(device_created, before_submit, after_submit, before_present, swapchain_created, color_space_set);
    rsf_census_observe_ngx_d3d12(ngx_created, ngx_evaluate, ngx_released);
    HANDLE thread = CreateThread(nullptr, 0, poll, nullptr, 0, nullptr);
    if (!thread) return false;
#ifdef RSF_RDR2_CAPTURE_TEST
    g->fixture_thread = thread;
#else
    CloseHandle(thread);
#endif
    log("native capture installed"); return true;
}
catch (...) { return false; }

#ifdef RSF_RDR2_CAPTURE_TEST
// The standalone GPU fixture owns its swapchain; production only accepts the guarded game owner.
void rsf_rdr2_capture_test_arm(void* swapchain) noexcept
{
    std::lock_guard<std::mutex> lock(g->mutex);
    g->fixture_swapchain = swapchain; g->pending = true;
}
void rsf_rdr2_capture_test_stop() noexcept
{
    g->fixture_stop = true;
    WaitForSingleObject(g->fixture_thread,5000); CloseHandle(g->fixture_thread);
}
#endif
