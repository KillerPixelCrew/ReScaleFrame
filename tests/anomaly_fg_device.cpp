// SPDX-License-Identifier: GPL-3.0-only
// Anomaly frame generation on a real D3D11 device and the real Runtime DLL, through the adapter's production stage
// (games/anomaly/src/fg_stage.cpp) called exactly as native_observer.cpp calls it from the engine boundaries:
//
//   SetCacheXform        fg::camera
//   Render Begin         fg::render_begin          (CPU simulation end, FRAME/SUBMISSION begin, SR source binding)
//   SR seam (Stage 5)    TemporalDriver::evaluate  (sr_temporal.cpp; the Runtime SR pass feeds FG depth/motion)
//   RenderToTarget(SVP)  fg::scope_frame           (scope frames, which do not Present)
//   CHUDManager::RenderUI  fg::ui_boundary         (SR-off depth/motion pass, FINAL_SCENE, HUD-less copy, FG dump)
//   End, before          fg::end_before            (submission end, WINDOW scope)
//   Present              the bridge facade the engine received from D3D11CreateDeviceAndSwapChain (cold start)
//   End, after           fg::end_after             (frame end, next frame begin, the one pacing sleep)
//
// Sequence: gameplay, a main-menu run (no UI boundary), gameplay with the overlay panel drawn in before_present, a
// scope run (every other engine frame an SVP frame without Present), gameplay again, a device reset (ResizeBuffers),
// gameplay, then (--switch) live provider changes. Checks: the NVIDIA adapter is used explicitly (vendor 0x10de) and
// the device and the swap chain's output adapters are recorded; generation is active and presents more frames than
// the engine rendered; scope and menu frames are never tagged by the Runtime; counters are consistent; Present never
// fails. Arguments: <sr: off|5> <backend: 1|3|5> <vendor root> [--switch]. 77 when the vendor root or an NVIDIA
// adapter is missing, or when DLSS-G cannot activate because this process was denied the foreground.
#include "fg_stage.h"
#include "motion_stage.h"
#include "sr_pass.h"
#include "sr_stage.h"
#include "sr_temporal.h"
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <d3d11_1.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

// ---- test doubles for the engine-side stages fg_stage reads (only these two entries) ------------------------------
namespace rsf::anomaly::sr {
StageOptions test_stage{};
const StageOptions& options() noexcept { return test_stage; }
}
namespace rsf::anomaly::motion {
FrameTarget test_target{};
FrameTarget frame_target() noexcept { return test_target; }
}

namespace {
using Microsoft::WRL::ComPtr;
namespace fg = rsf::anomaly::fg;
namespace sr = rsf::anomaly::sr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)
void log_line(void*, const char* message) { std::printf("  | %s\n", message); }
std::wstring widen(const std::string& text) { return std::wstring(text.begin(), text.end()); }

constexpr uint32_t width = 1280, height = 720;
struct Engine {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext1> context;
    ComPtr<IDXGISwapChain> chain;
    ComPtr<ID3D11RenderTargetView> backbuffer;
    ComPtr<ID3D11Texture2D> depth;               // HW.pBaseZB stand-in: D24S8, depth-stencil only
    ComPtr<ID3D11DepthStencilView> depth_view;
    ComPtr<ID3D11Texture2D> motion;              // the motion MRT stand-in (RGBA16F)
    ComPtr<ID3D11RenderTargetView> motion_view;
    // Stage 5: render-size SR inputs.
    uint32_t render_width{}, render_height{};
    ComPtr<ID3D11Texture2D> color, twin, sr_motion, sr_output;
    ComPtr<ID3D11RenderTargetView> color_view, sr_motion_view;
    ComPtr<ID3D11DepthStencilView> twin_view;
    HWND window{};
};
Engine engine;
void* depth_source() { return engine.depth_view.Get(); }

