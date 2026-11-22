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

extern "C" {
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

extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_install(const rsf_fg12_setup* request)
{
    rsf_fg12_platform_bind();
    return rsf_core_fg12_install(request);
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_install_explicit(const rsf_fg12_setup* request)
{
    rsf_fg12_platform_bind();
    return rsf_core_fg12_install_explicit(request);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_cpu(const rsf_game_cpu_event* event)
{
    rsf_fg12_platform_bind();
    rsf_core_fg12_cpu(event);
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_latency(const rsf_game_latency_event* event)
{
    rsf_fg12_platform_bind();
    return rsf_core_fg12_latency(event);
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_render_begin(uint64_t session, uint64_t source)
{
    rsf_fg12_platform_bind();
    return rsf_core_fg12_render_begin(session, source);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_capture(void* list, const rsf_frame_record* record, void* depth, void* motion)
{
    rsf_fg12_platform_bind();
    rsf_core_fg12_capture(list, record, depth, motion);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_hudless(void* list, const rsf_game_render_pass* pass)
{
    rsf_fg12_platform_bind();
    rsf_core_fg12_hudless(list, pass);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_submitted(uint64_t session, uint64_t source)
{
    rsf_fg12_platform_bind();
    rsf_core_fg12_submitted(session, source);
}
extern "C" RSF_RUNTIME_API int rsf_fg12_end_session(uint64_t session)
{
    rsf_fg12_platform_bind();
    return rsf_core_fg12_end_session(session);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_window(const rsf_game_render_pass* pass)
{
    rsf_fg12_platform_bind();
    rsf_core_fg12_window(pass);
}
extern "C" RSF_RUNTIME_API int rsf_fg12_status(rsf_native_fg_status* status)
{
    rsf_fg12_platform_bind();
    return rsf_core_fg12_status(status);
}
extern "C" RSF_RUNTIME_API int rsf_fg12_status_try(rsf_native_fg_status* status)
{
    rsf_fg12_platform_bind();
    return rsf_core_fg12_status_try(status);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_options(const rsf_native_fg_options* options)
{
    rsf_fg12_platform_bind();
    rsf_core_fg12_options(options);
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_capture_outputs(const char* directory)
{
    rsf_fg12_platform_bind();
    return rsf_core_fg12_capture_outputs(directory);
}

extern "C" const rsf_cpu_session_service* rsf_fg12_cpu_service()
{
    static constexpr rsf_cpu_session_service service{sizeof(service), RSF_CPU_SESSION_SERVICE_VERSION,
        rsf_fg12_cpu, rsf_native_latency_ping, nullptr};
    return &service;
}
