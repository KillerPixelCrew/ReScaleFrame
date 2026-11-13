// SPDX-License-Identifier: GPL-3.0-only
#include "fg_stage.h"
#include "fg_inputs.h"
#include "fg_overlay.h"
#include "fg_policy.h"
#include "motion_stage.h"
#include "sr_pass.h"
#include "build_profile.h"
#include "sr_settings.h"
#include "sr_stage.h"
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <commctrl.h>
#pragma comment(lib, "comctl32.lib")
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <ctime>
#include <filesystem>
#include <iterator>
#include <string>

namespace rsf::anomaly::fg {
namespace {
using Microsoft::WRL::ComPtr;
constexpr wchar_t engine_window_class[] = L"_XRAY_1.5";   // xrEngine Device_Initialize.cpp
constexpr uint64_t view_key = 1, viewport_key = 1, rhi_viewport_key = 1;

struct Totals {
    Counters policy{};
    uint64_t runtime_tagged{}, vendor_presented{}, captures{};
};
struct State {
    Options options{};
    std::wstring ini_directory;
    rsf_game_log_fn log{};
    void* user{};
    rsf_d3d11_present_api present{};
    rsf_d3d11_fg_api api{};
    std::atomic<bool> installed{}, install_attempted{};
    std::string streamline, fidelityfx, xess;   // UTF-8, alive for the process (the bridge copies them anyway)
    overlay::PresentSink overlay{};
    std::atomic<bool> overlay_set{};
    uint64_t session{};
    // Engine thread.
    Policy policy{};
    enum class Phase : uint8_t { Idle, Open, Rendering } phase{Phase::Idle};
    uint64_t next_id{}, id{};
    bool window_open{}, input_started{};
    uint32_t input_native_frame{};
    std::atomic<void*> facade{};          // the bridge's engine-facing chain, compared and used only while is_owner
    HWND window{};
    Decision decision{};
    rsf_game_render_pass identity{};
    uint32_t engine_frame{}, width{}, height{};
    sr::ViewCamera camera_now{}, camera_fed{}, camera_frame{};
    LARGE_INTEGER frequency{}, previous_begin{};
    float frame_time{};
    InputPass inputs{};
    void* (*depth_view_source)() = nullptr;
    uint64_t capture_next{}, captured{};
    std::string capture_dir;
    Totals reported{};
    uint64_t runtime_tagged_base{}, vendor_presented_base{};
    // Status for any thread.
    mutable SRWLOCK lock = SRWLOCK_INIT;
    Status status{};
};
State& state() noexcept { static auto* value = new State; return *value; }
// HW.pBaseZB of the verified executable: the hooks that call into this stage only exist on that image. The build is
// the one bootstrap selected by exact SHA-256 (install); without it there is no depth view.
std::atomic<const BuildProfile*> engine_build{};
void* engine_depth_view() noexcept {
    const auto* build = engine_build.load(std::memory_order_acquire);
    if (!build) return nullptr;
    auto* image = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    void* view = nullptr;
    std::memcpy(&view, image + build->sr->hw_base_zb_rva, sizeof(view));
    return view;
}

void say(const char* format, ...) noexcept {
    auto& s = state();
    if (!s.log) return;
    char text[768]{};
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(text, sizeof(text), format, arguments);
    va_end(arguments);
    try { s.log(s.user, text); } catch (...) {}
}
void bridge_log(void*, const char* message) noexcept { auto& s = state(); if (s.log && message) try { s.log(s.user, message); } catch (...) {} }
std::string utf8(const std::wstring& text) {
    if (text.empty()) return {};
    const int size = WideCharToMultiByte(CP_UTF8, 0, text.data(), int(text.size()), nullptr, 0, nullptr, nullptr);
    std::string out(size > 0 ? size_t(size) : 0u, '\0');
    if (size > 0) WideCharToMultiByte(CP_UTF8, 0, text.data(), int(text.size()), out.data(), size, nullptr, nullptr);
    return out;
}
const char* backend_name(uint32_t backend) noexcept {
    switch (backend) {
    case RSF_FG_BACKEND_DLSS: return "DLSS-G";
    case RSF_FG_BACKEND_FSR3: return "FSR3";
    case RSF_FG_BACKEND_FSR4: return "FSR4";
    case RSF_FG_BACKEND_XESS: return "XeSS";
    case RSF_FG_BACKEND_AUTO: return "Auto";
    default: return "Off";
    }
}
uint64_t qpc() noexcept { LARGE_INTEGER value{}; QueryPerformanceCounter(&value); return uint64_t(value.QuadPart); }

bool engine_window(HWND window) noexcept {
    if (!window || !IsWindow(window)) return false;
    DWORD process = 0;
    GetWindowThreadProcessId(window, &process);
    if (process != GetCurrentProcessId()) return false;
    wchar_t name[32]{};
    return GetClassNameW(window, name, int(std::size(name))) > 0 && !std::wcscmp(name, engine_window_class);
}

// ---- bridge callbacks (engine thread, inside the engine's Present on the bridge) --------------------------------------
void keep_borderless_placement(HWND window) noexcept;
int accept_window(void*, void* window) noexcept {
    if (!engine_window(static_cast<HWND>(window))) return 0;
    state().window = static_cast<HWND>(window);
    keep_borderless_placement(static_cast<HWND>(window));
    return 1;
}

// ---- borderless placement ------------------------------------------------------------------------------------------
// X-Ray's borderless mode (rs_screenmode borderless) makes the window WS_POPUP, sizes it to the monitor and moves it to
// the monitor origin (xray-monolith Device_create.cpp:205, Device_destroy.cpp:98, xr_ioc_cmd.cpp:545); it never puts a
// borderless window anywhere else. With the presentation bridge the game presents through a flip-model chain, so DWM
// switches the window's non-client rendering and posts WM_DWMNCRENDERINGCHANGED. uxtheme handles it before the window
// procedure (ThemePreWndProc -> OnOwpPreDwmNcRenderingChanged -> CThemeWnd::EnableDwmNcRendering, a SetWindowPos with
// SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED), and the window lands back at its CW_USEDEFAULT creation position
// (game trace: 0,0 after the engine's call, then 76,76 / 190,190 / 304,304 after uxtheme). The stock blt-model chain never
// changes non-client rendering, so this only happens with the bridge.
// The subclass restores the engine's placement at that moment: a WS_POPUP window without caption that exactly covers a
// monitor but is not at its origin goes back to the origin, synchronously, before the next frame. Windowed mode (caption)
// and exclusive fullscreen (already at the origin) never match.
constexpr UINT_PTR borderless_subclass_id = 0x52534642;   // "RSFB"
bool borderless_off_origin(HWND window, RECT& monitor_rect, RECT& rect) noexcept {
    const auto style = GetWindowLongPtrW(window, GWL_STYLE);
    if (!(style & WS_POPUP) || (style & WS_CAPTION) == WS_CAPTION || IsIconic(window) || !IsWindowVisible(window)) return false;
    MONITORINFO monitor{sizeof(monitor)};
    if (!GetWindowRect(window, &rect) || !GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &monitor)) return false;
    monitor_rect = monitor.rcMonitor;
    const auto& m = monitor_rect;
    return rect.right - rect.left == m.right - m.left && rect.bottom - rect.top == m.bottom - m.top &&
        (rect.left != m.left || rect.top != m.top);
}
LRESULT CALLBACK borderless_subclass(HWND window, UINT message, WPARAM wparam, LPARAM lparam, UINT_PTR, DWORD_PTR) {
    // The Runtime recognises its registered PCL message; the adapter forwards only message IDs.
    input_received(0, message);
    const auto result = DefSubclassProc(window, message, wparam, lparam);
    if (message == WM_DWMNCRENDERINGCHANGED || message == WM_WINDOWPOSCHANGED) {
        static bool correcting = false;
        static uint32_t corrections = 0;
        RECT m{}, rect{};
        if (!correcting && borderless_off_origin(window, m, rect)) {
            correcting = true;
            const bool moved = SetWindowPos(window, nullptr, m.left, m.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) != FALSE;
            correcting = false;
            if (++corrections <= 8)
                say("Anomaly window: borderless window moved to %ld,%ld (%s); restored to its monitor origin %ld,%ld, moved=%d.",
                    rect.left, rect.top, message == WM_DWMNCRENDERINGCHANGED ? "DWM non-client rendering change" : "position change",
                    m.left, m.top, moved ? 1 : 0);
        }
    }
    return result;
}
// The bridge accepts the window on the engine's thread, which owns the window (SetWindowSubclass requires that).
void keep_borderless_placement(HWND window) noexcept {
    static HWND subclassed{};
    if (!window || window == subclassed) return;
    if (SetWindowSubclass(window, &borderless_subclass, borderless_subclass_id, 0)) {
        subclassed = window;
        say("Anomaly window: borderless placement guard installed on the engine window.");
    }
}

void before_present(void*, void* chain) noexcept {
    auto& s = state();
    if (s.overlay_set.load(std::memory_order_acquire) && s.overlay.before_present)
        s.overlay.before_present(s.overlay.user, static_cast<IDXGISwapChain*>(chain));
}
void present_event(void* user, const rsf_observer_present_event* event) noexcept {
    auto& s = state();
    s.api.present_event(user, event);
    if (!event || (event->flags & DXGI_PRESENT_TEST)) return;
    if (!event->completed) {
        if (s.present.is_owner(event->swapchain)) s.facade.store(event->swapchain, std::memory_order_release);
        return;
    }
    if (SUCCEEDED(event->result) && s.phase == State::Phase::Rendering && s.present.is_owner(event->swapchain))
        s.policy.presented();
}
void latency_event(void* user, const rsf_observer_present_event* event) noexcept { state().api.latency_event(user, event); }

// ---- the engine-thread frame -------------------------------------------------------------------------------------------
bool cpu(uint32_t stage, uint32_t kinds = 0, uint32_t message = 0, uint32_t expected = 1, uint32_t flags = 0) noexcept {
    auto& s = state();
    rsf_game_cpu_event event{sizeof(event), stage, s.session, s.id, qpc(), uint64_t(s.frequency.QuadPart), expected, kinds, message,
        RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_INPUT_KEYED | flags};
    return s.api.cpu(&event) != 0;
}
// Frame reservation is an identity fact. Only the native input callback reports INPUT_SAMPLE.
void open_frame() noexcept {
    auto& s = state();
    s.id = ++s.next_id;
    s.input_started = false;
    cpu(RSF_GAME_CPU_FRAME_BEGIN);
    s.phase = State::Phase::Open;
}
rsf_game_render_pass make_identity(uint32_t role) noexcept {
    auto& s = state();
    rsf_game_render_pass pass{};
    pass.struct_size = sizeof(pass);
    pass.role = role;
    pass.session_id = s.session;
    pass.source_frame_id = pass.submission_id = pass.scope_id = s.id;
    pass.family_key = pass.view_key = pass.pass_key = view_key;
    pass.viewport_key = viewport_key;
    pass.native_frame = s.engine_frame;
    pass.screen = s.decision.screen;
    pass.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION |
        (s.decision.reset != Reset::None ? RSF_GAME_RENDER_RESET : 0u);
    pass.render_rect[2] = pass.output_rect[2] = int32_t(s.width);
    pass.render_rect[3] = pass.output_rect[3] = int32_t(s.height);
    return pass;
}
bool facade_live() noexcept {
    auto& s = state();
    void* chain = s.facade.load(std::memory_order_acquire);
    return chain && s.present.is_owner(chain);
}
// The engine's immediate context, through the facade's device. Not retained past the call: a renderer restart must be
// able to release the old device.
ComPtr<ID3D11DeviceContext> immediate() noexcept {
    auto& s = state();
    ComPtr<ID3D11DeviceContext> context;
    if (!facade_live()) return context;
    auto* chain = static_cast<IDXGISwapChain*>(s.facade.load(std::memory_order_acquire));
    ComPtr<ID3D11Device> device;
    if (SUCCEEDED(chain->GetDevice(IID_PPV_ARGS(&device)))) device->GetImmediateContext(&context);
    return context;
}
// The camera record the Runtime hands every provider when no SR pass supplied one (sr_pass.h build_pass without jitter).
bool fill_camera(rsf_game_render_pass& pass, float clip_to_previous[16]) noexcept {
    auto& s = state();
    const auto& current = s.camera_frame;
    if (!current.valid) return false;
    const auto& previous = s.decision.reset != Reset::None || !s.camera_fed.valid ? current : s.camera_fed;
    const auto planes = sr::projection_planes(current.projection);
    if (!planes.valid) return false;
    const auto view = sr::widen(current.view), projection = sr::widen(current.projection);
    sr::MatrixD clip_to_view{}, view_to_world{}, clip_to_world{}, from_previous{};
    const auto view_projection = sr::multiply(view, projection);
    const auto previous_view_projection = sr::multiply(sr::widen(previous.view), sr::widen(previous.projection));
    if (!sr::invert(projection, clip_to_view) || !sr::invert(view, view_to_world) || !sr::invert(view_projection, clip_to_world))
        return false;
    const auto to_previous = sr::multiply(clip_to_world, previous_view_projection);
    if (!sr::invert(to_previous, from_previous)) return false;
    auto& c = pass.camera;
    c.struct_size = sizeof(c);
    c.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    const auto store = [](float (&target)[16], const sr::MatrixD& m) { for (size_t i = 0; i < 16; ++i) target[i] = float(m[i]); };
    store(c.view_to_clip, projection);
    store(c.clip_to_view, clip_to_view);
    store(c.view_to_world, view_to_world);
    store(c.world_to_view, view);
    store(c.clip_to_previous_clip, to_previous);
    store(pass.previous_clip_to_clip, from_previous);
    c.render_width = c.output_width = s.width;
    c.render_height = c.output_height = s.height;
    c.near_plane = planes.near_plane;
    c.far_plane = planes.far_plane;
    c.vertical_fov_radians = planes.vertical_fov;
    c.depth_inverted = 0;
    c.frame_time_seconds = s.frame_time;
    pass.camera_valid = 1;
    for (size_t i = 0; i < 16; ++i) clip_to_previous[i] = float(to_previous[i]);
    return true;
}
// SR off (SRStage 0 or 1): the world was rendered at output size into HW.pBaseZB and the motion target.
bool adapter_inputs(ID3D11DeviceContext* context) noexcept {
    auto& s = state();
    if (sr::options().world_window()) return false;   // Stages 2-5 own the depth; only an SR evaluation can feed FG
    auto* view = static_cast<ID3D11DepthStencilView*>(s.depth_view_source ? s.depth_view_source() : engine_depth_view());
    if (!view) return false;
    ComPtr<ID3D11Resource> resource;
    ComPtr<ID3D11Texture2D> depth;
    view->GetResource(&resource);
    if (!resource || FAILED(resource.As(&depth))) return false;
    InputFrame frame{};
    frame.depth = depth.Get();
    frame.width = s.width; frame.height = s.height;
    const auto target = motion::frame_target();
    if (target.texture && target.written && target.main && target.width == s.width && target.height == s.height)
        frame.motion = static_cast<ID3D11Texture2D*>(target.texture);
    auto identity = make_identity(RSF_GAME_RENDER_SR);
    if (!fill_camera(identity, frame.clip_to_previous_clip)) return false;
    const auto result = s.inputs.run(context, frame, s.log, s.user);
    if (result != InputPass::Result::Ok) {
        static InputPass::Result reported = InputPass::Result::Ok;
        if (result != reported) { reported = result; say("Anomaly FG: SR-off depth/motion pass refused (%u)", unsigned(result)); }
        return false;
    }
    rsf_dlss_frame inputs{};
    inputs.struct_size = sizeof(inputs);
    inputs.abi_version = RSF_DLSS_ABI_VERSION;
    inputs.depth = s.inputs.depth();
    inputs.motion = s.inputs.motion();
    inputs.render_width = inputs.output_width = s.width;
    inputs.render_height = inputs.output_height = s.height;
    inputs.camera_motion_included = 1;
    inputs.reset = s.decision.reset != Reset::None;
    s.api.capture(context, &identity, &inputs);
    return true;
}
void maybe_dump(ID3D11DeviceContext* context) noexcept {
    auto& s = state();
    if (!s.options.capture || s.captured >= s.options.capture_count || s.engine_frame < s.capture_next) return;
    if (s.decision.skip != Skip::None) return;
    char prefix[96]{};
    std::snprintf(prefix, sizeof(prefix), "fg_f%u_s%llu", s.engine_frame, static_cast<unsigned long long>(s.id));
    const auto written = s.api.dump_inputs(context, s.id, s.capture_dir.c_str(), prefix);
    if (written <= 0) return;
    ++s.captured;
    s.capture_next = s.engine_frame + s.options.capture_interval;
    say("Anomaly FG capture %s: %d inputs (HUD-less, depth, motion) -> %s", prefix, int(written), s.capture_dir.c_str());
}
void publish() noexcept {
    auto& s = state();
    rsf_native_fg_status runtime{};
    runtime.struct_size = sizeof(runtime);
    const bool have = s.api.status && s.api.status(&runtime);
    const auto& c = s.policy.counters();
    AcquireSRWLockExclusive(&s.lock);
    auto& out = s.status;
    out.installed = true;
    out.owner = s.present.has_owner() != 0;
    out.backend = s.present.backend();
    out.choices = s.present.backend_choices();
    if (have) {
        out.requested_mode = runtime.requested.mode;
        out.effective_mode = runtime.vendor.effective_mode;
        out.generated = runtime.vendor.effective_generated_frames;
        out.reason = runtime.reason;
        out.runtime_tagged = runtime.tagged_frames;
        out.vendor_presented = runtime.vendor.total_presented;
    }
    out.source_frames = c.source_frames; out.level_frames = c.level_frames; out.tagged = c.tagged; out.fed = c.fed;
    out.presented = c.presented; out.unpresented = c.unpresented; out.captures = s.captured;
    ReleaseSRWLockExclusive(&s.lock);
}
}

