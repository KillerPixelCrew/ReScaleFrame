// SPDX-License-Identifier: MIT
#include <rescaleframe/d3d11_present_bridge.h>
#include "presentation/device_fixture_bridge.h"
#include "presentation/present_fixture_loops.h"
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/overlay_d3d12.h>
#include <windows.h>
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
using Microsoft::WRL::ComPtr;
static_assert(sizeof(rsf_fg_loop) == 192 && offsetof(rsf_fg_loop, state) == 184);
namespace {
void log(void*, const char* message) { std::printf("SDK: %s\n", message); }
int accept(void*, void*) { return 1; }
}
int main(int argc, char** argv) {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    if (argc != 5 && argc != 6 && argc != 7) { std::puts("SKIP: backend, SDK directory, vendor ID, API(11/12/12-hot), optional XeSS and Streamline directories"); return 77; }
    const uint32_t backend = uint32_t(std::strtoul(argv[1], nullptr, 0));
    const uint32_t vendor = uint32_t(std::strtoul(argv[3], nullptr, 0));
    const bool debug = std::getenv("RSF_UNITY_GPU_DEBUG") != nullptr;
    if (debug && rsf_present_fixture_debug12()) return 77;
    const bool overlay = std::strcmp(argv[4], "12-overlay") == 0;
    const bool mixed = std::strcmp(argv[4], "12-mixed") == 0 || overlay;
    const bool mfg_test = std::strcmp(argv[4], "12-mfg") == 0;
    const bool hot = std::strcmp(argv[4], "12-hot") == 0 || mixed || mfg_test;
    const bool flip11 = std::strcmp(argv[4], "11-flip") == 0;
    const bool legacy_hot = std::strcmp(argv[4], "11-hot") == 0 || flip11;
    const bool service = hot || std::strcmp(argv[4], "12-service") == 0;
    const bool native = std::strcmp(argv[4], "11") != 0 && !legacy_hot;
    const bool explicit_creation = std::getenv("RSF_EXPLICIT_SWAPCHAIN") != nullptr;
    if (explicit_creation && !native) return 1;
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter;
    if (const auto result = rsf_present_fixture_adapter(vendor, 1,
        reinterpret_cast<void**>(factory.GetAddressOf()), reinterpret_cast<void**>(adapter.GetAddressOf()))) return result;
    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"RSFSharedFG";
    if (!RegisterClassW(&wc)) return 1;
    const bool focus_test = mfg_test && std::getenv("RSF_MFG_FOCUS_TEST") != nullptr;
    HWND window = CreateWindowW(wc.lpszClassName, L"Shared generation fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 1280, 720, nullptr, nullptr, wc.hInstance, nullptr);
    if (!window) return 1;
    std::unique_ptr<void, decltype(&rsf_fg_bridge_fixture_focus_end)> focus{nullptr, rsf_fg_bridge_fixture_focus_end};
    // Failed GPU retirement can leave callback resources live until process exit.
    static std::unique_ptr<rsf_fg_loop_state, decltype(&rsf_fg_fixture_state_destroy)> state{
        rsf_fg_fixture_state_create(uint32_t(service), uint32_t(hot), uint32_t(flip11), uint32_t(mfg_test)),
        rsf_fg_fixture_state_destroy,
    };
    rsf_d3d11_present_setup setup{}; setup.struct_size = sizeof(setup); setup.runtime_directory_utf8 = argv[2];
    setup.backend = backend; setup.max_generated_frames = mfg_test ? 5u : 3u; setup.log = log; setup.accept_window = accept;
    setup.user = state.get(); setup.prepare = rsf_fg_fixture_prepare;
    setup.latency_event = rsf_fg_fixture_latency; setup.retire = rsf_fg_fixture_retire;
    // The fixture's engines: native D3D12 stands in for Unity, D3D11 for AC7.
    setup.depth_inverted = 1; setup.depth_infinite = native ? 0u : 1u; setup.units_to_meters = native ? 1.0f : 0.01f;
    setup.engine_type = native ? 2u : 1u; setup.engine_version_utf8 = native ? "6000.3" : "4.18.3";
    setup.project_id_utf8 = native ? "57a42c7e-faf0-4bda-a9f9-892870948ac1" : "a3ed1f08-3542-4698-b85c-e1a9908e861a";
    if (legacy_hot) {
        setup.runtime_switching = 1; setup.streamline_directory_utf8 = argv[2];
        setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = argc == 7 ? argv[6] : argv[2];
        setup.xess_directory_utf8 = argc >= 6 ? argv[5] : argv[2];
    }
    if (service) {
        rsf_fg12_setup request{}; request.struct_size = sizeof(request); request.backend = backend;
        request.runtime_directory_utf8 = argv[2]; request.max_generated_frames = mfg_test ? 5u : 3u;
        request.options = {sizeof(request.options), RSF_FG_FIXED, 1, RSF_REFLEX_OFF, 0, 0};
        request.depth_inverted = 1; request.units_to_meters = 1; request.log = log; request.accept_window = accept;
        request.engine_type = setup.engine_type; request.engine_version_utf8 = setup.engine_version_utf8;
        request.project_id_utf8 = setup.project_id_utf8;
        if (hot) {
            request.runtime_switching = 1; request.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
            request.fsr3_directory_utf8 = request.fsr4_directory_utf8 = argv[2];
            request.xess_directory_utf8 = argc >= 6 ? argv[5] : argv[2];
            request.streamline_directory_utf8 = argc == 7 ? argv[6] : nullptr;
        }
        if ((explicit_creation ? rsf_fg12_install_explicit(&request) : rsf_fg12_install(&request)) != RSF_BACKEND_OK) return 1;
    } else if (!(explicit_creation ? rsf_d3d11_present_prepare(&setup) : rsf_d3d11_present_install(&setup))) return 1;
    ComPtr<ID3D11Device> device11; ComPtr<ID3D11DeviceContext> context11;
    ComPtr<ID3D12Device> device12; ComPtr<ID3D12CommandQueue> queue;
    IUnknown* producer = nullptr;
    if (native) {
        if (rsf_present_fixture_device12(adapter.Get(), reinterpret_cast<void**>(device12.GetAddressOf()),
            reinterpret_cast<void**>(queue.GetAddressOf()))) return 1;
        producer = queue.Get();
    } else {
        if (rsf_present_fixture_device11(adapter.Get(), reinterpret_cast<void**>(device11.GetAddressOf()),
            reinterpret_cast<void**>(context11.GetAddressOf()))) return 1;
        producer = device11.Get();
    }
    DXGI_SWAP_CHAIN_DESC desc{}; desc.BufferDesc.Width = mixed ? 2560u : 1280u; desc.BufferDesc.Height = mixed ? 1600u : 720u;
    desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; desc.BufferCount = native || flip11 ? 3u : 1u;
    desc.OutputWindow = window; desc.Windowed = TRUE; desc.SwapEffect = native || flip11 ? DXGI_SWAP_EFFECT_FLIP_DISCARD : DXGI_SWAP_EFFECT_DISCARD;
    if (hot && native) desc.Flags |= DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT;
    ComPtr<IDXGISwapChain> chain;
    if (explicit_creation) {
        DXGI_SWAP_CHAIN_DESC1 explicit_desc{};
        explicit_desc.Width = desc.BufferDesc.Width; explicit_desc.Height = desc.BufferDesc.Height;
        explicit_desc.Format = desc.BufferDesc.Format; explicit_desc.SampleDesc = desc.SampleDesc;
        explicit_desc.BufferUsage = desc.BufferUsage; explicit_desc.BufferCount = desc.BufferCount;
        explicit_desc.SwapEffect = desc.SwapEffect; explicit_desc.Flags = desc.Flags;
        ComPtr<IDXGISwapChain1> created_chain;
        if (FAILED(rsf_d3d11_present_create_for_hwnd(factory.Get(), producer, window, &explicit_desc,
            reinterpret_cast<void**>(created_chain.GetAddressOf()))) || FAILED(created_chain.As(&chain))) return 1;
    } else if (FAILED(factory->CreateSwapChain(producer, &desc, &chain))) return 1;
    if (!rsf_d3d11_present_has_owner()) { std::puts("SKIP: provider refused, original engine chain retained"); return 77; }
    // The first engine query prepares the deferred provider outside the factory callback.
    ComPtr<IDXGISwapChain2> latency_chain;
    if (FAILED(chain.As(&latency_chain))) return 1;
    DXGI_SWAP_CHAIN_DESC1 created{};
    if (FAILED(latency_chain->GetDesc1(&created))) return 1;
    ComPtr<ID3D11RenderTargetView> cached_rtv;
    if (flip11) {
        ComPtr<ID3D11Texture2D> back;
        if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&back))) ||
            FAILED(device11->CreateRenderTargetView(back.Get(), nullptr, &cached_rtv))) return 1;
    }
    HANDLE application_wait = nullptr;
    if (hot && native) {
        if (FAILED(latency_chain->SetMaximumFrameLatency(1))) return 1;
        application_wait = latency_chain->GetFrameLatencyWaitableObject();
        if (!application_wait) return 1;
    }
    if (backend == RSF_FG_BACKEND_AUTO && !rsf_fg_fixture_expect_auto(rsf_fg_default_backend(vendor, 0))) return 1;
    rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
    if (!rsf_d3d11_present_graphics(&graphics)) return 1;
    auto* gpu = static_cast<ID3D12Device*>(graphics.native_device);
    const auto* fsr4_directory = legacy_hot && argc == 7 ? argv[6] : argv[2];
    const bool fsr4_supported = rsf_generation_fsr4_supported(gpu, fsr4_directory) != 0;
    const auto backend_choices = rsf_d3d11_present_backend_choices();
    rsf_sr12* dlss_sr = nullptr;
    if (hot && argc == 7) {
        rsf_sr12_setup sr{}; sr.struct_size = sizeof(sr); sr.abi_version = 1; sr.device = gpu;
        sr.backend = 1; sr.output_width = 1280; sr.output_height = 720; sr.inverted_depth = 1;
        sr.streamline_host = rsf_d3d11_present_host();
        const std::string interposer = std::string(argv[6]) + "\\sl.interposer.dll";
        sr.dlss.struct_size = sizeof(sr.dlss); sr.dlss.abi_version = RSF_DLSS_ABI_VERSION;
        sr.dlss.interposer_path_utf8 = interposer.c_str(); sr.dlss.plugin_directory_utf8 = argv[6];
        sr.dlss.engine = RSF_DLSS_ENGINE_UNITY; sr.dlss.engine_version_utf8 = "6000.3";
        sr.dlss.project_id_utf8 = "57a42c7e-faf0-4bda-a9f9-892870948ac1";
        sr.dlss.require_signature = 1; sr.dlss.log = log;
        if (rsf_sr12_create(&sr, &dlss_sr) != RSF_BACKEND_OK) return 1;
        std::puts("PASS: native DLSS SR registered before runtime FSR/XeSS switches");
    }
    static_assert(sizeof(rsf_fg_fixture_inputs) == 48);
    rsf_fg_fixture_inputs inputs{};
    if (rsf_fg_bridge_fixture_allocate(gpu, uint32_t(mixed), uint32_t(hot), &inputs)) return 1;
    rsf_fg_fixture_state_inputs(state.get(), &inputs);
    ComPtr<ID3D12Fence> fence; if (FAILED(gpu->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return 1;
    HANDLE done = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!done) return 1;
    ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> source_list;
    if (service && rsf_fg_bridge_fixture_commands(gpu, reinterpret_cast<void**>(allocator.GetAddressOf()),
        reinterpret_cast<void**>(source_list.GetAddressOf()))) return 1;
    ComPtr<ID3D12CommandAllocator> finish_allocator; ComPtr<ID3D12GraphicsCommandList> finish_list;
    if (overlay && rsf_fg_bridge_fixture_commands(gpu, reinterpret_cast<void**>(finish_allocator.GetAddressOf()),
        reinterpret_cast<void**>(finish_list.GetAddressOf()))) return 1;
    const rsf_fg_loop run{
        chain.Get(), gpu, queue.Get(), graphics.queue, fence.Get(), done, application_wait, window,
        device11.Get(), context11.Get(), cached_rtv.Get(),
        allocator.Get(), source_list.Get(), finish_allocator.Get(), finish_list.Get(),
        backend, vendor, uint32_t(native), uint32_t(service), uint32_t(hot), uint32_t(legacy_hot),
        uint32_t(flip11), uint32_t(mfg_test), uint32_t(overlay), uint32_t(argc == 7),
        uint32_t(focus_test), uint32_t(fsr4_supported), backend_choices, log,
        state.get(),
    };
    void* focus_lease = nullptr;
    const auto loop_result = rsf_fg_fixture_run_loop(&run, &focus_lease);
    focus.reset(focus_lease);
    if (loop_result < 0) return 1;
    CloseHandle(done);
    if (!loop_result && flip11) std::puts("PASS: DX11 cached buffer-zero view reached every physical presentation");
    if (application_wait) { CloseHandle(application_wait); if (!loop_result) std::puts("PASS: one application latency handle survived provider transitions"); }
    rsf_sr12_destroy(dlss_sr);
    if (overlay) rsf_overlay_d3d12_stop();
    if (debug) {
        const bool accepted = rsf_present_fixture_debug_errors(gpu) == 0 && loop_result == 0;
        std::printf("D3D12 overlay/presentation validation=%u\n", unsigned(accepted));
        if (!accepted) return 1;
    }
    return loop_result;
}
