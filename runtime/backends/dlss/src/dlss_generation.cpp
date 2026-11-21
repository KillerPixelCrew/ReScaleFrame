// SPDX-License-Identifier: MIT
#include <rescaleframe/streamline_host.h>
#include "streamline_diagnostics.h"
#include "streamline_frame.h"
#include "reflex_audit.h"
#include "streamline_load.h"
#include "mfg_unlock.h"
#include "../../common/fg_helpers.h"
#include "../../common/d3d12_helpers.h"
#include <wrl/client.h>
#include <dxgi1_6.h>
#include <cmath>
#include <cstddef>
#include <cstdio>
#include <new>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
#include <sl_dlss_g.h>
#include <sl_reflex.h>
#include <sl_pcl.h>
#include <sl_matrix_helpers.h>
#if RSF_HAVE_NVAPI_DIAGNOSTICS
#include <nvapi.h>
#endif
using Microsoft::WRL::ComPtr;

// Plain data exchanged with the Rust owner in runtime/orchestrator/src/streamline_host.rs, which
// owns host and session lifetime, admission, feature routing, frame tokens, marker ordering,
// option/status handoff and Reflex/PCL pacing. Every function below is a typed SDK, COM or Win32
// leaf it calls under its own locks.
struct rsf_slhost_init_preferences {
    const wchar_t* directory;
    const char* engine_version;
    const char* project_id;
    const uint32_t* features;
    uint64_t flags;
    uint32_t feature_count, engine_type, render_api, show_console, log_level;
};
struct rsf_slhost_reflex {
    uint32_t low_latency_available, latency_report_available;
};
struct rsf_slhost_request {
    uint32_t mode, frames, ui_recomposition;
    float dynamic_target_fps;
};
struct rsf_slhost_observed {
    uint32_t max_generated_frames, dynamic_supported, vsync_supported, min_dimension;
    int32_t status;
    uint32_t mfg_status, mfg_maximum;
    uint64_t presented;
    void* fence;
    uint64_t fence_value;
};
using rsf_slhost_required_entries = void*[12];
struct rsf_slhost_startup {
    uint32_t engine_count;
    uint32_t (*interposer_loaded)();
    uint32_t (*adapter_vendor)(void*, uint32_t*, void**);
    void (*report_vendor)(rsf_backend_log_fn, void*, uint32_t);
    void* (*load)(const wchar_t*, const rsf_streamline_host_setup*);
    void (*unload)(void*);
    void* (*allocate)(rsf_backend_log_fn, void*);
    void (*bind_required)(void*, const rsf_slhost_required_entries*);
    uint32_t (*init)(void*, const rsf_slhost_init_preferences*);
    void (*borrow_device)(void*, void*);
    uint32_t (*create_device)(void*, void*);
    void (*prime_mfg)(void*, uint32_t);
    uint32_t (*upgrade_device)(void*);
    uint32_t (*native_device)(void*);
    void (*start_audit)(void*);
    uint32_t (*register_device)(void*);
    uint32_t (*create_factory)(void*);
    uint32_t (*upgrade_factory)(void*);
    uint32_t (*queue_type)(void*);
    uint32_t (*queue_matches_device)(void*, void*, void**);
    uint32_t (*create_queue)(void*);
    uint32_t (*native_queue)(void*);
    uint32_t (*resolve_feature)(void*, uint32_t, const char*, uint32_t);
    void (*prepare_mfg)(void*);
    void (*pcl_defaults)(void*);
};
struct rsf_slhost_platform {
    const rsf_slhost_startup* startup;
    void (*dispose)(void*);
    int32_t (*token)(void*, uint32_t, void**);
    int32_t (*sleep)(void*, void*);
    int32_t (*marker)(void*, void*, uint32_t);
    int32_t (*common)(void*, void*, uint32_t, const void*);
    int32_t (*reflex_state)(void*, uint32_t, const uint64_t*, rsf_slhost_reflex*);
    int32_t (*reflex_options)(void*, uint32_t, uint32_t);
    int32_t (*pcl_message)(void*, uint32_t*);
    void (*driver_sleep)(void*);
    uint32_t (*ring_capture)(void*);
    void (*ring_write)(void*, const char*, uint64_t);
    int32_t (*generation_load)(void*, uint32_t, uint32_t, uint32_t*);
    uint32_t (*same_graphics)(void*, void*, void*);
    int32_t (*support)(void*, rsf_backend_log_fn, void*);
    void (*version)(void*, rsf_fg_status*);
    int32_t (*chain)(void*, const rsf_fg_swapchain_desc*, void**);
    int32_t (*options)(void*, const rsf_slhost_request*);
    void (*off)(void*);
    int32_t (*observe)(void*, rsf_slhost_observed*);
    int32_t (*frame)(void*, const rsf_fg_frame*);
    int32_t (*command)(void*, void*);
    int32_t (*constants)(void*, void*, const rsf_fg_frame*, uint32_t, uint32_t);
    int32_t (*tags)(void*, void*, void*, const rsf_fg_frame*);
};
static_assert(sizeof(rsf_slhost_platform) == 24 * sizeof(void*) &&
    sizeof(rsf_slhost_startup) == 25 * sizeof(void*) && offsetof(rsf_slhost_startup, interposer_loaded) == sizeof(void*));
static_assert(sizeof(rsf_slhost_init_preferences) == 64 && offsetof(rsf_slhost_init_preferences, flags) == 32 &&
    offsetof(rsf_slhost_init_preferences, log_level) == 56);
static_assert(sizeof(rsf_slhost_reflex) == 8 &&
    sizeof(rsf_slhost_request) == 16 && sizeof(rsf_slhost_observed) == 56 &&
    offsetof(rsf_slhost_observed, presented) == 32);
