// SPDX-License-Identifier: MIT
#include <rescaleframe/native_cpu_service.h>
#include "fg12_platform.h"
#include "../../backends/common/frame_capture12.h"
#include <dxgi1_4.h>

using Microsoft::WRL::ComPtr;

namespace {
rsf::FrameCapture12 dump;
}

extern "C" uint32_t rsf_fg12_platform_buffer_index(void* chain)
{
    ComPtr<IDXGISwapChain3> typed;
    return chain && SUCCEEDED(static_cast<IUnknown*>(chain)->QueryInterface(IID_PPV_ARGS(&typed)))
        ? typed->GetCurrentBackBufferIndex() : UINT32_MAX;
}

extern "C" int rsf_fg12_platform_install(const rsf_d3d11_present_setup* setup, int explicit_creation) try
{
    return explicit_creation ? rsf_d3d11_present_prepare(setup) : rsf_d3d11_present_install(setup);
} catch (...) { return 0; }

extern "C" uint32_t rsf_fg12_platform_allows_fg(const rsf_frame_record* record)
{
    return rsf_frame_allows_fg(record);
}

extern "C" uint32_t rsf_fg12_platform_depth_domains(const rsf_game_render_pass* pass)
{
    return rsf_game_depth_domains_requested(&pass->depth_domains);
}

extern "C" void rsf_fg12_platform_texture_desc(void* resource, rsf_fg12_texture_desc* output)
{
    const auto desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    *output = {desc.Width, desc.Height, uint32_t(desc.Format)};
}

extern "C" void* rsf_fg12_platform_texture(void* list, void* resource, void* reuse)
{
    auto desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    ComPtr<ID3D12Device> device;
    if (FAILED(static_cast<ID3D12GraphicsCommandList*>(list)->GetDevice(IID_PPV_ARGS(&device)))) return nullptr;
    auto* previous = static_cast<ID3D12Resource*>(reuse);
    if (previous) {
        const auto old = previous->GetDesc();
        ComPtr<ID3D12Device> previous_device;
        if (old.Dimension == desc.Dimension && old.Alignment == desc.Alignment && old.Width == desc.Width &&
            old.Height == desc.Height && old.DepthOrArraySize == desc.DepthOrArraySize &&
            old.MipLevels == desc.MipLevels && old.Format == desc.Format && old.Layout == desc.Layout &&
            old.SampleDesc.Count == desc.SampleDesc.Count && old.SampleDesc.Quality == desc.SampleDesc.Quality &&
            SUCCEEDED(previous->GetDevice(IID_PPV_ARGS(&previous_device))) && previous_device.Get() == device.Get()) {
            previous->AddRef();
            return previous;
        }
    }
    desc.Flags = D3D12_RESOURCE_FLAG_NONE;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    ComPtr<ID3D12Resource> texture;
    if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&texture)))) return nullptr;
    return texture.Detach();
}

extern "C" void rsf_fg12_platform_copy(void* list, void* source, uint32_t source_state, void* target, uint32_t target_state)
{
    const auto before = static_cast<D3D12_RESOURCE_STATES>(source_state);
    rsf::copy_transitioned(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(target),
        static_cast<D3D12_RESOURCE_STATES>(target_state), D3D12_RESOURCE_STATE_COMMON,
        static_cast<ID3D12Resource*>(source), before, before);
}

extern "C" void rsf_fg12_platform_transition(void* list, void* resource, uint32_t before, uint32_t after)
{
    rsf::transition(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(resource),
        static_cast<D3D12_RESOURCE_STATES>(before), static_cast<D3D12_RESOURCE_STATES>(after));
}

extern "C" void* rsf_fg12_platform_fence(void* queue)
{
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12Fence> fence;
    if (FAILED(static_cast<ID3D12CommandQueue*>(queue)->GetDevice(IID_PPV_ARGS(&device))) ||
        FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return nullptr;
    return fence.Detach();
}

extern "C" int rsf_fg12_platform_signal(void* queue, void* fence, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->Signal(static_cast<ID3D12Fence*>(fence), value));
}