// ---- options ------------------------------------------------------------------------------------------------------------
Options read_options(const wchar_t* ini) noexcept try {
    Options o{};
    if (!ini) return o;
    const auto number = [&](const wchar_t* key, uint32_t fallback) { return uint32_t(GetPrivateProfileIntW(L"Anomaly", key, int(fallback), ini)); };
    o.enabled = number(L"FG", 0) != 0;
    // The bridge installs by default so the overlay can turn FG on live (a saved FG=0 used to leave no bridge and no FG
    // choices for the whole session). FGBridge=0 keeps presentation stock unless FG=1.
    o.bridge = o.enabled || number(L"FGBridge", 1) != 0;
    o.backend = number(L"FGBackend", RSF_FG_BACKEND_AUTO);
    if (!rsf_fg_backend_known(o.backend) && o.backend != RSF_FG_BACKEND_AUTO) o.backend = RSF_FG_BACKEND_AUTO;
    o.generated = std::clamp(number(L"FGGeneratedFrames", 1), 1u, 3u);
    o.latency = std::min(number(L"Latency", RSF_REFLEX_ON), uint32_t(RSF_REFLEX_BOOST));
    o.frame_limit_us = std::min(number(L"FrameLimitUs", 0), 1000000u);
    o.scope_hold = std::clamp(number(L"FGScopeHold", 4), 1u, 64u);
    o.debug = number(L"FGDebug", 0) != 0;
    o.development = number(L"FGDevelopment", 0) != 0;
    o.capture = number(L"FGCapture", 0) != 0;
    o.capture_start = number(L"FGCaptureStart", 600);
    o.capture_count = std::min(number(L"FGCaptureCount", 3), 16u);
    o.capture_interval = std::max(number(L"FGCaptureInterval", 300), 1u);
    const auto base = std::filesystem::path(ini).parent_path();
    const auto text = [&](const wchar_t* key, const std::filesystem::path& fallback) {
        wchar_t value[MAX_PATH]{};
        GetPrivateProfileStringW(L"Anomaly", key, L"", value, MAX_PATH, ini);
        return value[0] ? std::wstring(value) : fallback.wstring();
    };
    // Same normalisation as SR: Streamline cannot load plugins from an extended-length "\\?\" directory.
    o.streamline_dir = sr::plain_directory(text(L"SRStreamlineDir", base / L"ReScaleFrame" / L"streamline"));
    o.fidelityfx_dir = sr::plain_directory(text(L"SRFidelityFXDir", base / L"ReScaleFrame" / L"fidelityfx"));
    o.xess_dir = sr::plain_directory(text(L"SRXeSSDir", base / L"ReScaleFrame" / L"xess"));
    std::time_t now = std::time(nullptr);
    std::tm local{};
    localtime_s(&local, &now);
    wchar_t stamp[32]{};
    std::wcsftime(stamp, std::size(stamp), L"%Y%m%d-%H%M%S", &local);
    o.capture_dir = text(L"FGCaptureDir", base / L"RSF-FGCapture" / stamp);
    return o;
} catch (...) { return {}; }

