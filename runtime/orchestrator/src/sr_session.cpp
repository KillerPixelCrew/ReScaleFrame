// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/sr_session.h>
#include "../../backends/common/sr_helpers.h"
#include <cstring>
#include <new>

// Graphics-owner state. Provider contexts and copied SDK paths are owned; device/log callbacks
// remain borrowed. last_* tracks submitted SR identity, independently of CPU/presentation IDs.
struct rsf_sr_session {
    rsf_sr_open_desc open{};
    char directories[4][1024]{};
    const rsf_sr_provider* provider = nullptr;
    void* backend = nullptr;
    rsf_sr_session_status status{};
    bool reset = true;
    uint64_t last_frame = 0;
    uint64_t last_session = 0;
    uint32_t last_view = 0;
    uint32_t last_generation = 0;
};

extern "C" rsf_backend_result rsf_sr_session_create(const rsf_sr_session_setup* setup,
                                                     rsf_sr_session** out)
{
    if (!setup || !out || setup->struct_size < sizeof(*setup) ||
        setup->open.struct_size < sizeof(rsf_sr_open_desc) || !setup->open.device ||
        !setup->open.output_width || !setup->open.output_height) {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    *out = nullptr;
    if (setup->abi_version != RSF_SR_SESSION_ABI_VERSION ||
        setup->open.abi_version != RSF_BACKEND_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (setup->open.api != RSF_API_D3D12) return RSF_BACKEND_ERROR_WRONG_API;
    auto* session = new (std::nothrow) rsf_sr_session;
    if (!session) return RSF_BACKEND_ERROR_INIT_FAILED;
    session->open = setup->open;
    session->open.runtime_directory_utf8 = nullptr;
    const char* paths[] = {setup->fsr2_directory_utf8, setup->fsr3_directory_utf8,
                          setup->fsr4_directory_utf8, setup->xess_directory_utf8};
    for (uint32_t i = 0; i < 4; ++i) {
        if (paths[i]) {
            if (std::strlen(paths[i]) >= sizeof(session->directories[i])) {
                delete session;
                return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
            }
            std::strcpy(session->directories[i], paths[i]);
        }
    }
    session->status.struct_size = sizeof(session->status);
    *out = session;
    return RSF_BACKEND_OK;
}

// Open/plan the replacement before closing the old context. force also rebuilds unchanged
// selections when exposure creation flags change; failures preserve the active provider.
static rsf_backend_result select_session(rsf_sr_session* session,
    rsf_sr_backend backend, rsf_quality quality, uint64_t version_id, bool force)
{
    if (!session || (backend != RSF_SR_NONE && (backend < RSF_SR_FSR2 || backend > RSF_SR_XESS)) ||
        quality > RSF_QUALITY_ULTRA_QUALITY) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& status = session->status;
    status.requested = backend;
    auto fail = [&](rsf_backend_result result) {
        status.last_switch_result = result;
        if (session->open.log) {
            char message[160];
            std::snprintf(message, sizeof(message), "SR switch to %u refused (%d); keeping %u",
                          backend, result, status.effective);
            session->open.log(session->open.log_user, message);
        }
        return result;
    };
    if (backend == RSF_SR_NONE) {
        if (session->backend) session->provider->close(session->backend);
        session->backend = nullptr;
        session->provider = nullptr;
        status.effective = RSF_SR_NONE;
        status.version_id = 0;
        status.version_name[0] = 0;
        status.last_switch_result = RSF_BACKEND_OK;
        session->reset = true;
        return RSF_BACKEND_OK;
    }
    if (!force && session->backend && status.effective == backend && status.quality == quality &&
        (!version_id || status.version_id == version_id)) {
        status.last_switch_result = RSF_BACKEND_OK;
        return RSF_BACKEND_OK;
    }
    const auto* provider = backend == RSF_SR_XESS ? rsf_xess_sr_provider() : rsf_fsr_sr_provider();
    rsf_sr_open_desc open = session->open;
    open.quality = quality;
    open.fsr_major = backend == RSF_SR_XESS ? 0 : backend;
    open.version_id = version_id;
    open.runtime_directory_utf8 = session->directories[backend - RSF_SR_FSR2];
    void* replacement = nullptr;
    auto result = provider->open(&open, &replacement);
    if (result != RSF_BACKEND_OK) return fail(result);
    uint32_t width = 0, height = 0;
    result = provider->plan(replacement, quality, &width, &height);
    if (result != RSF_BACKEND_OK || !width || !height || width > open.output_width ||
        height > open.output_height) {
        provider->close(replacement);
        return fail(result != RSF_BACKEND_OK ? result : RSF_BACKEND_ERROR_INIT_FAILED);
    }
    uint64_t effective_id = 0;
    const char* name = nullptr;
    if (provider->get_version) provider->get_version(replacement, &effective_id, &name);
    if (session->backend) session->provider->close(session->backend);
    session->backend = replacement;
    session->provider = provider;
    status.effective = backend;
    status.quality = quality;
    status.render_width = width;
    status.render_height = height;
    status.version_id = effective_id;
    std::snprintf(status.version_name, sizeof(status.version_name), "%s", name ? name : "unknown");
    status.last_switch_result = RSF_BACKEND_OK;
    session->reset = true;
    if (open.log) open.log(open.log_user, status.version_name);
    return RSF_BACKEND_OK;
}

extern "C" rsf_backend_result rsf_sr_session_select(rsf_sr_session* session,
    rsf_sr_backend backend, rsf_quality quality, uint64_t version_id)
{ return select_session(session, backend, quality, version_id, false); }
extern "C" rsf_backend_result rsf_sr_session_set_auto_exposure(rsf_sr_session* session, uint32_t enabled)
{
    if (!session || enabled > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (session->open.auto_exposure == enabled) return RSF_BACKEND_OK;
    const auto previous = session->open.auto_exposure;
    session->open.auto_exposure = enabled;
    const auto result = select_session(session, session->status.effective,
        session->status.quality, session->status.version_id, true);
    if (result != RSF_BACKEND_OK) session->open.auto_exposure = previous;
    return result;
}

extern "C" rsf_backend_result rsf_sr_session_evaluate(rsf_sr_session* session,
    void* command_context, const rsf_sr_frame* frame)
{
    if (!session) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto result = command_context ? rsf::validate_frame(frame) : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (result == RSF_BACKEND_OK && !session->backend) result = RSF_BACKEND_ERROR_NOT_READY;
    if (result == RSF_BACKEND_OK) {
        const auto& record = *frame->record;
        if (record.output_width != session->open.output_width ||
            record.output_height != session->open.output_height) {
            result = RSF_BACKEND_ERROR_STALE_RESOURCES;
        } else if (record.session_id == session->last_session && record.view_id == session->last_view &&
                   record.frame_id <= session->last_frame) {
            result = RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        } else {
            // Gaps, refusals and identity/resource changes reset history. Advance last_* after
            // a provider attempt even on refusal, so the same frame cannot be submitted twice.
            rsf_sr_frame submitted = *frame;
            submitted.reset |= session->reset || (record.flags & RSF_FRAME_FLAG_RESET) ||
                record.session_id != session->last_session || record.view_id != session->last_view ||
                record.frame_id != session->last_frame + 1 ||
                record.resource_generation != session->last_generation;
            result = session->provider->evaluate(session->backend, command_context, &submitted);
            session->last_frame = record.frame_id;
            session->last_session = record.session_id;
            session->last_view = record.view_id;
            session->last_generation = record.resource_generation;
        }
    }
    session->reset = result != RSF_BACKEND_OK;
    if (result != RSF_BACKEND_OK && session->status.last_frame_result != result && session->open.log) {
        char message[96];
        std::snprintf(message, sizeof(message), "SR frame refused (%d), backend %u", result, session->status.effective);
        session->open.log(session->open.log_user, message);
    }
    session->status.last_frame_result = result;
    if (result == RSF_BACKEND_OK) ++session->status.frames_evaluated;
    else ++session->status.frames_refused;
    return result;
}

extern "C" rsf_backend_result rsf_sr_session_get_status(const rsf_sr_session* session,
    rsf_sr_session_status* status)
{
    if (!session || !status || status->struct_size < sizeof(*status)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *status = session->status;
    return RSF_BACKEND_OK;
}
extern "C" void rsf_sr_session_destroy(rsf_sr_session* session)
{
    if (!session) return;
    if (session->backend) session->provider->close(session->backend);
    delete session;
}
