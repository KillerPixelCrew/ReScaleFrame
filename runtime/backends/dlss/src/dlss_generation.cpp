// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/streamline_host.h>
#include "streamline_frame.h"
#include "reflex_audit.h"
#include "../../common/fg_helpers.h"
#include <wrl/client.h>
#include <dxgi1_6.h>
#include <mutex>
#include <new>
#include <cstring>
#include <atomic>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
#include <sl_dlss_g.h>
#include <sl_reflex.h>
#include <sl_pcl.h>
#include <sl_matrix_helpers.h>
#include <chrono>
#if RSF_HAVE_NVAPI_DIAGNOSTICS
#include <nvapi.h>
#endif
bool rsf_dlss_verify_runtime_signature(const wchar_t* path);
using Microsoft::WRL::ComPtr;
struct rsf_streamline_host {
    HMODULE module = nullptr;
    bool initialized = false;
    uint32_t engine_type = 0;
    uint32_t profile = RSF_SL_PROFILE_DLSS_FG;
    bool signature_verified = false, development_runtime = false;
    rsf_backend_log_fn log = nullptr;
    void* log_user = nullptr;
    uint32_t borrowers = 0;
    ComPtr<ID3D12Device> device, native_device;
    ComPtr<ID3D12CommandQueue> queue, native_queue;
    ComPtr<IDXGIFactory2> factory;
    wchar_t directory[1024]{};
    const wchar_t* paths[1]{};
    sl::Feature features[4]{};
    char engine_version[128]{}, project_id[256]{};
    PFun_slInit* init{};
    PFun_slShutdown* shutdown{};
    PFun_slUpgradeInterface* upgrade{};
    PFun_slGetNativeInterface* native{};
    PFun_slSetD3DDevice* set_device{};
    PFun_slIsFeatureSupported* supported{};
    PFun_slGetFeatureFunction* function{};
    PFun_slGetFeatureVersion* version{};
    PFun_slSetFeatureLoaded* loaded{};
    bool generation_loaded = true;
    PFun_slGetNewFrameToken* new_token{};
    PFun_slSetConstants* constants{};
    PFun_slSetTagForFrame* tags{};
    PFun_slDLSSGGetState* get_state{};
    PFun_slDLSSGSetOptions* set_options{};
    PFun_slReflexSetOptions* reflex_options{};
    PFun_slReflexGetState* reflex_state{};
    PFun_slReflexSleep* sleep{};
    PFun_slPCLSetMarker* pcl_marker{};
    PFun_slPCLSetOptions* pcl_options{};
    PFun_slPCLGetState* pcl_state{};
    struct Token { uint64_t id = 0; sl::FrameToken* token = nullptr; bool common[2]{};
        uint32_t markers = 0; bool controller_sampled = false, sleep_claimed = false; } tokens[sl::MAX_FRAMES_IN_FLIGHT];
    uint32_t cursor = 0, native_index = 0;
    uint64_t last_begin = 0;
    uint64_t sleep_calls = 0, marker_calls = 0;
    uint32_t stall_rings = 0, baseline_rings = 0;
    rsf_reflex_mode reflex_mode = RSF_REFLEX_OFF;
    uint32_t frame_limit_us = 0;
    uint32_t pcl_message = 0;
    uint64_t input_counts[4]{};
    std::chrono::steady_clock::time_point last_report{}, last_slow_log{};
    rsf_fg_status cached_capability{};
    std::mutex marker_guard;
    std::mutex reflex_guard;
};
namespace {
std::mutex host_guard;
std::atomic<rsf_streamline_host*> logging_host{nullptr};
void report_driver_sleep(rsf_streamline_host& host)
{
#if RSF_HAVE_NVAPI_DIAGNOSTICS
    // Query only the module Streamline already initialized. Never create a second Reflex owner.
    auto module = GetModuleHandleW(L"nvapi64.dll");
    using Query = void*(__cdecl*)(unsigned int);
    auto query = module ? reinterpret_cast<Query>(GetProcAddress(module, "nvapi_QueryInterface")) : nullptr;
    auto status = query ? reinterpret_cast<decltype(&NvAPI_D3D_GetSleepStatus)>(query(0xaef96ca1u)) : nullptr;
    NV_GET_SLEEP_STATUS_PARAMS state{}; state.version = NV_GET_SLEEP_STATUS_PARAMS_VER;
    const auto result = status ? status(host.native_device.Get(), &state) : NVAPI_NO_IMPLEMENTATION;
    char text[320];
    DWORD foreground_process = 0;
    GetWindowThreadProcessId(GetForegroundWindow(), &foreground_process);
    std::snprintf(text, sizeof(text),
        "Reflex driver sleep status=%d low_latency=%u sleep_interval_us=%u app_called_sleep=%u vrr=%u cpl_vsync=%u fullscreen_iflip=%u fg_multiplier=%u dfg=%u dfg_target_us=%u foreground=%u",
        int(result), unsigned(state.bLowLatencyMode), state.sleepIntervalUs, unsigned(state.bUseGameSleep),
        unsigned(state.bFsVrr), unsigned(state.bCplVsyncOn), unsigned(state.bFullscreenIFlip),
        unsigned(state.fgMultiplier), unsigned(state.bDfgControl), state.dfgFrameTimeTargetUs,
        unsigned(foreground_process == GetCurrentProcessId()));
    if (host.log) host.log(host.log_user, text);
    rsf_reflex_audit_report();
#else
    (void)host;
#endif
}
// The driver's own record of its last 64 frames: marker times as it received them and its
// queue and GPU spans. Written when a sleep stalls, and a few times while it does not, to show
// what the driver saw before it chose its interval. Bounded; never per frame.
void report_latency_ring(rsf_streamline_host& host, const char* reason, uint64_t source)
{
    if (!host.log || !host.reflex_state) return;
    static sl::ReflexState state;
    static std::mutex ring_guard;
    std::lock_guard<std::mutex> owner(ring_guard);
    state = {};
    {
        std::lock_guard<std::mutex> lock(host.marker_guard);
        if (host.reflex_state(state) != sl::Result::eOk || !state.latencyReportAvailable) return;
    }
    char text[520];
    std::snprintf(text, sizeof(text),
        "Reflex ring %s source=%llu: one line per driver frame, microseconds from its SimulationStart; -1 means not recorded",
        reason, static_cast<unsigned long long>(source));
    host.log(host.log_user, text);
    uint64_t previous_sim = 0;
    for (int i = 0; i < sl::kReflexFrameReportCount; ++i) {
        const auto& r = state.frameReport[i];
        if (!r.frameID || !r.simStartTime) continue;
        const auto at = [&r](uint64_t time) { return time ? static_cast<long long>(time - r.simStartTime) : -1ll; };
        std::snprintf(text, sizeof(text),
            "Reflex ring frame=%llu sim_interval=%lld sim_end=%lld submit=%lld..%lld present=%lld..%lld driver=%lld..%lld os_queue=%lld..%lld gpu=%lld..%lld gpu_active=%u gpu_frame=%u copy=%u",
            static_cast<unsigned long long>(r.frameID),
            previous_sim ? static_cast<long long>(r.simStartTime - previous_sim) : -1ll, at(r.simEndTime),
            at(r.renderSubmitStartTime), at(r.renderSubmitEndTime), at(r.presentStartTime), at(r.presentEndTime),
            at(r.driverStartTime), at(r.driverEndTime), at(r.osRenderQueueStartTime), at(r.osRenderQueueEndTime),
            at(r.gpuRenderStartTime), at(r.gpuRenderEndTime), r.gpuActiveRenderTimeUs, r.gpuFrameTimeUs,
            state.frameReport2[i].crossAdapterCopyTimeUs);
        host.log(host.log_user, text);
        previous_sim = r.simStartTime;
    }
}
void message(sl::LogType type, const char* text)
{
    const auto* host = logging_host.load();
    if (host && !host->development_runtime && type != sl::LogType::eError && type != sl::LogType::eWarn) return;
    if (host && host->log) host->log(host->log_user, text);
}
template<class T> bool feature(rsf_streamline_host& host, sl::Feature id, const char* name, T*& out)
{
    void* pointer = nullptr;
    if (host.function(id, name, pointer) != sl::Result::eOk || !pointer) return false;
    out = reinterpret_cast<T*>(pointer); return true;
}
void dispose(rsf_streamline_host* host)
{
    rsf_reflex_audit_stop();
    host->queue.Reset(); host->native_queue.Reset(); host->factory.Reset();
    host->device.Reset(); host->native_device.Reset();
    if (host->initialized) host->shutdown();
    if (logging_host.load() == host) logging_host = nullptr;
    if (host->module) FreeLibrary(host->module);
    delete host;
}
bool copy_text(char* destination, size_t size, const char* source)
{
    if (!source || !source[0] || std::strlen(source) >= size) return false;
    std::memcpy(destination, source, std::strlen(source) + 1); return true;
}
struct DlssSession {
    rsf_streamline_host* host = nullptr;
    ComPtr<IDXGISwapChain4> chain;
    rsf_fg_status state{};
    sl::DLSSGState sdk_state{};
    rsf_fg_options options{};
    rsf_ui_mode ui_mode = RSF_UI_MODE_NONE;
    bool history_valid = false, prepared_enabled = false;
    uint64_t last_prepare = 0;
};
sl::FrameToken* find_token(rsf_streamline_host& host, uint64_t id)
{
    for (const auto& token : host.tokens) if (token.id == id) return token.token;
    return nullptr;
}
rsf_backend_result poll(DlssSession& self)
{
    if (self.host->get_state(sl::ViewportHandle(0), self.sdk_state, nullptr) != sl::Result::eOk)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    auto& state = self.state;
    state.max_generated_frames = self.sdk_state.numFramesToGenerateMax;
    state.dynamic_supported = self.sdk_state.bIsDynamicMFGSupported == sl::eTrue;
    state.vsync_supported = self.sdk_state.bIsVsyncSupportAvailable == sl::eTrue;
    state.min_dimension = self.sdk_state.minWidthOrHeight;
    state.vendor_status = static_cast<int32_t>(self.sdk_state.status);
    state.total_presented += self.sdk_state.numFramesActuallyPresented;
    state.valid_statistics = RSF_FG_STAT_TOTAL_PRESENTED | RSF_FG_STAT_ACTIVITY;
    std::lock_guard<std::mutex> marker_lock(self.host->marker_guard);
    sl::ReflexState reflex{};
    if (self.host->reflex_state(reflex) == sl::Result::eOk) {
        state.low_latency_available = reflex.lowLatencyAvailable;
        if (reflex.latencyReportAvailable) state.valid_statistics |= RSF_FG_STAT_LATENCY_AVAILABLE;
        const auto now = std::chrono::steady_clock::now();
        if (self.host->log && now - self.host->last_report >= std::chrono::seconds(1)) {
            self.host->last_report = now;
            const sl::ReflexReport* latest = &reflex.frameReport[0];
            for (const auto& report : reflex.frameReport) if (report.frameID > latest->frameID) latest = &report;
            const auto ms = [](uint64_t end, uint64_t start) { return end >= start && start ? double(end - start) / 1000.0 : -1.0; };
            char text[440];
            std::snprintf(text, sizeof(text),
                "Reflex driver report available=%u frame=%llu sim_ms=%.2f render_ms=%.2f present_ms=%.2f gpu_ms=%.2f gpu_active_us=%u gpu_frame_us=%u inputs_keyboard=%llu mouse=%llu controller=%llu pcl_ping=%llu",
                unsigned(reflex.latencyReportAvailable), static_cast<unsigned long long>(latest->frameID),
                ms(latest->simEndTime, latest->simStartTime), ms(latest->renderSubmitEndTime, latest->renderSubmitStartTime),
                ms(latest->presentEndTime, latest->presentStartTime), ms(latest->gpuRenderEndTime, latest->gpuRenderStartTime),
                latest->gpuActiveRenderTimeUs, latest->gpuFrameTimeUs,
                static_cast<unsigned long long>(self.host->input_counts[0]), static_cast<unsigned long long>(self.host->input_counts[1]),
                static_cast<unsigned long long>(self.host->input_counts[2]), static_cast<unsigned long long>(self.host->input_counts[3]));
            self.host->log(self.host->log_user, text);
            report_driver_sleep(*self.host);
        }
    }
    state.active = self.prepared_enabled && self.sdk_state.status == sl::DLSSGStatus::eOk &&
        self.sdk_state.numFramesActuallyPresented > 1;
    self.host->cached_capability = state;
    return RSF_BACKEND_OK;
}
void destroy(void* pointer)
{
    auto* self = static_cast<DlssSession*>(pointer); if (!self) return;
    if (self->chain) {
        if (self->state.effective_mode != RSF_FG_OFF) {
            sl::DLSSGOptions off{}; self->host->set_options(sl::ViewportHandle(0), off);
        }
        self->chain.Reset();
    }
    --self->host->borrowers; delete self;
}
rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    const auto header_result = rsf::fg_setup_header(setup, out, chain); if (header_result != 0) return header_result;
    if (setup->chain.ui_mode != RSF_UI_MODE_NONE && setup->chain.ui_mode != RSF_UI_MODE_BACKBUFFER_HUDLESS &&
        setup->chain.ui_mode != RSF_UI_MODE_BACKBUFFER_HUDLESS_UI) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (!setup->streamline_host) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    auto& host = *static_cast<rsf_streamline_host*>(setup->streamline_host);
    if ((setup->engine_type && setup->engine_type != host.engine_type) ||
        (setup->engine_version_utf8 && std::strcmp(setup->engine_version_utf8, host.engine_version)) ||
        (setup->project_id_utf8 && std::strcmp(setup->project_id_utf8, host.project_id)))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->require_signature && !host.signature_verified) return RSF_BACKEND_ERROR_LOAD_FAILED;
    if (setup->development_runtime && !host.development_runtime) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    if (setup->chain.d3d12_device != host.device.Get() || setup->chain.d3d12_queue != host.queue.Get())
        return RSF_BACKEND_ERROR_WRONG_API;
    if (!host.get_state || !host.set_options || !host.sleep || !host.pcl_marker || !host.reflex_options ||
        !host.reflex_state) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (!setup->chain.width || !setup->chain.height || setup->chain.buffer_count < 2)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto luid = host.native_device->GetAdapterLuid();
    sl::AdapterInfo info{}; info.deviceLUID = reinterpret_cast<uint8_t*>(const_cast<LUID*>(&luid));
    info.deviceLUIDSizeInBytes = sizeof(luid);
    const auto fg_support = host.supported(sl::kFeatureDLSS_G, info);
    const auto reflex_support = host.supported(sl::kFeatureReflex, info);
    if (fg_support != sl::Result::eOk || reflex_support != sl::Result::eOk) {
        if (setup->chain.log) {
            char text[160]{}; std::snprintf(text, sizeof(text), "DLSS-G support=%u Reflex support=%u", unsigned(fg_support), unsigned(reflex_support));
            setup->chain.log(setup->chain.log_user, text);
        }
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    auto* self = new (std::nothrow) DlssSession; if (!self) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->host = &host; ++host.borrowers; self->ui_mode = setup->chain.ui_mode;
    self->state.struct_size = sizeof(self->state); self->state.supported = 1;
    self->state.auto_supported = 1;
    self->state.pacing_owner = RSF_PACING_REFLEX;
    sl::FeatureVersion version{};
    if (host.version(sl::kFeatureDLSS_G, version) == sl::Result::eOk && version.versionNGX) {
        const auto& ngx = version.versionNGX;
        self->state.version_id = (uint64_t(ngx.major) << 48) | (uint64_t(ngx.minor) << 32) | ngx.build;
        std::snprintf(self->state.version_name, sizeof(self->state.version_name), "DLSS-G NGX %u.%u.%u (SL %u.%u.%u)",
            ngx.major, ngx.minor, ngx.build, version.versionSL.major, version.versionSL.minor, version.versionSL.build);
    } else {
        std::snprintf(self->state.version_name, sizeof(self->state.version_name), "DLSS-G runtime version unavailable");
    }
    if (setup->version_id && setup->version_id != self->state.version_id) {
        destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    auto result = RSF_BACKEND_OK;
    if (host.borrowers > 1) {
        self->state = host.cached_capability; self->state.active = 0;
        self->state.total_presented = 0; self->state.valid_statistics = 0;
    } else result = poll(*self);
    if (result != 0) { destroy(self); return result; }
    if (setup->chain.hwnd) {
        if (host.borrowers > 1) { destroy(self); return RSF_BACKEND_ERROR_NOT_READY; }
        auto description = rsf::chain_description(setup->chain);
        ComPtr<IDXGISwapChain1> swap;
        if (FAILED(host.factory->CreateSwapChainForHwnd(host.queue.Get(), static_cast<HWND>(setup->chain.hwnd),
            &description, nullptr, nullptr, &swap))) { destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED; }
        void* raw = swap.Detach();
        void* native_chain = nullptr;
        if (host.native(raw, &native_chain) != sl::Result::eOk || !native_chain) {
            static_cast<IUnknown*>(raw)->Release(); destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED;
        }
        const bool already_upgraded = native_chain != raw;
        static_cast<IUnknown*>(native_chain)->Release();
        if (!already_upgraded && host.upgrade(&raw) != sl::Result::eOk) {
            static_cast<IUnknown*>(raw)->Release(); destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED;
        }
        auto* upgraded = static_cast<IDXGISwapChain1*>(raw);
        const auto hr = upgraded->QueryInterface(IID_PPV_ARGS(&self->chain)); upgraded->Release();
        if (FAILED(hr)) { destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED; }
    }
    *out = self; *chain = self->chain.Get(); return RSF_BACKEND_OK;
}
rsf_backend_result configure(void* pointer, const rsf_fg_options* options)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto validation = rsf::fg_options_result(options); if (validation != 0) return validation;
    auto& self = *static_cast<DlssSession*>(pointer);
    if (!self.chain) return RSF_BACKEND_ERROR_NOT_READY;
    if (self.options.struct_size && self.options.mode == options->mode &&
        self.options.generated_frames == options->generated_frames &&
        self.options.dynamic_target_fps == options->dynamic_target_fps &&
        self.options.reflex_mode == options->reflex_mode && self.options.frame_limit_us == options->frame_limit_us)
        return RSF_BACKEND_OK;
    if (options->mode == RSF_FG_DYNAMIC && !self.state.dynamic_supported) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (options->mode == RSF_FG_FIXED && !options->generated_frames) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (options->mode != RSF_FG_OFF && !self.state.low_latency_available) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    // Active DLSS-G needs Reflex's runtime frame tracking. Preserve the requested
    // preference, while reporting the required effective mode separately.
    const uint32_t mode = options->mode != RSF_FG_OFF && options->reflex_mode == RSF_REFLEX_OFF ?
        RSF_REFLEX_ON : options->reflex_mode;
    const auto latency = rsf_streamline_host_reflex(self.host, mode, options->frame_limit_us);
    if (latency != 0) return latency;
    // A Reflex mode or limit change leaves generation and its history alone.
    if (self.options.struct_size && self.options.mode == options->mode &&
        self.options.generated_frames == options->generated_frames &&
        self.options.dynamic_target_fps == options->dynamic_target_fps) {
        self.options = *options; self.state.effective_reflex = mode;
        return RSF_BACKEND_OK;
    }
    sl::DLSSGOptions fg{}; fg.mode = static_cast<sl::DLSSGMode>(options->mode);
    fg.numFramesToGenerate = options->generated_frames < self.state.max_generated_frames ? options->generated_frames : self.state.max_generated_frames;
    fg.dynamicTargetFrameRate = options->dynamic_target_fps;
    fg.flags = sl::DLSSGFlags::eRetainResourcesWhenOff;
    fg.enableUserInterfaceRecomposition = self.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS_UI ? sl::eTrue : sl::eFalse;
    if (self.host->set_options(sl::ViewportHandle(0), fg) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.options = *options; self.state.effective_mode = options->mode;
    self.state.configured_mode = options->mode;
    self.state.effective_generated_frames = options->mode == RSF_FG_FIXED ? fg.numFramesToGenerate : 0;
    self.state.effective_reflex = mode; self.state.active = 0; self.history_valid = false;
    return RSF_BACKEND_OK;
}
rsf_backend_result begin(void* pointer, uint64_t id)
{
    if (!pointer || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return rsf_streamline_host_begin(static_cast<DlssSession*>(pointer)->host, id);
}
rsf_backend_result marker(void* pointer, rsf_latency_marker marker, uint64_t id, uint32_t controller)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return rsf_streamline_host_marker(static_cast<DlssSession*>(pointer)->host, id, marker, controller);
}
}
extern "C" rsf_backend_result rsf_streamline_host_begin(rsf_streamline_host* pointer, uint64_t id)
{
    const auto result = rsf_streamline_host_acquire(pointer, id);
    return result == RSF_BACKEND_OK ? rsf_streamline_host_sleep(pointer, id) : result;
}
extern "C" rsf_backend_result rsf_streamline_host_acquire(rsf_streamline_host* pointer, uint64_t id)
{
    if (!pointer || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& host = *pointer;
    std::lock_guard<std::mutex> lock(host.marker_guard);
    if (id <= host.last_begin || host.tokens[host.cursor].id) return RSF_BACKEND_ERROR_NOT_READY;
    if (host.native_index == UINT32_MAX) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    auto& slot = host.tokens[host.cursor]; const uint32_t index = ++host.native_index;
    if (host.new_token(slot.token, &index) != sl::Result::eOk || !slot.token) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    host.cursor = (host.cursor + 1) % sl::MAX_FRAMES_IN_FLIGHT; host.last_begin = id;
    slot.id = id; slot.common[0] = slot.common[1] = false; slot.markers = 0; slot.controller_sampled = false;
    slot.sleep_claimed = false;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_streamline_host_sleep(rsf_streamline_host* pointer, uint64_t id)
{
    if (!pointer || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    using Clock = std::chrono::steady_clock;
    const auto entered = Clock::now();
    char slow_text[280]{};
    auto& host = *pointer;
    std::unique_lock<std::mutex> lock(host.marker_guard);
    const auto locked = Clock::now();
    sl::FrameToken* token = nullptr;
    for (auto& slot : host.tokens) if (slot.id == id) {
        if (slot.sleep_claimed) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        slot.sleep_claimed = true; token = slot.token; break;
    }
    if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    const auto index = static_cast<uint32_t>(*token);
    const auto minted = Clock::now();
    lock.unlock();
    if (host.profile != RSF_SL_PROFILE_PCL) {
        // Present markers for the previous frame must remain able to advance while the
        // current CPU frame sleeps. Only option changes share the sleeper's owner lock.
        std::lock_guard<std::mutex> sleeper(host.reflex_guard);
        const auto sleeping = Clock::now();
        if (!host.sleep || host.sleep(*token) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        const auto slept = Clock::now();
        lock.lock();
        ++host.sleep_calls;
        const auto finished = Clock::now();
        // A requested limit makes long sleeps normal: with generation the driver waits up to
        // the limit times the presents per frame. Only sleeps well beyond that are stalls.
        const auto expected = std::chrono::microseconds(uint64_t(host.frame_limit_us) * 5 / 2);
        const auto slow = expected > std::chrono::milliseconds(8) ?
            std::chrono::duration_cast<std::chrono::steady_clock::duration>(expected) :
            std::chrono::duration_cast<std::chrono::steady_clock::duration>(std::chrono::milliseconds(8));
        if (host.log && finished - entered >= slow &&
            finished - host.last_slow_log >= std::chrono::seconds(1)) {
            host.last_slow_log = finished;
            const auto milliseconds = [](auto span) { return std::chrono::duration<double, std::milli>(span).count(); };
            std::snprintf(slow_text, sizeof(slow_text),
                "Reflex slow begin source=%llu SDKframe=%u mode=%u limit_us=%u marker_lock_ms=%.2f token_ms=%.2f options_lock_ms=%.2f sleep_ms=%.2f completion_lock_ms=%.2f",
                static_cast<unsigned long long>(id), index, uint32_t(host.reflex_mode), host.frame_limit_us,
                milliseconds(locked - entered), milliseconds(minted - locked), milliseconds(sleeping - minted),
                milliseconds(slept - sleeping), milliseconds(finished - slept));
        }
    }
    if (lock.owns_lock()) lock.unlock();
    if (slow_text[0] && host.log) {
        host.log(host.log_user, slow_text);
        report_driver_sleep(host);
        if (host.stall_rings < 8) { ++host.stall_rings; report_latency_ring(host, "stall", id); }
    } else if (host.log && host.baseline_rings < 10 &&
        ((host.sleep_calls & 2047u) == 1024u || host.sleep_calls == 16)) {
        // Unstalled reference for the same screen, about every 2048 frames, plus one at startup.
        ++host.baseline_rings; report_latency_ring(host, "baseline", id);
    }
    return RSF_BACKEND_OK;
}
bool rsf_streamline_common_set(rsf_streamline_host* host, uint64_t id, uint32_t viewport, const sl::Constants& constants)
{
    if (!host || !id || viewport >= 2) return false;
    std::lock_guard<std::mutex> lock(host->marker_guard);
    for (auto& slot : host->tokens) if (slot.id == id) {
        if (slot.common[viewport]) return false;
        if (host->constants(constants, *slot.token, sl::ViewportHandle(viewport)) != sl::Result::eOk) return false;
        slot.common[viewport] = true; return true;
    }
    return false;
}
bool rsf_streamline_common_exists(rsf_streamline_host* host, uint64_t id, uint32_t viewport)
{
    if (!host || !id || viewport >= 2) return false;
    // FG prepare already holds this owner lock while resolving its token and tags.
    for (const auto& slot : host->tokens) if (slot.id == id) return slot.common[viewport];
    return false;
}
extern "C" rsf_backend_result rsf_streamline_host_marker(rsf_streamline_host* pointer, uint64_t id, rsf_latency_marker marker, uint32_t controller)
{
    if (!pointer || !id || marker > RSF_LATENCY_INPUT_SAMPLE || controller > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& host = *pointer;
    if (!host.pcl_marker) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    std::lock_guard<std::mutex> lock(host.marker_guard);
    auto* token = find_token(host, id); if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    // PCL removed ordinary input sample marker 6. Marker 13 is for controllers only.
    if (marker == RSF_LATENCY_INPUT_SAMPLE && !controller) return RSF_BACKEND_OK;
    const auto mapped = marker == RSF_LATENCY_INPUT_SAMPLE ? sl::PCLMarker::eControllerInputSample : static_cast<sl::PCLMarker>(marker);
    for (const auto& slot : host.tokens) if (slot.id == id && marker <= RSF_LATENCY_PRESENT_END &&
        (slot.markers & (1u << marker))) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto result = host.pcl_marker(mapped, *token);
    if (result == sl::Result::eOk) ++host.marker_calls;
    if (result == sl::Result::eOk && marker <= RSF_LATENCY_PRESENT_END)
        for (auto& slot : host.tokens) if (slot.id == id) {
            slot.markers |= 1u << marker;
            // UE's native EndFrame follows Present. Keep the token until every required
            // marker has arrived, including configurations where simulation overlaps render.
            if (slot.markers == 63u) slot.id = 0;
        }
    return result == sl::Result::eOk ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
extern "C" int rsf_streamline_host_presented(rsf_streamline_host* host, uint64_t id)
{
    if (!host || !id) return 0;
    std::lock_guard<std::mutex> lock(host->marker_guard);
    for (const auto& slot : host->tokens) if (slot.id == id) return (slot.markers & (1u << RSF_LATENCY_PRESENT_END)) != 0;
    return 0;
}
extern "C" rsf_backend_result rsf_streamline_host_input(rsf_streamline_host* host, uint64_t id, uint32_t kinds, uint32_t message_id)
{
    if (!host || !id || kinds & ~7u) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(host->marker_guard);
    for (auto& slot : host->tokens) if (slot.id == id) {
        for (uint32_t i = 0; i < 3; ++i) if (kinds & (1u << i)) ++host->input_counts[i];
        sl::PCLMarker marker{};
        if (message_id && message_id == host->pcl_message) { marker = sl::PCLMarker::ePCLatencyPing; ++host->input_counts[3]; }
        else if ((kinds & 4u) && !slot.controller_sampled) { marker = sl::PCLMarker::eControllerInputSample; slot.controller_sampled = true; }
        else return RSF_BACKEND_OK; // Mouse/keyboard timing uses real dequeue records and PCL pings, never deprecated marker 6.
        if (!host->pcl_marker || host->pcl_marker(marker, *slot.token) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        ++host->marker_calls; return RSF_BACKEND_OK;
    }
    return RSF_BACKEND_ERROR_NOT_READY;
}
extern "C" rsf_backend_result rsf_streamline_host_abort(rsf_streamline_host* host, uint64_t id)
{
    if (!host || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(host->marker_guard);
    for (auto& slot : host->tokens) if (slot.id == id) slot.id = 0;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_streamline_host_reflex(rsf_streamline_host* host, rsf_reflex_mode mode, uint32_t limit)
{
    if (!host || mode > RSF_REFLEX_BOOST) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::scoped_lock lock(host->reflex_guard, host->marker_guard);
    if (!host->reflex_options || host->profile == RSF_SL_PROFILE_PCL) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    sl::ReflexState state{};
    if (!host->reflex_state || host->reflex_state(state) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if ((mode != RSF_REFLEX_OFF || limit) && !state.lowLatencyAvailable) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    sl::ReflexOptions options{}; options.frameLimitUs = limit;
    options.mode = mode == RSF_REFLEX_BOOST ? sl::ReflexMode::eLowLatencyWithBoost :
        mode == RSF_REFLEX_ON ? sl::ReflexMode::eLowLatency : sl::ReflexMode::eOff;
    if (host->reflex_options(options) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    host->reflex_mode = mode; host->frame_limit_us = limit;
    if (host->log) {
        char text[160]; std::snprintf(text, sizeof(text),
            "Reflex options applied: mode=%u frameLimitUs=%u markerOptimization=%u",
            uint32_t(mode), limit, uint32_t(options.useMarkersToOptimize));
        host->log(host->log_user, text);
        report_driver_sleep(*host);
    }
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_streamline_host_latency_status(rsf_streamline_host* host, rsf_streamline_latency_status* out)
{
    if (!host || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::scoped_lock lock(host->reflex_guard, host->marker_guard);
    *out = {}; out->struct_size = sizeof(*out); out->profile = host->profile;
    out->pacing_owner = host->profile == RSF_SL_PROFILE_PCL ? RSF_PACING_NONE : RSF_PACING_REFLEX;
    out->reflex_mode = host->reflex_mode; out->frame_limit_us = host->frame_limit_us;
    out->sleep_calls = host->sleep_calls; out->marker_calls = host->marker_calls; out->last_begin_id = host->last_begin;
    if (host->reflex_state) {
        sl::ReflexState state{};
        if (host->reflex_state(state) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        out->low_latency_available = state.lowLatencyAvailable; out->latency_report_available = state.latencyReportAvailable;
    }
    if (host->pcl_state) {
        sl::PCLState state{};
        if (host->pcl_state(state) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        out->pcl_message_id = state.statsWindowMessage;
    }
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_streamline_host_upgrade_chain(rsf_streamline_host* host, void** chain)
{
    if (!host || !chain || !*chain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    void* native = nullptr;
    if (host->native(*chain, &native) != sl::Result::eOk || !native) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const bool upgraded = native != *chain; static_cast<IUnknown*>(native)->Release();
    return upgraded || host->upgrade(chain) == sl::Result::eOk ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
namespace {
rsf_backend_result prepare(void* pointer, void* command, const rsf_fg_frame* frame)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<DlssSession*>(pointer);
    auto result = rsf::fg_frame(frame, self.host->native_device.Get()); if (result != 0) return result;
    if (!self.chain || frame->record->frame_id <= self.last_prepare) return RSF_BACKEND_ERROR_NOT_READY;
    const auto& record = *frame->record; const auto& camera = record.camera;
    const bool enabled = frame->interpolate && self.options.mode != RSF_FG_OFF;
    if (enabled) {
        if (!command) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        void* native_command = nullptr;
        if (self.host->native(command, &native_command) != sl::Result::eOk || !native_command)
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        result = rsf::fg_command(native_command, self.host->native_device.Get());
        static_cast<IUnknown*>(native_command)->Release(); if (result != 0) return result;
    }
    if (enabled && ((self.ui_mode != RSF_UI_MODE_NONE && !frame->hudless.resource) ||
        (self.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS_UI && !frame->ui.resource)))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    sl::DLSSGOptions options{};
    options.mode = enabled ? static_cast<sl::DLSSGMode>(self.options.mode) : sl::DLSSGMode::eOff;
    options.numFramesToGenerate = self.options.mode == RSF_FG_FIXED ?
        (self.options.generated_frames < self.state.max_generated_frames ? self.options.generated_frames : self.state.max_generated_frames) : 1;
    options.dynamicTargetFrameRate = self.options.dynamic_target_fps;
    options.flags = sl::DLSSGFlags::eRetainResourcesWhenOff;
    options.enableUserInterfaceRecomposition = self.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS_UI ? sl::eTrue : sl::eFalse;
    if (self.state.effective_mode != uint32_t(options.mode) &&
        self.host->set_options(sl::ViewportHandle(0), options) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.state.effective_mode = enabled ? self.options.mode : RSF_FG_OFF;
    self.state.effective_generated_frames = enabled && self.options.mode == RSF_FG_FIXED ? options.numFramesToGenerate : 0;
    self.state.active = 0; self.prepared_enabled = enabled;
    if (!enabled) { self.last_prepare = record.frame_id; self.history_valid = false; return RSF_BACKEND_OK; }
    std::lock_guard<std::mutex> lock(self.host->marker_guard);
    auto* token = find_token(*self.host, record.frame_id); if (!token) return RSF_BACKEND_ERROR_NOT_READY;
    sl::Constants constants{};
    auto matrix = [](const float* values) {
        sl::float4x4 output{}; for (uint32_t i = 0; i < 4; ++i)
            output[i] = {values[i * 4], values[i * 4 + 1], values[i * 4 + 2], values[i * 4 + 3]};
        return output;
    };
    constants.cameraViewToClip = matrix(camera.view_to_clip); constants.clipToCameraView = matrix(camera.clip_to_view);
    constants.clipToPrevClip = matrix(camera.clip_to_previous_clip);
    sl::matrixFullInvert(constants.prevClipToClip, constants.clipToPrevClip);
    for (uint32_t i = 0; i < 4; ++i) {
        const auto& row = constants.prevClipToClip[i];
        if (!std::isfinite(row.x) || !std::isfinite(row.y) || !std::isfinite(row.z) || !std::isfinite(row.w))
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    constants.jitterOffset = {camera.jitter_pixels[0], camera.jitter_pixels[1]}; constants.cameraPinholeOffset = {0, 0};
    constants.mvecScale = {frame->motion_scale_x / frame->motion.width, frame->motion_scale_y / frame->motion.height};
    constants.cameraPos = {camera.view_to_world[12], camera.view_to_world[13], camera.view_to_world[14]};
    constants.cameraRight = {camera.view_to_world[0], camera.view_to_world[1], camera.view_to_world[2]};
    constants.cameraUp = {camera.view_to_world[4], camera.view_to_world[5], camera.view_to_world[6]};
    constants.cameraFwd = {camera.view_to_world[8], camera.view_to_world[9], camera.view_to_world[10]};
    constants.cameraNear = camera.near_plane; constants.cameraFar = camera.far_plane;
    constants.cameraFOV = camera.vertical_fov_radians; constants.cameraAspectRatio = camera.view_to_clip[5] / camera.view_to_clip[0];
    constants.depthInverted = camera.depth_inverted ? sl::eTrue : sl::eFalse;
    constants.cameraMotionIncluded = sl::eTrue; constants.motionVectors3D = sl::eFalse;
    constants.motionVectorsJittered = camera.motion_jittered ? sl::eTrue : sl::eFalse;
    constants.reset = !self.history_valid || record.frame_id != self.last_prepare + 1 ||
        (record.flags & RSF_FRAME_FLAG_RESET) ? sl::eTrue : sl::eFalse;
    if (!rsf_streamline_common_exists(self.host, record.frame_id, 0)) {
        if (self.host->constants(constants, *token, sl::ViewportHandle(0)) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        for (auto& slot : self.host->tokens) if (slot.id == record.frame_id) slot.common[0] = true;
    }
    const rsf_backend_resource* inputs[] = {&frame->backbuffer, &frame->depth, &frame->motion, &frame->hudless, &frame->ui};
    const sl::BufferType types[] = {sl::kBufferTypeBackbuffer, sl::kBufferTypeDepth, sl::kBufferTypeMotionVectors,
        sl::kBufferTypeHUDLessColor, sl::kBufferTypeUIColorAndAlpha};
    sl::Resource resources[5]{}; sl::Extent extents[5]{}; sl::ResourceTag tags[5]{};
    uint32_t count = 0;
    for (uint32_t i = 0; i < 5; ++i) {
        if (!inputs[i]->resource) continue;
        resources[count] = sl::Resource(sl::ResourceType::eTex2d, inputs[i]->resource, inputs[i]->state);
        extents[count] = {inputs[i]->y, inputs[i]->x, inputs[i]->width, inputs[i]->height};
        tags[count] = sl::ResourceTag(&resources[count], types[i], sl::eValidUntilPresent, &extents[count]); ++count;
    }
    if (self.host->tags(*token, sl::ViewportHandle(0), tags, count, static_cast<sl::CommandBuffer*>(command)) != sl::Result::eOk)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.history_valid = true; self.last_prepare = record.frame_id; return RSF_BACKEND_OK;
}
rsf_backend_result after(void* pointer) { return pointer ? poll(*static_cast<DlssSession*>(pointer)) : RSF_BACKEND_ERROR_INVALID_ARGUMENT; }
rsf_backend_result status(void* pointer, rsf_fg_status* out)
{
    if (!pointer || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = static_cast<DlssSession*>(pointer)->state; return RSF_BACKEND_OK;
}
rsf_backend_result retirement(void* pointer, rsf_fg_retirement* out)
{
    if (!pointer || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto& state = static_cast<DlssSession*>(pointer)->sdk_state;
    out->fence = state.inputsProcessingCompletionFence; out->value = state.lastPresentInputsProcessingCompletionFenceValue;
    return RSF_BACKEND_OK;
}
rsf_backend_result abort_frame(void* pointer, uint64_t id)
{
    if (!pointer || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<DlssSession*>(pointer);
    const auto result = rsf_streamline_host_abort(self.host, id); if (result != 0) return result;
    self.history_valid = false; self.prepared_enabled = false; self.state.active = 0;
    return RSF_BACKEND_OK;
}
const rsf_generation_provider provider{sizeof(provider), create, configure, begin, marker, prepare, after, status, retirement, destroy, abort_frame};
}
static rsf_backend_result create_host(const rsf_streamline_host_setup* setup, void* existing_device, void* existing_queue, rsf_streamline_host** out)
{
    if (!setup || !out || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (setup->abi_version != RSF_STREAMLINE_HOST_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!setup->dxgi_adapter || !setup->runtime_directory_utf8 || setup->engine_type >= uint32_t(sl::EngineType::eCount) ||
        setup->profile > RSF_SL_PROFILE_PCL)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(host_guard);
    if (GetModuleHandleW(L"sl.interposer.dll")) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    auto* host = new (std::nothrow) rsf_streamline_host; if (!host) return RSF_BACKEND_ERROR_INIT_FAILED;
    host->log = setup->log; host->log_user = setup->log_user;
    host->engine_type = setup->engine_type; host->development_runtime = setup->development_runtime != 0;
    host->profile = setup->profile;
    if (!copy_text(host->engine_version, sizeof(host->engine_version), setup->engine_version_utf8) ||
        !copy_text(host->project_id, sizeof(host->project_id), setup->project_id_utf8) ||
        !MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, setup->runtime_directory_utf8, -1, host->directory, 1024) ||
        !((host->directory[1] == L':' && (host->directory[2] == L'\\' || host->directory[2] == L'/')) ||
        (host->directory[0] == L'\\' && host->directory[1] == L'\\'))) {
        dispose(host); return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    wchar_t path[1200]{}; swprintf_s(path, L"%s\\sl.interposer.dll", host->directory);
    if (setup->require_signature) {
        if (!rsf_dlss_verify_runtime_signature(path)) { dispose(host); return RSF_BACKEND_ERROR_LOAD_FAILED; }
        host->signature_verified = true;
    }
    host->module = LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!host->module) { dispose(host); return RSF_BACKEND_ERROR_LOAD_FAILED; }
#define CORE(member, function) host->member = rsf::entry<PFun_##function*>(host->module, #function); if (!host->member) { dispose(host); return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT; }
    CORE(init, slInit); CORE(shutdown, slShutdown); CORE(upgrade, slUpgradeInterface); CORE(native, slGetNativeInterface);
    CORE(set_device, slSetD3DDevice); CORE(supported, slIsFeatureSupported); CORE(function, slGetFeatureFunction);
    CORE(version, slGetFeatureVersion); CORE(loaded, slSetFeatureLoaded);
    CORE(new_token, slGetNewFrameToken); CORE(constants, slSetConstants); CORE(tags, slSetTagForFrame);
#undef CORE
    sl::Preferences preferences{};
    preferences.flags = sl::PreferenceFlags::eUseManualHooking | sl::PreferenceFlags::eUseFrameBasedResourceTagging;
    if (!host->development_runtime) preferences.flags |= sl::PreferenceFlags::eDisableDebugText;
    preferences.renderAPI = sl::RenderAPI::eD3D12; preferences.showConsole = host->development_runtime;
    preferences.logLevel = host->development_runtime ? sl::LogLevel::eVerbose : sl::LogLevel::eDefault;
    logging_host = host; preferences.logMessageCallback = message;
    uint32_t feature_count = 0;
    if (host->profile != RSF_SL_PROFILE_PCL || setup->load_dlss_sr) host->features[feature_count++] = sl::kFeatureDLSS;
    host->features[feature_count++] = sl::kFeaturePCL;
    if (host->profile != RSF_SL_PROFILE_PCL) host->features[feature_count++] = sl::kFeatureReflex;
    if (host->profile == RSF_SL_PROFILE_DLSS_FG) host->features[feature_count++] = sl::kFeatureDLSS_G;
    preferences.featuresToLoad = host->features; preferences.numFeaturesToLoad = feature_count;
    host->paths[0] = host->directory; preferences.pathsToPlugins = host->paths; preferences.numPathsToPlugins = 1;
    preferences.engine = static_cast<sl::EngineType>(setup->engine_type);
    preferences.engineVersion = host->engine_version; preferences.projectId = host->project_id;
    if (host->init(preferences, sl::kSDKVersion) != sl::Result::eOk) { dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED; }
    host->initialized = true;
    ID3D12Device* device = nullptr;
    if (existing_device && existing_queue) {
        device = static_cast<ID3D12Device*>(existing_device); device->AddRef();
    } else if (FAILED(D3D12CreateDevice(static_cast<IUnknown*>(setup->dxgi_adapter), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) {
        dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    void* upgraded = device;
    if (host->upgrade(&upgraded) != sl::Result::eOk) { device->Release(); dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED; }
    host->device.Attach(static_cast<ID3D12Device*>(upgraded));
    void* native = nullptr;
    if (host->native(host->device.Get(), &native) != sl::Result::eOk || !native) { dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED; }
    host->native_device.Attach(static_cast<ID3D12Device*>(native));
    rsf_reflex_audit_start(host->log, host->log_user);
    if (host->set_device(host->device.Get()) != sl::Result::eOk) { dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED; }
    IDXGIFactory2* factory = nullptr;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) { dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED; }
    upgraded = factory;
    if (host->upgrade(&upgraded) != sl::Result::eOk) { factory->Release(); dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED; }
    host->factory.Attach(static_cast<IDXGIFactory2*>(upgraded));
    D3D12_COMMAND_QUEUE_DESC description{}; description.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    if (FAILED(host->device->CreateCommandQueue(&description, IID_PPV_ARGS(&host->queue)))) { dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED; }
    native = nullptr;
    if (host->native(host->queue.Get(), &native) != sl::Result::eOk || !native) { dispose(host); return RSF_BACKEND_ERROR_INIT_FAILED; }
    host->native_queue.Attach(static_cast<ID3D12CommandQueue*>(native));
    if (host->profile == RSF_SL_PROFILE_DLSS_FG) {
        feature(*host, sl::kFeatureDLSS_G, "slDLSSGGetState", host->get_state);
        feature(*host, sl::kFeatureDLSS_G, "slDLSSGSetOptions", host->set_options);
    }
    if (host->profile != RSF_SL_PROFILE_PCL) {
        feature(*host, sl::kFeatureReflex, "slReflexSetOptions", host->reflex_options);
        feature(*host, sl::kFeatureReflex, "slReflexGetState", host->reflex_state);
        feature(*host, sl::kFeatureReflex, "slReflexSleep", host->sleep);
    }
    feature(*host, sl::kFeaturePCL, "slPCLSetMarker", host->pcl_marker);
    feature(*host, sl::kFeaturePCL, "slPCLGetState", host->pcl_state);
    if (feature(*host, sl::kFeaturePCL, "slPCLSetOptions", host->pcl_options)) {
        sl::PCLOptions pcl{}; host->pcl_options(pcl);
        sl::PCLState pcl_state{};
        if (host->pcl_state(pcl_state) == sl::Result::eOk) host->pcl_message = pcl_state.statsWindowMessage;
    }
    *out = host; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_streamline_host_create(const rsf_streamline_host_setup* setup, rsf_streamline_host** out) {
    return create_host(setup, nullptr, nullptr, out);
}
extern "C" rsf_backend_result rsf_streamline_host_adopt(const rsf_streamline_host_setup* setup, void* device, void* queue, rsf_streamline_host** out) {
    if (!device || !queue) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return create_host(setup, device, queue, out);
}
extern "C" rsf_backend_result rsf_streamline_host_graphics(rsf_streamline_host* host, rsf_streamline_graphics* out)
{
    if (!host || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    out->device = host->device.Get(); out->queue = host->queue.Get(); out->factory = host->factory.Get();
    out->native_device = host->native_device.Get(); out->native_queue = host->native_queue.Get(); return RSF_BACKEND_OK;
}
extern "C" void* rsf_streamline_host_module(rsf_streamline_host* host)
{ return host ? host->module : nullptr; }
extern "C" rsf_backend_result rsf_streamline_host_drain(rsf_streamline_host* host)
{
    if (!host || !host->queue || !host->device) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    ComPtr<ID3D12Fence> fence;
    if (FAILED(host->device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const bool ready = SUCCEEDED(host->queue->Signal(fence.Get(), 1)) &&
        SUCCEEDED(fence->SetEventOnCompletion(1, event)) && WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
    CloseHandle(event);
    return ready ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
extern "C" void* rsf_streamline_host_token(rsf_streamline_host* host, uint64_t id)
{
    if (!host || !id) return nullptr;
    std::lock_guard<std::mutex> lock(host->marker_guard);
    return find_token(*host, id);
}
extern "C" rsf_backend_result rsf_streamline_host_destroy(rsf_streamline_host* host)
{
    if (!host) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(host_guard);
    if (host->borrowers) return RSF_BACKEND_ERROR_NOT_READY;
    for (const auto& token : host->tokens) if (token.id) return RSF_BACKEND_ERROR_NOT_READY;
    dispose(host); return RSF_BACKEND_OK;
}
extern "C" const rsf_generation_provider* rsf_generation_dlss() { return &provider; }
extern "C" rsf_backend_result rsf_streamline_host_generation_load(rsf_streamline_host* host, uint32_t enabled) {
    if (!host || !host->loaded || host->borrowers) return RSF_BACKEND_ERROR_NOT_READY;
    if (host->generation_loaded == bool(enabled)) return RSF_BACKEND_OK;
    if (host->loaded(sl::kFeatureDLSS_G, enabled != 0) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    host->generation_loaded = enabled != 0; host->get_state = nullptr; host->set_options = nullptr;
    if (enabled && (!feature(*host, sl::kFeatureDLSS_G, "slDLSSGGetState", host->get_state) ||
        !feature(*host, sl::kFeatureDLSS_G, "slDLSSGSetOptions", host->set_options))) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    return RSF_BACKEND_OK;
}
extern "C" void* rsf_streamline_host_native(rsf_streamline_host* host, void* proxy) {
    if (!host || !proxy) return nullptr;
    void* native = nullptr;
    if (host->native(proxy, &native) != sl::Result::eOk || !native) return nullptr;
    static_cast<IUnknown*>(native)->Release(); return native;
}
#else
namespace {
rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    const auto result = rsf::fg_setup_header(setup, out, chain);
    return result != 0 ? result : RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result configure(void*, const rsf_fg_options*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result begin(void*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result marker(void*, rsf_latency_marker, uint64_t, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result prepare(void*, void*, const rsf_fg_frame*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result after(void*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result status(void*, rsf_fg_status*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result retirement(void*, rsf_fg_retirement*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
void destroy(void*) {}
rsf_backend_result abort_frame(void*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
const rsf_generation_provider provider{sizeof(provider), create, configure, begin, marker, prepare, after, status, retirement, destroy, abort_frame};
}
extern "C" const rsf_generation_provider* rsf_generation_dlss() { return &provider; }
extern "C" rsf_backend_result rsf_streamline_host_create(const rsf_streamline_host_setup*, rsf_streamline_host**) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_adopt(const rsf_streamline_host_setup*, void*, void*, rsf_streamline_host**) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_graphics(rsf_streamline_host*, rsf_streamline_graphics*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" void* rsf_streamline_host_module(rsf_streamline_host*) { return nullptr; }
extern "C" rsf_backend_result rsf_streamline_host_drain(rsf_streamline_host*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" void* rsf_streamline_host_token(rsf_streamline_host*, uint64_t) { return nullptr; }
extern "C" rsf_backend_result rsf_streamline_host_destroy(rsf_streamline_host*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_begin(rsf_streamline_host*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_acquire(rsf_streamline_host*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_sleep(rsf_streamline_host*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_marker(rsf_streamline_host*, uint64_t, rsf_latency_marker, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_abort(rsf_streamline_host*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" int rsf_streamline_host_presented(rsf_streamline_host*, uint64_t) { return 0; }
extern "C" rsf_backend_result rsf_streamline_host_input(rsf_streamline_host*, uint64_t, uint32_t, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_generation_load(rsf_streamline_host*, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" void* rsf_streamline_host_native(rsf_streamline_host*, void*) { return nullptr; }
extern "C" rsf_backend_result rsf_streamline_host_reflex(rsf_streamline_host*, rsf_reflex_mode, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_latency_status(rsf_streamline_host*, rsf_streamline_latency_status*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_upgrade_chain(rsf_streamline_host*, void**) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
#endif