void configure(const Options& options, rsf_game_log_fn log, void* user) noexcept {
    auto& s = state();
    if (s.install_attempted.load()) return;
    s.options = options; s.log = log; s.user = user;
    s.policy = Policy(options.scope_hold);
}
const Options& options() noexcept { return state().options; }

namespace {
bool bind(const rsf_d3d11_present_api& present, const rsf_d3d11_fg_api& api) noexcept {
    auto& s = state();
    if (present.struct_size < sizeof(present) || present.abi_version != RSF_D3D11_PRESENT_API_VERSION ||
        api.struct_size < sizeof(api) || api.abi_version != RSF_D3D11_FG_API_VERSION) return false;
    if (!present.install || !present.is_owner || !present.has_owner || !present.backend || !present.backend_choices ||
        !api.set_log || !api.options || !api.status || !api.reflex_ordering || !api.cpu || !api.pass || !api.bind_source ||
        !api.capture || !api.inputs_ready || !api.dump_inputs || !api.prepare || !api.retire || !api.present_event ||
        !api.latency_event) return false;
    s.present = present; s.api = api;
    return true;
}
bool start_bridge() noexcept {
    auto& s = state();
    QueryPerformanceFrequency(&s.frequency);
    s.session = qpc();
    s.streamline = utf8(s.options.streamline_dir);
    s.fidelityfx = utf8(s.options.fidelityfx_dir);
    s.xess = utf8(s.options.xess_dir);
    s.capture_dir = utf8(s.options.capture_dir);
    s.capture_next = s.options.capture_start;
    s.api.set_log(&bridge_log, nullptr);
    rsf_native_fg_options options{sizeof(options), s.options.enabled ? RSF_FG_FIXED : RSF_FG_OFF, s.options.generated,
        s.options.latency, s.options.frame_limit_us, s.options.debug ? 1u : 0u};
    s.api.options(&options);
    rsf_d3d11_present_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.runtime_directory_utf8 = s.streamline.c_str();
    setup.development_runtime = s.options.development ? 1u : 0u;
    setup.log = &bridge_log;
    setup.accept_window = &accept_window;
    setup.before_present = &before_present;
    setup.present_event = &present_event;
    setup.latency_event = &latency_event;
    setup.retire = s.api.retire;
    setup.prepare = s.api.prepare;
    setup.debug_timing = s.options.debug ? 1u : 0u;
    setup.backend = s.options.backend;
    setup.max_generated_frames = UINT32_MAX;
    // X-Ray: metres, standard (not reversed) device depth with a finite far plane (sr_pass.h, projection_planes).
    setup.depth_inverted = 0; setup.depth_infinite = 0;
    setup.units_to_meters = sr::view_space_to_meters;
    setup.runtime_switching = 1;
    setup.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
    setup.streamline_directory_utf8 = s.streamline.c_str();
    setup.fsr3_directory_utf8 = s.fidelityfx.c_str();
    setup.fsr4_directory_utf8 = s.fidelityfx.c_str();
    setup.xess_directory_utf8 = s.xess.c_str();
    setup.engine_type = uint32_t(RSF_DLSS_ENGINE_CUSTOM);
    setup.engine_version_utf8 = sr::engine_version;
    setup.project_id_utf8 = sr::project_id;
    if (!s.present.install(&setup)) {
        say("Anomaly FG refused: the presentation bridge did not install (backend %s); presentation stays stock.",
            backend_name(s.options.backend));
        return false;
    }
    s.installed.store(true, std::memory_order_release);
    static OverlayLink link{};
    link.foreign_chain = &foreign_chain;
    link.enabled = s.options.enabled ? 1u : 0u; link.backend = s.options.backend; link.generated = s.options.generated;
    link.latency = s.options.latency; link.frame_limit_us = s.options.frame_limit_us;
    overlay_link.store(&link, std::memory_order_release);
    say("Anomaly FG: presentation bridge installed at cold start, before device creation. FG=%u backend=%s generated=%u "
        "Latency=%u FrameLimitUs=%u scope_hold=%u. Built and fixture-tested only; not game-accepted.",
        unsigned(s.options.enabled), backend_name(s.options.backend), s.options.generated, s.options.latency,
        s.options.frame_limit_us, s.options.scope_hold);
    publish();
    return true;
}
}

