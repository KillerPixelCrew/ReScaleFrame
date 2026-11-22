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
#include <cstdint>
#include <cstdio>
#include <cstdarg>
#include <cstddef>
#include <cwchar>
#include <memory>
#include <iterator>
#include <mutex>
#include <new>

namespace {
using Microsoft::WRL::ComPtr;
using Pipeline = rsf_rdr2_capture_pipeline;
using DrawEvent = rsf_rdr2_capture_draw;
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
using PipelineFacts = bool (*)(const void*, Pipeline*);
void remember_pipeline(void* result, const void* descriptor, PipelineFacts read)
{
    Pipeline facts{};
    const bool known = read(descriptor, &facts);
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_record_pipeline(g->capture, result, known ? &facts : nullptr);
}
static_assert(MH_OK == 0 && MH_ERROR_ENABLED == 5 && MH_UNKNOWN == -1);
constexpr rsf_rdr2_capture_hook_ops hook_ops{
    [](void* object, uint32_t slot) noexcept { return (*static_cast<void***>(object))[slot]; },
    [](void* target, const void* detour, void** original) noexcept {
        return static_cast<int32_t>(MH_CreateHook(target, const_cast<void*>(detour), original));
    },
    [](void* target) noexcept { return static_cast<int32_t>(MH_EnableHook(target)); },
    [](uint32_t slot, void* target, int32_t result) noexcept {
        const char* status = result == RSF_RDR2_CAPTURE_UNOBSERVED_HOOK_TARGET
            ? "unobserved vtable target" : MH_StatusToString(static_cast<MH_STATUS>(result));
        log("hook slot=%u target=%p result=%s", slot, target, status);
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
HRESULT STDMETHODCALLTYPE create_stream_pso(ID3D12Device2* device,
    const D3D12_PIPELINE_STATE_STREAM_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original<CreateStreamPso>(RSF_RDR2_CAPTURE_CREATE_STREAM_PSO)(device, desc, iid, out);
    if (SUCCEEDED(hr) && out && *out) remember_pipeline(*out, desc, rsf_rdr2_capture_stream_facts);
    return hr;
}
HRESULT STDMETHODCALLTYPE load_stream_pso(ID3D12PipelineLibrary1* library, LPCWSTR name,
    const D3D12_PIPELINE_STATE_STREAM_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original<LoadStreamPso>(RSF_RDR2_CAPTURE_LOAD_STREAM_PSO)(library, name, desc, iid, out);
    if (SUCCEEDED(hr) && out && *out) remember_pipeline(*out, desc, rsf_rdr2_capture_stream_facts);
    return hr;
}

HRESULT STDMETHODCALLTYPE create_pso(ID3D12Device* d, const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original<CreatePso>(RSF_RDR2_CAPTURE_CREATE_PSO)(d, desc, iid, out);
    if (SUCCEEDED(hr) && out && *out) remember_pipeline(*out, desc, rsf_rdr2_capture_graphics_facts);
    return hr;
}
HRESULT STDMETHODCALLTYPE load_pso(ID3D12PipelineLibrary* library, LPCWSTR name,
    const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc, REFIID iid, void** out)
{
    const auto hr = original<LoadPso>(RSF_RDR2_CAPTURE_LOAD_PSO)(library, name, desc, iid, out);
    if (SUCCEEDED(hr) && out && *out) remember_pipeline(*out, desc, rsf_rdr2_capture_graphics_facts);
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
bool copy_resource(void* list, void* target, uint32_t state,
    rsf_rdr2_capture_gpu_leases* leases, rsf_rdr2_capture_image* image) noexcept
{
    CopyScope copying;
    int32_t allocation_hresult{};
    if (!rsf_rdr2_capture_d3d12_copy(list, target, state, leases, image, &allocation_hresult)) {
        if (allocation_hresult) log("readback allocation failed hr=%08x", static_cast<uint32_t>(allocation_hresult));
        return false;
    }
    image->tick = GetTickCount64();
    read_local(g->base + 0x542904c, &image->frame, sizeof(image->frame));
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
    const char* key, bool required = true)
{
    ID3D12Resource* resource{};
    if ((!ngx_get(parameters, 9, key, resource) && !ngx_get(parameters, 8, key, resource)) || !resource) {
        if (required) rsf_rdr2_capture_input_resource_refused(g->capture, &id, input_role, true);
        return;
    }
    const auto before = rsf_rdr2_capture_resource_state(g->capture, list, resource);
    const auto result = rsf_rdr2_capture_input_shot(g->capture, &id, input_role, list, resource, before, copy_resource);
    if (result == RSF_RDR2_CAPTURE_COPY_NO_SLOT) throw std::bad_alloc{};
    if (result == RSF_RDR2_CAPTURE_COPY_REFUSED) {
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
            input_resource(list, parameters, id, RSF_RDR2_CAPTURE_OUTPUT, "Output");
        return;
    }
    if (!rsf_rdr2_capture_begin_input(g->capture, list, handle, &id)) return;
    FinishInput finish;
    input_camera(id);
    rsf_rdr2_capture_input_scalars(g->capture, &id, parameters,
        [](const void* params, const char* key, bool floating, double* output) noexcept {
        if (floating) {
            float value{};
            if (!ngx_get(params, 14, key, value)) return false;
            *output = value;
            return true;
        }
        uint32_t value{};
        if (ngx_get(params, 12, key, value)) {
            *output = value;
            return true;
        }
        int32_t alternate{};
        if (!ngx_get(params, 11, key, alternate)) return false;
        *output = alternate;
        return true;
    });
    input_resource(list, parameters, id, RSF_RDR2_CAPTURE_COLOR, "Color");
    input_resource(list, parameters, id, RSF_RDR2_CAPTURE_DEPTH, "Depth");
    input_resource(list, parameters, id, RSF_RDR2_CAPTURE_MOTION, "MotionVectors");
    input_resource(list, parameters, id, RSF_RDR2_CAPTURE_EXPOSURE, "ExposureTexture", false);
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
    DrawEvent event{0, 0, vertices, instances, reinterpret_cast<uint64_t>(list), 0,
        reinterpret_cast<uint64_t>(caller), binding.pso, binding.pipeline, {}, 0, binding.pipeline_known};
    const auto result = rsf_rdr2_capture_draw_shot(g->capture, list, target, D3D12_RESOURCE_STATE_RENDER_TARGET,
        copy_resource, rsf_rdr2_capture_d3d12_completed, &event);
    if (result == RSF_RDR2_CAPTURE_COPY_NO_SLOT) {
        lock.unlock();
        log("snapshot failed");
    } else if (result == RSF_RDR2_CAPTURE_DRAW_SAVE_EVENT) {
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
void before_submit(void* queue, uint32_t count, void* const* lists) noexcept try
{
    std::lock_guard<std::mutex> lock(g->mutex);
    rsf_rdr2_capture_before_submit(g->capture, queue, count, lists);
}
catch (...) { log("submission observation lock failed"); }

void after_submit(void* queue, uint32_t, void* const*) noexcept try
{
    uint32_t result{};
    {
        std::lock_guard<std::mutex> lock(g->mutex);
        result = rsf_rdr2_capture_after_submit(g->capture, queue, rsf_rdr2_capture_d3d12_ops());
    }
    switch (result) {
    case RSF_RDR2_GPU_NO_SIGNAL_NEEDED:
    case RSF_RDR2_GPU_SIGNAL_ACCEPTED: return;
    case RSF_RDR2_GPU_CREATE_REFUSED: log("cannot create readback fence; retaining submitted readbacks"); return;
    case RSF_RDR2_GPU_SIGNAL_REFUSED: log("readback fence Signal failed"); return;
    case RSF_RDR2_GPU_UNPAIRED_SUBMISSION: log("submission pairing failed"); return;
    default: log("readback fence owner unavailable; retaining submitted readbacks"); return;
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
    void* completed_target{};
    bool armed{};
    rsf_rdr2_capture_progress progress{};
    size_t pending{};
    {
        std::lock_guard<std::mutex> lock(g->mutex);
        armed = rsf_rdr2_capture_d3d12_present(g->capture, pointer, &completed_target);
        if (armed) rsf_rdr2_request_camera_capture();
        if (completed_target || armed) progress = capture_progress();
        if (completed_target) pending = rsf_rdr2_capture_pending_count(g->capture);
    }
    if (completed_target) {
        log("capture %u recording complete: %u draws, %zu readbacks resource=%p", progress.sequence, progress.draws,
            pending, completed_target);
    }
    if (armed) log("capture %u armed", progress.sequence);
}
catch (...) { log("present observation failed"); }
bool write_shot(const rsf_rdr2_capture_gpu_leases* leases, const rsf_rdr2_capture_image* image,
    const char* role) noexcept
{
    int32_t map_hresult{};
    const bool ok = rsf_rdr2_capture_d3d12_write(leases, image,
        reinterpret_cast<const uint16_t*>(g->paths.directory), wcslen(g->paths.directory), GetCurrentProcessId(),
        role, &map_hresult);
    if (map_hresult) log("readback Map failed hr=%08x role=%s", static_cast<uint32_t>(map_hresult), role);
    return ok;
}
#ifdef RSF_RDR2_CAPTURE_TEST
constexpr bool fixture = true;
#else
constexpr bool fixture = false;
#endif

constexpr rsf_rdr2_capture_poll_ops poll_ops{
    [](void* context) noexcept { static_cast<std::mutex*>(context)->lock(); },
    [](void* context) noexcept { static_cast<std::mutex*>(context)->unlock(); },
    rsf_rdr2_capture_d3d12_completed,
    rsf_rdr2_capture_d3d12_removed,
    write_shot,
    [](uint32_t sequence) noexcept {
        log("capture %u timed out waiting for scene or DLSS input; retaining explicit incomplete result", sequence);
    },
    [](double value, char* buffer, size_t size) noexcept { return std::snprintf(buffer, size, "%.12g", value); },
    [](uint32_t sequence, bool saved, size_t inputs, size_t draws, uint32_t written) noexcept {
        log("capture %u bundle saved=%d input_frames=%zu draws=%zu readbacks=%u", sequence, saved, inputs, draws, written);
    },
};
DWORD WINAPI poll(void*) noexcept
{
    rsf_rdr2_capture_poll(g->capture, &g->mutex, &poll_ops,
        reinterpret_cast<const uint16_t*>(g->paths.request), reinterpret_cast<const uint16_t*>(g->paths.directory),
        wcslen(g->paths.directory), g->base, fixture);
    return 0;
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

extern "C" bool rsf_rdr2_install_native_capture(const wchar_t* directory) noexcept
{
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
