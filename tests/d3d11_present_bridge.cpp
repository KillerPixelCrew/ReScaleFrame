// SPDX-License-Identifier: MIT
#include <rescaleframe/d3d11_present_bridge.h>
#include "presentation/d3d11_marker_bridge.h"
#include "presentation/device_fixture_bridge.h"
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>
#include "presentation/d3d11_scope_fixture.h"
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstddef>
#include <cwchar>
#include <algorithm>
using Microsoft::WRL::ComPtr;
extern "C" int rsf_d3d11_fixture_shared_sr(void* host, void* device, void* context,
    void* const* textures, rsf_dlss_log_fn log, rsf_dlss_frame* frame);

struct NativeCommand {
    NativeCommand* next = nullptr;
    void (*execute)(void*, NativeCommand*) = nullptr;
};
struct NativeList {
    NativeCommand* root = nullptr;
    NativeCommand** tail = &root;
    bool executing = false;
    unsigned char padding[3]{};
    uint32_t count = 0, uid = 1;
};
static_assert(sizeof(NativeCommand) == 16 && sizeof(NativeList) == 32 &&
    offsetof(NativeList, count) == 0x14 && offsetof(NativeList, uid) == 0x18);
extern "C" void rsf_d3d11_fixture_append(NativeList*, NativeCommand*);
extern "C" void rsf_d3d11_fixture_run(NativeList*, void (*)(NativeList*, NativeCommand*));

bool queued_passes_ok = true;
void append(NativeList& list, NativeCommand& command) {
    rsf_d3d11_fixture_append(&list, &command);
}
void run(NativeList& list) {
    rsf_d3d11_fixture_run(&list, [](NativeList* owner, NativeCommand* command) noexcept {
        try { command->execute(owner, command); }
        catch (...) { queued_passes_ok = false; }
    });
}
// Optional: hold one frame's scene submission open, to exercise the render hitch sampler.
DWORD submission_stall_ms = 0;
uint32_t expected_presentation_path = RSF_NATIVE_SCENE_PRESENT_NONE;
ID3D11DeviceContext* final_context = nullptr;
void cpu_event(uint64_t id, uint32_t stage);
void queued_pass(void*, void*, const rsf_game_render_pass* pass, uint32_t begin) {
    if (pass->role == RSF_GAME_RENDER_SUBMISSION) rsf_native_fg_submission(pass, begin);
    if (pass->role == RSF_GAME_RENDER_FRAME) rsf_native_fg_frame(pass, begin);
    if (pass->role == RSF_GAME_RENDER_FINAL_SCENE) {
        queued_passes_ok &= rsf_native_scene_pass(pass, begin) != 0;
        if (!begin && final_context) rsf_native_fg_final(final_context, pass);
    }
    if (pass->role == RSF_GAME_RENDER_TEXTURE_BINDING && !begin)
        queued_passes_ok &= rsf_native_scene_texture_binding(pass) != 0;
    if (pass->role == RSF_GAME_RENDER_WINDOW) {
        if (begin) {
            const auto expected = expected_presentation_path == RSF_NATIVE_SCENE_PRESENT_DIRECT ?
                RSF_NATIVE_SCENE_PRESENT_DIRECT : RSF_NATIVE_SCENE_PRESENT_NONE;
            queued_passes_ok &= rsf_native_scene_present_path(pass) == expected;
            auto stale = *pass; ++stale.source_frame_id;
            queued_passes_ok &= rsf_native_scene_present_path(&stale) == RSF_NATIVE_SCENE_PRESENT_NONE;
            auto other_viewport = *pass; ++other_viewport.viewport_key;
            queued_passes_ok &= rsf_native_scene_present_path(&other_viewport) == RSF_NATIVE_SCENE_PRESENT_NONE;
        }
        if (!begin) rsf_native_fg_window_end(pass);
        queued_passes_ok &= rsf_native_window_scope(pass, begin) != 0;
    }
}
struct PresentCommand : NativeCommand {
    IDXGISwapChain* chain = nullptr;
    bool simulation_pending = false;
    HRESULT result = E_FAIL;
    PresentCommand() { execute = [](void*, NativeCommand* command) {
        auto& present = *static_cast<PresentCommand*>(command);
        rsf_game_render_pass window{}; window.struct_size = sizeof(window);
        if (!rsf_native_window_match(present.chain, &window) ||
            rsf_native_scene_present_path(&window) != expected_presentation_path) return;
        present.result = present.chain->Present(0, 0);
        // RenderSubmitEnd precedes PresentStart, so PresentEnd retires the token unless the
        // game thread's SimulationEnd is still outstanding.
        queued_passes_ok &= (rsf_streamline_host_token(rsf_d3d11_present_host(), window.source_frame_id) != nullptr) ==
            present.simulation_pending;
    }; }
};
struct EvaluateCommand : NativeCommand {
    ID3D11DeviceContext* context = nullptr;
    const rsf_game_render_pass* source = nullptr;
    const rsf_dlss_frame* frame = nullptr;
    rsf_dlss_result result = RSF_DLSS_ERROR_NOT_READY;
    EvaluateCommand() { execute = [](void*, NativeCommand* command) {
        auto& evaluate = *static_cast<EvaluateCommand*>(command);
        if (submission_stall_ms) { Sleep(submission_stall_ms); submission_stall_ms = 0; }
        rsf_native_fg_scene(evaluate.source);
        evaluate.result = rsf_native_fg_evaluate(evaluate.context, evaluate.frame);
        rsf_native_fg_scene(nullptr);
    }; }
};
struct SimulationEndCommand : NativeCommand {
    uint64_t source = 0;
    SimulationEndCommand() { execute = [](void*, NativeCommand* command) {
        cpu_event(static_cast<SimulationEndCommand*>(command)->source, RSF_GAME_CPU_SIMULATION_END);
    }; }
};
uint64_t marker_reentry_source{};
uint32_t marker_reentries{};
void log_message(void*, const char* message) {
    std::printf("SDK: %s\n", message);
    if (marker_reentry_source && std::strstr(message,"actual graphics submit begin")) {
        const auto source = marker_reentry_source; marker_reentry_source = 0; ++marker_reentries;
        rsf_native_latency_render(source,1); // synchronous logger reentry must not open a second interval
    }
}
int accept_window(void* user, void* window) { return user == window; }
void present_event(void*, const rsf_observer_present_event* event) {
    rsf_native_window_present(event);
}
void latency_event(void*, const rsf_observer_present_event* event) { rsf_native_fg_present(event); }
bool renderer_startup_observed = false;
void before_present(void*, void* chain) {
    rsf_observer_status status{}; status.struct_size = sizeof(status);
    void* device = nullptr; void* context = nullptr;
    if (rsf_observer_get_status(&status) == RSF_OBSERVER_OK && status.present_width && status.present_height &&
        rsf_observer_acquire_device(&device, &context) == RSF_OBSERVER_OK) {
        ComPtr<ID3D11Device> actual;
        if (SUCCEEDED(static_cast<IDXGISwapChain*>(chain)->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get() == device)
            renderer_startup_observed = true;
    }
    if (device) static_cast<IUnknown*>(device)->Release();
    if (context) static_cast<IUnknown*>(context)->Release();
}
void cpu_event(uint64_t id, uint32_t stage) {
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{sizeof(event), stage, 42, id, uint64_t(now.QuadPart), uint64_t(frequency.QuadPart), 1};
    if (rsf_native_cpu_event(&event)) rsf_native_fg_cpu(&event);
}
namespace {
const rsf_d3d11_marker_fixture_calls marker_calls{
    []() {
        rsf_native_fg_options options{sizeof(options),RSF_FG_OFF,1,RSF_REFLEX_OFF,0,1};
        rsf_native_fg_options_set(&options);
        rsf_native_fg_set_log(log_message, nullptr);
    },
    []() -> uint64_t { LARGE_INTEGER qpc{}; QueryPerformanceCounter(&qpc); return uint64_t(qpc.QuadPart); },
    []() -> uint64_t { LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency); return uint64_t(frequency.QuadPart); },
    [](const rsf_game_cpu_event* event) {
        return rsf_native_cpu_dispatch(event, [](void*, const rsf_game_cpu_event* fact) { rsf_native_fg_cpu(fact); }, nullptr);
    },
    rsf_native_cpu_read,
    rsf_native_cpu_counters_get,
    rsf_native_fg_frame,
    rsf_native_fg_submission,
    rsf_native_fg_reflex_ordering_set,
    rsf_native_latency_render,
    rsf_native_latency_present,
    rsf_d3d11_present_abort,
    rsf_native_latency_frame_get,
    rsf_d3d11_present_generation,
    [](uint64_t frame) { return rsf_streamline_host_token(rsf_d3d11_present_host(), frame); },
    [](uint64_t* count) {
        rsf_streamline_latency_status status{}; status.struct_size = sizeof(status);
        const auto result = rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &status);
        *count = status.sleep_calls;
        return result;
    },
    [](uint64_t frame) { marker_reentry_source = frame; },
    [] { return marker_reentries; },
};
} // namespace

