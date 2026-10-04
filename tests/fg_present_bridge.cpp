// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/overlay_d3d12.h>
#include <windows.h>
#include <d3d11.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
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
ComPtr<ID3D12DescriptorHeap> color_heap;
void log(void*, const char* message) { std::printf("SDK: %s\n", message); }
int accept(void*, void*) { return 1; }
void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* texture, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to) {
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {texture, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}
void prepare(void*, void*, void*, void* commands, void* back, rsf_streamline_host*, void* session, uint32_t) {
    const auto* api = rsf_d3d11_present_provider();
    if (!service && (!api || !session)) return;
    auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
    rsf_fg_options options{sizeof(options), RSF_FG_ABI_VERSION, RSF_FG_FIXED, 1, 0, RSF_REFLEX_OFF, 0};
    if (!service) accepted &= api->configure(session, &options) == RSF_BACKEND_OK;
    auto* heap = gpu_heap.Get(); list->SetDescriptorHeaps(1, &heap);
    auto gpu = gpu_heap->GetGPUDescriptorHandleForHeapStart(); auto cpu = cpu_heap->GetCPUDescriptorHandleForHeapStart();
    const float values[]{0.5f, 0, 0, 0};
    for (auto* texture : {depth.Get(), motion.Get()}) {
        barrier(list, texture, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        list->ClearUnorderedAccessViewFloat(gpu, cpu, texture, values, 0, nullptr);
        barrier(list, texture, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        gpu.ptr += stride; cpu.ptr += stride;
    }
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 1; record.frame_id = frame_id; record.resource_generation = 1;
    record.view_id = 1;
    LARGE_INTEGER now{}; QueryPerformanceCounter(&now); record.input_qpc = uint64_t(now.QuadPart);
    record.phase = RSF_PHASE_UI_COMPLETE; record.screen = RSF_SCREEN_FLIGHT;
    const auto back_desc = static_cast<ID3D12Resource*>(back)->GetDesc();
    record.output_width = static_cast<uint32_t>(back_desc.Width); record.output_height = back_desc.Height;
    record.render_width = static_cast<uint32_t>(depth->GetDesc().Width); record.render_height = depth->GetDesc().Height;
    record.frame_time_ms = 16.67f;
    auto& camera = record.camera; camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    camera.near_plane = 0.1f; camera.far_plane = 1000; camera.vertical_fov_radians = 1;
    camera.depth_inverted = 1;
    for (uint32_t i = 0; i < 4; ++i) camera.view_to_clip[i * 5] = camera.clip_to_view[i * 5] =
        camera.view_to_world[i * 5] = camera.world_to_view[i * 5] = camera.clip_to_previous_clip[i * 5] = 1;
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
        auto* color = static_cast<ID3D12Resource*>(back);
        ComPtr<ID3D12Device> owner; color->GetDevice(IID_PPV_ARGS(&owner));
        owner->CreateRenderTargetView(color, nullptr, color_heap->GetCPUDescriptorHandleForHeapStart());
        barrier(list, color, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
        const float scene[]{0.2f, 0.4f, 0.6f, 1};
        list->ClearRenderTargetView(color_heap->GetCPUDescriptorHandleForHeapStart(), scene, 0, nullptr);
        rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.session_id = 1;
        pass.source_frame_id = record.frame_id; pass.view_key = 1; pass.color_input = back;
        rsf_fg12_hudless(commands, &pass);
        barrier(list, color, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
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
        accepted &= api->marker(session, RSF_LATENCY_RENDER_SUBMIT_END, frame_id, 0) == RSF_BACKEND_OK;
        accepted &= api->marker(session, RSF_LATENCY_PRESENT_START, frame_id, 0) == RSF_BACKEND_OK;
    } else accepted &= api->marker(session, RSF_LATENCY_PRESENT_END, frame_id, 0) == RSF_BACKEND_OK;
}
void retire(void*, void* session) {
    const auto* api = rsf_d3d11_present_provider(); rsf_fg_status status{}; status.struct_size = sizeof(status);
    if (!api || !session) return;
    accepted &= api->status(session, &status) == RSF_BACKEND_OK;
    observed_active += status.active;
    rsf_fg_retirement retirement{}; retirement.struct_size = sizeof(retirement);
    accepted &= api->retirement(session, &retirement) == RSF_BACKEND_OK;
}
}
int main(int argc, char** argv) {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    if (argc != 5 && argc != 6 && argc != 7) { std::puts("SKIP: backend, SDK directory, vendor ID, API(11/12/12-hot), optional XeSS and Streamline directories"); return 77; }
    const uint32_t backend = uint32_t(std::strtoul(argv[1], nullptr, 0));
    const uint32_t vendor = uint32_t(std::strtoul(argv[3], nullptr, 0));
    const bool debug = std::getenv("RSF_UNITY_GPU_DEBUG") != nullptr;
    if (debug) {
        ComPtr<ID3D12Debug> layer;
        if (FAILED(D3D12GetDebugInterface(IID_PPV_ARGS(&layer)))) return 77;
        layer->EnableDebugLayer();
    }
    overlay = std::strcmp(argv[4], "12-overlay") == 0;
    mixed = std::strcmp(argv[4], "12-mixed") == 0 || overlay;
    hot = std::strcmp(argv[4], "12-hot") == 0 || mixed;
    legacy_hot = std::strcmp(argv[4], "11-hot") == 0;
    service = hot || std::strcmp(argv[4], "12-service") == 0;
    const bool native = std::strcmp(argv[4], "11") != 0 && !legacy_hot;
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return 1;
    for (UINT i = 0;; ++i) {
        ComPtr<IDXGIAdapter1> candidate; if (factory->EnumAdapters1(i, &candidate) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_ADAPTER_DESC1 desc{}; candidate->GetDesc1(&desc);
        if (desc.VendorId == vendor && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) { adapter = candidate; break; }
    }
    if (!adapter) return 77;
    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"RSFSharedFG";
    if (!RegisterClassW(&wc)) return 1;
    HWND window = CreateWindowW(wc.lpszClassName, L"Shared generation fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 1280, 720, nullptr, nullptr, wc.hInstance, nullptr);
    if (!window) return 1;
    rsf_d3d11_present_setup setup{}; setup.struct_size = sizeof(setup); setup.runtime_directory_utf8 = argv[2];
    setup.backend = backend; setup.max_generated_frames = 3; setup.log = log; setup.accept_window = accept;
    setup.prepare = prepare; setup.latency_event = latency; setup.retire = retire;
    if (legacy_hot) {
        setup.runtime_switching = 1; setup.streamline_directory_utf8 = argv[2];
        setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = argc == 7 ? argv[6] : argv[2];
        setup.xess_directory_utf8 = argc >= 6 ? argv[5] : argv[2];
    }
    if (service) {
        rsf_fg12_setup request{}; request.struct_size = sizeof(request); request.backend = backend;
        request.runtime_directory_utf8 = argv[2]; request.max_generated_frames = 3;
        request.options = {sizeof(request.options), RSF_FG_FIXED, 1, RSF_REFLEX_OFF, 0, 0};
        request.depth_inverted = 1; request.units_to_meters = 1; request.log = log; request.accept_window = accept;
        if (hot) {
            request.runtime_switching = 1; request.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
            request.fsr3_directory_utf8 = request.fsr4_directory_utf8 = argv[2];
            request.xess_directory_utf8 = argc >= 6 ? argv[5] : argv[2];
            request.streamline_directory_utf8 = argc == 7 ? argv[6] : nullptr;
        }
        if (rsf_fg12_install(&request) != RSF_BACKEND_OK) return 1;
    } else if (!rsf_d3d11_present_install(&setup)) return 1;
    ComPtr<ID3D11Device> device11; ComPtr<ID3D11DeviceContext> context11;
    ComPtr<ID3D12Device> device12; ComPtr<ID3D12CommandQueue> queue;
    IUnknown* producer = nullptr;
    if (native) {
        if (FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device12)))) return 1;
        D3D12_COMMAND_QUEUE_DESC desc{}; desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        if (FAILED(device12->CreateCommandQueue(&desc, IID_PPV_ARGS(&queue)))) return 1;
        producer = queue.Get();
    } else {
        if (FAILED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, 0, nullptr, 0,
            D3D11_SDK_VERSION, &device11, nullptr, &context11))) return 1;
        producer = device11.Get();
    }
    DXGI_SWAP_CHAIN_DESC desc{}; desc.BufferDesc.Width = mixed ? 2560u : 1280u; desc.BufferDesc.Height = mixed ? 1600u : 720u;
    desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; desc.BufferCount = native ? 3u : 1u;
    desc.OutputWindow = window; desc.Windowed = TRUE; desc.SwapEffect = native ? DXGI_SWAP_EFFECT_FLIP_DISCARD : DXGI_SWAP_EFFECT_DISCARD;
    ComPtr<IDXGISwapChain> chain;
    if (FAILED(factory->CreateSwapChain(producer, &desc, &chain))) return 1;
    if (!rsf_d3d11_present_has_owner()) { std::puts("SKIP: provider refused, original engine chain retained"); return 77; }
    rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
    if (!rsf_d3d11_present_graphics(&graphics)) return 1;
    auto* gpu = static_cast<ID3D12Device*>(graphics.native_device);
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
    D3D12_DESCRIPTOR_HEAP_DESC heaps{}; heaps.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV; heaps.NumDescriptors = 2;
    if (FAILED(gpu->CreateDescriptorHeap(&heaps, IID_PPV_ARGS(&cpu_heap)))) return 1;
    heaps.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(gpu->CreateDescriptorHeap(&heaps, IID_PPV_ARGS(&gpu_heap)))) return 1;
    stride = gpu->GetDescriptorHandleIncrementSize(heaps.Type);
    if (hot) {
        D3D12_DESCRIPTOR_HEAP_DESC desc_color{}; desc_color.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV; desc_color.NumDescriptors = 1;
        if (FAILED(gpu->CreateDescriptorHeap(&desc_color, IID_PPV_ARGS(&color_heap)))) return 1;
    }
    auto cpu = cpu_heap->GetCPUDescriptorHandleForHeapStart(); auto visible = gpu_heap->GetCPUDescriptorHandleForHeapStart();
    for (auto* target : {std::addressof(depth), std::addressof(motion)}) {
        D3D12_RESOURCE_DESC texture{}; texture.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        texture.Width = mixed ? 1485u : 1280u; texture.Height = mixed ? 928u : 720u; texture.DepthOrArraySize = texture.MipLevels = 1;
        texture.SampleDesc.Count = 1; texture.Format = target == std::addressof(depth) ? DXGI_FORMAT_R32_FLOAT : DXGI_FORMAT_R16G16_FLOAT;
        texture.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
        if (FAILED(gpu->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &texture, D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(target->GetAddressOf())))) return 1;
        gpu->CreateUnorderedAccessView(target->Get(), nullptr, nullptr, cpu);
        gpu->CopyDescriptorsSimple(1, visible, cpu, heaps.Type); cpu.ptr += stride; visible.ptr += stride;
    }
    const auto* api = rsf_d3d11_present_provider(); auto* session = rsf_d3d11_present_session();
    ComPtr<ID3D12Fence> fence; if (FAILED(gpu->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return 1;
    HANDLE done = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!done) return 1;
    ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> source_list;
    if (service && (FAILED(gpu->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator))) ||
        FAILED(gpu->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator.Get(), nullptr, IID_PPV_ARGS(&source_list))) ||
        FAILED(source_list->Close()))) return 1;
    ComPtr<ID3D12CommandAllocator> finish_allocator; ComPtr<ID3D12GraphicsCommandList> finish_list;
    if (overlay && (FAILED(gpu->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&finish_allocator))) ||
        FAILED(gpu->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, finish_allocator.Get(), nullptr, IID_PPV_ARGS(&finish_list))) ||
        FAILED(finish_list->Close()))) return 1;
    uint64_t expected_tagged = 0;
    ComPtr<ID3D12Resource> held_buffer;
    if (hot && FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&held_buffer)))) return 1;
    uint32_t active_fsr = 0, active_xess = 0;
    std::atomic<unsigned> bad_queries{0}, query_count{0};
    std::jthread queries;
    if (hot) queries = std::jthread([held = chain, &bad_queries, &query_count](std::stop_token stop) {
        UINT previous = 0;
        while (!stop.stop_requested()) {
            BOOL fullscreen = FALSE;
            const auto result = held->GetFullscreenState(&fullscreen, nullptr);
            if (FAILED(result) && result != DXGI_ERROR_WAS_STILL_DRAWING) ++bad_queries;
            UINT count = 0;
            if (SUCCEEDED(held->GetLastPresentCount(&count))) {
                if (count < previous) ++bad_queries;
                previous = count;
            }
            DXGI_FRAME_STATISTICS statistics{}; (void)held->GetFrameStatistics(&statistics);
            ++query_count; std::this_thread::yield();
        }
    });
    for (frame_id = 1; frame_id <= 40; ++frame_id) {
        const auto current_backend = rsf_d3d11_present_backend();
        session = rsf_d3d11_present_session();
        if (service) {
            for (auto stage : {RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_PACING, RSF_GAME_CPU_INPUT_SAMPLE,
                RSF_GAME_CPU_SIMULATION_BEGIN, RSF_GAME_CPU_SIMULATION_END}) {
                LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
                rsf_game_cpu_event event{sizeof(event), stage, 1, frame_id, uint64_t(now.QuadPart), uint64_t(frequency.QuadPart), 0, 0, 0};
                rsf_fg12_cpu(&event);
            }
            ComPtr<IDXGISwapChain3> chain3; if (FAILED(chain.As(&chain3))) return 1;
            ComPtr<ID3D12Resource> back;
            if (FAILED(chain3->GetBuffer(chain3->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&back))) ||
                FAILED(allocator->Reset()) || FAILED(source_list->Reset(allocator.Get(), nullptr))) return 1;
            // An input refusal must not leak the shared CPU token and stall later frames.
            if (!hot || frame_id != 7) prepare(nullptr, nullptr, nullptr, source_list.Get(), back.Get(), nullptr, session, 0);
            if (overlay) barrier(source_list.Get(), back.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
            if (FAILED(source_list->Close())) return 1;
            ID3D12CommandList* commands[]{source_list.Get()}; queue->ExecuteCommandLists(1, commands);
            if (hot || frame_id != 11) rsf_fg12_submitted(1, frame_id);
            rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.session_id = 1;
            pass.native_frame = pass.source_frame_id = frame_id; pass.view_key = !hot && frame_id == 10 ? 2u : 1u; pass.swapchain = chain.Get();
            std::thread render_callback([&pass] { rsf_fg12_window(&pass); });
            render_callback.join();
            if (overlay) {
                rsf_overlay_stats stats{}; stats.struct_size = sizeof(stats); stats.backend_name = "Fixture";
                stats.show_performance_hud = 1; stats.output_width = 2560; stats.output_height = 1600;
                rsf_overlay_intent intent{}; intent.struct_size = sizeof(intent);
                accepted &= rsf_overlay_d3d12_frame(gpu, queue.Get(), chain.Get(), &stats, &intent, log, nullptr) != 0;
                if (FAILED(finish_allocator->Reset()) || FAILED(finish_list->Reset(finish_allocator.Get(), nullptr))) return 1;
                barrier(finish_list.Get(), back.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
                if (FAILED(finish_list->Close())) return 1;
                ID3D12CommandList* finished[]{finish_list.Get()}; queue->ExecuteCommandLists(1, finished);
            }
        } else {
            if (legacy_hot) {
                accepted &= rsf_d3d11_present_acquire(frame_id) == RSF_BACKEND_OK;
                accepted &= rsf_d3d11_present_begin(frame_id, nullptr) == RSF_BACKEND_OK;
            } else accepted &= api->begin_frame(session, frame_id) == RSF_BACKEND_OK;
            for (auto marker : {RSF_LATENCY_SIMULATION_START, RSF_LATENCY_SIMULATION_END, RSF_LATENCY_RENDER_SUBMIT_START})
                accepted &= legacy_hot ? rsf_d3d11_present_marker(rsf_d3d11_present_generation(), frame_id, marker, 0) == RSF_BACKEND_OK :
                    api->marker(session, marker, frame_id, 0) == RSF_BACKEND_OK;
        }
        const auto presented = chain->Present(0, 0);
        if (FAILED(presented)) {
            std::printf("Present failed frame=%llu backend=%u HRESULT=%08lx device=%08lx\n",
                static_cast<unsigned long long>(frame_id), rsf_d3d11_present_backend(), presented, gpu->GetDeviceRemovedReason());
            return 1;
        }
        if (legacy_hot) {
            rsf_d3d11_present_abort(frame_id);
            if (frame_id == 8) accepted &= rsf_d3d11_present_request(3) == RSF_BACKEND_OK;
            if (frame_id == 15) accepted &= rsf_d3d11_present_request(5) == RSF_BACKEND_OK;
            if (frame_id == 25) accepted &= rsf_d3d11_present_request(0) == RSF_BACKEND_OK;
            if (frame_id == 32) accepted &= rsf_d3d11_present_request(1) == RSF_BACKEND_OK;
            const auto active = rsf_d3d11_present_backend();
            if (frame_id == 9 || frame_id == 16 || frame_id == 26 || frame_id == 33)
                std::printf("legacy runtime switch frame=%llu backend=%u result=%d\n", static_cast<unsigned long long>(frame_id), active, rsf_d3d11_present_switch_result());
            if (frame_id == 9) accepted &= active == 3;
            if (frame_id == 16) accepted &= active == 5;
            if (frame_id == 26) accepted &= active == 0;
            if (frame_id == 33) accepted &= active == 1;
        }
        if (service) {
            rsf_native_fg_status result{}; result.struct_size = sizeof(result); rsf_fg12_status(&result);
            if (hot ? current_backend != 0 && frame_id != 7 : frame_id != 10 && frame_id != 11) ++expected_tagged;
            observed_active += result.vendor.active; accepted &= result.last_result == RSF_BACKEND_OK && result.tagged_frames == expected_tagged;
            if (!hot && (frame_id == 10 || frame_id == 11)) accepted &= result.reason != 0;
            if (hot) {
                if (current_backend == 3) active_fsr += result.vendor.active;
                if (current_backend == 5) active_xess += result.vendor.active;
                ComPtr<ID3D12Resource> same; accepted &= SUCCEEDED(chain->GetBuffer(0, IID_PPV_ARGS(&same))) && same.Get() == held_buffer.Get();
                if (frame_id == 2 && current_backend == 0) accepted &= rsf_d3d11_present_request(3) == RSF_BACKEND_OK;
                if (frame_id == 8) accepted &= rsf_d3d11_present_request(0) == RSF_BACKEND_OK;
                if (frame_id == 15) accepted &= rsf_d3d11_present_request(5) == RSF_BACKEND_OK;
                if (frame_id == 25) accepted &= rsf_d3d11_present_request(3) == RSF_BACKEND_OK;
                if (frame_id == 34 && vendor == 0x10de) accepted &= rsf_d3d11_present_request(4) == RSF_BACKEND_OK;
                if (frame_id == 35 && vendor == 0x10de) accepted &= rsf_d3d11_present_backend() == 3 && rsf_d3d11_present_switch_result() == RSF_BACKEND_ERROR_NOT_SUPPORTED;
                if (frame_id == 35 && argc == 7) accepted &= rsf_d3d11_present_request(1) == RSF_BACKEND_OK;
                if (frame_id == 36 && argc == 7) accepted &= rsf_d3d11_present_backend() == 1;
            }
        }
        auto* native_queue = static_cast<ID3D12CommandQueue*>(graphics.queue);
        if (FAILED(native_queue->Signal(fence.Get(), frame_id)) || FAILED(fence->SetEventOnCompletion(frame_id, done)) ||
            WaitForSingleObject(done, 10000) != WAIT_OBJECT_0) return 1;
    }
    rsf_fg_status status{}; status.struct_size = sizeof(status);
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
    rsf_sr12_destroy(dlss_sr);
    if (overlay) rsf_overlay_d3d12_stop();
    if (debug) {
        ComPtr<ID3D12InfoQueue> messages;
        if (FAILED(gpu->QueryInterface(IID_PPV_ARGS(&messages)))) return 1;
        for (UINT64 i = 0; i < messages->GetNumStoredMessagesAllowedByRetrievalFilter(); ++i) {
            SIZE_T bytes = 0; messages->GetMessage(i, nullptr, &bytes);
            auto storage = std::make_unique<unsigned char[]>(bytes);
            auto* message = reinterpret_cast<D3D12_MESSAGE*>(storage.get());
            if (SUCCEEDED(messages->GetMessage(i, message, &bytes)) && message->Severity <= D3D12_MESSAGE_SEVERITY_ERROR) {
                std::printf("D3D12 validation: %s\n", message->pDescription); accepted = false;
            }
        }
        std::printf("D3D12 overlay/presentation validation=%u\n", accepted);
    }
    return accepted && (observed_active || legacy_hot) ? 0 : 1;
}