bool make_backbuffer_view() {
    ComPtr<ID3D11Texture2D> buffer;
    engine.backbuffer.Reset();
    return SUCCEEDED(engine.chain->GetBuffer(0, IID_PPV_ARGS(&buffer))) &&
        SUCCEEDED(engine.device->CreateRenderTargetView(buffer.Get(), nullptr, &engine.backbuffer));
}
ComPtr<ID3D11Texture2D> texture(uint32_t w, uint32_t h, DXGI_FORMAT format, UINT bind) {
    D3D11_TEXTURE2D_DESC desc{w, h, 1, 1, format, {1, 0}, D3D11_USAGE_DEFAULT, bind, 0, 0};
    ComPtr<ID3D11Texture2D> out;
    engine.device->CreateTexture2D(&desc, nullptr, &out);
    return out;
}
void describe_adapter(const char* role, IDXGIAdapter* adapter) {
    DXGI_ADAPTER_DESC desc{};
    if (adapter && SUCCEEDED(adapter->GetDesc(&desc)))
        std::printf("%s adapter: %ls (vendor 0x%04x, device 0x%04x, LUID %08lx:%08lx)\n", role, desc.Description, desc.VendorId,
            desc.DeviceId, static_cast<unsigned long>(desc.AdapterLuid.HighPart), static_cast<unsigned long>(desc.AdapterLuid.LowPart));
    else std::printf("%s adapter: unavailable\n", role);
}
// X-Ray conventions (sr_pass.h): row vectors, build_projection_HAT, standard depth.
sr::ViewCamera camera_at(float x) {
    sr::ViewCamera c{};
    c.view = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -x, 0, 0, 1};
    const float n = 0.1f, f = 1000.0f, q = f / (f - n), fov = 1.2f, aspect = float(width) / float(height);
    const float h = 1.0f / std::tan(fov / 2), w = h / aspect;
    c.projection = {w, 0, 0, 0, 0, h, 0, 0, 0, 0, q, 1, 0, 0, -q * n, 0};
    c.valid = true;
    return c;
}
void clear_rect(ID3D11View* view, const float color[4], LONG x, LONG y, LONG w, LONG h) {
    const D3D11_RECT rect{x, y, x + w, y + h};
    engine.context->ClearView(view, color, &rect, 1);
}

enum class Kind { Gameplay, Menu, Scope, Overlay };
struct Totals { uint64_t presents{}, failed_presents{}, vendor_presented_start{}, runtime_tagged_start{}; };

bool foreground_owned() { return GetForegroundWindow() == engine.window; }
bool overlay_open = false;
uint64_t overlay_draws = 0;
void overlay_sink(void*, IDXGISwapChain*) noexcept {
    if (!overlay_open) return;
    const float panel[4]{0.1f, 0.1f, 0.1f, 1.0f};
    clear_rect(engine.backbuffer.Get(), panel, 900, 80, 320, 400);   // after the HUD-less tag, before the upload
    ++overlay_draws;
}
}