bool install(HMODULE runtime, const BuildProfile& build) noexcept try {
    auto& s = state();
    if (!s.options.selected() || s.install_attempted.exchange(true)) return false;
    engine_build.store(&build, std::memory_order_release);
    const auto present_query = reinterpret_cast<rsf_get_d3d11_present_api_fn>(
        reinterpret_cast<void*>(GetProcAddress(runtime, RSF_D3D11_PRESENT_API_ENTRY_POINT)));
    const auto fg_query = reinterpret_cast<rsf_get_d3d11_fg_api_fn>(
        reinterpret_cast<void*>(GetProcAddress(runtime, RSF_D3D11_FG_API_ENTRY_POINT)));
    if (!present_query || !fg_query) { say("Anomaly FG refused: the Runtime has no %s.", present_query ? "rsf_get_d3d11_fg_api" : "rsf_get_d3d11_present_api"); return false; }
    rsf_d3d11_present_api present{};
    present.struct_size = sizeof(present); present.abi_version = RSF_D3D11_PRESENT_API_VERSION;
    rsf_d3d11_fg_api api{};
    api.struct_size = sizeof(api); api.abi_version = RSF_D3D11_FG_API_VERSION;
    if (present_query(&present) != RSF_OK || fg_query(&api) != RSF_OK || !bind(present, api)) {
        say("Anomaly FG refused: Runtime FG/presentation tables refused (version or size).");
        return false;
    }
    return start_bridge();
} catch (...) { return false; }

