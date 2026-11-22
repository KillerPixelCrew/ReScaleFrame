// SPDX-License-Identifier: MIT
#include "input_dump_platform.h"
#include <rescaleframe/capture_names.h>
#include <rescaleframe/input_dump_d3d12.h>

_Static_assert(sizeof(rsf_dump_services) == 48, "capture service ABI size");

static BOOL CALLBACK bind_services(PINIT_ONCE once, PVOID parameter, PVOID* context)
{
    (void)once;
    (void)parameter;
    (void)context;
    static const rsf_dump_services services = {
        sizeof(services), rsf_common_sha256_file, rsf_dump_texture_bytes,
        rsf_dump_texture_png, rsf_dump_pixels_png, rsf_game_depth_domains_requested,
    };
    rsf_core_dump_bind_services(&services);
    return TRUE;
}

static void bind_platform(void)
{
    static INIT_ONCE once = INIT_ONCE_STATIC_INIT;
    InitOnceExecuteOnce(&once, bind_services, NULL, NULL);
}

void rsf_core_dump_bind_native_services(void)
{
    /* Rust capture owners bind the same service table at admission. */
    bind_platform();
}

void rsf_core_dump_capture_sr_parameters(const rsf_frame_record*, uint32_t,
    float, float, float, uint32_t);

void rsf_capture_sr_parameters(const rsf_frame_record* record, uint32_t backend, float x,
    float y, float exposure, uint32_t reset)
{
    bind_platform();
    rsf_core_dump_capture_sr_parameters(record, backend, x, y, exposure, reset);
}