static_assert(sizeof(rsf_streamline_host_setup) == 80 && offsetof(rsf_streamline_host_setup, engine_type) == 24 &&
    offsetof(rsf_streamline_host_setup, engine_version_utf8) == 32 && offsetof(rsf_streamline_host_setup, log) == 56 &&
    offsetof(rsf_streamline_host_setup, profile) == 72 && RSF_STREAMLINE_HOST_ABI_VERSION == 2 &&
    RSF_SL_PROFILE_DLSS_FG == 0 && RSF_SL_PROFILE_PCL == 2);
static_assert(sl::MAX_FRAMES_IN_FLIGHT == 6);
static_assert(sl::kFeatureDLSS == 0 && sl::kFeatureReflex == 3 && sl::kFeaturePCL == 4 && sl::kFeatureDLSS_G == 1000 &&
    uint64_t(sl::PreferenceFlags::eDisableDebugText) == (1u << 1) &&
    uint64_t(sl::PreferenceFlags::eUseManualHooking) == (1u << 2) &&
    uint64_t(sl::PreferenceFlags::eUseFrameBasedResourceTagging) == (1u << 7) &&
    uint32_t(sl::RenderAPI::eD3D12) == 1 && uint32_t(sl::LogLevel::eDefault) == 1 &&
    uint32_t(sl::LogLevel::eVerbose) == 2 && D3D12_COMMAND_LIST_TYPE_DIRECT == 0);
static_assert(uint32_t(sl::PCLMarker::eSimulationStart) == RSF_LATENCY_SIMULATION_START &&
    uint32_t(sl::PCLMarker::ePresentEnd) == RSF_LATENCY_PRESENT_END && RSF_LATENCY_INPUT_SAMPLE == 6 &&
    uint32_t(sl::PCLMarker::ePCLatencyPing) == 8 && uint32_t(sl::PCLMarker::eControllerInputSample) == 13);
static_assert(uint32_t(sl::DLSSGStatus::eOk) == 0 && uint32_t(sl::DLSSGMode::eOff) == RSF_FG_OFF &&
    uint32_t(sl::DLSSGMode::eOn) == RSF_FG_FIXED && uint32_t(sl::DLSSGMode::eAuto) == RSF_FG_AUTO &&
    uint32_t(sl::DLSSGMode::eDynamic) == RSF_FG_DYNAMIC);
static_assert(RSF_MFG_UNLOCK_READY == 1 && RSF_MFG_UNLOCK_REFUSED == 2 && RSF_FG_STAT_LATENCY_AVAILABLE == 4 &&
    RSF_UI_MODE_NONE == 1 && RSF_UI_MODE_BACKBUFFER_HUDLESS == 4 && RSF_UI_MODE_BACKBUFFER_HUDLESS_UI == 8);

extern "C" {
int32_t rsf_core_slhost_create(const rsf_streamline_host_setup*, void*, void*, rsf_streamline_host**,
    const rsf_slhost_platform*);
int32_t rsf_core_slhost_destroy(rsf_streamline_host*, const rsf_slhost_platform*);
void* rsf_core_slhost_native(rsf_streamline_host*);
void* rsf_core_slhost_module(const rsf_streamline_host*);
void rsf_core_vk_sdk_unload(void*);
void* rsf_core_slhost_token(rsf_streamline_host*, uint64_t);
int32_t rsf_core_slhost_begin(rsf_streamline_host*, uint64_t, const rsf_slhost_platform*);
int32_t rsf_core_slhost_acquire(rsf_streamline_host*, uint64_t, const rsf_slhost_platform*);
int32_t rsf_core_slhost_sleep(rsf_streamline_host*, uint64_t, const rsf_slhost_platform*);
int32_t rsf_core_slhost_marker(rsf_streamline_host*, uint64_t, uint32_t, uint32_t, const rsf_slhost_platform*);
uint32_t rsf_core_slhost_common(rsf_streamline_host*, uint64_t, uint32_t, const void*, const rsf_slhost_platform*);
int32_t rsf_core_slhost_presented(rsf_streamline_host*, uint64_t);
int32_t rsf_core_slhost_input(rsf_streamline_host*, uint64_t, uint32_t, uint32_t, const rsf_slhost_platform*);
int32_t rsf_core_slhost_abort(rsf_streamline_host*, uint64_t);
int32_t rsf_core_slhost_reset_sources(rsf_streamline_host*);
int32_t rsf_core_slhost_retire_through(rsf_streamline_host*, uint64_t);
int32_t rsf_core_slhost_reflex(rsf_streamline_host*, uint32_t, uint32_t, const rsf_slhost_platform*);
int32_t rsf_core_slhost_latency_status(rsf_streamline_host*, rsf_streamline_latency_status*, const rsf_slhost_platform*);
int32_t rsf_core_slhost_generation_load(rsf_streamline_host*, uint32_t, const rsf_slhost_platform*);
int32_t rsf_core_slhost_session_create(const rsf_generation_setup*, void**, void**, const rsf_slhost_platform*);
int32_t rsf_core_slhost_session_configure(void*, const rsf_fg_options*, const rsf_slhost_platform*);
int32_t rsf_core_slhost_session_begin(void*, uint64_t, const rsf_slhost_platform*);
int32_t rsf_core_slhost_session_marker(void*, uint32_t, uint64_t, uint32_t, const rsf_slhost_platform*);
int32_t rsf_core_slhost_session_prepare(void*, void*, const rsf_fg_frame*, const rsf_slhost_platform*);
int32_t rsf_core_slhost_session_after(void*, const rsf_slhost_platform*);
int32_t rsf_core_slhost_session_status(void*, rsf_fg_status*);
int32_t rsf_core_slhost_session_retirement(void*, rsf_fg_retirement*);
void rsf_core_slhost_session_destroy(void*, const rsf_slhost_platform*);
int32_t rsf_core_slhost_session_abort(void*, uint64_t);
}