bool install_with(const rsf_d3d11_present_api& present, const rsf_d3d11_fg_api& api) noexcept {
    auto& s = state();
    if (!s.options.selected() || s.install_attempted.exchange(true)) return false;
    if (!bind(present, api)) return false;
    return start_bridge();
}
bool installed() noexcept { return state().installed.load(std::memory_order_acquire); }
void set_depth_view_source(void* (*source)()) noexcept { state().depth_view_source = source; }

void set_overlay_sink(const overlay::PresentSink& sink) noexcept {
    auto& s = state();
    if (s.overlay_set.load()) return;
    s.overlay = sink;
    s.overlay_set.store(true, std::memory_order_release);
}
bool foreign_chain(IDXGISwapChain* chain) noexcept {
    auto& s = state();
    return s.installed.load(std::memory_order_acquire) && s.present.has_owner() && !s.present.is_owner(chain);
}

// ---- native boundaries ----------------------------------------------------------------------------------------------------
void camera(const float* view, const float* projection) noexcept {
    auto& s = state();
    if (!installed() || !view || !projection) return;
    std::memcpy(s.camera_now.view.data(), view, sizeof(float) * 16);
    std::memcpy(s.camera_now.projection.data(), projection, sizeof(float) * 16);
    s.camera_now.valid = true;
    for (size_t i = 0; i < 16; ++i)
        if (!std::isfinite(view[i]) || !std::isfinite(projection[i])) s.camera_now.valid = false;
}

