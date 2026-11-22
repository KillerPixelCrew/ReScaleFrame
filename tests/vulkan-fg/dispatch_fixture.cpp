// SPDX-License-Identifier: MIT
// Synthetic concrete providers test dispatch/admission only, without GPU/SDKs.
#include "../../runtime/orchestrator/src/native_fg_vulkan_dispatch.cpp"
#include "../vulkan_presentation_fixture.h"
#include "dispatch_abi.h"
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <thread>
#include <condition_variable>
#include <chrono>

struct rsf_ffxgvk {};
struct rsf_dlssgvk {};
namespace {
rsf_ffxgvk ffx;
rsf_dlssgvk dlss;
rsf_dispatch_fixture_state fixture{};
void require(bool value, const char* message) {
    if (!value) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}
rsf_backend_result retire() { if (fixture.refuse_destroy) return RSF_BACKEND_ERROR_NOT_READY; ++fixture.destroys; return RSF_BACKEND_OK; }
}
extern "C" {
rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup* setup, rsf_ffxgvk** out, uint64_t* chain) {
    ++fixture.ffx_creates; fixture.borrowed_setup = setup;
    if (fixture.refuse_create) return RSF_BACKEND_ERROR_INIT_FAILED;
    *chain = 33;
    if (fixture.missing_owner) return RSF_BACKEND_OK;
    *out = &ffx;
    return fixture.fail_after_create ? RSF_BACKEND_ERROR_INIT_FAILED : RSF_BACKEND_OK;
}
rsf_backend_result rsf_dlssgvk_create(const rsf_fgvk_setup* setup, rsf_dlssgvk** out, uint64_t* chain) {
    ++fixture.dlss_creates; fixture.borrowed_setup = setup; *out = &dlss; *chain = 11; return RSF_BACKEND_OK;
}
int32_t rsf_ffxgvk_images(rsf_ffxgvk* value, uint32_t* count, uint64_t*) { require(value == &ffx, "FFX image owner"); *count = 3; return 0; }
int32_t rsf_dlssgvk_images(rsf_dlssgvk* value, uint32_t* count, uint64_t*) { require(value == &dlss, "DLSS image owner"); *count = 4; return 0; }
int32_t rsf_ffxgvk_acquire(rsf_ffxgvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    require(timeout == 123 && semaphore == 4 && fence == 5, "WSI acquire arguments preserved"); *index = 2; return 2;
}
int32_t rsf_dlssgvk_acquire(rsf_dlssgvk*, uint64_t, uint64_t, uint64_t, uint32_t* index) { *index = 1; return 1000001003; }
rsf_backend_result rsf_ffxgvk_enable(rsf_ffxgvk*, uint32_t value) { fixture.enabled = value; fixture.generated_count = 1; return RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_enable(rsf_dlssgvk*, uint32_t value, uint32_t count) {
    if (count > 3) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    fixture.enabled = value; fixture.generated_count = count; return RSF_BACKEND_OK;
}
rsf_backend_result rsf_dlssgvk_image_layout(const rsf_dlssgvk*, uint32_t* layout) { *layout = 1000001002; return RSF_BACKEND_OK; }
rsf_backend_result rsf_ffxgvk_prepare(rsf_ffxgvk*, void*, const rsf_fgvk_frame*) { fixture.calls |= 1; return RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_prepare(rsf_dlssgvk*, void*, const rsf_fgvk_frame*) { fixture.calls |= 16; return RSF_BACKEND_OK; }
rsf_backend_result rsf_ffxgvk_hudless(rsf_ffxgvk*, void*, const rsf_fgvk_frame*) { fixture.calls |= 2; return RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_hudless(rsf_dlssgvk*, void*, const rsf_fgvk_frame*) { fixture.calls |= 32; return RSF_BACKEND_OK; }
void rsf_ffxgvk_submitted(rsf_ffxgvk*, void* const*, uint32_t) { fixture.calls |= 4; }
void rsf_dlssgvk_submitted(rsf_dlssgvk*, void* const*, uint32_t) { fixture.calls |= 64; }
int32_t rsf_ffxgvk_present(rsf_ffxgvk*, const void*) { fixture.calls |= 8; return 1000001003; }
int32_t rsf_dlssgvk_present(rsf_dlssgvk*, const void*) { fixture.calls |= 128; return -1000001004; }
int32_t rsf_ffxgvk_present_source(rsf_ffxgvk* owner, const void* info, const rsf_frame_record* source) {
    fixture.presented_source = source; ++fixture.explicit_presents; return rsf_ffxgvk_present(owner, info);
}
int32_t rsf_dlssgvk_present_source(rsf_dlssgvk* owner, const void* info, const rsf_frame_record* source) {
    fixture.presented_source = source; ++fixture.explicit_presents; return rsf_dlssgvk_present(owner, info);
}
rsf_backend_result rsf_ffxgvk_get_status(rsf_ffxgvk*, rsf_fgvk_status* status) { status->prepared = 30; return RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_get_status(rsf_dlssgvk*, rsf_dlssgvk_status* status) {
    if (fixture.refuse_state) return RSF_BACKEND_ERROR_NOT_READY;
    status->presentation = {}; status->presentation.struct_size = sizeof(rsf_fgvk_status);
    status->presentation.abi_version = RSF_FGVK_ABI_VERSION; status->presentation.prepared = 10;
    status->max_generated_frames = 3; return RSF_BACKEND_OK;
}
rsf_backend_result rsf_ffxgvk_drain(rsf_ffxgvk*) {
    if (fixture.fail_drain) {
        const rsf::latency::PresentationShutdown shutdown;
        require(!shutdown, "latency SDK shutdown cannot overlap a provider drain");
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    return fixture.refuse_destroy ? RSF_BACKEND_ERROR_NOT_READY : RSF_BACKEND_OK;
}
rsf_backend_result rsf_dlssgvk_drain(rsf_dlssgvk*) { return fixture.refuse_destroy ? RSF_BACKEND_ERROR_NOT_READY : RSF_BACKEND_OK; }
rsf_backend_result rsf_ffxgvk_destroy(rsf_ffxgvk*) { return retire(); }
rsf_backend_result rsf_dlssgvk_destroy(rsf_dlssgvk*) { return retire(); }
}
#if defined(RSF_HAVE_FSR4_FG_VK)
struct rsf_fsr4gvk {};
namespace {
rsf_fsr4gvk fsr4;
}
int32_t rsf_fsr4gvk_create(const rsf_fgvk_setup* setup, const rsf_fsr4gvk_options* options, rsf_fsr4gvk** out, uint64_t* chain) {
    ++fixture.fsr4_creates; fixture.borrowed_setup = setup;
    require(options->struct_size == sizeof(*options) && options->abi_version == 1 &&
        options->window == reinterpret_cast<void*>(17) && options->timeline_enabled == 1 && options->generated_count == 1 &&
        setup->graphics.runtime_directory_utf8[0] == 'F', "FSR4 forwards HWND, timeline and FidelityFX runtime directory");
    if (!fixture.fsr4_create_result || fixture.fsr4_partial) { *out = &fsr4; *chain = 44; }
    return fixture.fsr4_create_result;
}
int32_t rsf_fsr4gvk_destroy(rsf_fsr4gvk* value) { require(value == &fsr4, "FSR4 destruction owner"); return fixture.refuse_destroy ? -77 : 0; }
int32_t rsf_fsr4gvk_drain(rsf_fsr4gvk*) { return fixture.refuse_destroy ? -77 : 0; }
int32_t rsf_fsr4gvk_images(rsf_fsr4gvk* value, uint32_t* count, uint64_t*) { require(value == &fsr4, "FSR4 image owner"); *count = 3; return 0; }
int32_t rsf_fsr4gvk_acquire(rsf_fsr4gvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    require(timeout == 123 && semaphore == 4 && fence == 5, "FSR4 acquire preserves synchronization arguments"); *index = 1; return 2;
}
int32_t rsf_fsr4gvk_enable_options(rsf_fsr4gvk*, uint32_t value, uint32_t count) {
    require(count == 1, "FSR4 MFG never reaches concrete provider"); fixture.enabled = value; fixture.generated_count = count; return 0;
}
uint32_t rsf_fsr4gvk_max_generated(const rsf_fsr4gvk*) { return 1; }
int32_t rsf_fsr4gvk_prepare(rsf_fsr4gvk*, void*, const rsf_fgvk_frame*) { fixture.calls |= 4096; return 0; }
int32_t rsf_fsr4gvk_hudless(rsf_fsr4gvk*, void*, const rsf_fgvk_frame*) { fixture.calls |= 8192; return 0; }
void rsf_fsr4gvk_submitted(rsf_fsr4gvk*, void* const*, uint32_t) { fixture.calls |= 16384; }
int32_t rsf_fsr4gvk_present(rsf_fsr4gvk*, const void*) { fixture.calls |= 32768; return 1000001003; }
int32_t rsf_fsr4gvk_present_source(rsf_fsr4gvk* owner, const void* info, const rsf_frame_record* source) {
    fixture.presented_source = source; ++fixture.explicit_presents; return rsf_fsr4gvk_present(owner, info);
}
int32_t rsf_fsr4gvk_status(rsf_fsr4gvk*, rsf_fgvk_status* status) { status->prepared = 40; status->result = -77; return 0; }
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
struct rsf_xessgvk {};
namespace {
rsf_xessgvk xess;
std::mutex sleeper_guard;std::condition_variable sleeper_changed;bool block_sleep=false,entered_sleep=false,finish_sleep=false;
}
int32_t rsf_xessgvk_create(const rsf_fgvk_setup* setup, const rsf_xessgvk_options* options, rsf_xessgvk** out, uint64_t* chain) {
    ++fixture.xess_creates; fixture.borrowed_setup = setup;
    require(options->window == reinterpret_cast<void*>(17) && options->timeline_enabled == 1 &&
        options->intel_runtime_directory[0] == u'D' && options->generated_count == 3, "XeSS host lease and MFG count forwarded");
    *out = &xess; *chain = 55; return 0;
}
int32_t rsf_xessgvk_destroy(rsf_xessgvk*) { return fixture.refuse_destroy ? -77 : 0; }
int32_t rsf_xessgvk_drain(rsf_xessgvk*) { return fixture.refuse_destroy ? 2 : 0; }
int32_t rsf_xessgvk_images(rsf_xessgvk*, uint32_t* count, uint64_t*) { *count = 3; return 0; }
int32_t rsf_xessgvk_acquire(rsf_xessgvk*, uint64_t, uint64_t, uint64_t, uint32_t* index) { *index = 2; return 2; }
int32_t rsf_xessgvk_enable_options(rsf_xessgvk*, uint32_t value, uint32_t count) {
    if (count > 5) return -77;
    fixture.enabled = value; fixture.generated_count = count; return 0;
}
uint32_t rsf_xessgvk_max_generated(const rsf_xessgvk*) { return 5; }
int32_t rsf_xessgvk_prepare(rsf_xessgvk*, void*, const rsf_fgvk_frame*) { fixture.calls |= 256; return 0; }
int32_t rsf_xessgvk_hudless(rsf_xessgvk*, void*, const rsf_fgvk_frame*) { fixture.calls |= 512; return 0; }
void rsf_xessgvk_submitted(rsf_xessgvk*, void* const*, uint32_t) { fixture.calls |= 1024; }
int32_t rsf_xessgvk_present(rsf_xessgvk*, const void*) { fixture.calls |= 2048; return 1000001003; }
int32_t rsf_xessgvk_present_source(rsf_xessgvk* owner, const void* info, const rsf_frame_record* source) {
    fixture.presented_source = source; ++fixture.explicit_presents; return rsf_xessgvk_present(owner, info);
}
int32_t rsf_xessgvk_status(rsf_xessgvk*, rsf_fgvk_status* status) { status->prepared = 50; status->result = -77; return 0; }
int32_t rsf_xessgvk_latency_options(rsf_xessgvk*, uint32_t mode, uint32_t interval) {
    require(interval == 5000, "XeLL frame interval forwarded");
    fixture.xell.result = fixture.refuse_latency ? -77 : 0; if (!fixture.xell.result) fixture.xell.mode = mode; return fixture.xell.result;
}
int32_t rsf_xessgvk_sleep(rsf_xessgvk*, uint32_t frame) {
    require(frame == 42, "XeLL source frame forwarded"); fixture.xell.result = fixture.refuse_latency ? -77 : 0;
    if (!fixture.xell.result) ++fixture.xell.sleep_calls;
    std::unique_lock lock(sleeper_guard);
    if(block_sleep){entered_sleep=true;sleeper_changed.notify_all();sleeper_changed.wait(lock,[]{return finish_sleep;});}
    return fixture.xell.result;
}
int32_t rsf_xessgvk_latency(rsf_xessgvk*, uint32_t frame, uint32_t marker) {
    require(frame == 42 && marker <= 6, "XeLL original marker identity forwarded"); fixture.xell.result = fixture.refuse_latency ? -77 : 0;
    if (!fixture.xell.result) ++fixture.xell.marker_calls; return fixture.xell.result;
}
int32_t rsf_xessgvk_latency_status(rsf_xessgvk*, rsf_fgvk_latency_status* status) { *status = fixture.xell; return 0; }
#endif

extern "C" rsf_dispatch_fixture_state* rsf_fixture_dispatch_state() { return &fixture; }
extern "C" void rsf_fixture_block_sleep() {
#if defined(RSF_HAVE_XESS_FG_VK)
    const std::lock_guard lock(sleeper_guard);
    block_sleep = true;
    entered_sleep = finish_sleep = false;
#endif
}
extern "C" bool rsf_fixture_wait_sleep() {
#if defined(RSF_HAVE_XESS_FG_VK)
    std::unique_lock lock(sleeper_guard);
    return sleeper_changed.wait_for(lock, std::chrono::seconds(2), [] { return entered_sleep; });
#else
    return false;
#endif
}
extern "C" void rsf_fixture_release_sleep() {
#if defined(RSF_HAVE_XESS_FG_VK)
    const std::lock_guard lock(sleeper_guard);
    block_sleep = false;
    finish_sleep = true;
    sleeper_changed.notify_all();
#endif
}
int main() {
    fixture.generated_count = 1;
    fixture.xell = {sizeof(fixture.xell), RSF_FGVK_LATENCY_ABI_VERSION, 1, 1, 0, 0, 0};
#if defined(RSF_HAVE_FSR4_FG_VK)
    return rsf_fixture_vulkan_dispatch_run(true, false);
#elif defined(RSF_HAVE_XESS_FG_VK)
    return rsf_fixture_vulkan_dispatch_run(false, true);
#else
    return rsf_fixture_vulkan_dispatch_run(false, false);
#endif
}