namespace {
constexpr uint32_t entry_get_state = 1, entry_set_options = 2, entry_reflex_options = 4, entry_reflex_state = 8,
    entry_sleep = 16, entry_pcl_marker = 32, entry_pcl_state = 64, entry_pcl_options = 128;

// The typed SDK, COM and Win32 state of one host.
struct Native {
    bool initialized = false;
    rsf_backend_log_fn log = nullptr;
    void* log_user = nullptr;
    ComPtr<ID3D12Device> device, native_device;
    ComPtr<ID3D12CommandQueue> queue, native_queue;
    ComPtr<IDXGIFactory2> factory;
    const wchar_t* directory{};
    const wchar_t* paths[1]{};
    PFun_slInit* init{};
    PFun_slShutdown* shutdown{};
    PFun_slUpgradeInterface* upgrade{};
    PFun_slGetNativeInterface* native{};
    PFun_slSetD3DDevice* set_device{};
    PFun_slIsFeatureSupported* supported{};
    PFun_slGetFeatureFunction* function{};
    PFun_slGetFeatureVersion* version{};
    PFun_slSetFeatureLoaded* loaded{};
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
    HRESULT (*common_present)(IDXGISwapChain*, UINT, UINT, bool&){};
    HRESULT (*common_after_present)(UINT){};
    rsf::MfgUnlock mfg;
};
// One driver ring snapshot. The Rust owner serializes its capture and write.
sl::ReflexState ring;

Native& as_native(void* native) { return *static_cast<Native*>(native); }
sl::FrameToken& as_token(void* token) { return *static_cast<sl::FrameToken*>(token); }
Native* native_of(rsf_streamline_host* host) { return static_cast<Native*>(rsf_core_slhost_native(host)); }

uint32_t resolved(const Native& host)
{
    return (host.get_state ? entry_get_state : 0) | (host.set_options ? entry_set_options : 0) |
        (host.reflex_options ? entry_reflex_options : 0) | (host.reflex_state ? entry_reflex_state : 0) |
        (host.sleep ? entry_sleep : 0) | (host.pcl_marker ? entry_pcl_marker : 0) | (host.pcl_state ? entry_pcl_state : 0);
}

void report_driver_sleep(Native& host)
{
#if RSF_HAVE_NVAPI_DIAGNOSTICS
    // Query only the module Streamline already initialized. Never create a second Reflex owner.
    const auto query = rsf_nvapi_query();
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
void message(sl::LogType type, const char* text)
{
    rsf_core_streamline_log_emit(RSF_STREAMLINE_LOG_D3D12_HOST,
        uint32_t(type == sl::LogType::eError || type == sl::LogType::eWarn),
        text, rsf::emit_streamline_log);
}
template<class T> bool feature(Native& host, sl::Feature id, const char* name, T*& out)
{
    void* pointer = nullptr;
    if (host.function(id, name, pointer) != sl::Result::eOk || !pointer) return false;
    out = reinterpret_cast<T*>(pointer); return true;
}
void dispose(Native* host)
{
    rsf_reflex_audit_stop();
    host->queue.Reset(); host->native_queue.Reset(); host->factory.Reset();
    host->device.Reset(); host->native_device.Reset();
    if (host->initialized) host->shutdown();
    host->mfg.release();
    rsf_core_streamline_log_clear(RSF_STREAMLINE_LOG_D3D12_HOST, uintptr_t(host));
    delete host;
}
// Borrowed native alias, valid through the supplied proxy object's lifetime.
void* native_alias(Native& host, void* proxy)
{
    void* native = nullptr;
    if (host.native(proxy, &native) != sl::Result::eOk || !native) return nullptr;
    static_cast<IUnknown*>(native)->Release(); return native;
}
rsf_backend_result upgrade_chain(Native& host, void** chain)
{
    void* native = native_alias(host, *chain);
    if (!native) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const bool upgraded = native != *chain;
    return upgraded || host.upgrade(chain) == sl::Result::eOk ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

uint32_t interposer_loaded() noexcept { return GetModuleHandleW(L"sl.interposer.dll") != nullptr; }

uint32_t adapter_vendor(void* candidate, uint32_t* vendor, void** retained) noexcept
{
    ComPtr<IDXGIAdapter> adapter; DXGI_ADAPTER_DESC adapter_description{};
    if (FAILED(static_cast<IUnknown*>(candidate)->QueryInterface(IID_PPV_ARGS(&adapter))) ||
        FAILED(adapter->GetDesc(&adapter_description))) return 0;
    *vendor = adapter_description.VendorId;
    *retained = static_cast<IUnknown*>(adapter.Detach());
    return 1;
}

void report_vendor(rsf_backend_log_fn log, void* user, uint32_t vendor) noexcept
{
    rsf::say(log, user, "Streamline host refused: adapter vendor 0x%04x is not NVIDIA", unsigned(vendor));
}

void* load(const wchar_t* path, const rsf_streamline_host_setup* setup) noexcept
{
    return rsf::load_interposer(path, setup->require_signature != 0, setup->log, setup->log_user);
}

void* allocate(rsf_backend_log_fn log, void* user) noexcept
{
    auto* host = new (std::nothrow) Native;
    if (host) { host->log = log; host->log_user = user; }
    return host;
}

void bind_required(void* native, const rsf_slhost_required_entries* addresses) noexcept
{
    auto& host = as_native(native);
    const auto& [init, shutdown, upgrade, native_interface, set_device, supported, function, version, loaded,
        new_token, constants, tags] = *addresses;
    host.init = reinterpret_cast<PFun_slInit*>(init);
    host.shutdown = reinterpret_cast<PFun_slShutdown*>(shutdown);
    host.upgrade = reinterpret_cast<PFun_slUpgradeInterface*>(upgrade);
    host.native = reinterpret_cast<PFun_slGetNativeInterface*>(native_interface);
    host.set_device = reinterpret_cast<PFun_slSetD3DDevice*>(set_device);
    host.supported = reinterpret_cast<PFun_slIsFeatureSupported*>(supported);
    host.function = reinterpret_cast<PFun_slGetFeatureFunction*>(function);
    host.version = reinterpret_cast<PFun_slGetFeatureVersion*>(version);
    host.loaded = reinterpret_cast<PFun_slSetFeatureLoaded*>(loaded);
    host.new_token = reinterpret_cast<PFun_slGetNewFrameToken*>(new_token);
    host.constants = reinterpret_cast<PFun_slSetConstants*>(constants);
    host.tags = reinterpret_cast<PFun_slSetTagForFrame*>(tags);
}

uint32_t init(void* native, const rsf_slhost_init_preferences* request) noexcept
{
    auto& host = as_native(native);
    host.directory = request->directory;
    host.paths[0] = host.directory;
    sl::Preferences preferences{};
    preferences.flags = static_cast<sl::PreferenceFlags>(request->flags);
    preferences.renderAPI = static_cast<sl::RenderAPI>(request->render_api);
    preferences.showConsole = request->show_console != 0;
    preferences.logLevel = static_cast<sl::LogLevel>(request->log_level);
    rsf_core_streamline_log_set(RSF_STREAMLINE_LOG_D3D12_HOST, uintptr_t(native),
        host.log, host.log_user, uint32_t(preferences.showConsole));
    preferences.logMessageCallback = message;
    preferences.featuresToLoad = request->features; preferences.numFeaturesToLoad = request->feature_count;
    preferences.pathsToPlugins = host.paths; preferences.numPathsToPlugins = 1;
    preferences.engine = static_cast<sl::EngineType>(request->engine_type);
    preferences.engineVersion = request->engine_version; preferences.projectId = request->project_id;
    host.initialized = host.init(preferences, sl::kSDKVersion) == sl::Result::eOk;
    return host.initialized;
}

void borrow_device(void* native, void* borrowed) noexcept
{
    auto* device = static_cast<ID3D12Device*>(borrowed);
    device->AddRef();
    as_native(native).device.Attach(device);
}

uint32_t create_device(void* native, void* adapter) noexcept
{
    ID3D12Device* device = nullptr;
    if (FAILED(D3D12CreateDevice(static_cast<IUnknown*>(adapter), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device))))
        return 0;
    as_native(native).device.Attach(device);
    return 1;
}

void prime_mfg(void* native, uint32_t require_signature) noexcept
{
    auto& host = as_native(native);
    host.mfg.prime(host.device.Get(), host.directory, require_signature != 0, host.log, host.log_user);
}

template<class T> uint32_t upgrade_owned(Native& host, ComPtr<T>& object)
{
    auto* original = object.Detach();
    void* upgraded = original;
    if (host.upgrade(&upgraded) != sl::Result::eOk) { original->Release(); return 0; }
    object.Attach(static_cast<T*>(upgraded));
    return 1;
}

uint32_t upgrade_device(void* native) noexcept
{
    auto& host = as_native(native);
    return upgrade_owned(host, host.device);
}

template<class T> uint32_t retain_native(Native& host, T* proxy, ComPtr<T>& object)
{
    void* native = nullptr;
    if (host.native(proxy, &native) != sl::Result::eOk || !native) return 0;
    object.Attach(static_cast<T*>(native));
    return 1;
}

uint32_t native_device(void* native) noexcept
{
    auto& host = as_native(native);
    return retain_native(host, host.device.Get(), host.native_device);
}

void start_audit(void* native) noexcept
{
    auto& host = as_native(native);
    rsf_reflex_audit_start(host.log, host.log_user);
}

uint32_t register_device(void* native) noexcept
{
    auto& host = as_native(native);
    return host.set_device(host.device.Get()) == sl::Result::eOk;
}

uint32_t create_factory(void* native) noexcept
{
    IDXGIFactory2* factory = nullptr;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) return 0;
    as_native(native).factory.Attach(factory);
    return 1;
}

uint32_t upgrade_factory(void* native) noexcept
{
    auto& host = as_native(native);
    return upgrade_owned(host, host.factory);
}

uint32_t queue_type(void* queue) noexcept
{
    return uint32_t(static_cast<ID3D12CommandQueue*>(queue)->GetDesc().Type);
}

uint32_t queue_matches_device(void* native, void* queue, void** retained) noexcept
{
    ComPtr<ID3D12Device> queue_device;
    const bool matches = SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->GetDevice(IID_PPV_ARGS(&queue_device))) &&
        queue_device.Get() == as_native(native).native_device.Get();
    *retained = static_cast<IUnknown*>(queue_device.Detach());
    return matches;
}