void input_begin(uint32_t engine_frame) noexcept {
    auto& s = state();
    if (!installed() || s.phase == State::Phase::Rendering) return;
    if (s.phase == State::Phase::Open && s.input_started) {
        if (s.input_native_frame == engine_frame) {
            cpu(RSF_GAME_CPU_INPUT_SAMPLE); cpu(RSF_GAME_CPU_SIMULATION_BEGIN); return;
        }
        // A simulation frame can run without rendering while inactive/loading. Close its own identity now.
        cpu(RSF_GAME_CPU_SIMULATION_END, 0, 0, 0);
        cpu(RSF_GAME_CPU_FRAME_END, 0, 0, 0, RSF_GAME_CPU_RENDER_SKIPPED);
        s.phase = State::Phase::Idle;
    }
    if (s.phase == State::Phase::Idle) open_frame();
    s.input_native_frame = engine_frame; s.input_started = true;
    cpu(RSF_GAME_CPU_INPUT_SAMPLE);
    cpu(RSF_GAME_CPU_SIMULATION_BEGIN);
}
void input_received(uint32_t kinds, uint32_t message_id) noexcept {
    auto& s = state();
    if (!installed()) return;
    if (!kinds) {
        // A window message outside a frame has no source identity. The core alone may match or expire it.
        rsf_game_cpu_event event{sizeof(event),RSF_GAME_CPU_INPUT_EVENT,s.session,
            s.phase == State::Phase::Idle ? 0 : s.id,qpc(),uint64_t(s.frequency.QuadPart),0,0,message_id,
            RSF_GAME_CPU_EVENT_VERSION,RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE};
        (void)s.api.cpu(&event);
    } else if (s.phase == State::Phase::Open) cpu(RSF_GAME_CPU_INPUT_EVENT,kinds,message_id);
}