// The copied-status cache contract (invalidation, generation, concurrent readers and retired
// publishers) moved with its owner to d3d11_present_tests.rs. The public entry keeps its refusals.
bool status_snapshot_contract() {
    rsf_d3d11_present_status cold{sizeof(cold),RSF_PRESENT_STATUS_VERSION};
    if (rsf_d3d11_present_status_get(&cold) != RSF_BACKEND_ERROR_NOT_READY || cold.valid) return false;
    auto short_header = cold; short_header.struct_size = sizeof(short_header) - 1;
    if (rsf_d3d11_present_status_get(&short_header) != RSF_BACKEND_ERROR_INVALID_ARGUMENT) return false;
    auto bad_version = cold; ++bad_version.version;
    if (rsf_d3d11_present_status_get(&bad_version) != RSF_BACKEND_ERROR_ABI_MISMATCH) return false;
    std::puts("PASS: cold provider status refusals");
    return true;
}

int main(int argc, char** argv) {
    if (!status_snapshot_contract()) return 1;
    if (argc == 2 && std::strcmp(argv[1], "--status-snapshot") == 0) return 0;
    if (argc == 3 && std::strcmp(argv[1], "--proxy-smoke") == 0)
        return rsf_d3d11_fixture_proxy_smoke(argv[2]);
    if (argc != 3 && argc != 4 && argc != 6) { std::puts("SKIP: supply Streamline directory, adapter vendor ID, optional --enable or --switch FSR-directory XeSS-directory"); return 77; }
    const bool switching_visible = argc == 6 && std::strcmp(argv[3], "--switch-visible") == 0;
    // --borderless: X-Ray's borderless sequence (xray-monolith Device_create.cpp:205) on a window whose swap chain the
    // bridge owns. The window must land at the monitor origin, as it does without the bridge.
    const bool borderless_fixture = argc == 6 && std::strcmp(argv[3], "--borderless") == 0;
    const bool switching = switching_visible || borderless_fixture || (argc == 6 && std::strcmp(argv[3], "--switch") == 0);
    const bool pacing_fixture = argc == 4 && std::strcmp(argv[3], "--enable-visible-pacing") == 0;
    const bool ordering_only = argc == 4 && std::strcmp(argv[3], "--ordering-only") == 0;
    const bool ten_bit = pacing_fixture || (argc == 4 && std::strcmp(argv[3], "--enable-visible-10bit") == 0);
    const bool visible_fixture = switching_visible || ten_bit || (argc == 4 && std::strcmp(argv[3], "--enable-visible") == 0);
    const bool enabled_fixture = switching || ordering_only || visible_fixture || (argc == 4 && std::strcmp(argv[3], "--enable") == 0);
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    SetUnhandledExceptionFilter([](EXCEPTION_POINTERS* exception) -> LONG {
        std::fprintf(stderr, "fixture exception 0x%08lx at %p\n", exception->ExceptionRecord->ExceptionCode,
            exception->ExceptionRecord->ExceptionAddress);
        void* frames[24]{}; const auto count = CaptureStackBackTrace(0, 24, frames, nullptr);
        for (USHORT i = 0; i < count; ++i) {
            MEMORY_BASIC_INFORMATION region{}; VirtualQuery(frames[i], &region, sizeof(region));
            char module[MAX_PATH]{}; GetModuleFileNameA(static_cast<HMODULE>(region.AllocationBase), module, MAX_PATH);
            std::fprintf(stderr, "  %s + 0x%llx\n", module,
                static_cast<unsigned long long>(uintptr_t(frames[i]) - uintptr_t(region.AllocationBase)));
        }
        return EXCEPTION_EXECUTE_HANDLER;
    });
    // Anomaly runs system-DPI-aware (its window reports awareness 1 at the system DPI); match it.
    if (borderless_fixture) SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
    std::puts("fixture: creating D3D11 graphics");
    if (GetEnvironmentVariableA("RSF_TEST_D3D12_DEBUG", nullptr, 0) && rsf_present_fixture_debug12()) return 1;
    ComPtr<IDXGIFactory2> factory; ComPtr<IDXGIAdapter1> adapter;
    const auto vendor = uint32_t(std::strtoul(argv[2], nullptr, 0));
    if (const auto result = rsf_present_fixture_adapter(vendor, 0,
        reinterpret_cast<void**>(factory.GetAddressOf()), reinterpret_cast<void**>(adapter.GetAddressOf()))) return result;
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    if (rsf_present_fixture_device11(adapter.Get(), reinterpret_cast<void**>(device.GetAddressOf()),
        reinterpret_cast<void**>(context.GetAddressOf()))) return 77;
    rsf_observer_options observer{}; observer.struct_size = sizeof(observer); observer.abi_version = RSF_OBSERVER_ABI_VERSION;
    observer.format = DXGI_FORMAT_R16G16_FLOAT; // AC7's configured filter excludes the facade backbuffer.
    observer.capacity = 1;
    if (rsf_observer_install(&observer) != RSF_OBSERVER_OK) return 1;
    std::puts("fixture: D3D11 observer installed");
    WNDCLASSW wc{}; wc.hInstance = GetModuleHandleW(nullptr); wc.lpfnWndProc = DefWindowProcW;
    wc.lpszClassName = L"RSFPresentBridgeFixture";
    if (!RegisterClassW(&wc)) return 1;
    RECT initial{0, 0, 1280, 720};
    if (borderless_fixture) { initial = {0, 0, 640, 480}; AdjustWindowRect(&initial, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, FALSE); }
    // X-Ray: an ANSI class and WS_EX_TOPMOST (xray-monolith Device_Initialize.cpp).
    WNDCLASSA xray{}; xray.hInstance = wc.hInstance; xray.lpfnWndProc = DefWindowProcA; xray.lpszClassName = "_XRAY_FIXTURE";
    if (borderless_fixture && !RegisterClassA(&xray)) return 1;
    auto window = borderless_fixture ?
        CreateWindowExA(WS_EX_TOPMOST, xray.lpszClassName, "Presentation bridge fixture", WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            190, 190, initial.right - initial.left, initial.bottom - initial.top, nullptr, nullptr, wc.hInstance, nullptr) :
        CreateWindowW(wc.lpszClassName, L"Presentation bridge fixture", WS_OVERLAPPEDWINDOW,
            0, 0, 1280, 720, nullptr, nullptr, wc.hInstance, nullptr);
    if (!window) return 1;
    if (borderless_fixture) ShowWindow(window, SW_SHOW);
    if (visible_fixture) { ShowWindow(window, SW_SHOW); SetForegroundWindow(window); }
    rsf_d3d11_present_setup setup{}; setup.struct_size = sizeof(setup);
    setup.runtime_directory_utf8 = argv[1]; setup.log = log_message; setup.user = window; setup.accept_window = accept_window;
    setup.development_runtime = std::strstr(argv[1], "development") != nullptr;
    setup.present_event = present_event; setup.latency_event = latency_event;
    setup.before_present = before_present;
    setup.debug_timing = 1;
    setup.prepare = rsf_native_fg_prepare; setup.retire = rsf_native_fg_retire;
    setup.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
    // AC7's conventions and Streamline identity, as its plugin states them.
    setup.depth_inverted = 1; setup.depth_infinite = 1; setup.units_to_meters = 0.01f;
    setup.engine_type = 1; setup.engine_version_utf8 = "4.18.3"; setup.project_id_utf8 = "a3ed1f08-3542-4698-b85c-e1a9908e861a";
    if (switching) {
        setup.backend = RSF_FG_BACKEND_DLSS; setup.runtime_switching = 1; setup.max_generated_frames = UINT32_MAX;
        setup.streamline_directory_utf8 = argv[1]; setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = argv[4];
        setup.xess_directory_utf8 = argv[5];
    }
    if (!rsf_d3d11_present_install(&setup)) return 1;
    std::puts("fixture: presentation interception installed");
    DXGI_SWAP_CHAIN_DESC desc{}; desc.BufferDesc.Width = 1280; desc.BufferDesc.Height = 720;
    desc.BufferDesc.Format = ten_bit ? DXGI_FORMAT_R10G10B10A2_UNORM : DXGI_FORMAT_R8G8B8A8_UNORM; desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; desc.BufferCount = 1;
    desc.OutputWindow = window; desc.Windowed = TRUE; desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    if (borderless_fixture) { desc.BufferDesc.Width = 640; desc.BufferDesc.Height = 480; desc.BufferCount = 2; }
    ComPtr<IDXGISwapChain> chain;
    if (FAILED(factory->CreateSwapChain(device.Get(), &desc, &chain))) return 1;
    if (borderless_fixture) {
        const auto result = rsf_d3d11_fixture_borderless(chain.Get(), window);
        chain.Reset(); DestroyWindow(window);
        return result;
    }
    if (!rsf_d3d11_present_host()) {
        if (rsf_d3d11_fixture_clear_and_present(device.Get(), context.Get(), chain.Get(), 0)) return 1;
        std::puts("PASS fallback: original D3D11 buffer, draw and Present after FG startup refusal; FG capability skipped"); return 77;
    }
    rsf_streamline_graphics presentation_graphics{}, transfer_graphics{};
    presentation_graphics.struct_size = sizeof(presentation_graphics); transfer_graphics.struct_size = sizeof(transfer_graphics);
    if (rsf_streamline_host_graphics(rsf_d3d11_present_host(), &presentation_graphics) != RSF_BACKEND_OK ||
        !rsf_d3d11_present_graphics(&transfer_graphics) || presentation_graphics.queue == transfer_graphics.queue ||
        presentation_graphics.device != transfer_graphics.device) return 1;
    std::puts("PASS: SR/upload use a separate queue on the same presentation device");
    // Synthetic GPU evidence for the same host used by FG. This is not a game scene or a
    // latency measurement. It catches a second Streamline owner/token and D3D11/12 state errors.
    ComPtr<ID3D11Texture2D> inputs[4], hints[5];
    void* textures[9]{};
    if (rsf_d3d11_fixture_inputs(device.Get(), context.Get(), textures)) return 1;
    for (unsigned index = 0; index != 4; ++index) inputs[index].Attach(static_cast<ID3D11Texture2D*>(textures[index]));
    for (unsigned index = 0; index != 5; ++index) hints[index].Attach(static_cast<ID3D11Texture2D*>(textures[index + 4]));
    rsf_dlss_frame frame{};
    if (rsf_d3d11_fixture_shared_sr(rsf_d3d11_present_host(), device.Get(), context.Get(), textures,
        log_message, &frame)) return 1;
    ComPtr<IDXGISwapChain4> modern; ComPtr<IUnknown> identity, modern_identity; ComPtr<ID3D11Device> returned_device;
    if (FAILED(chain.As(&modern)) || FAILED(chain.As(&identity)) || FAILED(modern.As(&modern_identity)) ||
        identity.Get() != modern_identity.Get() || FAILED(chain->GetDevice(IID_PPV_ARGS(&returned_device))) || returned_device.Get() != device.Get()) return 1;
    if (rsf_d3d11_fixture_clear_and_present(device.Get(), context.Get(), chain.Get(), 1)) return 1;
    rsf_observer_status observed_presenter{}; observed_presenter.struct_size = sizeof(observed_presenter);
    if (!renderer_startup_observed || rsf_observer_get_status(&observed_presenter) != RSF_OBSERVER_OK ||
        observed_presenter.frames_presented != 12 || observed_presenter.present_width != 1280 || observed_presenter.present_height != 720) return 1;
    std::puts("PASS: renderer startup sees the facade's presenting device/output and 12 source Presents");
    if (enabled_fixture) {
        // Match the SR fixture's actual output extent for both sampled and direct paths.
        if (FAILED(chain->ResizeBuffers(1, 512, 512, DXGI_FORMAT_UNKNOWN, 0))) return 1;
        if (visible_fixture) { ShowWindow(window, SW_SHOW); SetForegroundWindow(window); }
        std::printf("fixture window=%p foreground=%p\n", static_cast<void*>(window), static_cast<void*>(GetForegroundWindow()));
        if (rsf_dlss_share_host(rsf_d3d11_present_host(), log_message, nullptr) != RSF_DLSS_OK) return 1;
        rsf_native_fg_options options{sizeof(options), RSF_FG_FIXED, 1, RSF_REFLEX_ON, 0, 1};
        // Optional: a limit on rendered frames, to measure what the driver's limiter does with it.
        char limit_text[16]{};
        if (GetEnvironmentVariableA("RSF_TEST_FRAME_LIMIT_US", limit_text, sizeof(limit_text)))
            options.frame_limit_us = uint32_t(std::strtoul(limit_text, nullptr, 0));
        if (GetEnvironmentVariableA("RSF_TEST_SUBMISSION_STALL_MS", limit_text, sizeof(limit_text)))
            submission_stall_ms = DWORD(std::strtoul(limit_text, nullptr, 0));
        LARGE_INTEGER limit_frequency{}; QueryPerformanceFrequency(&limit_frequency);
        double active_ms = 0, inactive_ms = 0; uint32_t active_frames = 0, inactive_frames = 0;
        rsf_native_fg_options_set(&options); rsf_native_fg_set_log(log_message, nullptr);
        final_context = context.Get();
        D3D11_TEXTURE2D_DESC scene_desc{}; scene_desc.Width = scene_desc.Height = 512;
        scene_desc.MipLevels = scene_desc.ArraySize = scene_desc.SampleDesc.Count = 1;
        scene_desc.Format = ten_bit ? DXGI_FORMAT_R10G10B10A2_UNORM : DXGI_FORMAT_R8G8B8A8_UNORM;
        scene_desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
        ComPtr<ID3D11Texture2D> hudless; ComPtr<ID3D11RenderTargetView> hudless_target;
        if (FAILED(device->CreateTexture2D(&scene_desc, nullptr, &hudless)) ||
            FAILED(device->CreateRenderTargetView(hudless.Get(), nullptr, &hudless_target))) return 1;
        rsf_game_render_pass source{}; source.struct_size = sizeof(source); source.session_id = 42;
        source.family_key = 1; source.view_key = 2; source.viewport_key = 3; source.screen = RSF_SCREEN_FLIGHT;
        source.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
        source.camera.struct_size = sizeof(source.camera); source.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        source.camera.render_width = source.camera.render_height = source.camera.output_width = source.camera.output_height = 512;
        source.camera.near_plane = 0.1f; source.camera.vertical_fov_radians = 1; source.camera.depth_inverted = 1;
        std::memcpy(source.camera.view_to_clip, frame.camera_view_to_clip, 64);
        std::memcpy(source.camera.clip_to_view, frame.clip_to_camera_view, 64);
        std::memcpy(source.camera.clip_to_previous_clip, frame.clip_to_prev_clip, 64);
        source.camera.view_to_world[0] = source.camera.view_to_world[5] = source.camera.view_to_world[10] = source.camera.view_to_world[15] = 1;
        source.camera.world_to_view[0] = source.camera.world_to_view[5] = source.camera.world_to_view[10] = source.camera.world_to_view[15] = 1;
        source.camera.frame_time_seconds = 1.0f / 60;
        source.output_rect[2] = source.output_rect[3] = 512;
        rsf_ac7_render_scopes* scopes = nullptr;
        if (!rsf_ac7_render_scopes_create_passes(42, 8, nullptr, queued_pass, nullptr, &scopes)) return 1;
        const rsf_d3d11_scope_fixture_calls scope_calls{rsf_ac7_render_scope_open, rsf_ac7_render_scope_close};
        bool sampled_active = false, direct_active = false;
        const uint64_t frame_count = pacing_fixture ? 240 : switching ? 120 : 16;
        uint32_t provider_activity[6]{};
        uint64_t sleeps_expected = 1;
        for (uint64_t id = 2; id < 2 + frame_count; ++id) {
            const auto frame_provider = rsf_d3d11_present_backend();
            const bool direct = id >= 2 + frame_count / 2;
            expected_presentation_path = direct ? RSF_NATIVE_SCENE_PRESENT_DIRECT : RSF_NATIVE_SCENE_PRESENT_SAMPLED;
            source.screen = id < 2 + frame_count / 3 ? RSF_SCREEN_FLIGHT :
                id < 2 + 2 * frame_count / 3 ? RSF_SCREEN_HANGAR : RSF_SCREEN_BRIEFING;
            options.reflex_mode = id < 2 + frame_count / 3 ? RSF_REFLEX_OFF :
                id < 2 + 2 * frame_count / 3 ? RSF_REFLEX_ON : RSF_REFLEX_BOOST;
            // Requests beyond this GPU's capability must clamp without disabling generation.
            if (switching) options.generated_frames = (id % 12 < 6) ? 1u : 3u;
            rsf_native_fg_options_set(&options);
            LARGE_INTEGER frame_started{}; QueryPerformanceCounter(&frame_started);
            cpu_event(id, RSF_GAME_CPU_FRAME_BEGIN);
            rsf_streamline_latency_status reserved{}; reserved.struct_size = sizeof(reserved);
            if (rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &reserved) != RSF_BACKEND_OK ||
                reserved.sleep_calls != sleeps_expected) { std::printf("sleep count source=%llu actual=%llu expected=%llu\n", static_cast<unsigned long long>(id), static_cast<unsigned long long>(reserved.sleep_calls), static_cast<unsigned long long>(sleeps_expected)); return 1; }
            source.source_frame_id = source.submission_id = source.native_frame = source.scope_id = id;
            NativeList commands;
            rsf_ac7_render_ticket *frame_ticket = nullptr, *submission_ticket = nullptr, *window_ticket = nullptr;
            if (!rsf_d3d11_fixture_scope_begin(&scope_calls, scopes, &commands, &source, &frame_ticket)) return 1;
            run(commands); // Native BeginFrame can execute before game-thread Reflex sleep.
            cpu_event(id, RSF_GAME_CPU_PACING);
            rsf_native_fg_status pacing{}; pacing.struct_size=sizeof(pacing);
            if (!rsf_native_fg_status_get(&pacing)) return 1;
            // Pacing is a core choice, independent of which FG provider is selected.
            if (pacing.vendor.pacing_owner==RSF_PACING_REFLEX) ++sleeps_expected;
            rsf_streamline_latency_status paced{}; paced.struct_size=sizeof(paced);
            if (rsf_streamline_host_latency_status(rsf_d3d11_present_host(),&paced)!=RSF_BACKEND_OK ||
                paced.sleep_calls!=sleeps_expected) return 1;
            if (!switching && rsf_streamline_host_sleep(rsf_d3d11_present_host(), id) != RSF_BACKEND_ERROR_INVALID_ARGUMENT) return 1;
            MSG message{}; PeekMessageW(&message, window, 0, 0, PM_NOREMOVE);
            cpu_event(id, RSF_GAME_CPU_INPUT_SAMPLE);
            LARGE_INTEGER input_time{}, input_frequency{};
            QueryPerformanceCounter(&input_time); QueryPerformanceFrequency(&input_frequency);
            rsf_game_cpu_event input{sizeof(input), RSF_GAME_CPU_INPUT_EVENT, 42, id,
                uint64_t(input_time.QuadPart), uint64_t(input_frequency.QuadPart), 1,
                RSF_GAME_INPUT_MOUSE | RSF_GAME_INPUT_KEYBOARD, WM_MOUSEMOVE};
            if (!rsf_native_cpu_event(&input)) return 1;
            rsf_native_fg_cpu(&input);
            input.input_kind = RSF_GAME_INPUT_CONTROLLER; input.message_id = 0;
            if (!rsf_native_cpu_event(&input)) return 1;
            rsf_native_fg_cpu(&input); rsf_native_fg_cpu(&input); // SDK controller marker is once per frame.
            rsf_native_cpu_frame input_record{}; input_record.struct_size = sizeof(input_record);
            if (!rsf_native_cpu_read(42, id, &input_record) || input_record.stage_mask != 3 ||
                input_record.input_mask != 7 || input_record.input_events[0] != 1 ||
                input_record.input_events[1] != 1 || input_record.input_events[2] != 1) return 1;
            cpu_event(id, RSF_GAME_CPU_SIMULATION_BEGIN);
            const bool late_simulation_end = (id & 1u) != 0;
            if (!late_simulation_end) cpu_event(id, RSF_GAME_CPU_SIMULATION_END);
            if (!rsf_d3d11_fixture_scope_scene(&scope_calls, scopes, &commands, &source, &submission_ticket)) return 1;
            frame.reset = id == 2;
            EvaluateCommand evaluate; evaluate.context = context.Get(); evaluate.source = &source; evaluate.frame = &frame;
            append(commands, evaluate);
            ComPtr<ID3D11Texture2D> backbuffer; ComPtr<ID3D11RenderTargetView> target;
            if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&backbuffer))) || FAILED(device->CreateRenderTargetView(backbuffer.Get(), nullptr, &target))) return 1;
            const float color[]{0.25f, 0.2f, 0.6f, 1}; context->ClearRenderTargetView(target.Get(), color);
            context->ClearRenderTargetView(hudless_target.Get(), color);
            if (!rsf_d3d11_fixture_scope_window(&scope_calls, scopes, &commands, &source,
                submission_ticket, backbuffer.Get(), hudless.Get(), window, chain.Get(), uint32_t(direct), &window_ticket)) return 1;
            PresentCommand present; present.chain = chain.Get(); present.simulation_pending = late_simulation_end;
            append(commands, present);
            if (!rsf_ac7_render_scope_close(window_ticket, &commands)) return 1;
            SimulationEndCommand simulation_end; simulation_end.source = id;
            if (late_simulation_end) append(commands, simulation_end);
            if (!rsf_ac7_render_scope_close(frame_ticket, &commands)) return 1;
            run(commands);
            cpu_event(id, RSF_GAME_CPU_FRAME_END);
            if (!queued_passes_ok || evaluate.result != RSF_DLSS_OK || FAILED(present.result) ||
                rsf_streamline_host_token(rsf_d3d11_present_host(), id)) {
                auto* gpu = static_cast<ID3D12Device*>(transfer_graphics.native_device);
                std::printf("AC7 adapter failed source=%llu SR=%d Present=%08lx device=%08lx passes=%u\n",
                    static_cast<unsigned long long>(id), evaluate.result, present.result, gpu->GetDeviceRemovedReason(), unsigned(queued_passes_ok));
                rsf_present_fixture_debug_errors(gpu);
                return 1;
            }
            rsf_native_fg_status activity{}; activity.struct_size = sizeof(activity);
            if (!rsf_native_fg_status_get(&activity)) return 1;
            const auto provider = frame_provider;
            // Present may complete a queued provider switch. Historical frame eligibility
            // then belongs to the old provider, while latency status describes the new owner.
            if (provider == RSF_FG_BACKEND_DLSS && rsf_d3d11_present_backend() == provider && activity.vendor.effective_reflex !=
                (activity.vendor.effective_mode == RSF_FG_OFF ? options.reflex_mode : options.reflex_mode ? options.reflex_mode : RSF_REFLEX_ON)) { std::printf("Reflex mismatch source=%llu effective=%u requested=%u mode=%u\n", static_cast<unsigned long long>(id), activity.vendor.effective_reflex, options.reflex_mode, activity.vendor.effective_mode); return 1; }
            if (switching) {
                std::printf("adapter source=%llu backend=%u mode=%u active=%u result=%d generated=%u max=%u\n",
                    static_cast<unsigned long long>(id), provider, activity.vendor.effective_mode, activity.vendor.active,
                    activity.last_result, activity.vendor.effective_generated_frames, activity.vendor.max_generated_frames);
                if (activity.vendor.active) {
                    ++provider_activity[provider];
                    if (activity.vendor.effective_generated_frames != std::min(options.generated_frames, activity.vendor.max_generated_frames)) { std::printf("multiplier mismatch source=%llu effective=%u requested=%u max=%u\n", static_cast<unsigned long long>(id), activity.vendor.effective_generated_frames, options.generated_frames, activity.vendor.max_generated_frames); return 1; }
                }
                uint32_t next = UINT32_MAX;
                if (id == 13 || id == 53) next = RSF_FG_BACKEND_FSR3;
                if (id == 25 || id == 65) next = RSF_FG_BACKEND_XESS;
                if (id == 37) next = 0;
                if (id == 43 || id == 75) next = RSF_FG_BACKEND_DLSS;
                if (next != UINT32_MAX && rsf_d3d11_present_request(next) != RSF_BACKEND_OK) return 1;
                if (activity.last_result) return 1;
            }
            if (direct) direct_active |= activity.vendor.active != 0;
            else sampled_active |= activity.vendor.active != 0;
            LARGE_INTEGER frame_ended{}; QueryPerformanceCounter(&frame_ended);
            const double frame_ms = double(frame_ended.QuadPart - frame_started.QuadPart) * 1000.0 / double(limit_frequency.QuadPart);
            if (activity.vendor.active) { active_ms += frame_ms; ++active_frames; }
            else { inactive_ms += frame_ms; ++inactive_frames; }
            if (!pacing_fixture) Sleep(16);
        }
        std::printf("frame limit: requested %u us between rendered frames; generating %u frames mean %.2f ms, not generating %u frames mean %.2f ms\n",
            options.frame_limit_us, active_frames, active_frames ? active_ms / active_frames : 0.0,
            inactive_frames, inactive_frames ? inactive_ms / inactive_frames : 0.0);
        rsf_ac7_render_scopes_quiesce(scopes);
        if (!rsf_ac7_render_scopes_destroy(scopes)) return 1;
        observed_presenter = {}; observed_presenter.struct_size = sizeof(observed_presenter);
        if (rsf_observer_get_status(&observed_presenter) != RSF_OBSERVER_OK ||
            observed_presenter.frames_presented != 12 + frame_count) return 1;
        std::puts("PASS: application frame count matches source Presents");
        std::printf("PASS: %llu sampled/direct queued frames across flight/hangar/briefing; wrong source/viewport and missing sampled binding rejected; all CPU tokens retired\n",
            static_cast<unsigned long long>(frame_count));
        std::printf("present paths: sampled active=%u direct active=%u\n", unsigned(sampled_active), unsigned(direct_active));
        if (!ordering_only && (!sampled_active || !direct_active)) return 1;
        rsf_native_fg_status observed{}; observed.struct_size = sizeof(observed);
        rsf_native_fg_status_get(&observed);
        std::printf("synthetic enabled FG: tagged=%llu mode=%u generated=%u active=%u aggregate presents=%llu result=%d SDK status=%d\n",
            static_cast<unsigned long long>(observed.tagged_frames), observed.vendor.effective_mode, observed.vendor.effective_generated_frames,
            observed.vendor.active, static_cast<unsigned long long>(observed.vendor.total_presented), observed.last_result, observed.vendor.vendor_status);
        if (!observed.tagged_frames || observed.last_result || observed.vendor.vendor_status ||
            (!ordering_only && !switching && !observed.vendor.active)) return 1;
        rsf_streamline_latency_status latency{}; latency.struct_size = sizeof(latency);
        rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &latency);
        std::printf("latency sleeps=%llu markers=%llu\n", static_cast<unsigned long long>(latency.sleep_calls), static_cast<unsigned long long>(latency.marker_calls));
        if (rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &latency) != RSF_BACKEND_OK ||
            latency.sleep_calls != sleeps_expected || latency.marker_calls != frame_count * 7) return 1;
        if (switching) {
            std::printf("AC7 adapter active samples: DLSS=%u FSR3=%u XeSS=%u; live multiplier requests 2x/4x clamp to SDK limit\n",
                provider_activity[1], provider_activity[3], provider_activity[5]);
            if (!provider_activity[3] || !provider_activity[5]) return 1;
            if (!provider_activity[1]) {
                if (visible_fixture && GetForegroundWindow() == window) return 1;
                std::puts("NOT RUN: DLSS activity acceptance requires a foreground game; this fixture was denied foreground ownership");
            }
        }
        const auto input_probe = frame_count + 2;
        if (!latency.pcl_message_id || rsf_streamline_host_begin(rsf_d3d11_present_host(), input_probe) != RSF_BACKEND_OK ||
            rsf_streamline_host_input(rsf_d3d11_present_host(), input_probe, 0, latency.pcl_message_id) != RSF_BACKEND_OK ||
            rsf_streamline_host_abort(rsf_d3d11_present_host(), input_probe) != RSF_BACKEND_OK) return 1;
        std::puts("PASS: keyboard/mouse dequeue records, controller marker deduplication and SDK-registered PCL ping routing");
        const auto unpresented = frame_count + 3;
        cpu_event(unpresented, RSF_GAME_CPU_FRAME_BEGIN);
        cpu_event(unpresented, RSF_GAME_CPU_PACING);
        for (uint32_t stage = RSF_GAME_CPU_INPUT_SAMPLE; stage <= RSF_GAME_CPU_FRAME_END; ++stage) cpu_event(unpresented, stage);
        source.source_frame_id = unpresented; rsf_native_fg_submission(&source, 1); rsf_native_fg_submission(&source, 0);
        source.role = RSF_GAME_RENDER_WINDOW; source.swapchain = chain.Get();
        if (!rsf_streamline_host_token(rsf_d3d11_present_host(), unpresented)) return 1;
        rsf_native_fg_window_end(&source);
        if (rsf_streamline_host_token(rsf_d3d11_present_host(), unpresented)) return 1;
        const auto abort_started = GetTickCount64();
        cpu_event(unpresented + 1, RSF_GAME_CPU_FRAME_BEGIN);
        if (GetTickCount64() - abort_started >= 500 ||
            !rsf_streamline_host_token(rsf_d3d11_present_host(), unpresented + 1)) return 1;
        // The abandoned window also releases the next frame's pacing join.
        const auto milliseconds = [](LARGE_INTEGER begin) {
            LARGE_INTEGER end{}, frequency{}; QueryPerformanceCounter(&end); QueryPerformanceFrequency(&frequency);
            return double(end.QuadPart - begin.QuadPart) * 1000.0 / double(frequency.QuadPart);
        };
        LARGE_INTEGER released{}; QueryPerformanceCounter(&released);
        cpu_event(unpresented + 1, RSF_GAME_CPU_PACING);
        const auto released_ms = milliseconds(released);
        // A frame that never reaches Present or EndFrame holds the next sleep for one bounded wait.
        cpu_event(unpresented + 2, RSF_GAME_CPU_FRAME_BEGIN);
        LARGE_INTEGER held{}; QueryPerformanceCounter(&held);
        cpu_event(unpresented + 2, RSF_GAME_CPU_PACING);
        const auto held_ms = milliseconds(held);
        std::printf("pacing join: released %.2f ms, bounded wait %.2f ms\n", released_ms, held_ms);
        // A requested frame limit puts its own driver sleep into both measurements.
        if (!options.frame_limit_us && (released_ms >= 18.0 || held_ms < 18.0 || held_ms >= 200.0)) return 1;
        for (uint64_t id = unpresented + 1; id <= unpresented + 2; ++id) {
            if (rsf_streamline_host_abort(rsf_d3d11_present_host(), id) != RSF_BACKEND_OK) return 1;
            cpu_event(id, RSF_GAME_CPU_FRAME_END);
        }
        std::puts("PASS: an abandoned window releases the next frame's sleep; a frame without completion holds it for one bounded wait");
        options.mode = RSF_FG_OFF; rsf_native_fg_options_set(&options);
        if (rsf_dlss_release_resources() != RSF_DLSS_OK || rsf_dlss_shutdown() != RSF_DLSS_OK) return 1;
    }
    if (!enabled_fixture && !rsf_d3d11_marker_fixture(chain.Get(), &marker_calls)) return 1;
    context->ClearState();
    if (FAILED(chain->ResizeBuffers(1, 1024, 576, DXGI_FORMAT_UNKNOWN, 0))) return 1;
    ComPtr<ID3D11Texture2D> resized;
    if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&resized)))) return 1;
    D3D11_TEXTURE2D_DESC size{}; resized->GetDesc(&size);
    if (size.Width != 1024 || size.Height != 576 || FAILED(chain->Present(0, DXGI_PRESENT_TEST))) return 1;
    resized.Reset(); returned_device.Reset(); modern_identity.Reset(); identity.Reset(); modern.Reset();
    rsf_d3d11_chain_dispatch final_release{}; final_release.method = RSF_D3D11_DISPATCH_RELEASE;
    final_release.receiver = chain.Detach();
    rsf_d3d11_chain_dispatch_owned(&final_release);
    if (!final_release.handled || final_release.result.references || rsf_d3d11_present_has_owner()) return 1;
    DestroyWindow(window); UnregisterClassW(wc.lpszClassName, wc.hInstance);
    std::puts("PASS: shared D3D12 DLSS SR and CPU token; D3D11 COM identity/buffers, Off Present, retained-reference resize refusal, resize and retirement");
    return 0;
}