uint32_t create_queue(void* native) noexcept
{
    auto& host = as_native(native);
    D3D12_COMMAND_QUEUE_DESC description{}; description.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    return SUCCEEDED(host.device->CreateCommandQueue(&description, IID_PPV_ARGS(&host.queue)));
}

uint32_t native_queue(void* native) noexcept
{
    auto& host = as_native(native);
    return retain_native(host, host.queue.Get(), host.native_queue);
}

uint32_t resolve_feature(void* native, uint32_t id, const char* name, uint32_t entry) noexcept
{
    auto& host = as_native(native);
    switch (entry) {
    case entry_get_state: return feature(host, id, name, host.get_state);
    case entry_set_options: return feature(host, id, name, host.set_options);
    case entry_reflex_options: return feature(host, id, name, host.reflex_options);
    case entry_reflex_state: return feature(host, id, name, host.reflex_state);
    case entry_sleep: return feature(host, id, name, host.sleep);
    case entry_pcl_marker: return feature(host, id, name, host.pcl_marker);
    case entry_pcl_state: return feature(host, id, name, host.pcl_state);
    case entry_pcl_options: return feature(host, id, name, host.pcl_options);
    default: return 0;
    }
}

void prepare_mfg(void* native) noexcept
{
    auto& host = as_native(native);
    host.mfg.prepare(host.native_device.Get(), reinterpret_cast<void*>(host.get_state), host.log, host.log_user);
}

