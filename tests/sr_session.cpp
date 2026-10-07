// SPDX-License-Identifier: GPL-3.0-only
/**
 * @file
 * Check SR controller replacement and history policy with deterministic providers.
 * The executable links the controller directly and substitutes FSR/XeSS entry points.
 * Owned SDK-path copies, failed replacement cleanup, generation/ID validation and
 * history resets are checked without touching the opaque device/resource tokens.
 */
#include <rescaleframe/sr_session.h>
#include <cstdio>
#include <cstring>
#include <limits>
#include <new>

namespace {
bool passed = true;
uint32_t closes = 0;
uint32_t reset_seen = 0;
bool fail_open = false, fail_plan = false, fail_frame = false;
// Each provider context owns one allocation, allowing close counts to detect replacement leaks.
struct Fake { uint32_t family; };
void check(bool ok, const char* text) { if (!ok) { std::fprintf(stderr, "FAIL: %s\n", text); passed = false; } }
rsf_backend_result probe(const rsf_backend_probe_desc*, rsf_backend_caps*) { return RSF_BACKEND_OK; }
rsf_backend_result open(const rsf_sr_open_desc* desc, void** session)
{
    *session = nullptr;
    if (fail_open) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    check(desc->runtime_directory_utf8 && std::strcmp(desc->runtime_directory_utf8, "copied-path") == 0,
          "session must own a copy of its SDK paths");
    *session = new Fake{desc->fsr_major};
    return RSF_BACKEND_OK;
}
rsf_backend_result plan(void*, rsf_quality, uint32_t* width, uint32_t* height)
{
    if (fail_plan) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    *width = *height = 64; return RSF_BACKEND_OK;
}
rsf_backend_result evaluate(void*, void*, const rsf_sr_frame* frame)
{
    reset_seen = frame->reset;
    return fail_frame ? RSF_BACKEND_ERROR_FEATURE_FAILED : RSF_BACKEND_OK;
}
rsf_backend_result release(void*) { return RSF_BACKEND_OK; }
void close(void* pointer) { delete static_cast<Fake*>(pointer); ++closes; }
rsf_backend_result version(void* pointer, uint64_t* id, const char** name)
{
    *id = static_cast<Fake*>(pointer)->family; *name = "fixture"; return RSF_BACKEND_OK;
}
const rsf_sr_provider provider{sizeof(provider), probe, open, plan, evaluate, release, close, version};
}
// Link-time substitutes keep controller failure/history tests independent of vendor DLLs.
extern "C" const rsf_sr_provider* rsf_fsr_sr_provider() { return &provider; }
extern "C" const rsf_sr_provider* rsf_xess_sr_provider() { return &provider; }
int main()
{
    char path[] = "copied-path";
    rsf_sr_session_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_SR_SESSION_ABI_VERSION;
    setup.open.struct_size = sizeof(setup.open); setup.open.abi_version = RSF_BACKEND_ABI_VERSION;
    setup.open.api = RSF_API_D3D12; setup.open.device = &setup;
    setup.open.output_width = setup.open.output_height = 128;
    setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = setup.xess_directory_utf8 = path;
    rsf_sr_session* session = nullptr;
    check(rsf_sr_session_create(&setup, &session) == RSF_BACKEND_OK, "create");
    // Poison caller storage after creation; every later provider open must see the owned copy.
    path[0] = 'X';
    check(rsf_sr_session_select(session, RSF_SR_FSR2, RSF_QUALITY_QUALITY, 0) == 0, "select FSR2");
    fail_open = true;
    check(rsf_sr_session_select(session, RSF_SR_FSR4, RSF_QUALITY_QUALITY, 0) == RSF_BACKEND_ERROR_NOT_SUPPORTED, "refuse FSR4");
    rsf_sr_session_status status{}; status.struct_size = sizeof(status);
    rsf_sr_session_get_status(session, &status);
    check(status.requested == RSF_SR_FSR4 && status.effective == RSF_SR_FSR2 && closes == 0, "refusal preserves effective backend");
    fail_open = false; fail_plan = true;
    check(rsf_sr_session_select(session, RSF_SR_XESS, RSF_QUALITY_QUALITY, 0) != 0 && closes == 1, "failed plan closes replacement only");
    fail_plan = false;
    check(rsf_sr_session_select(session, RSF_SR_XESS, RSF_QUALITY_QUALITY, 0) == 0 && closes == 2, "commit replacement closes original");
    rsf_frame_record record{};
    record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.frame_id = record.session_id = record.resource_generation = 1;
    record.render_width = record.render_height = 64;
    record.output_width = record.output_height = 128; record.frame_time_ms = 16;
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.camera.near_plane = 0.1f; record.camera.vertical_fov_radians = 1;
    rsf_sr_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record;
    frame.pre_exposure = frame.view_space_to_meters = frame.motion_scale_x = frame.motion_scale_y = 1;
    rsf_backend_resource* resources[] = {&frame.color, &frame.depth, &frame.motion, &frame.output};
    for (auto* resource : resources) {
        resource->struct_size = sizeof(*resource); resource->resource = &frame; resource->generation = 1;
        resource->width = resource->height = 128;
    }
    check(rsf_sr_session_evaluate(session, &setup, &frame) == 0 && reset_seen, "first frame reset");
    ++record.frame_id;
    check(rsf_sr_session_evaluate(session, &setup, &frame) == 0 && !reset_seen, "contiguous frame keeps history");
    check(rsf_sr_session_evaluate(session, &setup, &frame) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "duplicate refused");
    ++record.frame_id;
    check(rsf_sr_session_evaluate(session, &setup, &frame) == 0 && reset_seen, "history reset after refused input");
    record.frame_id += 2;
    check(rsf_sr_session_evaluate(session, &setup, &frame) == 0 && reset_seen, "frame gap reset");
    ++record.frame_id; ++record.resource_generation;
    check(rsf_sr_session_evaluate(session, &setup, &frame) == RSF_BACKEND_ERROR_STALE_RESOURCES, "stale generation refused");
    for (auto* resource : resources) resource->generation = record.resource_generation;
    check(rsf_sr_session_evaluate(session, &setup, &frame) == 0 && reset_seen, "new surfaces reset");
    ++record.frame_id; fail_frame = true;
    check(rsf_sr_session_evaluate(session, &setup, &frame) == RSF_BACKEND_ERROR_FEATURE_FAILED, "vendor failure reported");
    ++record.frame_id; fail_frame = false;
    check(rsf_sr_session_evaluate(session, &setup, &frame) == 0 && reset_seen, "vendor failure invalidates history");
    ++record.frame_id; frame.jitter_x = std::numeric_limits<float>::quiet_NaN();
    check(rsf_sr_session_evaluate(session, &setup, &frame) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "nonfinite jitter refused");
    frame.jitter_x = 0;
    check(rsf_sr_session_select(session, RSF_SR_NONE, RSF_QUALITY_NATIVE, 0) == 0, "disable");
    check(rsf_sr_session_evaluate(session, &setup, &frame) == RSF_BACKEND_ERROR_NOT_READY, "disabled evaluation refused");
    rsf_sr_session_destroy(session);
    check(closes == 3, "each session closed once");
    setup.abi_version++;
    check(rsf_sr_session_create(&setup, &session) == RSF_BACKEND_ERROR_ABI_MISMATCH, "wrong ABI");
    return passed ? 0 : 1;
}