int main(int argc, char** argv) {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    if (argc < 4) { std::puts("SKIP: <sr off|5> <backend 1|3|5> <vendor root> [--switch]"); return 77; }
    const bool stage5 = std::strcmp(argv[1], "5") == 0;
    const uint32_t backend = uint32_t(std::strtoul(argv[2], nullptr, 0));
    const std::string root = argv[3];
    const bool switching = argc > 4 && std::strcmp(argv[4], "--switch") == 0;
    if (GetFileAttributesA((root + "\\streamline\\sl.interposer.dll").c_str()) == INVALID_FILE_ATTRIBUTES) {
        std::printf("SKIP: no vendor package at %s\n", root.c_str()); return 77;
    }
    // Explicitly the NVIDIA adapter: on this hybrid laptop adapter 0 is the Intel iGPU.
    ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return 1;
    ComPtr<IDXGIAdapter1> nvidia;
    for (UINT i = 0;; ++i) {
        ComPtr<IDXGIAdapter1> candidate;
        if (factory->EnumAdapters1(i, &candidate) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_ADAPTER_DESC1 desc{};
        candidate->GetDesc1(&desc);
        std::printf("adapter %u: %ls vendor 0x%04x%s\n", i, desc.Description, desc.VendorId, desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE ? " (software)" : "");
        if (!nvidia && desc.VendorId == 0x10de && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) nvidia = candidate;
    }
    if (!nvidia) { std::puts("SKIP: no NVIDIA adapter"); return 77; }

    WNDCLASSW wc{};
    wc.hInstance = GetModuleHandleW(nullptr); wc.lpfnWndProc = DefWindowProcW; wc.lpszClassName = L"_XRAY_1.5";
    if (!RegisterClassW(&wc)) return 1;
    RECT frame{0, 0, LONG(width), LONG(height)};
    AdjustWindowRect(&frame, WS_OVERLAPPEDWINDOW, FALSE);
    engine.window = CreateWindowW(wc.lpszClassName, L"Anomaly FG fixture", WS_OVERLAPPEDWINDOW, 0, 0,
        frame.right - frame.left, frame.bottom - frame.top, nullptr, nullptr, wc.hInstance, nullptr);
    if (!engine.window) return 1;
    ShowWindow(engine.window, SW_SHOW);
    SetForegroundWindow(engine.window);

    // Cold start, as rsf_anomaly_start does it before the engine creates its device.
    fg::Options options{};
    options.bridge = options.enabled = true;
    options.backend = backend; options.generated = 1; options.latency = 1; options.scope_hold = 4;
    options.streamline_dir = widen(root + "\\streamline");
    options.fidelityfx_dir = widen(root + "\\fidelityfx");
    options.xess_dir = widen(root + "\\xess");
    options.capture = true; options.capture_start = 0; options.capture_count = 1; options.capture_interval = 1;
    char temp[MAX_PATH]{};
    GetTempPathA(MAX_PATH, temp);
    const std::string capture_dir = std::string(temp) + "rsf-anomaly-fg-fixture";
    options.capture_dir = widen(capture_dir);
    fg::configure(options, log_line, nullptr);
    fg::set_depth_view_source(&depth_source);
    HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (!runtime || !fg::install(runtime, rsf::anomaly::standard_build)) { std::puts("FAIL: the bridge did not install"); return 1; }
    fg::set_overlay_sink({&overlay_sink, nullptr, nullptr});
    sr::test_stage.stage = stage5 ? 5u : 0u;

    sr::TemporalDriver driver;
    if (stage5) {
        sr::TemporalOptions temporal{};
        temporal.stage = 5;
        temporal.streamline = root + "\\streamline"; temporal.fidelityfx = root + "\\fidelityfx"; temporal.xess = root + "\\xess";
        temporal.initial = {true, sr::backend_dlss, 1, 0};
        driver.configure(temporal, log_line, nullptr);
        if (!driver.bind(runtime)) { std::puts("FAIL: rsf_get_d3d11_sr_api refused"); return 1; }
        driver.request(temporal.initial);
    }

    // The engine's own creation call (dx10HW.cpp): D3D11CreateDeviceAndSwapChain, two buffers, sequential, windowed.
    DXGI_SWAP_CHAIN_DESC desc{};
    desc.BufferDesc = {width, height, {60, 1}, DXGI_FORMAT_R8G8B8A8_UNORM};
    desc.SampleDesc = {1, 0}; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; desc.BufferCount = 2;
    desc.OutputWindow = engine.window; desc.Windowed = TRUE; desc.SwapEffect = DXGI_SWAP_EFFECT_SEQUENTIAL;
    ComPtr<ID3D11DeviceContext> context;
    const D3D_FEATURE_LEVEL levels[]{D3D_FEATURE_LEVEL_11_0};
    if (FAILED(D3D11CreateDeviceAndSwapChain(nvidia.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, 0, levels, 1, D3D11_SDK_VERSION,
            &desc, &engine.chain, &engine.device, nullptr, &context)) || FAILED(context.As(&engine.context))) {
        std::puts("FAIL: D3D11CreateDeviceAndSwapChain"); return 1;
    }
    rsf_d3d11_present_api present{};
    present.struct_size = sizeof(present); present.abi_version = RSF_D3D11_PRESENT_API_VERSION;
    if (rsf_get_d3d11_present_api(&present) != RSF_OK) return 1;
    if (!present.is_owner(engine.chain.Get())) {
        std::puts("SKIP: the bridge refused this chain (no provider for this device); presentation stayed stock");
        return 77;
    }
    std::puts("PASS: D3D11CreateDeviceAndSwapChain returned the bridge's facade at cold start");
    {
        ComPtr<IDXGIDevice> dxgi; ComPtr<IDXGIAdapter> adapter;
        if (SUCCEEDED(engine.device.As(&dxgi)) && SUCCEEDED(dxgi->GetAdapter(&adapter))) describe_adapter("device", adapter.Get());
        ComPtr<IDXGIOutput> output; ComPtr<IDXGIAdapter> output_adapter;
        if (SUCCEEDED(engine.chain->GetContainingOutput(&output)) && SUCCEEDED(output->GetParent(IID_PPV_ARGS(&output_adapter))))
            describe_adapter("swap chain output", output_adapter.Get());
        else describe_adapter("swap chain output", nullptr);
    }
    if (!make_backbuffer_view()) return 1;
    engine.depth = texture(width, height, DXGI_FORMAT_D24_UNORM_S8_UINT, D3D11_BIND_DEPTH_STENCIL);
    engine.motion = texture(width, height, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    if (!engine.depth || !engine.motion || FAILED(engine.device->CreateDepthStencilView(engine.depth.Get(), nullptr, &engine.depth_view)) ||
        FAILED(engine.device->CreateRenderTargetView(engine.motion.Get(), nullptr, &engine.motion_view))) return 1;

    if (stage5) {
        driver.boundary(engine.device.Get(), width, height);
        if (!driver.running() || !driver.planned_render(width, height, engine.render_width, engine.render_height)) {
            std::printf("FAIL: the SR pipeline did not start on the shared host (%s)\n", driver.status().reason.c_str()); return 1;
        }
        std::printf("SR stage 5: render %ux%u -> %ux%u\n", engine.render_width, engine.render_height, width, height);
        engine.color = texture(engine.render_width, engine.render_height, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
        engine.twin = texture(engine.render_width, engine.render_height, DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE);
        engine.sr_motion = texture(engine.render_width, engine.render_height, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
        engine.sr_output = texture(width, height, DXGI_FORMAT(driver.output_format()), D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
        D3D11_DEPTH_STENCIL_VIEW_DESC dsv{};
        dsv.Format = DXGI_FORMAT_D24_UNORM_S8_UINT; dsv.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
        if (!engine.color || !engine.twin || !engine.sr_motion || !engine.sr_output ||
            FAILED(engine.device->CreateRenderTargetView(engine.color.Get(), nullptr, &engine.color_view)) ||
            FAILED(engine.device->CreateRenderTargetView(engine.sr_motion.Get(), nullptr, &engine.sr_motion_view)) ||
            FAILED(engine.device->CreateDepthStencilView(engine.twin.Get(), &dsv, &engine.twin_view))) return 1;
    }

    uint32_t engine_frame = 1000;
    uint64_t sr_evaluated = 0, sr_refused = 0, frames_rendered = 0;
    Totals totals{};
    bool active_seen = false;
    uint64_t active_by_backend[8]{}, generated_windows = 0;
    sr::ViewCamera previous_camera{};
    uint64_t native_frame = 0;
    uint32_t jitter_index = 0;
    auto status = [] { rsf_native_fg_status s{}; s.struct_size = sizeof(s); rsf_d3d11_fg_api api{}; api.struct_size = sizeof(api);
        api.abi_version = RSF_D3D11_FG_API_VERSION; rsf_get_d3d11_fg_api(&api); api.status(&s); return s; };

    auto run_frame = [&](Kind kind, bool scope) {
        ++engine_frame; ++frames_rendered;
        const float x = 0.002f * float(engine_frame);
        const auto camera = camera_at(x);
        fg::camera(camera.view.data(), camera.projection.data());
        fg::render_begin(engine_frame, width, height);
        // World: a moving box over a gradient background; depth 0.5; motion target valid for the box.
        const float t = float(engine_frame % 240) / 240.0f;
        const float background[4]{0.15f + 0.3f * t, 0.25f, 0.45f, 1.0f}, box_color[4]{0.9f, 0.6f, 0.1f, 1.0f};
        engine.context->ClearView(engine.backbuffer.Get(), background, nullptr, 0);
        const LONG box_x = LONG(100 + (engine_frame * 6) % 900);
        clear_rect(engine.backbuffer.Get(), box_color, box_x, 300, 160, 160);
        engine.context->ClearDepthStencilView(engine.depth_view.Get(), D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 0.5f, 0);
        const float invalid[4]{0, 0, 0, 0}, box_motion[4]{6.0f / float(width), 0, 1, 0};
        engine.context->ClearView(engine.motion_view.Get(), invalid, nullptr, 0);
        clear_rect(engine.motion_view.Get(), box_motion, box_x, 300, 160, 160);
        rsf::anomaly::motion::test_target = {engine.motion.Get(), width, height, engine_frame, true, true};
        if (stage5 && !scope) {
            // The SR seam: render-size jittered inputs through the adapter's TemporalDriver and the Runtime pipeline.
            const float color[4]{0.5f, 0.4f, 0.3f, 1.0f}, motion[4]{0, 0, 1, 0};
            engine.context->ClearView(engine.color_view.Get(), color, nullptr, 0);
            engine.context->ClearView(engine.sr_motion_view.Get(), motion, nullptr, 0);
            engine.context->ClearDepthStencilView(engine.twin_view.Get(), D3D11_CLEAR_DEPTH, 0.5f, 0);
            sr::PassInputs in{};
            in.native_frame = ++native_frame;
            in.render_width = engine.render_width; in.render_height = engine.render_height;
            in.output_width = width; in.output_height = height;
            in.current = camera; in.previous = previous_camera.valid ? previous_camera : camera;
            const float halton[8][2]{{0.0f, -0.16f}, {-0.25f, 0.16f}, {0.25f, -0.38f}, {-0.37f, 0.05f}, {0.12f, -0.27f}, {-0.12f, 0.27f}, {0.37f, -0.05f}, {-0.43f, 0.38f}};
            in.jitter.frame = native_frame; in.jitter.applied = true; in.jitter.previous_applied = jitter_index > 0;
            in.jitter.render_width = in.jitter.previous_render_width = engine.render_width;
            in.jitter.render_height = in.jitter.previous_render_height = engine.render_height;
            in.jitter.current_x = halton[jitter_index % 8][0]; in.jitter.current_y = halton[jitter_index % 8][1];
            in.jitter.previous_x = halton[(jitter_index + 7) % 8][0]; in.jitter.previous_y = halton[(jitter_index + 7) % 8][1];
            ++jitter_index;
            in.frame_time_seconds = 1.0f / 60;
            in.color = engine.color.Get(); in.color_output = engine.sr_output.Get();
            in.depth = engine.twin.Get(); in.motion = engine.sr_motion.Get();
            if (driver.evaluate(engine.context.Get(), in, {})) ++sr_evaluated; else ++sr_refused;
            previous_camera = camera;
        }
        if (scope) fg::scope_frame();
        if (kind != Kind::Menu) {
            fg::ui_boundary();
            const float hud[4]{1, 1, 1, 1};
            clear_rect(engine.backbuffer.Get(), hud, 40, 620, 300, 60);   // the HUD, drawn after the HUD-less boundary
        }
        if (stage5) driver.boundary(engine.device.Get(), width, height);   // sr::frame_end's drained boundary
        fg::end_before();
        if (!scope) {
            const HRESULT result = engine.chain->Present(0, 0);
            ++totals.presents;
            if (FAILED(result)) { ++totals.failed_presents; std::printf("Present failed %08lx\n", static_cast<unsigned long>(result)); }
        }
        fg::end_after();
        MSG message{};
        while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) { TranslateMessage(&message); DispatchMessageW(&message); }
        const auto s = status();
        if (s.vendor.active && s.vendor.effective_mode != RSF_FG_OFF) {
            active_seen = true;
            ++active_by_backend[present.backend() & 7];
        }
        Sleep(6);
    };
    auto runtime_tagged = [&] { return status().tagged_frames; };
    auto window = [&](const char* name, Kind kind, uint32_t frames, bool alternate_scope = false) {
        const auto start_tagged = runtime_tagged();
        const auto start_vendor = status().vendor.total_presented;
        const auto start_presents = totals.presents;
        for (uint32_t i = 0; i < frames; ++i) run_frame(kind, alternate_scope && (i % 2) == 0);
        const auto tagged = runtime_tagged() - start_tagged;
        const auto vendor = status().vendor.total_presented - start_vendor;
        const auto presents = totals.presents - start_presents;
        std::printf("window %-10s frames=%u engine_presents=%llu runtime_tagged=%llu vendor_presented=%llu backend=%u\n", name, frames,
            static_cast<unsigned long long>(presents), static_cast<unsigned long long>(tagged), static_cast<unsigned long long>(vendor),
            present.backend());
        if (vendor > presents) ++generated_windows;
        return tagged;
    };

    window("warmup", Kind::Gameplay, 60);
    const auto gameplay_tagged = window("gameplay", Kind::Gameplay, 60);
    CHECK(window("menu", Kind::Menu, 20) == 0);
    overlay_open = true;
    const auto overlay_tagged = window("overlay", Kind::Overlay, 40);
    overlay_open = false;
    // The scope window: the first few frames may still finish history from gameplay before the hold starts; the
    // Runtime must not tag any frame from the first SVP frame on.
    const auto scope_tagged = window("scope", Kind::Scope, 40, true);
    window("gameplay2", Kind::Gameplay, 60);
    fg::device_reset();
    engine.backbuffer.Reset();
    const HRESULT resized = engine.chain->ResizeBuffers(2, width, height, DXGI_FORMAT_UNKNOWN, 0);
    CHECK(SUCCEEDED(resized));
    if (!make_backbuffer_view()) return 1;
    const auto after_reset = window("reset", Kind::Gameplay, 60);
    if (switching) {
        for (const uint32_t next : {uint32_t(RSF_FG_BACKEND_FSR3), uint32_t(RSF_FG_BACKEND_XESS), 0u, uint32_t(RSF_FG_BACKEND_DLSS)}) {
            const auto requested = present.request(next);
            std::printf("switch request backend=%u result=%d\n", next, requested);
            // The provider changes at the next drained Present and its counters start over: settle, then measure.
            for (int i = 0; i < 6; ++i) run_frame(Kind::Gameplay, false);
            window(next == RSF_FG_BACKEND_FSR3 ? "fsr3" : next == RSF_FG_BACKEND_XESS ? "xess" : next ? "dlss" : "off", Kind::Gameplay, 80);
            std::printf("switch result=%d effective backend=%u\n", present.switch_result(), present.backend());
        }
    }

    const auto st = fg::status();
    const auto rs = status();
    std::printf("adapter counters: source=%llu level=%llu tagged=%llu fed=%llu presented=%llu unpresented=%llu captures=%llu\n",
        static_cast<unsigned long long>(st.source_frames), static_cast<unsigned long long>(st.level_frames),
        static_cast<unsigned long long>(st.tagged), static_cast<unsigned long long>(st.fed), static_cast<unsigned long long>(st.presented),
        static_cast<unsigned long long>(st.unpresented), static_cast<unsigned long long>(st.captures));
    std::printf("runtime: tagged=%llu vendor_presented=%llu effective_mode=%u generated=%u reason=%u last_result=%d vendor_status=%d\n",
        static_cast<unsigned long long>(rs.tagged_frames), static_cast<unsigned long long>(rs.vendor.total_presented),
        rs.vendor.effective_mode, rs.vendor.effective_generated_frames, rs.reason, rs.last_result, rs.vendor.vendor_status);
    std::printf("active samples: DLSS=%llu FSR3=%llu XeSS=%llu; overlay draws=%llu; SR evaluated=%llu refused=%llu\n",
        static_cast<unsigned long long>(active_by_backend[1]), static_cast<unsigned long long>(active_by_backend[3]),
        static_cast<unsigned long long>(active_by_backend[5]), static_cast<unsigned long long>(overlay_draws),
        static_cast<unsigned long long>(sr_evaluated), static_cast<unsigned long long>(sr_refused));
    CHECK(totals.failed_presents == 0);
    CHECK(st.source_frames == frames_rendered);
    CHECK(st.presented + st.unpresented == st.source_frames);
    CHECK(st.fed > 0 && st.tagged >= st.fed);
    CHECK(st.captures == 1);
    CHECK(overlay_draws > 0);
    if (stage5) CHECK(sr_evaluated > 0);
    if (backend == RSF_FG_BACKEND_DLSS && !active_seen && !foreground_owned()) {
        std::puts("NOT RUN: DLSS-G activity needs the foreground; this fixture was denied foreground ownership");
        return failures ? 1 : 77;
    }
    CHECK(active_seen);
    CHECK(gameplay_tagged > 0 && overlay_tagged > 0 && after_reset > 0);
    CHECK(scope_tagged == 0);   // the first frame of the window is an SVP frame; the rest are held
    CHECK(generated_windows > 0);
    if (switching) CHECK(active_by_backend[3] > 0 && active_by_backend[5] > 0);
    if (failures) { std::printf("%d failure(s)\n", failures); return 1; }
    std::printf("PASS: Anomaly FG (%s, backend %u) generated presented frames through the shared bridge\n", stage5 ? "SR stage 5" : "SR off", backend);
    return 0;
}