void pcl_defaults(void* native) noexcept
{
    sl::PCLOptions options{};
    as_native(native).pcl_options(options);
}

void dispose_native(void* native) noexcept { dispose(static_cast<Native*>(native)); }

int32_t token(void* native, uint32_t index, void** out) noexcept
{
    sl::FrameToken* token = nullptr;
    if (as_native(native).new_token(token, &index) != sl::Result::eOk || !token) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    *out = token; return RSF_BACKEND_OK;
}

int32_t sleep(void* native, void* token) noexcept
{
    auto& host = as_native(native);
    return host.sleep && host.sleep(as_token(token)) == sl::Result::eOk ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

int32_t marker(void* native, void* token, uint32_t marker) noexcept
{
    auto& host = as_native(native);
    return host.pcl_marker && host.pcl_marker(static_cast<sl::PCLMarker>(marker), as_token(token)) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

int32_t common(void* native, void* token, uint32_t viewport, const void* constants) noexcept
{
    return as_native(native).constants(*static_cast<const sl::Constants*>(constants), as_token(token),
        sl::ViewportHandle(viewport)) == sl::Result::eOk ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

int32_t reflex_state(void* native, uint32_t report, const uint64_t* inputs, rsf_slhost_reflex* out) noexcept
{
    auto& host = as_native(native);
    sl::ReflexState reflex{};
    if (!host.reflex_state || host.reflex_state(reflex) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    out->low_latency_available = reflex.lowLatencyAvailable;
    out->latency_report_available = reflex.latencyReportAvailable;
    if (!report || !host.log) return RSF_BACKEND_OK;
    const sl::ReflexReport* latest = &reflex.frameReport[0];
    for (const auto& item : reflex.frameReport) if (item.frameID > latest->frameID) latest = &item;
    const auto ms = [](uint64_t end, uint64_t start) { return end >= start && start ? double(end - start) / 1000.0 : -1.0; };
    char text[440];
    std::snprintf(text, sizeof(text),
        "Reflex driver report available=%u frame=%llu sim_ms=%.2f render_ms=%.2f present_ms=%.2f gpu_ms=%.2f gpu_active_us=%u gpu_frame_us=%u inputs_keyboard=%llu mouse=%llu controller=%llu pcl_ping=%llu",
        unsigned(reflex.latencyReportAvailable), static_cast<unsigned long long>(latest->frameID),
        ms(latest->simEndTime, latest->simStartTime), ms(latest->renderSubmitEndTime, latest->renderSubmitStartTime),
        ms(latest->presentEndTime, latest->presentStartTime), ms(latest->gpuRenderEndTime, latest->gpuRenderStartTime),
        latest->gpuActiveRenderTimeUs, latest->gpuFrameTimeUs,
        static_cast<unsigned long long>(inputs[0]), static_cast<unsigned long long>(inputs[1]),
        static_cast<unsigned long long>(inputs[2]), static_cast<unsigned long long>(inputs[3]));
    host.log(host.log_user, text);
    report_driver_sleep(host);
    return RSF_BACKEND_OK;
}

int32_t reflex_options(void* native, uint32_t mode, uint32_t limit) noexcept
{
    auto& host = as_native(native);
    sl::ReflexOptions options{}; options.frameLimitUs = limit;
    options.mode = mode == RSF_REFLEX_BOOST ? sl::ReflexMode::eLowLatencyWithBoost :
        mode == RSF_REFLEX_ON ? sl::ReflexMode::eLowLatency : sl::ReflexMode::eOff;
    if (!host.reflex_options || host.reflex_options(options) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (host.log) {
        char text[160]; std::snprintf(text, sizeof(text),
            "Reflex options applied: mode=%u frameLimitUs=%u markerOptimization=%u",
            mode, limit, uint32_t(options.useMarkersToOptimize));
        host.log(host.log_user, text);
        report_driver_sleep(host);
    }
    return RSF_BACKEND_OK;
}

int32_t pcl_message(void* native, uint32_t* out) noexcept
{
    auto& host = as_native(native);
    sl::PCLState state{};
    if (!host.pcl_state || host.pcl_state(state) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    *out = state.statsWindowMessage; return RSF_BACKEND_OK;
}

void driver_sleep(void* native) noexcept { report_driver_sleep(as_native(native)); }

// The driver's own record of its last 64 frames: marker times as it received them and its
// queue and GPU spans. Written when a sleep stalls, and a few times while it does not, to show
// what the driver saw before it chose its interval.
uint32_t ring_capture(void* native) noexcept
{
    auto& host = as_native(native);
    ring = {};
    return host.reflex_state && host.reflex_state(ring) == sl::Result::eOk && ring.latencyReportAvailable;
}

void ring_write(void* native, const char* reason, uint64_t source) noexcept
{
    auto& host = as_native(native);
    if (!host.log) return;
    char text[520];
    std::snprintf(text, sizeof(text),
        "Reflex ring %s source=%llu: one line per driver frame, microseconds from its SimulationStart; -1 means not recorded",
        reason, static_cast<unsigned long long>(source));
    host.log(host.log_user, text);
    uint64_t previous_sim = 0;
    for (int i = 0; i < sl::kReflexFrameReportCount; ++i) {
        const auto& r = ring.frameReport[i];
        if (!r.frameID || !r.simStartTime) continue;
        const auto at = [&r](uint64_t time) { return time ? static_cast<long long>(time - r.simStartTime) : -1ll; };
        std::snprintf(text, sizeof(text),
            "Reflex ring frame=%llu sim_interval=%lld sim_end=%lld submit=%lld..%lld present=%lld..%lld driver=%lld..%lld os_queue=%lld..%lld gpu=%lld..%lld gpu_active=%u gpu_frame=%u copy=%u",
            static_cast<unsigned long long>(r.frameID),
            previous_sim ? static_cast<long long>(r.simStartTime - previous_sim) : -1ll, at(r.simEndTime),
            at(r.renderSubmitStartTime), at(r.renderSubmitEndTime), at(r.presentStartTime), at(r.presentEndTime),
            at(r.driverStartTime), at(r.driverEndTime), at(r.osRenderQueueStartTime), at(r.osRenderQueueEndTime),
            at(r.gpuRenderStartTime), at(r.gpuRenderEndTime), r.gpuActiveRenderTimeUs, r.gpuFrameTimeUs,
            ring.frameReport2[i].crossAdapterCopyTimeUs);
        host.log(host.log_user, text);
        previous_sim = r.simStartTime;
    }
}

// FEATURE_FAILED means the plugin state did not change. Any other result reports the
// feature functions now resolved.
int32_t generation_load(void* native, uint32_t enabled, uint32_t require_signature, uint32_t* entries) noexcept
{
    auto& host = as_native(native);
    if (enabled) host.mfg.prime(host.native_device.Get(), host.directory, require_signature != 0, host.log, host.log_user);
    if (host.loaded(sl::kFeatureDLSS_G, enabled != 0) != sl::Result::eOk) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    host.get_state = nullptr; host.set_options = nullptr;
    const bool found = !enabled || (feature(host, sl::kFeatureDLSS_G, "slDLSSGGetState", host.get_state) &&
        feature(host, sl::kFeatureDLSS_G, "slDLSSGSetOptions", host.set_options));
    *entries = resolved(host);
    if (!found) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    if (enabled) host.mfg.prepare(host.native_device.Get(), reinterpret_cast<void*>(host.get_state), host.log, host.log_user);
    return RSF_BACKEND_OK;
}

uint32_t same_graphics(void* native, void* device, void* queue) noexcept
{
    auto& host = as_native(native);
    return device == host.device.Get() && queue == host.queue.Get();
}

int32_t support(void* native, rsf_backend_log_fn log, void* user) noexcept
{
    auto& host = as_native(native);
    const auto luid = host.native_device->GetAdapterLuid();
    sl::AdapterInfo info{}; info.deviceLUID = reinterpret_cast<uint8_t*>(const_cast<LUID*>(&luid));
    info.deviceLUIDSizeInBytes = sizeof(luid);
    const auto fg_support = host.supported(sl::kFeatureDLSS_G, info);
    const auto reflex_support = host.supported(sl::kFeatureReflex, info);
    if (fg_support == sl::Result::eOk && reflex_support == sl::Result::eOk) return RSF_BACKEND_OK;
    if (log) {
        char text[160]{}; std::snprintf(text, sizeof(text), "DLSS-G support=%u Reflex support=%u", unsigned(fg_support), unsigned(reflex_support));
        log(user, text);
    }
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
}

void version(void* native, rsf_fg_status* status) noexcept
{
    sl::FeatureVersion version{};
    if (as_native(native).version(sl::kFeatureDLSS_G, version) == sl::Result::eOk && version.versionNGX) {
        const auto& ngx = version.versionNGX;
        status->version_id = (uint64_t(ngx.major) << 48) | (uint64_t(ngx.minor) << 32) | ngx.build;
        std::snprintf(status->version_name, sizeof(status->version_name), "DLSS-G NGX %u.%u.%u (SL %u.%u.%u)",
            ngx.major, ngx.minor, ngx.build, version.versionSL.major, version.versionSL.minor, version.versionSL.build);
    } else {
        std::snprintf(status->version_name, sizeof(status->version_name), "DLSS-G runtime version unavailable");
    }
}

// Returns one owned, upgraded IDXGISwapChain4 reference.
int32_t chain(void* native, const rsf_fg_swapchain_desc* desc, void** out) noexcept
{
    auto& host = as_native(native);
    auto description = rsf::chain_description(*desc);
    ComPtr<IDXGISwapChain1> swap;
    if (FAILED(host.factory->CreateSwapChainForHwnd(host.queue.Get(), static_cast<HWND>(desc->hwnd),
        &description, nullptr, nullptr, &swap))) return RSF_BACKEND_ERROR_INIT_FAILED;
    void* raw = swap.Detach();
    if (upgrade_chain(host, &raw) != RSF_BACKEND_OK) {
        static_cast<IUnknown*>(raw)->Release(); return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    auto* upgraded = static_cast<IDXGISwapChain1*>(raw);
    IDXGISwapChain4* chain4 = nullptr;
    const auto hr = upgraded->QueryInterface(IID_PPV_ARGS(&chain4)); upgraded->Release();
    if (FAILED(hr)) return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = chain4; return RSF_BACKEND_OK;
}

int32_t options(void* native, const rsf_slhost_request* request) noexcept
{
    auto& host = as_native(native);
    sl::DLSSGOptions fg{}; fg.mode = static_cast<sl::DLSSGMode>(request->mode);
    fg.numFramesToGenerate = request->frames;
    fg.dynamicTargetFrameRate = request->dynamic_target_fps;
    fg.flags = sl::DLSSGFlags::eRetainResourcesWhenOff;
    fg.enableUserInterfaceRecomposition = request->ui_recomposition ? sl::eTrue : sl::eFalse;
    return host.set_options && host.set_options(sl::ViewportHandle(0), fg) == sl::Result::eOk ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

void off(void* native) noexcept
{
    auto& host = as_native(native);
    sl::DLSSGOptions disabled{};
    if (host.set_options) host.set_options(sl::ViewportHandle(0), disabled);
}

int32_t observe(void* native, rsf_slhost_observed* out) noexcept
{
    auto& host = as_native(native);
    sl::DLSSGState sdk{};
    if (!host.get_state || host.get_state(sl::ViewportHandle(0), sdk, nullptr) != sl::Result::eOk)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    out->max_generated_frames = sdk.numFramesToGenerateMax;
    out->dynamic_supported = sdk.bIsDynamicMFGSupported == sl::eTrue;
    out->vsync_supported = sdk.bIsVsyncSupportAvailable == sl::eTrue;
    out->min_dimension = sdk.minWidthOrHeight;
    out->status = static_cast<int32_t>(sdk.status);
    host.mfg.read_status(out->mfg_status, out->mfg_maximum);
    out->presented = sdk.numFramesActuallyPresented;
    out->fence = sdk.inputsProcessingCompletionFence;
    out->fence_value = sdk.lastPresentInputsProcessingCompletionFenceValue;
    return RSF_BACKEND_OK;
}

int32_t frame(void* native, const rsf_fg_frame* frame) noexcept
{
    return rsf::fg_frame(frame, as_native(native).native_device.Get());
}

int32_t command(void* native, void* command) noexcept
{
    auto& host = as_native(native);
    void* native_command = native_alias(host, command);
    if (!native_command) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return rsf::fg_command(native_command, host.native_device.Get());
}

// Builds the frame's common constants. Sets them only when SR has not already done so.
int32_t constants(void* native, void* token, const rsf_fg_frame* frame, uint32_t reset, uint32_t apply) noexcept
{
    const auto& camera = frame->record->camera;
    sl::Constants constants{};
    constants.cameraViewToClip = rsf::sl_matrix(camera.view_to_clip); constants.clipToCameraView = rsf::sl_matrix(camera.clip_to_view);
    constants.clipToPrevClip = rsf::sl_matrix(camera.clip_to_previous_clip);
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
    constants.cameraFwd = rsf::sl_camera_forward(camera,1.0f);
    constants.cameraNear = camera.near_plane; constants.cameraFar = camera.far_plane;
    constants.cameraFOV = camera.vertical_fov_radians; constants.cameraAspectRatio = camera.view_to_clip[5] / camera.view_to_clip[0];
    constants.depthInverted = camera.depth_inverted ? sl::eTrue : sl::eFalse;
    constants.cameraMotionIncluded = sl::eTrue; constants.motionVectors3D = sl::eFalse;
    constants.motionVectorsJittered = camera.motion_jittered ? sl::eTrue : sl::eFalse;
    constants.reset = reset ? sl::eTrue : sl::eFalse;
    if (apply && as_native(native).constants(constants, as_token(token), sl::ViewportHandle(0)) != sl::Result::eOk)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    return RSF_BACKEND_OK;
}

int32_t tags(void* native, void* token, void* command, const rsf_fg_frame* frame) noexcept
{
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
    return as_native(native).tags(as_token(token), sl::ViewportHandle(0), tags, count,
        static_cast<sl::CommandBuffer*>(command)) == sl::Result::eOk ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}

constexpr rsf_slhost_startup startup{
    .engine_count = uint32_t(sl::EngineType::eCount),
    .interposer_loaded = interposer_loaded,
    .adapter_vendor = adapter_vendor,
    .report_vendor = report_vendor,
    .load = load,
    .unload = rsf_core_vk_sdk_unload,
    .allocate = allocate,
    .bind_required = bind_required,
    .init = init,
    .borrow_device = borrow_device,
    .create_device = create_device,
    .prime_mfg = prime_mfg,
    .upgrade_device = upgrade_device,
    .native_device = native_device,
    .start_audit = start_audit,
    .register_device = register_device,
    .create_factory = create_factory,
    .upgrade_factory = upgrade_factory,
    .queue_type = queue_type,
    .queue_matches_device = queue_matches_device,
    .create_queue = create_queue,
    .native_queue = native_queue,
    .resolve_feature = resolve_feature,
    .prepare_mfg = prepare_mfg,
    .pcl_defaults = pcl_defaults,
};
constexpr rsf_slhost_platform platform{&startup,
    dispose_native, token, sleep, marker, common,
    reflex_state, reflex_options, pcl_message, driver_sleep, ring_capture, ring_write, generation_load, same_graphics,
    support, version, chain, options, off, observe, frame, command, constants, tags};

rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    const auto header = rsf::fg_setup_header(setup, out, chain);
    return header != RSF_BACKEND_OK ? header : rsf_core_slhost_session_create(setup, out, chain, &platform);
}
rsf_backend_result configure(void* session, const rsf_fg_options* options)
{
    return rsf_core_slhost_session_configure(session, options, &platform);
}
rsf_backend_result begin(void* session, uint64_t id) { return rsf_core_slhost_session_begin(session, id, &platform); }
rsf_backend_result session_marker(void* session, rsf_latency_marker marker, uint64_t id, uint32_t controller)
{
    return rsf_core_slhost_session_marker(session, marker, id, controller, &platform);
}
rsf_backend_result prepare(void* session, void* command, const rsf_fg_frame* frame)
{
    return rsf_core_slhost_session_prepare(session, command, frame, &platform);
}
rsf_backend_result after(void* session) { return rsf_core_slhost_session_after(session, &platform); }
void destroy(void* session) { rsf_core_slhost_session_destroy(session, &platform); }
const rsf_generation_provider provider{sizeof(provider), create, configure, begin, session_marker, prepare, after,
    rsf_core_slhost_session_status, rsf_core_slhost_session_retirement, destroy, rsf_core_slhost_session_abort};
}

// Internal to this backend module: shared D3D12 DLSS SR sets this frame's common constants.
bool rsf_streamline_common_set(rsf_streamline_host* host, uint64_t id, uint32_t viewport, const sl::Constants& constants)
{
    return rsf_core_slhost_common(host, id, viewport, &constants, &platform) != 0;
}
extern "C" rsf_backend_result rsf_streamline_host_create(const rsf_streamline_host_setup* setup, rsf_streamline_host** out)
{
    return rsf_core_slhost_create(setup, nullptr, nullptr, out, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_adopt(const rsf_streamline_host_setup* setup, void* device, void* queue, rsf_streamline_host** out)
{
    if (!device || !queue) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return rsf_core_slhost_create(setup, device, queue, out, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_graphics(rsf_streamline_host* host, rsf_streamline_graphics* out)
{
    if (!host || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto& native = *native_of(host);
    out->device = native.device.Get(); out->queue = native.queue.Get(); out->factory = native.factory.Get();
    out->native_device = native.native_device.Get(); out->native_queue = native.native_queue.Get(); return RSF_BACKEND_OK;
}
extern "C" void* rsf_streamline_host_module(rsf_streamline_host* host)
{ return rsf_core_slhost_module(host); }
extern "C" rsf_backend_result rsf_streamline_host_drain(rsf_streamline_host* host)
{
    if (!host) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& native = *native_of(host);
    if (!native.queue || !native.device) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    ComPtr<ID3D12Fence> fence;
    if (FAILED(native.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const bool ready = SUCCEEDED(native.queue->Signal(fence.Get(), 1)) && SUCCEEDED(rsf::wait_fence(fence.Get(), 1, event));
    CloseHandle(event);
    return ready ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
extern "C" void* rsf_streamline_host_token(rsf_streamline_host* host, uint64_t id) { return rsf_core_slhost_token(host, id); }
extern "C" rsf_backend_result rsf_streamline_host_destroy(rsf_streamline_host* host)
{
    return rsf_core_slhost_destroy(host, &platform);
}
extern "C" const rsf_generation_provider* rsf_generation_dlss() { return &provider; }
extern "C" rsf_backend_result rsf_streamline_host_begin(rsf_streamline_host* host, uint64_t id)
{
    return rsf_core_slhost_begin(host, id, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_acquire(rsf_streamline_host* host, uint64_t id)
{
    return rsf_core_slhost_acquire(host, id, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_sleep(rsf_streamline_host* host, uint64_t id)
{
    return rsf_core_slhost_sleep(host, id, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_marker(rsf_streamline_host* host, uint64_t id, rsf_latency_marker marker, uint32_t controller)
{
    return rsf_core_slhost_marker(host, id, marker, controller, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_abort(rsf_streamline_host* host, uint64_t id) { return rsf_core_slhost_abort(host, id); }
extern "C" rsf_backend_result rsf_streamline_host_reset_sources(rsf_streamline_host* host) { return rsf_core_slhost_reset_sources(host); }
extern "C" rsf_backend_result rsf_streamline_host_retire_through(rsf_streamline_host* host, uint64_t id)
{
    return rsf_core_slhost_retire_through(host, id);
}
extern "C" rsf_backend_result rsf_streamline_host_present_common(rsf_streamline_host* host,
    void* chain, uint32_t interval, uint32_t flags, uint32_t begin)
{
    if (!host || !chain || begin > 1) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& native = *native_of(host);
    if (!native.initialized) return RSF_BACKEND_ERROR_NOT_READY;
    if (!native.common_present || !native.common_after_present) {
        void* before = nullptr; void* after = nullptr;
        if (native.function(sl::kFeatureCommon, "slHookPresent", before) != sl::Result::eOk ||
            native.function(sl::kFeatureCommon, "slHookAfterPresent", after) != sl::Result::eOk || !before || !after)
            return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
        native.common_present = reinterpret_cast<decltype(native.common_present)>(before);
        native.common_after_present = reinterpret_cast<decltype(native.common_after_present)>(after);
    }
    bool skip = false;
    const auto result = begin ? native.common_present(static_cast<IDXGISwapChain*>(chain), interval, flags, skip) :
        native.common_after_present(flags);
    return SUCCEEDED(result) && !skip ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
extern "C" int rsf_streamline_host_presented(rsf_streamline_host* host, uint64_t id) { return rsf_core_slhost_presented(host, id); }
extern "C" rsf_backend_result rsf_streamline_host_input(rsf_streamline_host* host, uint64_t id, uint32_t kinds, uint32_t message_id)
{
    return rsf_core_slhost_input(host, id, kinds, message_id, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_reflex(rsf_streamline_host* host, rsf_reflex_mode mode, uint32_t limit)
{
    return rsf_core_slhost_reflex(host, mode, limit, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_latency_status(rsf_streamline_host* host, rsf_streamline_latency_status* out)
{
    return rsf_core_slhost_latency_status(host, out, &platform);
}
extern "C" rsf_backend_result rsf_streamline_host_upgrade_chain(rsf_streamline_host* host, void** chain)
{
    if (!host || !chain || !*chain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return upgrade_chain(*native_of(host), chain);
}
extern "C" rsf_backend_result rsf_streamline_host_generation_load(rsf_streamline_host* host, uint32_t enabled)
{
    return rsf_core_slhost_generation_load(host, enabled, &platform);
}
extern "C" void* rsf_streamline_host_native(rsf_streamline_host* host, void* proxy)
{
    if (!host || !proxy) return nullptr;
    return native_alias(*native_of(host), proxy);
}
#else
extern "C" const rsf_generation_provider* rsf_generation_dlss() { return rsf::not_compiled_provider(); }
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
extern "C" rsf_backend_result rsf_streamline_host_reset_sources(rsf_streamline_host*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_retire_through(rsf_streamline_host*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_present_common(rsf_streamline_host*, void*, uint32_t, uint32_t, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" int rsf_streamline_host_presented(rsf_streamline_host*, uint64_t) { return 0; }
extern "C" rsf_backend_result rsf_streamline_host_input(rsf_streamline_host*, uint64_t, uint32_t, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_generation_load(rsf_streamline_host*, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" void* rsf_streamline_host_native(rsf_streamline_host*, void*) { return nullptr; }
extern "C" rsf_backend_result rsf_streamline_host_reflex(rsf_streamline_host*, rsf_reflex_mode, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_latency_status(rsf_streamline_host*, rsf_streamline_latency_status*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
extern "C" rsf_backend_result rsf_streamline_host_upgrade_chain(rsf_streamline_host*, void**) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
#endif
