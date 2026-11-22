// SPDX-License-Identifier: MIT
#include <rescaleframe/d3d11_present_bridge.h>
#include "presentation/device_fixture_bridge.h"
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
#include <initializer_list>
#include <memory>
#include <string>
#include <thread>
#include <atomic>
extern "C" {
struct rsf_fg_bridge_frame_plan {
    uint32_t produce_input, submit, view;
    int32_t overlay_visible;
    uint32_t expected_tag, expect_refusal;
    int32_t legacy_request, legacy_expected, hot_request;
};
static_assert(sizeof(rsf_fg_bridge_frame_plan) == 36);
void rsf_fg_bridge_fixture_plan(uint64_t, uint32_t, uint32_t, uint32_t, uint32_t, uint32_t,
    rsf_fg_bridge_frame_plan*);
uint32_t rsf_fg_bridge_fixture_mfg_count(uint64_t);
void rsf_fg_bridge_fixture_barrier(void*, void*, uint32_t, uint32_t);
void rsf_fg_bridge_fixture_inputs(void*, void*, void*, void*, void*, uint32_t);
void rsf_fg_bridge_fixture_frame(void*, void*, uint64_t, uint64_t, uint32_t, rsf_frame_record*);
int32_t rsf_fg_bridge_fixture_hudless(void*, void*, void*, uint64_t, uint32_t, uint32_t, uint32_t);
int32_t rsf_fg_bridge_fixture_capture_flip(void*, void*, void**);
uint64_t rsf_fg_bridge_fixture_source(uint64_t, uint32_t);
}
using Microsoft::WRL::ComPtr;
namespace {
uint64_t frame_id = 0;
bool accepted = true;
ComPtr<ID3D12Resource> depth, motion;
ComPtr<ID3D12DescriptorHeap> cpu_heap, gpu_heap;
uint32_t stride = 0;
uint64_t observed_active = 0;
bool service = false;
bool hot = false;
bool mixed = false;
bool overlay = false;
bool legacy_hot = false;
bool flip11 = false;
ComPtr<ID3D12Resource> flip_readback;
uint64_t flip_capture_frame = 0;
bool mfg_test = false;
// Retain the provider through a Unity-style session epoch change as well as ids >32 bits.
uint64_t source_frame() { return rsf_fg_bridge_fixture_source(frame_id, uint32_t(hot)); }
ComPtr<ID3D12DescriptorHeap> color_heap;
void log(void*, const char* message) { std::printf("SDK: %s\n", message); }
int accept(void*, void*) { return 1; }
void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* texture, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to) {
    rsf_fg_bridge_fixture_barrier(list, texture, uint32_t(from), uint32_t(to));
}
void prepare(void*, void*, void*, void* commands, void* back, rsf_streamline_host*, void* session, uint32_t) {
    const auto* api = rsf_d3d11_present_provider();
    auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
    if (flip11) {
        flip_capture_frame = frame_id;
        if (!rsf_fg_bridge_fixture_capture_flip(commands, back, reinterpret_cast<void**>(flip_readback.GetAddressOf()))) {
            accepted = false;
            return;
        }
    }
    if (!service && (!api || !session)) return;
    rsf_fg_options options{sizeof(options), RSF_FG_ABI_VERSION, RSF_FG_FIXED, 1, 0, RSF_REFLEX_OFF, 0};
    if (!service) accepted &= api->configure(session, &options) == RSF_BACKEND_OK;
    rsf_fg_bridge_fixture_inputs(commands, depth.Get(), motion.Get(), cpu_heap.Get(), gpu_heap.Get(), stride);
    LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
    rsf_frame_record record{};
    rsf_fg_bridge_fixture_frame(back, depth.Get(), source_frame(), uint64_t(now.QuadPart), RSF_PHASE_UI_COMPLETE, &record);
    rsf_fg_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record;
    frame.interpolate = 1; frame.motion_scale_x = frame.motion_scale_y = 1;
    rsf_backend_resource* resources[]{&frame.backbuffer, &frame.depth, &frame.motion};
    void* textures[]{back, depth.Get(), motion.Get()};
    for (uint32_t i = 0; i < 3; ++i) {
        auto& r = *resources[i]; r.struct_size = sizeof(r); r.resource = textures[i]; r.generation = 1;
        r.width = i ? record.render_width : record.output_width; r.height = i ? record.render_height : record.output_height;
        r.state = i ? D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE : D3D12_RESOURCE_STATE_PRESENT;
    }
    if (service) rsf_fg12_capture(commands, &record, depth.Get(), motion.Get());
    if (hot) {
        if (!rsf_fg_bridge_fixture_hudless(commands, back, color_heap.Get(), frame_id, uint32_t(mfg_test),
            record.output_width, record.output_height)) {
            for (auto* texture : {depth.Get(), motion.Get()}) barrier(list, texture,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
            accepted = false;
            return;
        }
        rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.session_id = 1;
        pass.source_frame_id = record.frame_id; pass.view_key = 1; pass.color_input = back;
        rsf_fg12_hudless(commands, &pass);
        barrier(list, static_cast<ID3D12Resource*>(back), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
    }
    const auto result = service ? RSF_BACKEND_OK : api->prepare(session, commands, &frame);
    if (result != RSF_BACKEND_OK) std::printf("prepare frame=%llu refused=%d\n", static_cast<unsigned long long>(frame_id), result);
    accepted &= result == RSF_BACKEND_OK;
    for (auto* texture : {depth.Get(), motion.Get()}) barrier(list, texture,
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
}
void latency(void*, const rsf_observer_present_event* event) {
    const auto* api = rsf_d3d11_present_provider(); auto* session = rsf_d3d11_present_session();
    if (!api || !session) return;
    if (!event->completed) {
        accepted &= api->marker(session, RSF_LATENCY_RENDER_SUBMIT_END, source_frame(), 0) == RSF_BACKEND_OK;
        accepted &= api->marker(session, RSF_LATENCY_PRESENT_START, source_frame(), 0) == RSF_BACKEND_OK;
    } else accepted &= api->marker(session, RSF_LATENCY_PRESENT_END, source_frame(), 0) == RSF_BACKEND_OK;
}
void retire(void*, void* session) {
    const auto* api = rsf_d3d11_present_provider(); rsf_fg_status status{}; status.struct_size = sizeof(status);
    if (!api || !session) return;
    accepted &= api->status(session, &status) == RSF_BACKEND_OK;
    observed_active += status.active;
    void* fence = nullptr; uint64_t value = 0;
    accepted &= rsf_d3d11_present_retire(session, nullptr, &fence, &value) == RSF_BACKEND_OK && fence && value;
}
}
int main(int argc, char** argv) {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    if (argc != 5 && argc != 6 && argc != 7) { std::puts("SKIP: backend, SDK directory, vendor ID, API(11/12/12-hot), optional XeSS and Streamline directories"); return 77; }
    const uint32_t backend = uint32_t(std::strtoul(argv[1], nullptr, 0));
    const uint32_t vendor = uint32_t(std::strtoul(argv[3], nullptr, 0));
    const bool debug = std::getenv("RSF_UNITY_GPU_DEBUG") != nullptr;
    if (debug && rsf_present_fixture_debug12()) return 77;
    overlay = std::strcmp(argv[4], "12-overlay") == 0;
    mixed = std::strcmp(argv[4], "12-mixed") == 0 || overlay;
    mfg_test = std::strcmp(argv[4], "12-mfg") == 0;
    hot = std::strcmp(argv[4], "12-hot") == 0 || mixed || mfg_test;
    flip11 = std::strcmp(argv[4], "11-flip") == 0;
    legacy_hot = std::strcmp(argv[4], "11-hot") == 0 || flip11;
    service = hot || std::strcmp(argv[4], "12-service") == 0;
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
    rsf_d3d11_present_setup setup{}; setup.struct_size = sizeof(setup); setup.runtime_directory_utf8 = argv[2];
    setup.backend = backend; setup.max_generated_frames = mfg_test ? 5u : 3u; setup.log = log; setup.accept_window = accept;
    setup.prepare = prepare; setup.latency_event = latency; setup.retire = retire;
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
    if (backend == RSF_FG_BACKEND_AUTO) {
        const auto expected = rsf_fg_default_backend(vendor, 0);
        if (rsf_d3d11_present_backend() != expected || (expected && !rsf_d3d11_present_session())) {
            std::puts("FAIL: Auto must create the resolved provider, not silently fall back to Off");
            return 1;
        }
        std::printf("PASS: Auto instantiated provider %u before the first Present\n", expected);
    }
    rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
    if (!rsf_d3d11_present_graphics(&graphics)) return 1;
    auto* gpu = static_cast<ID3D12Device*>(graphics.native_device);
    const auto* fsr4_directory = legacy_hot && argc == 7 ? argv[6] : argv[2];
    const bool fsr4_supported = rsf_generation_fsr4_supported(gpu, fsr4_directory) != 0;
    accepted &= bool(rsf_d3d11_present_backend_choices() & (1u << RSF_FG_BACKEND_FSR4)) == fsr4_supported;
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
    cpu_heap.Attach(static_cast<ID3D12DescriptorHeap*>(inputs.cpu_heap));
    gpu_heap.Attach(static_cast<ID3D12DescriptorHeap*>(inputs.gpu_heap));
    color_heap.Attach(static_cast<ID3D12DescriptorHeap*>(inputs.color_heap));
    depth.Attach(static_cast<ID3D12Resource*>(inputs.depth));
    motion.Attach(static_cast<ID3D12Resource*>(inputs.motion));
    stride = inputs.stride;
    const auto* api = rsf_d3d11_present_provider(); auto* session = rsf_d3d11_present_session();
    ComPtr<ID3D12Fence> fence; if (FAILED(gpu->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return 1;
    HANDLE done = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!done) return 1;
    ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> source_list;
    if (service && rsf_fg_bridge_fixture_commands(gpu, reinterpret_cast<void**>(allocator.GetAddressOf()),
        reinterpret_cast<void**>(source_list.GetAddressOf()))) return 1;
    ComPtr<ID3D12CommandAllocator> finish_allocator; ComPtr<ID3D12GraphicsCommandList> finish_list;
    if (overlay && rsf_fg_bridge_fixture_commands(gpu, reinterpret_cast<void**>(finish_allocator.GetAddressOf()),
        reinterpret_cast<void**>(finish_list.GetAddressOf()))) return 1;
    uint64_t expected_tagged = 0;
    ComPtr<ID3D12Resource> held_buffer;
    if (hot && FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&held_buffer)))) return 1;
    uint32_t active_fsr = 0, active_xess = 0;
    std::atomic<unsigned> bad_queries{0}, query_count{0};
    std::jthread queries;
    if (hot) queries = std::jthread([held = chain, &bad_queries, &query_count](std::stop_token stop) {
        UINT previous = 0;
        while (!stop.stop_requested()) {
            bad_queries += rsf_fg_bridge_fixture_query(held.Get(), &previous);
            ++query_count; std::this_thread::yield();
        }
    });
    uint32_t mfg_active[6]{};
    uint64_t previous_total = 0;
    if (focus_test) {
        focus.reset(rsf_fg_bridge_fixture_focus_start(window));
        if (!focus) return 1;
    }
    for (frame_id = 1; frame_id <= (mfg_test ? 130u : 40u); ++frame_id) {
        if (focus_test && !rsf_fg_bridge_fixture_focus_valid(focus.get())) {
            std::puts("FAIL: focus test ended at its time limit or after a human focus change");
            accepted = false; break;
        }
        if (mfg_test) {
            MSG message{};
            while (PeekMessageW(&message, window, 0, 0, PM_REMOVE)) DispatchMessageW(&message);
        }
        const uint32_t generated = mfg_test ? rsf_fg_bridge_fixture_mfg_count(frame_id) : 0;
        if (generated) {
            rsf_native_fg_options changed{sizeof(changed), frame_id == 90 ? RSF_FG_OFF : RSF_FG_FIXED, generated, RSF_REFLEX_ON, 0, 0};
            rsf_fg12_options(&changed);
        }
        // Keep exactly the same engine handle across Off, FSR, XeSS and DLSS replacement.
        if (application_wait && WaitForSingleObject(application_wait, 10000) != WAIT_OBJECT_0) {
            std::printf("FAIL: retained application latency handle stalled at frame %llu\n", static_cast<unsigned long long>(frame_id)); return 1;
        }
        const auto current_backend = rsf_d3d11_present_backend();
        const auto capability_epoch = rsf_d3d11_present_generation();
        rsf_fg_bridge_frame_plan plan{};
        rsf_fg_bridge_fixture_plan(frame_id, uint32_t(hot), uint32_t(mfg_test), vendor, current_backend,
            uint32_t(argc == 7), &plan);
        session = rsf_d3d11_present_session();
        if (service) {
            for (auto stage : {RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_PACING, RSF_GAME_CPU_INPUT_SAMPLE,
                RSF_GAME_CPU_SIMULATION_BEGIN, RSF_GAME_CPU_SIMULATION_END}) {
                LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
                rsf_game_cpu_event event{sizeof(event), stage, 1, source_frame(), uint64_t(now.QuadPart), uint64_t(frequency.QuadPart), 0, 0, 0};
                rsf_fg12_cpu(&event);
            }
            const rsf_fg_fixture_submission submission{
                chain.Get(), gpu, queue.Get(), allocator.Get(), source_list.Get(),
                finish_allocator.Get(), finish_list.Get(), session, source_frame(),
                plan.produce_input, plan.submit, plan.view, uint32_t(overlay), plan.overlay_visible,
                [](void* commands, void* backbuffer, void* session) {
                    prepare(nullptr, nullptr, nullptr, commands, backbuffer, nullptr, session, 0);
                }, log,
            };
            auto frame_accepted = uint32_t(accepted);
            if (rsf_fg_bridge_fixture_submit(&submission, &frame_accepted)) return 1;
            accepted &= frame_accepted != 0;
        } else {
            if (legacy_hot) {
                accepted &= rsf_d3d11_present_acquire(frame_id) == RSF_BACKEND_OK;
                accepted &= rsf_d3d11_present_begin(frame_id, nullptr) == RSF_BACKEND_OK;
            } else accepted &= api->begin_frame(session, frame_id) == RSF_BACKEND_OK;
            for (auto marker : {RSF_LATENCY_SIMULATION_START, RSF_LATENCY_SIMULATION_END, RSF_LATENCY_RENDER_SUBMIT_START})
                accepted &= legacy_hot ? rsf_d3d11_present_marker(rsf_d3d11_present_generation(), frame_id, marker, 0) == RSF_BACKEND_OK :
                    api->marker(session, marker, frame_id, 0) == RSF_BACKEND_OK;
        }
        if (flip11) {
            // DX11 keeps this view across Present; it does not select a DX12-style index.
            const float color[]{float(frame_id % 2), 0.5f, 0.25f, 1};
            context11->ClearRenderTargetView(cached_rtv.Get(), color);
        }
        const auto presented = chain->Present(0, 0);
        if (FAILED(presented)) {
            std::printf("Present failed frame=%llu backend=%u HRESULT=%08lx device=%08lx\n",
                static_cast<unsigned long long>(frame_id), rsf_d3d11_present_backend(), presented, gpu->GetDeviceRemovedReason());
            return 1;
        }
        if (legacy_hot) {
            rsf_d3d11_present_abort(frame_id);
            if (plan.legacy_request >= 0)
                accepted &= rsf_d3d11_present_request(uint32_t(plan.legacy_request)) == RSF_BACKEND_OK;
            const auto active = rsf_d3d11_present_backend();
            if (plan.legacy_expected >= 0) {
                std::printf("legacy runtime switch frame=%llu backend=%u result=%d\n",
                    static_cast<unsigned long long>(frame_id), active, rsf_d3d11_present_switch_result());
                accepted &= active == uint32_t(plan.legacy_expected);
            }
        }
        if (service) {
            rsf_native_fg_status result{}; result.struct_size = sizeof(result); rsf_fg12_status(&result);
            if (const auto* current = rsf_d3d11_present_provider()) {
                rsf_fg_status capability{}; capability.struct_size = sizeof(capability);
                if (auto* current_session = rsf_d3d11_present_session(); current_session && current->status(current_session, &capability) == RSF_BACKEND_OK) {
                    const bool deferred = !result.vendor.supported && !result.vendor.max_generated_frames &&
                        capability_epoch != rsf_d3d11_present_generation();
                    const bool same = deferred || (result.vendor.max_generated_frames == capability.max_generated_frames && result.vendor.supported == capability.supported);
                    if (deferred) std::printf("Capability report deferred after provider switch frame=%llu backend=%u\n",
                        static_cast<unsigned long long>(frame_id), rsf_d3d11_present_backend());
                    if (!same) std::printf("Capability mismatch frame=%llu backend=%u cached=%u/%u current=%u/%u\n",
                        static_cast<unsigned long long>(frame_id), rsf_d3d11_present_backend(), result.vendor.supported, result.vendor.max_generated_frames,
                        capability.supported, capability.max_generated_frames);
                    accepted &= same;
                }
            }
            expected_tagged += plan.expected_tag;
            observed_active += result.vendor.active; accepted &= result.last_result == RSF_BACKEND_OK && result.tagged_frames == expected_tagged;
            if (mfg_test && current_backend == 1) {
                const auto& vendor_state = result.vendor;
                const uint64_t delta = vendor_state.total_presented >= previous_total ? vendor_state.total_presented - previous_total : 0;
                if (vendor_state.active && vendor_state.effective_generated_frames < 6 &&
                    delta >= vendor_state.effective_generated_frames + 1u)
                    ++mfg_active[vendor_state.effective_generated_frames];
                previous_total = vendor_state.total_presented;
                accepted &= vendor_state.mfg_unlock_status == RSF_MFG_UNLOCK_READY && vendor_state.max_generated_frames >= 5;
                std::printf("MFG frame=%llu admission=%u max=%u configured=%u active=%u presented_delta=%llu\n",
                    static_cast<unsigned long long>(frame_id), vendor_state.mfg_unlock_status, vendor_state.max_generated_frames,
                    vendor_state.effective_generated_frames, vendor_state.active, static_cast<unsigned long long>(delta));
            }
            if (plan.expect_refusal) accepted &= result.reason != 0;
            if (hot) {
                if (current_backend == 3) active_fsr += result.vendor.active;
                if (current_backend == 5) active_xess += result.vendor.active;
                ComPtr<ID3D12Resource> same; accepted &= SUCCEEDED(chain->GetBuffer(0, IID_PPV_ARGS(&same))) && same.Get() == held_buffer.Get();
                if (frame_id == 35 && vendor == 0x10de) accepted &= rsf_d3d11_present_backend() == 3 && rsf_d3d11_present_switch_result() == RSF_BACKEND_ERROR_NOT_SUPPORTED;
                if (plan.hot_request >= 0) {
                    const auto expected = plan.hot_request == 4 && !fsr4_supported ?
                        RSF_BACKEND_ERROR_NOT_SUPPORTED : RSF_BACKEND_OK;
                    accepted &= rsf_d3d11_present_request(uint32_t(plan.hot_request)) == expected;
                }
                if (frame_id == 36 && argc == 7) accepted &= rsf_d3d11_present_backend() == 1;
            }
        }
        auto* native_queue = static_cast<ID3D12CommandQueue*>(flip11 ? rsf_d3d11_present_queue() : graphics.queue);
        if (rsf_fg_bridge_fixture_drain(native_queue, fence.Get(), done, frame_id)) return 1;
        if (flip11) {
            const auto matched = rsf_fg_bridge_fixture_check_flip(flip_readback.Get(), frame_id);
            if (matched < 0) return 1;
            if (!matched) {
                rsf_d3d11_fixture_flip_source(device11.Get(), context11.Get(), cached_rtv.Get(), flip_capture_frame);
                return 1;
            }
        }
    }
    rsf_fg_status status{}; status.struct_size = sizeof(status);
    if (mfg_test) {
        std::printf("MFG confirmed samples 2x=%u 3x=%u 4x=%u 6x=%u\n", mfg_active[1], mfg_active[2], mfg_active[3], mfg_active[5]);
        accepted &= mfg_active[1] && mfg_active[2] && mfg_active[3] && mfg_active[5];
    }
    if (hot) {
        queries.request_stop(); queries.join();
        std::printf("concurrent engine queries=%u failures=%u\n", query_count.load(), bad_queries.load());
        accepted &= query_count.load() != 0 && bad_queries.load() == 0;
    }
    if (hot) {
        rsf_native_fg_status result{}; result.struct_size = sizeof(result); rsf_fg12_status(&result); status = result.vendor;
        std::printf("runtime switches: FSR active=%u XeSS active=%u buffer identity preserved=%u\n", active_fsr, active_xess, accepted);
        accepted &= active_fsr != 0 && active_xess != 0;
    } else if (legacy_hot) rsf_d3d11_present_provider()->status(rsf_d3d11_present_session(), &status);
    else api->status(session, &status);
    std::printf("backend=%u API=%u version=%s accepted=%u active_samples=%llu callbacks=%llu total_presented=%llu\n",
        backend, native ? 12u : 11u, status.version_name, accepted, static_cast<unsigned long long>(observed_active),
        static_cast<unsigned long long>(status.generated_callbacks), static_cast<unsigned long long>(status.total_presented));
    CloseHandle(done);
    if (flip11) std::puts("PASS: DX11 cached buffer-zero view reached every physical presentation");
    if (application_wait) { CloseHandle(application_wait); std::puts("PASS: one application latency handle survived provider transitions"); }
    rsf_sr12_destroy(dlss_sr);
    if (overlay) rsf_overlay_d3d12_stop();
    if (debug) {
        accepted &= rsf_present_fixture_debug_errors(gpu) == 0;
        std::printf("D3D12 overlay/presentation validation=%u\n", accepted);
    }
    return accepted && (observed_active || legacy_hot) ? 0 : 1;
}
