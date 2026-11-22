// SPDX-License-Identifier: MIT
#include <rescaleframe/native_cpu_service.h>
#include "fg12_platform.h"
#include "../../backends/common/frame_capture12.h"

namespace {
rsf::FrameCapture12 dump;
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