void render_begin(uint32_t engine_frame, uint32_t width, uint32_t height) noexcept {
    auto& s = state();
    if (!installed() || s.phase == State::Phase::Rendering) return;
    if (s.phase == State::Phase::Idle) open_frame(); // Runtime labels a missing input callback as a substitute
    s.engine_frame = engine_frame; s.width = width; s.height = height;
    s.camera_frame = s.camera_now;
    LARGE_INTEGER now{};
    QueryPerformanceCounter(&now);
    s.frame_time = s.previous_begin.QuadPart && s.frequency.QuadPart ?
        float(double(now.QuadPart - s.previous_begin.QuadPart) / double(s.frequency.QuadPart)) : 0.0f;
    s.previous_begin = now;
    bool cut = false;
    if (s.camera_frame.valid && s.camera_fed.valid)
        cut = sr::camera_cut(sr::pose_of(s.camera_fed.view), sr::pose_of(s.camera_frame.view), sr::CutRule{});
    s.decision = s.policy.begin(engine_frame, width, height, cut);
    cpu(RSF_GAME_CPU_SIMULATION_END);
    auto frame = make_identity(RSF_GAME_RENDER_FRAME);
    frame.family_key = frame.view_key = 0;
    s.api.pass(nullptr, &frame, 1);
    const auto submission = make_identity(RSF_GAME_RENDER_SUBMISSION);
    s.api.pass(nullptr, &submission, 1);
    // SR passes built by the SR stage carry no frame identity; while bound they feed this source.
    s.identity = make_identity(RSF_GAME_RENDER_SR);
    s.api.bind_source(&s.identity);
    s.window_open = false;
    s.phase = State::Phase::Rendering;
}

void scope_frame() noexcept {
    auto& s = state();
    if (!installed() || s.phase != State::Phase::Rendering) return;
    s.policy.scope_frame();
    s.decision = s.policy.current();
}

void ui_boundary() noexcept {
    auto& s = state();
    if (!installed() || s.phase != State::Phase::Rendering) return;
    s.decision = s.policy.ui_boundary();
    if (!s.decision.tag) return;
    const auto immediate_context = immediate();
    auto* context = immediate_context.Get();
    if (!context) { s.policy.inputs(false); return; }
    if (!s.api.inputs_ready(s.id)) adapter_inputs(context);
    s.policy.inputs(s.api.inputs_ready(s.id) != 0);
    auto* chain = static_cast<IDXGISwapChain*>(s.facade.load(std::memory_order_acquire));
    ComPtr<ID3D11Texture2D> buffer;
    if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&buffer)))) return;
    // The presented-resolution frame after world post-processing (phase_pp), bullet tracers and the scope copy, before
    // HitMarker, the game UI, fonts, the HUD target and the pause text (CHUDManager::RenderUI) and everything after.
    auto final_scene = make_identity(RSF_GAME_RENDER_FINAL_SCENE);
    final_scene.scene_surface = buffer.Get();
    s.api.pass(context, &final_scene, 1);
    s.api.pass(context, &final_scene, 0);
    maybe_dump(context);
    if (s.decision.skip == Skip::None && s.camera_frame.valid) s.camera_fed = s.camera_frame;
}