// Forward the public symbols through this archive so embedded and DLL builds
// both pull in the same native leaves and emit the existing export directives.
#define RSF_DUMP_EXPORT(result, name, signature, arguments) \
    result rsf_core_dump_##name signature; \
    RSF_RUNTIME_API result rsf_input_dump_##name signature \
    { bind_platform(); return rsf_core_dump_##name arguments; }
#define RSF_DUMP_EXPORT_VOID(name, signature, arguments) \
    void rsf_core_dump_##name signature; \
    RSF_RUNTIME_API void rsf_input_dump_##name signature \
    { bind_platform(); rsf_core_dump_##name arguments; }
RSF_DUMP_EXPORT_VOID(configure, (const char* dir, const char* game, const char* hash, rsf_dump_log_fn log, void* user),
    (dir, game, hash, log, user))
RSF_DUMP_EXPORT_VOID(request, (void), ())
RSF_DUMP_EXPORT_VOID(cancel, (uint64_t session), (session))
RSF_DUMP_EXPORT_VOID(poll, (void), ())
RSF_DUMP_EXPORT(int, begin_record, (const rsf_frame_record* record, uint32_t backend), (record, backend))
RSF_DUMP_EXPORT(int, wants, (const rsf_frame_record* record), (record))
RSF_DUMP_EXPORT(int, wants_frame, (uint64_t frame), (frame))
RSF_DUMP_EXPORT(int, current, (rsf_frame_record* record), (record))
RSF_DUMP_EXPORT(int, requested, (void), ())
RSF_DUMP_EXPORT(uint64_t, reserve, (const rsf_frame_record* record, const char* name, const char* description, rsf_dump_view view, float scale),
    (record, name, description, view, scale))
RSF_DUMP_EXPORT(int, pixels, (uint64_t ticket, const char* name, const rsf_dump_pixels* pixels, uint32_t api, uint32_t format),
    (ticket, name, pixels, api, format))
RSF_DUMP_EXPORT_VOID(failed, (uint64_t ticket, const char* name, int32_t result), (ticket, name, result))
RSF_DUMP_EXPORT_VOID(complete, (const rsf_frame_record* record, int32_t result), (record, result))
RSF_DUMP_EXPORT(uint64_t, gpu_submission, (const rsf_frame_record* record), (record))
RSF_DUMP_EXPORT_VOID(gpu_completed, (uint64_t submission, uint32_t api, void* device), (submission, api, device))
RSF_DUMP_EXPORT(int, gpu_ready, (uint64_t ticket, const char* name, uint32_t api, void* device), (ticket, name, api, device))
RSF_DUMP_EXPORT_VOID(pass_metadata, (const rsf_game_render_pass* pass), (pass))
RSF_DUMP_EXPORT_VOID(result, (const rsf_frame_record* record, const char* stage, uint32_t backend, int32_t result),
    (record, stage, backend, result))
RSF_DUMP_EXPORT_VOID(sr_frame, (const rsf_sr_frame* frame, uint32_t backend, uint32_t input_sdr), (frame, backend, input_sdr))
RSF_DUMP_EXPORT(int, texture_record, (void* context, const rsf_frame_record* record, const char* name, const char* description,
    void* texture, rsf_dump_view view, float scale), (context, record, name, description, texture, view, scale))
RSF_DUMP_EXPORT(int, active, (void), ())
RSF_DUMP_EXPORT_VOID(present, (void* swapchain, void* context, const rsf_game_render_pass* window), (swapchain, context, window))
RSF_DUMP_EXPORT_VOID(presented, (const rsf_game_render_pass* window, int32_t result), (window, result))
RSF_DUMP_EXPORT_VOID(pass, (void* context, const rsf_game_render_pass* pass, uint32_t begin), (context, pass, begin))
RSF_DUMP_EXPORT_VOID(texture, (void* context, const char* name, const char* description, void* texture, rsf_dump_view view, float scale),
    (context, name, description, texture, view, scale))
RSF_DUMP_EXPORT_VOID(sr, (void* context, const rsf_dlss_frame* frame, uint32_t backend), (context, frame, backend))
RSF_DUMP_EXPORT_VOID(sr_result, (int32_t result), (result))
RSF_DUMP_EXPORT_VOID(fg, (void* context, const rsf_game_render_pass* pass, const rsf_dlss_frame* frame), (context, pass, frame))
RSF_DUMP_EXPORT_VOID(fg_prepare, (const rsf_game_render_pass* pass, const rsf_fg_frame* frame, int32_t result), (pass, frame, result))
RSF_DUMP_EXPORT(int32_t, named_texture, (void* context, void* source, const rsf_named_frame_dump* request, rsf_named_frame_dump_report* report),
    (context, source, request, report))
#undef RSF_DUMP_EXPORT
#undef RSF_DUMP_EXPORT_VOID

int32_t rsf_core_dump12_create(rsf_input_dump12**);
RSF_RUNTIME_API int32_t rsf_input_dump12_create(rsf_input_dump12** out)
{
    return rsf_core_dump12_create(out);
}

#define RSF_DUMP12_EXPORT(result, name, signature, arguments) \
    result rsf_core_dump12_##name signature; \
    RSF_RUNTIME_API result rsf_input_dump12_##name signature \
    { return rsf_core_dump12_##name arguments; }
RSF_DUMP12_EXPORT(int32_t, record, (rsf_input_dump12* self, void* list, void* texture, const rsf_frame_record* record,
    const char* name, uint32_t state, uint32_t slot, rsf_dump_view view, float scale),
    (self, list, texture, record, name, state, slot, view, scale))
RSF_DUMP12_EXPORT(int32_t, submitted, (rsf_input_dump12* self, void* queue, uint32_t slot), (self, queue, slot))
RSF_DUMP12_EXPORT(int32_t, poll, (rsf_input_dump12* self), (self))
RSF_DUMP12_EXPORT(int32_t, destroy, (rsf_input_dump12* self), (self))
#undef RSF_DUMP12_EXPORT

void rsf_core_dump12_abandon(rsf_input_dump12*, uint32_t);
RSF_RUNTIME_API void rsf_input_dump12_abandon(rsf_input_dump12* self, uint32_t slot)
{
    rsf_core_dump12_abandon(self, slot);
}
