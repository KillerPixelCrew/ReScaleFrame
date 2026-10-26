// SPDX-License-Identifier: GPL-3.0-only
// Synthetic concrete providers test dispatch/admission only, without GPU/SDKs.
#include "../runtime/orchestrator/src/native_fg_vulkan_dispatch.cpp"
#include <cstdio>
#include <cstdlib>

struct rsf_ffxgvk {};
struct rsf_dlssgvk {};
namespace {
rsf_ffxgvk ffx;
rsf_dlssgvk dlss;
uint32_t ffx_creates = 0, dlss_creates = 0, destroys = 0;
uint32_t enabled = 0, generated_count = 1, calls = 0;
bool refuse_destroy = false, refuse_create = false, refuse_state = false;
const rsf_fgvk_setup* borrowed_setup = nullptr;
void require(bool value, const char* message) {
    if (!value) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}
rsf_backend_result retire() { if (refuse_destroy) return RSF_BACKEND_ERROR_NOT_READY; ++destroys; return RSF_BACKEND_OK; }
}
extern "C" {
rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup* setup, rsf_ffxgvk** out, uint64_t* chain) {
    ++ffx_creates; borrowed_setup = setup;
    if (refuse_create) return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = &ffx; *chain = 33; return RSF_BACKEND_OK;
}
rsf_backend_result rsf_dlssgvk_create(const rsf_fgvk_setup* setup, rsf_dlssgvk** out, uint64_t* chain) {
    ++dlss_creates; borrowed_setup = setup; *out = &dlss; *chain = 11; return RSF_BACKEND_OK;
}
int32_t rsf_ffxgvk_images(rsf_ffxgvk* value, uint32_t* count, uint64_t*) { require(value == &ffx, "FFX image owner"); *count = 3; return 0; }
int32_t rsf_dlssgvk_images(rsf_dlssgvk* value, uint32_t* count, uint64_t*) { require(value == &dlss, "DLSS image owner"); *count = 4; return 0; }
int32_t rsf_ffxgvk_acquire(rsf_ffxgvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    require(timeout == 123 && semaphore == 4 && fence == 5, "WSI acquire arguments preserved"); *index = 2; return 2;
}
int32_t rsf_dlssgvk_acquire(rsf_dlssgvk*, uint64_t, uint64_t, uint64_t, uint32_t* index) { *index = 1; return 1000001003; }
rsf_backend_result rsf_ffxgvk_enable(rsf_ffxgvk*, uint32_t value) { enabled = value; generated_count = 1; return RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_enable(rsf_dlssgvk*, uint32_t value, uint32_t count) {
    if (count > 3) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    enabled = value; generated_count = count; return RSF_BACKEND_OK;
}
rsf_backend_result rsf_dlssgvk_image_layout(const rsf_dlssgvk*, uint32_t* layout) { *layout = 1000001002; return RSF_BACKEND_OK; }
rsf_backend_result rsf_ffxgvk_prepare(rsf_ffxgvk*, void*, const rsf_fgvk_frame*) { calls |= 1; return RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_prepare(rsf_dlssgvk*, void*, const rsf_fgvk_frame*) { calls |= 16; return RSF_BACKEND_OK; }
rsf_backend_result rsf_ffxgvk_hudless(rsf_ffxgvk*, void*, const rsf_fgvk_frame*) { calls |= 2; return RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_hudless(rsf_dlssgvk*, void*, const rsf_fgvk_frame*) { calls |= 32; return RSF_BACKEND_OK; }
void rsf_ffxgvk_submitted(rsf_ffxgvk*, void* const*, uint32_t) { calls |= 4; }
void rsf_dlssgvk_submitted(rsf_dlssgvk*, void* const*, uint32_t) { calls |= 64; }
int32_t rsf_ffxgvk_present(rsf_ffxgvk*, const void*) { calls |= 8; return 1000001003; }
int32_t rsf_dlssgvk_present(rsf_dlssgvk*, const void*) { calls |= 128; return -1000001004; }
rsf_backend_result rsf_ffxgvk_get_status(rsf_ffxgvk*, rsf_fgvk_status* status) { status->prepared = 30; return RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_get_status(rsf_dlssgvk*, rsf_dlssgvk_status* status) {
    if (refuse_state) return RSF_BACKEND_ERROR_NOT_READY;
    status->presentation = {}; status->presentation.struct_size = sizeof(rsf_fgvk_status);
    status->presentation.abi_version = RSF_FGVK_ABI_VERSION; status->presentation.prepared = 10;
    status->max_generated_frames = 3; return RSF_BACKEND_OK;
}
rsf_backend_result rsf_ffxgvk_drain(rsf_ffxgvk*) { return refuse_destroy ? RSF_BACKEND_ERROR_NOT_READY : RSF_BACKEND_OK; }
rsf_backend_result rsf_dlssgvk_drain(rsf_dlssgvk*) { return refuse_destroy ? RSF_BACKEND_ERROR_NOT_READY : RSF_BACKEND_OK; }
rsf_backend_result rsf_ffxgvk_destroy(rsf_ffxgvk*) { return retire(); }
rsf_backend_result rsf_dlssgvk_destroy(rsf_dlssgvk*) { return retire(); }
}
#if defined(RSF_HAVE_FSR4_FG_VK)
struct rsf_fsr4gvk {};
namespace {
rsf_fsr4gvk fsr4;
uint32_t fsr4_creates = 0;
int32_t fsr4_create_result = 0;
bool fsr4_partial = false;
}
int32_t rsf_fsr4gvk_create(const rsf_fgvk_setup* setup, const rsf_fsr4gvk_options* options, rsf_fsr4gvk** out, uint64_t* chain) {
    ++fsr4_creates; borrowed_setup = setup;
    require(options->struct_size == sizeof(*options) && options->abi_version == 1 &&
        options->window == reinterpret_cast<void*>(17) && options->timeline_enabled == 1 && options->generated_count == 1 &&
        setup->graphics.runtime_directory_utf8[0] == 'F', "FSR4 forwards HWND, timeline and FidelityFX runtime directory");
    if (!fsr4_create_result || fsr4_partial) { *out = &fsr4; *chain = 44; }
    return fsr4_create_result;
}
int32_t rsf_fsr4gvk_destroy(rsf_fsr4gvk* value) { require(value == &fsr4, "FSR4 destruction owner"); return refuse_destroy ? -77 : 0; }
int32_t rsf_fsr4gvk_drain(rsf_fsr4gvk*) { return refuse_destroy ? -77 : 0; }
int32_t rsf_fsr4gvk_images(rsf_fsr4gvk* value, uint32_t* count, uint64_t*) { require(value == &fsr4, "FSR4 image owner"); *count = 3; return 0; }
int32_t rsf_fsr4gvk_acquire(rsf_fsr4gvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    require(timeout == 123 && semaphore == 4 && fence == 5, "FSR4 acquire preserves synchronization arguments"); *index = 1; return 2;
}
int32_t rsf_fsr4gvk_enable_options(rsf_fsr4gvk*, uint32_t value, uint32_t count) {
    require(count == 1, "FSR4 MFG never reaches concrete provider"); enabled = value; generated_count = count; return 0;
}
uint32_t rsf_fsr4gvk_max_generated(const rsf_fsr4gvk*) { return 1; }
int32_t rsf_fsr4gvk_prepare(rsf_fsr4gvk*, void*, const rsf_fgvk_frame*) { calls |= 4096; return 0; }
int32_t rsf_fsr4gvk_hudless(rsf_fsr4gvk*, void*, const rsf_fgvk_frame*) { calls |= 8192; return 0; }
void rsf_fsr4gvk_submitted(rsf_fsr4gvk*, void* const*, uint32_t) { calls |= 16384; }
int32_t rsf_fsr4gvk_present(rsf_fsr4gvk*, const void*) { calls |= 32768; return 1000001003; }
int32_t rsf_fsr4gvk_status(rsf_fsr4gvk*, rsf_fgvk_status* status) { status->prepared = 40; status->result = -77; return 0; }
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
struct rsf_xessgvk {};
namespace {
rsf_xessgvk xess;
uint32_t xess_creates = 0;
bool refuse_latency = false;
rsf_fgvk_latency_status xell{sizeof(xell), RSF_FGVK_LATENCY_ABI_VERSION, 1, 1, 0, 0, 0};
}
int32_t rsf_xessgvk_create(const rsf_fgvk_setup* setup, const rsf_xessgvk_options* options, rsf_xessgvk** out, uint64_t* chain) {
    ++xess_creates; borrowed_setup = setup;
    require(options->window == reinterpret_cast<void*>(17) && options->timeline_enabled == 1 &&
        options->intel_runtime_directory[0] == u'D' && options->generated_count == 3, "XeSS host lease and MFG count forwarded");
    *out = &xess; *chain = 55; return 0;
}
int32_t rsf_xessgvk_destroy(rsf_xessgvk*) { return refuse_destroy ? -77 : 0; }
int32_t rsf_xessgvk_drain(rsf_xessgvk*) { return refuse_destroy ? 2 : 0; }
int32_t rsf_xessgvk_images(rsf_xessgvk*, uint32_t* count, uint64_t*) { *count = 3; return 0; }
int32_t rsf_xessgvk_acquire(rsf_xessgvk*, uint64_t, uint64_t, uint64_t, uint32_t* index) { *index = 2; return 2; }
int32_t rsf_xessgvk_enable_options(rsf_xessgvk*, uint32_t value, uint32_t count) {
    if (count > 5) return -77;
    enabled = value; generated_count = count; return 0;
}
uint32_t rsf_xessgvk_max_generated(const rsf_xessgvk*) { return 5; }
int32_t rsf_xessgvk_prepare(rsf_xessgvk*, void*, const rsf_fgvk_frame*) { calls |= 256; return 0; }
int32_t rsf_xessgvk_hudless(rsf_xessgvk*, void*, const rsf_fgvk_frame*) { calls |= 512; return 0; }
void rsf_xessgvk_submitted(rsf_xessgvk*, void* const*, uint32_t) { calls |= 1024; }
int32_t rsf_xessgvk_present(rsf_xessgvk*, const void*) { calls |= 2048; return 1000001003; }
int32_t rsf_xessgvk_status(rsf_xessgvk*, rsf_fgvk_status* status) { status->prepared = 50; status->result = -77; return 0; }
int32_t rsf_xessgvk_latency_options(rsf_xessgvk*, uint32_t mode, uint32_t interval) {
    require(interval == 5000, "XeLL frame interval forwarded");
    xell.result = refuse_latency ? -77 : 0; if (!xell.result) xell.mode = mode; return xell.result;
}
int32_t rsf_xessgvk_sleep(rsf_xessgvk*, uint32_t frame) {
    require(frame == 42, "XeLL source frame forwarded"); xell.result = refuse_latency ? -77 : 0;
    if (!xell.result) ++xell.sleep_calls; return xell.result;
}
int32_t rsf_xessgvk_latency(rsf_xessgvk*, uint32_t frame, uint32_t marker) {
    require(frame == 42 && marker <= 6, "XeLL original marker identity forwarded"); xell.result = refuse_latency ? -77 : 0;
    if (!xell.result) ++xell.marker_calls; return xell.result;
}
int32_t rsf_xessgvk_latency_status(rsf_xessgvk*, rsf_fgvk_latency_status* status) { *status = xell; return 0; }
#endif
int main() {
    rsf_fgvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_FGVK_ABI_VERSION;
    setup.graphics.struct_size = sizeof(setup.graphics); setup.graphics.abi_version = RSF_SRVK_ABI_VERSION;
    setup.graphics.backend = 5; // SR selection must not choose the FG provider.
    rsf_fgvk* owner = nullptr; uint64_t chain = 0;
    require(rsf_fgvk_create_for_backend(nullptr, 1, 1, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !owner && !chain, "null setup clears outputs");
    --setup.struct_size;
    require(rsf_fgvk_create(&setup, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short setup refused"); ++setup.struct_size;
    ++setup.abi_version;
    require(rsf_fgvk_create(&setup, &owner, &chain) == RSF_BACKEND_ERROR_ABI_MISMATCH, "setup ABI refused"); --setup.abi_version;
    auto invalid = setup; ++invalid.abi_version; invalid.graphics = {};
    require(rsf_fgvk_create(&invalid, &owner, &chain) == RSF_BACKEND_ERROR_ABI_MISMATCH,
        "outer ABI mismatch takes precedence over embedded size");
    invalid = setup; --invalid.graphics.struct_size; ++invalid.graphics.abi_version;
    require(rsf_fgvk_create(&invalid, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT,
        "embedded size takes precedence over embedded ABI");
    invalid = setup; ++invalid.graphics.abi_version;
    require(rsf_fgvk_create(&invalid, &owner, &chain) == RSF_BACKEND_ERROR_ABI_MISMATCH, "embedded ABI refused");
    require(rsf_fgvk_create_for_backend(&setup, 2, 1, &owner, &chain) == RSF_BACKEND_ERROR_NOT_SUPPORTED && !ffx_creates && !dlss_creates,
        "unsupported FG provider never allocates a concrete owner");
    require(rsf_fgvk_create_for_backend(&setup, 3, 2, &owner, &chain) == RSF_BACKEND_ERROR_NOT_SUPPORTED && !ffx_creates,
        "FFX unsupported MFG request never clamps or allocates");
    require(rsf_fgvk_create(&setup, &owner, &chain) == RSF_BACKEND_OK && owner && chain == 33 && borrowed_setup == &setup && !enabled,
        "legacy create preserves FFX/count1 and borrowed setup");
    uint32_t value = 0;
    require(rsf_fgvk_backend(owner, &value) == RSF_BACKEND_OK && value == 3, "legacy FG independent of XeSS SR selection");
    require(rsf_fgvk_image_layout(owner, &value) == RSF_BACKEND_OK && value == 5, "FFX image layout");
    require(rsf_fgvk_max_generated(owner, &value) == RSF_BACKEND_OK && value == 1, "FFX count limit");
    rsf_fgvk* second = nullptr; uint64_t second_chain = 0;
    require(rsf_fgvk_create_for_backend(&setup, 1, 3, &second, &second_chain) == RSF_BACKEND_ERROR_NOT_READY && !second && !second_chain && !dlss_creates,
        "single admission spans both providers");
    require(rsf_fgvk_enable_options(owner, 1, 2) == RSF_BACKEND_ERROR_NOT_SUPPORTED && !enabled, "invalid FFX options preserve request");
    require(rsf_fgvk_enable(owner, 1) == RSF_BACKEND_OK && enabled && generated_count == 1, "legacy enable preserves count1");
    void* command = reinterpret_cast<void*>(1); rsf_fgvk_frame frame{};
    require(rsf_fgvk_prepare(owner, command, &frame) == RSF_BACKEND_OK && rsf_fgvk_hudless(owner, command, &frame) == RSF_BACKEND_OK, "FFX recording routed");
    rsf_fgvk_submitted(owner, &command, 1);
    require(rsf_fgvk_present(owner, &frame) == 1000001003 && calls == 15, "FFX submitted/Present and result routed");
    require(rsf_fgvk_acquire(owner, 123, 4, 5, &value) == 2 && value == 2, "positive Vulkan timeout preserved");
    rsf_fgvk_status status{}; status.struct_size = sizeof(status); status.abi_version = RSF_FGVK_ABI_VERSION;
    require(rsf_fgvk_get_status(owner, &status) == RSF_BACKEND_OK && status.prepared == 30, "FFX status routed");
    refuse_destroy = true;
    require(rsf_fgvk_destroy(owner) == RSF_BACKEND_ERROR_NOT_READY && rsf_fgvk_backend(owner, &value) == RSF_BACKEND_OK && value == 3,
        "failed destroy retains concrete owner");
    require(rsf_fgvk_create_for_backend(&setup, 1, 3, &second, &second_chain) == RSF_BACKEND_ERROR_NOT_READY && !dlss_creates,
        "failed destroy retains global admission");
    refuse_destroy = false; require(rsf_fgvk_destroy(owner) == RSF_BACKEND_OK, "FFX retirement retry succeeds");
    require(rsf_fgvk_create_for_backend(&setup, 1, 3, &owner, &chain) == RSF_BACKEND_OK && chain == 11 && generated_count == 3,
        "DLSS creation forwards MFG count3");
    require(rsf_fgvk_enable(owner, 1) == RSF_BACKEND_OK && enabled && generated_count == 3, "legacy enable retains selected DLSS MFG count");
    require(rsf_fgvk_enable_options(owner, 0, 4) == RSF_BACKEND_ERROR_NOT_SUPPORTED && enabled && generated_count == 3,
        "unsupported DLSS options preserve both count and enabled state");
    require(rsf_fgvk_image_layout(owner, &value) == RSF_BACKEND_OK && value == 1000001002, "DLSS proxy image layout forwarded");
    require(rsf_fgvk_max_generated(owner, &value) == RSF_BACKEND_OK && value == 3, "DLSS real reported limit forwarded");
    status.prepared = 0; require(rsf_fgvk_get_status(owner, &status) == RSF_BACKEND_OK && status.prepared == 10 && status.abi_version == 1, "DLSS nested ABI1 status copied");
    refuse_state = true; value = 77;
    require(rsf_fgvk_max_generated(owner, &value) == RSF_BACKEND_ERROR_NOT_READY && value == 77, "failed capability query preserves output"); refuse_state = false;
    calls = 0; (void)rsf_fgvk_prepare(owner, command, &frame); (void)rsf_fgvk_hudless(owner, command, &frame); rsf_fgvk_submitted(owner, &command, 1);
    require(rsf_fgvk_present(owner, &frame) == -1000001004 && calls == 240, "DLSS recording/submission/WSI error forwarded");
    require(rsf_fgvk_destroy(owner) == RSF_BACKEND_OK, "DLSS retirement succeeds");
    refuse_destroy = true; owner = nullptr;
    require(rsf_fgvk_create_for_backend(&setup, 1, 4, &owner, &chain) == RSF_BACKEND_ERROR_NOT_READY && owner && chain == 11,
        "failed unsupported-count cleanup returns retained owner for retry");
    require(rsf_fgvk_create(&setup, &second, &second_chain) == RSF_BACKEND_ERROR_NOT_READY && !second,
        "partial failed creation retains global admission");
    refuse_destroy = false; require(rsf_fgvk_destroy(owner) == RSF_BACKEND_OK, "partial create cleanup retry succeeds");
    refuse_create = true; require(rsf_fgvk_create(&setup, &owner, &chain) == RSF_BACKEND_ERROR_INIT_FAILED && !owner && !chain,
        "concrete create error releases clean generic admission"); refuse_create = false;
    require(rsf_fgvk_create(&setup, &owner, &chain) == RSF_BACKEND_OK && rsf_fgvk_destroy(owner) == RSF_BACKEND_OK,
        "creation resumes after clean factory failure");
    owner = nullptr;
    require(rsf_fgvk_create_for_backend(&setup, 4, 3, &owner, &chain) == RSF_BACKEND_ERROR_NOT_SUPPORTED && !owner && !chain,
        "FSR4 rejects unsupported MFG before concrete creation or fallback");
#if defined(RSF_HAVE_FSR4_FG_VK)
    require(rsf_fgvk_create_for_backend(&setup, 4, 1, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !fsr4_creates,
        "FSR4 needs explicit host contract");
    rsf_fgvk_host_options fsr4_host{sizeof(fsr4_host), RSF_FGVK_HOST_ABI_VERSION, reinterpret_cast<void*>(17), 1, nullptr};
    require(rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !fsr4_creates,
        "FSR4 requires FidelityFX runtime directory rather than Intel host path");
    setup.graphics.runtime_directory_utf8 = "FidelityFX";
    --fsr4_host.struct_size;
    require(rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short FSR4 host refused");
    ++fsr4_host.struct_size; ++fsr4_host.abi_version;
    require(rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &owner, &chain) == RSF_BACKEND_ERROR_ABI_MISMATCH, "FSR4 host ABI refused");
    --fsr4_host.abi_version; fsr4_host.timeline_enabled = 0;
    require(rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "FSR4 external timeline must be enabled");
    fsr4_host.timeline_enabled = 1;
    const auto before_fsr4_ffx = ffx_creates;
    fsr4_create_result = RSF_FSR4FG_NOT_SUPPORTED;
    require(rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &owner, &chain) == RSF_BACKEND_ERROR_NOT_SUPPORTED &&
        !owner && !chain && ffx_creates == before_fsr4_ffx, "missing exact FSR4 provider never falls back to FFX3");
    fsr4_create_result = 0;
    require(rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &owner, &chain) == RSF_BACKEND_OK && chain == 44 &&
        borrowed_setup == &setup && !enabled && generated_count == 1, "FSR4 create is SR-independent and starts Off");
    require(rsf_fgvk_backend(owner, &value) == RSF_BACKEND_OK && value == 4 &&
        rsf_fgvk_image_layout(owner, &value) == RSF_BACKEND_OK && value == 5 &&
        rsf_fgvk_max_generated(owner, &value) == RSF_BACKEND_OK && value == 1, "FSR4 owner, shader-read layout and count-one limit");
    require(rsf_fgvk_enable(owner, 1) == RSF_BACKEND_OK && enabled && generated_count == 1 &&
        rsf_fgvk_enable_options(owner, 0, 2) == RSF_BACKEND_ERROR_NOT_SUPPORTED && enabled,
        "FSR4 invalid options retain prior active count-one state");
    calls = 0; (void)rsf_fgvk_prepare(owner, command, &frame); (void)rsf_fgvk_hudless(owner, command, &frame);
    rsf_fgvk_submitted(owner, &command, 1);
    require(rsf_fgvk_present(owner, &frame) == 1000001003 && calls == 61440 &&
        rsf_fgvk_acquire(owner, 123, 4, 5, &value) == 2 && value == 1 &&
        rsf_fgvk_images(owner, &value, nullptr) == 0 && value == 3, "FSR4 recording and all WSI routes preserve results");
    require(rsf_fgvk_get_status(owner, &status) == RSF_BACKEND_OK && status.prepared == 40 && status.result == -77,
        "FSR4 status preserves private SDK result");
    refuse_destroy = true;
    require(rsf_fgvk_drain(owner) == RSF_BACKEND_ERROR_FEATURE_FAILED && rsf_fgvk_destroy(owner) == RSF_BACKEND_ERROR_FEATURE_FAILED &&
        rsf_fgvk_create(&setup, &second, &second_chain) == RSF_BACKEND_ERROR_NOT_READY,
        "FSR4 failed retirement preserves owner and process admission");
    refuse_destroy = false; require(rsf_fgvk_destroy(owner) == RSF_BACKEND_OK, "FSR4 retirement retry succeeds");
    fsr4_create_result = -77; fsr4_partial = refuse_destroy = true;
    require(rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &owner, &chain) == RSF_BACKEND_ERROR_FEATURE_FAILED && owner && chain == 44,
        "FSR4 failed partial cleanup returns retained owner and chain");
    refuse_destroy = false; require(rsf_fgvk_destroy(owner) == RSF_BACKEND_OK, "FSR4 partial owner retires on retry");
    fsr4_partial = false; fsr4_create_result = 0;
#else
    require(rsf_fgvk_create_for_backend(&setup, 4, 1, &owner, &chain) == RSF_BACKEND_ERROR_NOT_COMPILED && !owner && !chain,
        "missing FSR4 build is explicit without provider fallback");
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    require(rsf_fgvk_create_for_backend(&setup, 5, 3, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !xess_creates,
        "legacy creation cannot select XeSS without its host contract");
    const uint16_t intel[] = {u'D', u':', 0};
    rsf_fgvk_host_options host{sizeof(host), RSF_FGVK_HOST_ABI_VERSION, reinterpret_cast<void*>(17), 1, intel};
    --host.struct_size;
    require(rsf_fgvk_create_with_host(&setup, 5, 3, &host, &owner, &chain) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short host contract refused"); ++host.struct_size;
    ++host.abi_version;
    require(rsf_fgvk_create_with_host(&setup, 5, 3, &host, &owner, &chain) == RSF_BACKEND_ERROR_ABI_MISMATCH, "host ABI mismatch refused"); --host.abi_version;
    setup.graphics.backend = 1;
    require(rsf_fgvk_create_with_host(&setup, 5, 3, &host, &owner, &chain) == RSF_BACKEND_OK && chain == 55 && generated_count == 3,
        "explicit XeSS FG remains independent of DLSS SR and preserves requested count");
    require(rsf_fgvk_backend(owner, &value) == RSF_BACKEND_OK && value == 5 &&
        rsf_fgvk_image_layout(owner, &value) == RSF_BACKEND_OK && value == 5 &&
        rsf_fgvk_max_generated(owner, &value) == RSF_BACKEND_OK && value == 5, "XeSS owner/layout/native count dispatched");
    require(rsf_fgvk_create(&setup, &second, &second_chain) == RSF_BACKEND_ERROR_NOT_READY, "XeSS shares process owner admission");
    require(rsf_fgvk_enable(owner, 1) == RSF_BACKEND_OK && generated_count == 3 && enabled, "legacy XeSS enable preserves MFG count");
    require(rsf_fgvk_enable_options(owner, 0, 6) == RSF_BACKEND_ERROR_FEATURE_FAILED && enabled && generated_count == 3, "XeSS unsupported count is never clamped");
    calls = 0; (void)rsf_fgvk_prepare(owner, command, &frame); (void)rsf_fgvk_hudless(owner, command, &frame); rsf_fgvk_submitted(owner, &command, 1);
    require(rsf_fgvk_present(owner, &frame) == 1000001003 && calls == 3840 &&
        rsf_fgvk_acquire(owner, 0, 0, 0, &value) == 2 && value == 2, "XeSS WSI results and submission routes preserved");
    require(rsf_fgvk_get_status(owner, &status) == RSF_BACKEND_OK && status.prepared == 50 && status.result == -77,
        "native XeSS errors remain in bounded status");
    require(rsf_fgvk_latency_options(owner, 2, 5000) == RSF_BACKEND_ERROR_NOT_SUPPORTED && xell.mode == 1, "XeLL Boost refused without state mutation");
    require(rsf_fgvk_latency_options(owner, 0, 5000) == RSF_BACKEND_OK && xell.mode == 0, "XeLL Off options forwarded");
    require(rsf_fgvk_latency_sleep(owner, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT &&
        rsf_fgvk_latency_sleep(owner, uint64_t(UINT32_MAX) + 1) == RSF_BACKEND_ERROR_INVALID_ARGUMENT &&
        rsf_fgvk_latency_marker(owner, 42, 7) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "XeLL domain rejects invalid or truncated identities");
    require(rsf_fgvk_latency_sleep(owner, 42) == RSF_BACKEND_OK && rsf_fgvk_latency_marker(owner, 42, 6) == RSF_BACKEND_OK,
        "XeLL sleep and supported marker reach private owner");
    refuse_latency = true;
    require(rsf_fgvk_latency_sleep(owner, 42) == RSF_BACKEND_ERROR_FEATURE_FAILED, "XeLL native sleep failure preserved as public refusal");
    rsf_fgvk_latency_status latency{}; latency.struct_size = sizeof(latency); latency.abi_version = RSF_FGVK_LATENCY_ABI_VERSION;
    require(rsf_fgvk_latency_get_status(owner, &latency) == RSF_BACKEND_OK && latency.sleep_calls == 1 && latency.marker_calls == 1 &&
        latency.result == -77 && latency.mode == 0, "XeLL counts cover accepted calls and retain exact native error");
    refuse_latency = false; refuse_destroy = true;
    require(rsf_fgvk_drain(owner) == RSF_BACKEND_ERROR_FEATURE_FAILED && rsf_fgvk_destroy(owner) == RSF_BACKEND_ERROR_FEATURE_FAILED &&
        rsf_fgvk_create(&setup, &second, &second_chain) == RSF_BACKEND_ERROR_NOT_READY, "XeSS failed retirement preserves process owner");
    refuse_destroy = false; require(rsf_fgvk_destroy(owner) == RSF_BACKEND_OK, "XeSS owner retry retires safely");
#else
    require(rsf_fgvk_create_for_backend(&setup, 5, 3, &owner, &chain) == RSF_BACKEND_ERROR_NOT_COMPILED && !owner && !chain,
        "XeSS missing SDK is explicit and does not allocate");
#endif
    require(rsf_fgvk_destroy(nullptr) == RSF_BACKEND_OK && rsf_fgvk_drain(nullptr) == RSF_BACKEND_OK, "null retirement remains idempotent");
    std::puts("Synthetic Vulkan FG provider dispatch and ownership checks passed");
}