void end_before() noexcept {
    auto& s = state();
    if (!installed() || s.phase != State::Phase::Rendering) return;
    const auto submission = make_identity(RSF_GAME_RENDER_SUBMISSION);
    s.api.pass(nullptr, &submission, 0);
    s.api.bind_source(nullptr);
    if (!facade_live()) return;
    auto* chain = static_cast<IDXGISwapChain*>(s.facade.load(std::memory_order_acquire));
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(chain->GetDesc(&desc))) return;
    auto window = make_identity(RSF_GAME_RENDER_WINDOW);
    window.window_key = uint64_t(uintptr_t(desc.OutputWindow));
    window.rhi_viewport_key = rhi_viewport_key;
    window.swapchain = chain;
    window.family_key = window.view_key = 0;
    s.identity = window;
    s.window_open = s.api.pass(nullptr, &window, 1) != 0;
}

void end_after() noexcept {
    auto& s = state();
    if (!installed() || s.phase != State::Phase::Rendering) return;
    if (s.window_open) s.api.pass(nullptr, &s.identity, 0);
    s.window_open = false;
    auto frame = make_identity(RSF_GAME_RENDER_FRAME);
    frame.family_key = frame.view_key = 0;
    s.api.pass(nullptr, &frame, 0);
    cpu(RSF_GAME_CPU_FRAME_END);
    s.policy.end();
    publish();
    s.phase = State::Phase::Idle;
}

void device_reset() noexcept {
    auto& s = state();
    if (!installed()) return;
    s.policy.device_reset();
    s.inputs.release();   // rebuilt at the next SR-off frame, on whatever device then presents
}

void report(void (*emit)(void*, const char*), void* user) noexcept {
    auto& s = state();
    if (!installed() || !emit) return;
    const auto& c = s.policy.counters();
    const auto& r = s.reported.policy;
    rsf_native_fg_status runtime{};
    runtime.struct_size = sizeof(runtime);
    s.api.status(&runtime);
    const auto d = [](uint64_t a, uint64_t b) { return static_cast<unsigned long long>(a - b); };
    char text[1024]{};
    std::snprintf(text, sizeof(text),
        "Anomaly FG window: source=%llu level=%llu tagged=%llu fed=%llu presented=%llu unpresented=%llu scope_frames=%llu "
        "skipped no_level=%llu scope=%llu no_present=%llu no_inputs=%llu resets first=%llu device=%llu discontinuity=%llu "
        "scope_end=%llu size=%llu cut=%llu | runtime tagged=%llu vendor_presented=%llu captures=%llu",
        d(c.source_frames, r.source_frames), d(c.level_frames, r.level_frames), d(c.tagged, r.tagged), d(c.fed, r.fed),
        d(c.presented, r.presented), d(c.unpresented, r.unpresented), d(c.scope_frames, r.scope_frames),
        d(c.skipped[1], r.skipped[1]), d(c.skipped[2], r.skipped[2]), d(c.skipped[3], r.skipped[3]), d(c.skipped[4], r.skipped[4]),
        d(c.resets[1], r.resets[1]), d(c.resets[2], r.resets[2]), d(c.resets[3], r.resets[3]), d(c.resets[4], r.resets[4]),
        d(c.resets[5], r.resets[5]), d(c.resets[6], r.resets[6]),
        d(runtime.tagged_frames, s.reported.runtime_tagged), d(runtime.vendor.total_presented, s.reported.vendor_presented),
        d(s.captured, s.reported.captures));
    emit(user, text);
    std::snprintf(text, sizeof(text),
        "Anomaly FG state: owner=%u backend=%s choices=0x%x switch_result=%d requested_mode=%u generated_requested=%u "
        "effective_mode=%u effective_generated=%u active=%u reason=%u last_result=%d reflex=%u pacing=%u vendor_status=%d",
        unsigned(s.present.has_owner()), backend_name(s.present.backend()), s.present.backend_choices(),
        s.present.switch_result ? int(s.present.switch_result()) : 0, runtime.requested.mode, runtime.requested.generated_frames,
        runtime.vendor.effective_mode, runtime.vendor.effective_generated_frames, runtime.vendor.active, runtime.reason,
        runtime.last_result, runtime.vendor.effective_reflex, runtime.vendor.pacing_owner, runtime.vendor.vendor_status);
    emit(user, text);
    s.reported.policy = c;
    s.reported.runtime_tagged = runtime.tagged_frames;
    s.reported.vendor_presented = runtime.vendor.total_presented;
    s.reported.captures = s.captured;
}

Status status() noexcept {
    auto& s = state();
    AcquireSRWLockShared(&s.lock);
    const auto out = s.status;
    ReleaseSRWLockShared(&s.lock);
    return out;
}
}