extern "C" void* rsf_fg12_platform_readback(void* source, rsf_fg12_readback_layout* output)
{
    return rsf::allocate_readback12(source, output, false);
}

extern "C" void rsf_fg12_platform_readback_copy(void* list, void* source, uint32_t source_state, void* buffer,
    const rsf_fg12_readback_layout* layout)
{
    rsf::copy_readback12(list, source, source_state, buffer, layout);
}

extern "C" const uint8_t* rsf_fg12_platform_map(void* buffer, uint64_t bytes)
{
    return rsf::map_readback12(buffer, bytes);
}

extern "C" void rsf_fg12_platform_unmap(void* buffer)
{
    rsf::unmap_readback12(buffer);
}

extern "C" void rsf_fg12_platform_dump_record(void* list, void* source, uint32_t state, const rsf_frame_record* record,
    const char* role, uint32_t slot)
{
    (void)dump.record(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(source),
        static_cast<D3D12_RESOURCE_STATES>(state), *record, role, slot);
}

extern "C" void rsf_fg12_platform_dump_flush() { (void)dump.flush_ready(); }

extern "C" void rsf_fg12_platform_dump_submitted(void* queue, const rsf_frame_record* record)
{
    (void)dump.submitted(static_cast<ID3D12CommandQueue*>(queue), UINT32_MAX, record);
}

extern "C" void rsf_fg12_platform_dump_prepare(const rsf_fg_frame* frame, rsf_backend_result result)
{
    rsf_input_dump_fg_prepare(nullptr, frame, result);
}

extern "C" {
void rsf_core_fg12_bind_platform(const rsf_fg12_calls*);
rsf_backend_result rsf_core_fg12_install(const rsf_fg12_setup*);
rsf_backend_result rsf_core_fg12_install_explicit(const rsf_fg12_setup*);
void rsf_core_fg12_cpu(const rsf_game_cpu_event*);
rsf_backend_result rsf_core_fg12_latency(const rsf_game_latency_event*);
rsf_backend_result rsf_core_fg12_render_begin(uint64_t, uint64_t);
void rsf_core_fg12_capture(void*, const rsf_frame_record*, void*, void*);
void rsf_core_fg12_hudless(void*, const rsf_game_render_pass*);
void rsf_core_fg12_submitted(uint64_t, uint64_t);
int rsf_core_fg12_end_session(uint64_t);
void rsf_core_fg12_window(const rsf_game_render_pass*);
int rsf_core_fg12_status(rsf_native_fg_status*);
int rsf_core_fg12_status_try(rsf_native_fg_status*);
void rsf_core_fg12_options(const rsf_native_fg_options*);
rsf_backend_result rsf_core_fg12_capture_outputs(const char*);
}

namespace {
void bind_platform()
{
    static constexpr rsf_fg12_calls calls{
        .struct_size = sizeof(rsf_fg12_calls),
        .buffer_index = rsf_fg12_platform_buffer_index,
        .install = rsf_fg12_platform_install,
        .allows_fg = rsf_fg12_platform_allows_fg,
        .depth_domains = rsf_fg12_platform_depth_domains,
        .texture_desc = rsf_fg12_platform_texture_desc,
        .texture = rsf_fg12_platform_texture,
        .copy = rsf_fg12_platform_copy,
        .transition = rsf_fg12_platform_transition,
        .fence = rsf_fg12_platform_fence,
        .signal = rsf_fg12_platform_signal,
        .readback = rsf_fg12_platform_readback,
        .readback_copy = rsf_fg12_platform_readback_copy,
        .map = rsf_fg12_platform_map,
        .unmap = rsf_fg12_platform_unmap,
        .dump_record = rsf_fg12_platform_dump_record,
        .dump_flush = rsf_fg12_platform_dump_flush,
        .dump_submitted = rsf_fg12_platform_dump_submitted,
        .dump_prepare = rsf_fg12_platform_dump_prepare,
        .present_queue = rsf_d3d11_present_queue,
        .source_queue = rsf_d3d11_present_source_queue,
        .present_retire = rsf_d3d11_present_retire,
        .is_owner = rsf_d3d11_present_is_owner,
        .has_owner = rsf_d3d11_present_has_owner,
        .present_provider = rsf_d3d11_present_provider,
        .present_backend = rsf_d3d11_present_backend,
        .present_generation = rsf_d3d11_present_generation,
        .host_abort = rsf_streamline_host_abort,
        .fg_set_log = rsf_native_fg_set_log,
        .fg_options_set = rsf_native_fg_options_set,
        .fg_cpu = rsf_native_fg_cpu,
        .latency_status_get = rsf_native_latency_status_get,
        .latency_frame_get = rsf_native_latency_frame_get,
        .latency_graphics = rsf_native_latency_graphics,
        .latency_render = rsf_native_latency_render,
        .latency_present = rsf_native_latency_present,
        .latency_end_session = rsf_native_latency_end_session,
        .cpu_dispatch = rsf_native_cpu_dispatch,
        .dump_poll = rsf_input_dump_poll,
        .dump_begin_record = rsf_input_dump_begin_record,
        .dump_current = rsf_input_dump_current,
        .dump_requested = rsf_input_dump_requested,
        .dump_complete = rsf_input_dump_complete,
        .dump_result = rsf_input_dump_result,
    };
    static const bool bound = (rsf_core_fg12_bind_platform(&calls), true);
    (void)bound;
}
}

extern "C" void rsf_fg12_platform_bind() { bind_platform(); }

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_install(const rsf_fg12_setup* request)
{
    bind_platform();
    return rsf_core_fg12_install(request);
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_install_explicit(const rsf_fg12_setup* request)
{
    bind_platform();
    return rsf_core_fg12_install_explicit(request);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_cpu(const rsf_game_cpu_event* event)
{
    bind_platform();
    rsf_core_fg12_cpu(event);
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_latency(const rsf_game_latency_event* event)
{
    bind_platform();
    return rsf_core_fg12_latency(event);
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_render_begin(uint64_t session, uint64_t source)
{
    bind_platform();
    return rsf_core_fg12_render_begin(session, source);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_capture(void* list, const rsf_frame_record* record, void* depth, void* motion)
{
    bind_platform();
    rsf_core_fg12_capture(list, record, depth, motion);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_hudless(void* list, const rsf_game_render_pass* pass)
{
    bind_platform();
    rsf_core_fg12_hudless(list, pass);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_submitted(uint64_t session, uint64_t source)
{
    bind_platform();
    rsf_core_fg12_submitted(session, source);
}
extern "C" RSF_RUNTIME_API int rsf_fg12_end_session(uint64_t session)
{
    bind_platform();
    return rsf_core_fg12_end_session(session);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_window(const rsf_game_render_pass* pass)
{
    bind_platform();
    rsf_core_fg12_window(pass);
}
extern "C" RSF_RUNTIME_API int rsf_fg12_status(rsf_native_fg_status* status)
{
    bind_platform();
    return rsf_core_fg12_status(status);
}
extern "C" RSF_RUNTIME_API int rsf_fg12_status_try(rsf_native_fg_status* status)
{
    bind_platform();
    return rsf_core_fg12_status_try(status);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_options(const rsf_native_fg_options* options)
{
    bind_platform();
    rsf_core_fg12_options(options);
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_capture_outputs(const char* directory)
{
    bind_platform();
    return rsf_core_fg12_capture_outputs(directory);
}

extern "C" const rsf_cpu_session_service* rsf_fg12_cpu_service()
{
    static constexpr rsf_cpu_session_service service{sizeof(service), RSF_CPU_SESSION_SERVICE_VERSION,
        rsf_fg12_cpu, rsf_native_latency_ping, nullptr};
    return &service;
}
