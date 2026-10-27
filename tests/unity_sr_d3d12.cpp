// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/streamline_host.h>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
#endif
#include <rescaleframe/unity_bridge.h>
#include <rescaleframe/unity_sr_host.h>
#include <rescaleframe/rdr2_sr_host.h>
#include <rescaleframe/rdr2_frontend.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <DirectXMath.h>
#include <DirectXPackedVector.h>
#include <wrl/client.h>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
static void logger(void*, const char* message) { std::fprintf(stderr, "%s\n", message); }
static void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after}; list->ResourceBarrier(1, &b);
}
static bool texture(ID3D12Device* device, uint32_t width, uint32_t height, DXGI_FORMAT format,
    D3D12_RESOURCE_FLAGS flags, D3D12_RESOURCE_STATES state, ComPtr<ID3D12Resource>& out)
{
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width; desc.Height = height; desc.DepthOrArraySize = desc.MipLevels = 1;
    desc.Format = format; desc.SampleDesc.Count = 1; desc.Flags = flags;
    if (std::getenv("RSF_SR_TYPELESS_INPUTS")) {
        if (format == DXGI_FORMAT_R16G16B16A16_FLOAT) desc.Format = DXGI_FORMAT_R16G16B16A16_TYPELESS;
        if (format == DXGI_FORMAT_R16G16_FLOAT) desc.Format = DXGI_FORMAT_R16G16_TYPELESS;
    }
    return SUCCEEDED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, nullptr, IID_PPV_ARGS(&out)));
}
static bool buffer(ID3D12Device* device, uint64_t size, D3D12_HEAP_TYPE type, D3D12_RESOURCE_STATES state, ComPtr<ID3D12Resource>& out)
{
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = type;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; desc.Width = size;
    desc.Height = desc.DepthOrArraySize = desc.MipLevels = 1; desc.SampleDesc.Count = 1; desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    return SUCCEEDED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, nullptr, IID_PPV_ARGS(&out)));
}
static bool validation(ID3D12Device* device)
{
    if (!std::getenv("RSF_UNITY_GPU_DEBUG")) return true;
    ComPtr<ID3D12InfoQueue> info;
    if (FAILED(device->QueryInterface(IID_PPV_ARGS(&info)))) return false;
    bool ok = true;
    const auto count = info->GetNumStoredMessagesAllowedByRetrievalFilter();
    for (UINT64 i = 0; i < count; ++i) {
        SIZE_T length = 0; info->GetMessage(i, nullptr, &length);
        std::vector<unsigned char> storage(length);
        auto* message = reinterpret_cast<D3D12_MESSAGE*>(storage.data());
        if (SUCCEEDED(info->GetMessage(i, message, &length)) && message->Severity <= D3D12_MESSAGE_SEVERITY_ERROR) {
            std::fprintf(stderr, "D3D12 validation: %s\n", message->pDescription); ok = false;
        }
    }
    std::fprintf(stderr, "D3D12 debug validation %s\n", ok ? "passed" : "failed");
    return ok;
}
static int rdr2_generation(ID3D12Device* device, ID3D12CommandQueue* queue,
    ID3D12CommandAllocator* allocator, ID3D12GraphicsCommandList* list, rsf_game_host_services& host,
    rsf_game_render_pass pass)
{
    WNDCLASSW type{}; type.lpfnWndProc = DefWindowProcW; type.hInstance = GetModuleHandleW(nullptr);
    type.lpszClassName = L"sgaWindow";
    if (!RegisterClassW(&type)) return 1;
    HWND window = CreateWindowExW(0, type.lpszClassName, L"RDR2 integration fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 640, 360, nullptr, nullptr, type.hInstance, nullptr);
    if (!window) return 1;
    // Create after the RDR2 host installs interception, before its first SR registration.
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGISwapChain1> created;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return 1;
    DXGI_SWAP_CHAIN_DESC1 desc{};
    desc.Width = 640; desc.Height = 360; desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 3; desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    if (FAILED(factory->CreateSwapChainForHwnd(queue, window, &desc, nullptr, nullptr, &created))) return 1;
    ComPtr<IDXGISwapChain3> chain;
    if (FAILED(created.As(&chain))) return 1;
    rsf_native_fg_status initial{}; initial.struct_size = sizeof(initial);
    if (!rsf_fg12_status(&initial) || !initial.available) return 1;
    D3D12_DESCRIPTOR_HEAP_DESC heap_desc{}; heap_desc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV; heap_desc.NumDescriptors = 1;
    ComPtr<ID3D12DescriptorHeap> rtv;
    ComPtr<ID3D12Fence> completion;
    if (FAILED(device->CreateDescriptorHeap(&heap_desc, IID_PPV_ARGS(&rtv))) ||
        FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&completion)))) return 1;
    auto latency = [&](uint32_t kind, uint64_t frame) {
        LARGE_INTEGER ticks{}, frequency{}; QueryPerformanceCounter(&ticks); QueryPerformanceFrequency(&frequency);
        rsf_game_latency_event event{sizeof(event), kind, pass.session_id, frame,
            static_cast<uint64_t>(ticks.QuadPart), static_cast<uint64_t>(frequency.QuadPart)};
        return host.process_latency && host.process_latency(host.user, &event) == 0;
    };
    pass.screen = RSF_SCREEN_FLIGHT; pass.flags = RSF_GAME_RENDER_PRIMARY;
    // Loading may run more CPU inputs than the bookkeeping ring without producing an image.
    // One later ordered Present must retire every older SDK token, including overwritten CPU entries.
    for (uint64_t frame = 1; frame <= 140; ++frame)
        for (uint32_t marker : {RSF_GAME_LATENCY_SLEEP, RSF_LATENCY_INPUT_SAMPLE, RSF_LATENCY_SIMULATION_START,
            RSF_LATENCY_SIMULATION_END}) if (!latency(marker, frame)) return 1;
    if (FAILED(chain->Present(0, 0)) || !latency(RSF_LATENCY_PRESENT_END, 140)) return 1;
    for (uint64_t ordinal = 0; ordinal < 40; ++ordinal) {
        const uint64_t frame = ordinal + 141;
        if (ordinal && (FAILED(allocator->Reset()) || FAILED(list->Reset(allocator, nullptr)))) return 1;
        pass.native_frame = pass.source_frame_id = frame;
        for (uint32_t marker : {RSF_GAME_LATENCY_SLEEP, RSF_LATENCY_INPUT_SAMPLE, RSF_LATENCY_SIMULATION_START,
            RSF_LATENCY_SIMULATION_END, RSF_LATENCY_RENDER_SUBMIT_START}) if (!latency(marker, frame)) return 1;
        if (host.process_upscale(host.user, list, &pass) != 0) return 1;
        ComPtr<ID3D12Resource> target;
        if (FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&target)))) return 1;
        device->CreateRenderTargetView(target.Get(), nullptr, rtv->GetCPUDescriptorHandleForHeapStart());
        barrier(list, target.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
        const float color[] = {0.25f, 0.5f, 0.75f, 1};
        list->ClearRenderTargetView(rtv->GetCPUDescriptorHandleForHeapStart(), color, 0, nullptr);
        auto scene = pass; scene.role = RSF_GAME_RENDER_FINAL_SCENE; scene.pass_key = 1;
        scene.color_input = scene.scene_surface = target.Get(); scene.swapchain = chain.Get();
        host.render_pass(host.user, list, &scene, 0);
        barrier(list, target.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
        if (FAILED(list->Close())) return 1;
        ID3D12CommandList* lists[] = {list}; void* pointers[] = {list};
        host.submit(host.user, queue, frame, 1, pointers, 1); queue->ExecuteCommandLists(1, lists);
        host.submit(host.user, queue, frame, 1, pointers, 0);
        if (!latency(RSF_LATENCY_PRESENT_START, frame)) return 1;
        scene.role = RSF_GAME_RENDER_WINDOW;
        host.render_pass(host.user, nullptr, &scene, 1);
        if (std::getenv("RSF_RDR2_TEST_OVERLAY") && (ordinal == 4 || ordinal == 20))
            rsf_rdr2_overlay_toggle();
        if (std::getenv("RSF_RDR2_TEST_OVERLAY") && ordinal>=4 && ordinal<20) {
            if (!rsf_rdr2_overlay_visible()) return 1;
            int count=0;unsigned attempts=0;
            do {count=ShowCursor(FALSE);} while(count>=0 && ++attempts<512);
            if (count>=0) {std::fprintf(stderr,"Overlay cursor hide loop failed to terminate.\n");return 1;}
        }
        host.present(host.user, chain.Get(), 0, 0, 1);
        const auto presented = chain->Present(0, 0);
        host.present(host.user, chain.Get(), 0, 0, 0);
        if (FAILED(presented) || !latency(RSF_LATENCY_PRESENT_END, frame) || !latency(RSF_LATENCY_RENDER_SUBMIT_END, frame)) return 1;
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!event || FAILED(queue->Signal(completion.Get(), frame)) ||
            FAILED(completion->SetEventOnCompletion(frame, event)) || WaitForSingleObject(event, 30000) != WAIT_OBJECT_0) return 1;
        CloseHandle(event);
    }
    rsf_native_fg_status status{}; status.struct_size = sizeof(status);
    if (!rsf_fg12_status(&status)) return 1;
    std::fprintf(stderr, "RDR2 cold FG: available=%u tagged=%llu mode=%u reason=%u result=%d.\n",
        status.available, static_cast<unsigned long long>(status.tagged_frames),
        status.vendor.configured_mode, status.reason, status.last_result);
    // The host is process-owned, as in RDR2. Keep its presentation interface alive to exit.
    (void)chain.Detach(); (void)created.Detach();
    return validation(device) && status.available && status.tagged_frames && status.last_result == 0 &&
        status.vendor.configured_mode != RSF_FG_OFF ? 0 : 1;
}
int main(int argc, char** argv)
{
    static_assert(sizeof(rsf_camera_frame) == 384 && sizeof(rsf_unity_packet) == 520 && sizeof(rsf_unity_native_api) == 64);
    rsf_sr12* invalid = reinterpret_cast<rsf_sr12*>(1);
    if (rsf_sr12_create(nullptr, &invalid) != RSF_BACKEND_ERROR_INVALID_ARGUMENT || invalid ||
        rsf_sr12_evaluate(nullptr, nullptr, nullptr, 0) != RSF_BACKEND_ERROR_INVALID_ARGUMENT) return 1;
    if (argc == 1) { std::puts("PASS: native ABI and null/short setup refusal."); return 0; }
    if (argc != 5 && argc != 6 && argc != 7) { std::fprintf(stderr, "Usage: test backend streamline-directory fidelityfx-directory xess-directory [vendor-id] [shared]\n"); return 2; }
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> selected;
    const bool debug_requested = std::getenv("RSF_UNITY_GPU_DEBUG") != nullptr;
    if (debug_requested) {
        ComPtr<ID3D12Debug> debug;
        if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
        else { std::fprintf(stderr, "D3D12 debug interface unavailable.\n"); return 77; }
    }
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return 77;
    if (argc >= 6) {
        const auto wanted = static_cast<uint32_t>(std::strtoul(argv[5], nullptr, 0));
        for (UINT index = 0;; ++index) {
            ComPtr<IDXGIAdapter1> candidate;
            if (factory->EnumAdapters1(index, &candidate) != S_OK) break;
            DXGI_ADAPTER_DESC1 description{}; candidate->GetDesc1(&description);
            if (description.VendorId == wanted && !(description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) { selected = candidate; break; }
        }
        if (!selected) return 77;
    }
    ComPtr<ID3D12Device> device;
    if (FAILED(D3D12CreateDevice(selected.Get(), D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&device)))) return 77;
    ComPtr<IDXGIAdapter1> adapter;
    if (SUCCEEDED(factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter)))) {
        DXGI_ADAPTER_DESC1 description{}; adapter->GetDesc1(&description);
        char name[256]{}; WideCharToMultiByte(CP_UTF8, 0, description.Description, -1, name, 256, nullptr, nullptr);
        std::fprintf(stderr, "D3D12 fixture adapter %s vendor=0x%x device=0x%x\n", name, description.VendorId, description.DeviceId);
    }
    D3D12_COMMAND_QUEUE_DESC queue_desc{};
    ComPtr<ID3D12CommandQueue> queue; ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Fence> fence;
    if (FAILED(device->CreateCommandQueue(&queue_desc, IID_PPV_ARGS(&queue))) ||
        FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator))) ||
        FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator.Get(), nullptr, IID_PPV_ARGS(&list))) ||
        FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return 1;
    rsf_sr12_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = 1; setup.device = device.Get();
    setup.backend = static_cast<uint32_t>(std::strtoul(argv[1], nullptr, 10)); setup.quality = 1;
    setup.output_width = 640; setup.output_height = 360; setup.inverted_depth = 1;
    setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = argv[3]; setup.xess_directory_utf8 = argv[4];
    std::string interposer = std::string(argv[2]) + "\\sl.interposer.dll";
    setup.dlss.struct_size = sizeof(setup.dlss); setup.dlss.abi_version = RSF_DLSS_ABI_VERSION;
    setup.dlss.interposer_path_utf8 = interposer.c_str(); setup.dlss.plugin_directory_utf8 = argv[2];
    setup.dlss.engine = RSF_DLSS_ENGINE_UNITY; setup.dlss.engine_version_utf8 = RSF_UNITY_ENGINE_VERSION;
    setup.dlss.project_id_utf8 = RSF_UNITY_PROJECT_ID; setup.dlss.require_signature = 1;
    if (const auto* preset = std::getenv("RSF_DLSS_TEST_PRESET")) {
        char* end = nullptr;
        const auto value = std::strtoul(preset, &end, 10);
        if (!end || *end || value > UINT32_MAX || !rsf_dlss_preset_valid(static_cast<uint32_t>(value))) return 2;
        setup.dlss.preset = static_cast<uint32_t>(value);
        std::fprintf(stderr, "Requested DLSS model preset %u\n", setup.dlss.preset);
    }
    setup.log = setup.dlss.log = logger;
    if (std::getenv("RSF_SR_ENGINE_EXTENTS")) {
        setup.abi_version = 2; setup.engine_input_extents = 1;
        setup.render_width = 426; setup.render_height = 240;
    }
    rsf_streamline_host* shared = nullptr;
    if (argc == 7) {
        rsf_streamline_host_setup registration{sizeof(registration), RSF_STREAMLINE_HOST_ABI_VERSION,
            argv[2], adapter.Get(), RSF_DLSS_ENGINE_UNITY, RSF_UNITY_ENGINE_VERSION, setup.dlss.project_id_utf8, 1, 0, logger, nullptr, RSF_SL_PROFILE_DLSS_FG, 1};
        if (rsf_streamline_host_adopt(&registration, device.Get(), queue.Get(), &shared) != RSF_BACKEND_OK ||
            rsf_streamline_host_acquire(shared, 1) != RSF_BACKEND_OK ||
            rsf_streamline_host_sleep(shared, 1) != RSF_BACKEND_OK) return 1;
        for (auto marker : {RSF_LATENCY_SIMULATION_START, RSF_LATENCY_SIMULATION_END, RSF_LATENCY_RENDER_SUBMIT_START})
            if (rsf_streamline_host_marker(shared, 1, marker, 0) != RSF_BACKEND_OK) return 1;
        setup.streamline_host = shared;
    }
    rsf_sr12* sr = nullptr;
    rsf_game_host_services rdr2{};
    const char* host_ini = std::getenv("RSF_RDR2_HOST_INI");
    const char* host_root = std::getenv("RSF_RDR2_HOST_ROOT");
    int32_t created{};
    if (host_ini && host_root) {
        wchar_t ini[32768]{}, root[32768]{};
        if (!MultiByteToWideChar(CP_UTF8, 0, host_ini, -1, ini, 32768) ||
            !MultiByteToWideChar(CP_UTF8, 0, host_root, -1, root, 32768)) return 2;
        rdr2.struct_size = sizeof(rdr2); rdr2.abi_version = RSF_GAME_ABI_VERSION;
        rdr2.session_id = 7; rdr2.log = logger;
        created = rsf_rdr2_sr_host_create(ini, root, &rdr2);
    } else created = rsf_sr12_create(&setup, &sr);
    if (created != RSF_BACKEND_OK) { std::fprintf(stderr, "Provider %u preparation refused %d\n", setup.backend, created); return 77; }
    uint32_t width = 426, height = 240;
    if (sr) rsf_sr12_plan(sr, &width, &height);
    if (setup.abi_version == 2 && (width != setup.render_width || height != setup.render_height)) return 1;
    std::array<ComPtr<ID3D12Resource>, 4> images;
    if (!texture(device.Get(), width, height, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST, images[0]) ||
        !texture(device.Get(), width, height, DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST, images[1]) ||
        !texture(device.Get(), width, height, DXGI_FORMAT_R16G16_FLOAT, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST, images[2]) ||
        !texture(device.Get(), 640, 360, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_DEST, images[3])) return 1;
    std::array<ComPtr<ID3D12Resource>, 3> uploads;
    for (size_t i = 0; i < 3; ++i) {
        auto desc = images[i]->GetDesc(); D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{}; uint64_t total = 0;
        device->GetCopyableFootprints(&desc, 0, 1, 0, &footprint, nullptr, nullptr, &total);
        if (!buffer(device.Get(), total, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ, uploads[i])) return 1;
        void* mapped = nullptr; if (FAILED(uploads[i]->Map(0, nullptr, &mapped))) return 1;
        std::memset(mapped, 0, static_cast<size_t>(total));
        for (uint32_t y = 0; y < height; ++y) for (uint32_t x = 0; x < width; ++x) {
            auto* pixel = static_cast<unsigned char*>(mapped) + footprint.Footprint.RowPitch*y;
            if (i == 0) { auto* values = reinterpret_cast<uint16_t*>(pixel) + x*4;
                for (size_t c = 0; c < 4; ++c) values[c] = DirectX::PackedVector::XMConvertFloatToHalf(c == 3 ? 1.0f : 0.25f * static_cast<float>(c+1)); }
            if (i == 1) reinterpret_cast<float*>(pixel)[x] = 0.5f;
        }
        uploads[i]->Unmap(0, nullptr);
        D3D12_TEXTURE_COPY_LOCATION src{}; src.pResource = uploads[i].Get(); src.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; src.PlacedFootprint = footprint;
        D3D12_TEXTURE_COPY_LOCATION dst{}; dst.pResource = images[i].Get(); dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        barrier(list.Get(), images[i].Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    ComPtr<ID3D12Resource> exposure, exposure_upload;
    if (std::getenv("RSF_SR_EXTERNAL_EXPOSURE")) {
        if (!texture(device.Get(), 1, 1, DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_FLAG_NONE,
            D3D12_RESOURCE_STATE_COPY_DEST, exposure) || !buffer(device.Get(), 256, D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ, exposure_upload)) return 1;
        void* value{};
        if (FAILED(exposure_upload->Map(0, nullptr, &value))) return 1;
        *static_cast<float*>(value) = 1;
        exposure_upload->Unmap(0, nullptr);
        D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = exposure_upload.Get();
        from.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; from.PlacedFootprint.Footprint = {DXGI_FORMAT_R32_FLOAT, 1, 1, 1, 256};
        D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = exposure.Get(); to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
        barrier(list.Get(), exposure.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        std::fprintf(stderr, "Fixture supplies external 1x1 R32_FLOAT exposure=1.\n");
    }
    rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR; pass.camera_valid = 1;
    pass.session_id = 7; pass.view_key = pass.history_key = 11; pass.native_frame = 1; pass.resource_generation = 1;
    pass.color_input = images[0].Get(); pass.depth = images[1].Get(); pass.motion = images[2].Get(); pass.color_output = images[3].Get();
    pass.exposure = exposure.Get();
    pass.motion_to_uv[0] = pass.motion_to_uv[1] = 1;
    pass.camera.struct_size = sizeof(pass.camera); pass.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    pass.camera.render_width = width; pass.camera.render_height = height; pass.camera.output_width = 640; pass.camera.output_height = 360;
    pass.camera.near_plane = 0.1f; pass.camera.far_plane = 1000; pass.camera.vertical_fov_radians = 1.04719755f;
    pass.camera.depth_inverted = 1; pass.camera.frame_time_seconds = 1.0f/60;
    DirectX::XMFLOAT4X4 identity{}, projection{}, inverse{};
    DirectX::XMStoreFloat4x4(&identity, DirectX::XMMatrixIdentity());
    auto proj = DirectX::XMMatrixPerspectiveFovRH(pass.camera.vertical_fov_radians, 640.0f/360.0f, 1000, 0.1f);
    DirectX::XMStoreFloat4x4(&projection, proj); DirectX::XMStoreFloat4x4(&inverse, DirectX::XMMatrixInverse(nullptr, proj));
    std::memcpy(pass.camera.view_to_clip, &projection, 64); std::memcpy(pass.camera.clip_to_view, &inverse, 64);
    std::memcpy(pass.camera.view_to_world, &identity, 64); std::memcpy(pass.camera.world_to_view, &identity, 64);
    std::memcpy(pass.camera.clip_to_previous_clip, &identity, 64); std::memcpy(pass.previous_clip_to_clip, &identity, 64);
    if (!setup.backend) {
        barrier(list.Get(), images[0].Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->CopyResource(images[3].Get(), images[0].Get());
        barrier(list.Get(), images[0].Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        pass.color_output = nullptr; pass.role = RSF_GAME_RENDER_FG_INPUTS;
    }
    if (rdr2.process_latency && std::getenv("RSF_RDR2_COLD_FG"))
        return rdr2_generation(device.Get(), queue.Get(), allocator.Get(), list.Get(), rdr2, pass);
    auto evaluated = rdr2.process_upscale ? rdr2.process_upscale(rdr2.user, list.Get(), &pass) :
        rsf_sr12_evaluate(sr, list.Get(), &pass, 0);
    std::fprintf(stderr, "Provider %u: %ux%u -> 640x360, evaluate=%d\n", setup.backend, width, height, evaluated);
    if (rdr2.render_pass && evaluated == 0) {
        barrier(list.Get(), images[3].Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_RENDER_TARGET);
        auto scene = pass;
        scene.role = RSF_GAME_RENDER_FINAL_SCENE; scene.color_input = scene.scene_surface = images[3].Get(); scene.pass_key = 1;
        rdr2.render_pass(rdr2.user, list.Get(), &scene, 0);
        barrier(list.Get(), images[3].Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_DEST);
    }
    if (rdr2.process_upscale && evaluated == 0) {
        for (uint64_t frame = 2; frame <= 3; ++frame) {
            pass.native_frame = frame;
            if (rdr2.process_upscale(rdr2.user, list.Get(), &pass) != 0) return 1;
        }
        pass.native_frame = 4;
        if (rdr2.process_upscale(rdr2.user, list.Get(), &pass) != RSF_ERROR_BUSY) return 1;
        std::fprintf(stderr, "PASS: three unsubmitted recordings retained; fourth refused without a producer wait.\n");
    }
#if RSF_HAVE_STREAMLINE
    if (shared) {
        // FG has separate completed-frame constants on viewport zero. Setting them
        // after SR on the same token must succeed even when their values differ.
        auto set_constants = reinterpret_cast<PFun_slSetConstants*>(reinterpret_cast<void*>(
            GetProcAddress(static_cast<HMODULE>(rsf_streamline_host_module(shared)), "slSetConstants")));
        auto* token = static_cast<sl::FrameToken*>(rsf_streamline_host_token(shared, 1));
        sl::Constants constants{};
        std::memcpy(&constants.cameraViewToClip, pass.camera.view_to_clip, 64);
        std::memcpy(&constants.clipToCameraView, pass.camera.clip_to_view, 64);
        std::memcpy(&constants.clipToPrevClip, pass.camera.clip_to_previous_clip, 64);
        std::memcpy(&constants.prevClipToClip, pass.previous_clip_to_clip, 64);
        constants.cameraNear = 0.2f; constants.cameraFar = 1000;
        constants.cameraFOV = pass.camera.vertical_fov_radians; constants.cameraAspectRatio = 640.0f / 360.0f;
        constants.depthInverted = sl::eTrue; constants.reset = sl::eTrue;
        if (!set_constants || !token || set_constants(constants, *token, sl::ViewportHandle(0u)) != sl::Result::eOk)
            return 1;
        std::fprintf(stderr, "PASS: independent SR/FG constants accepted on the same source token.\n");
    }
#endif
    auto desc = images[3]->GetDesc(); D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{}; uint64_t bytes = 0;
    device->GetCopyableFootprints(&desc, 0, 1, 0, &footprint, nullptr, nullptr, &bytes);
    ComPtr<ID3D12Resource> readback; if (!buffer(device.Get(), bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST, readback)) return 1;
    barrier(list.Get(), images[3].Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION src{}; src.pResource = images[3].Get(); src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION dst{}; dst.pResource = readback.Get(); dst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; dst.PlacedFootprint = footprint;
    list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
    if (FAILED(list->Close())) return 1;
    ID3D12CommandList* submitted[] = {list.Get()};
    void* submitted_pointers[] = {list.Get()};
    if (rdr2.submit) rdr2.submit(rdr2.user, queue.Get(), 1, 1, submitted_pointers, 1);
    queue->ExecuteCommandLists(1, submitted);
    if (rdr2.submit) rdr2.submit(rdr2.user, queue.Get(), 1, 1, submitted_pointers, 0);
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event || FAILED(queue->Signal(fence.Get(), 1)) || FAILED(fence->SetEventOnCompletion(1, event)) || WaitForSingleObject(event, 30000) != WAIT_OBJECT_0) return 1;
    CloseHandle(event);
    if (rdr2.process_upscale && evaluated == 0) {
        if (FAILED(allocator->Reset()) || FAILED(list->Reset(allocator.Get(), nullptr))) return 1;
        barrier(list.Get(), images[3].Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
        pass.native_frame = 5;
        if (rdr2.process_upscale(rdr2.user, list.Get(), &pass) != 0) return 1;
        barrier(list.Get(), images[3].Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        if (FAILED(list->Close())) return 1;
        rdr2.submit(rdr2.user, queue.Get(), 2, 1, submitted_pointers, 1);
        queue->ExecuteCommandLists(1, submitted);
        rdr2.submit(rdr2.user, queue.Get(), 2, 1, submitted_pointers, 0);
        event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!event || FAILED(queue->Signal(fence.Get(), 2)) || FAILED(fence->SetEventOnCompletion(2, event)) ||
            WaitForSingleObject(event, 30000) != WAIT_OBJECT_0) return 1;
        CloseHandle(event);
        std::fprintf(stderr, "PASS: submitted recording slots retired and reused on a reset command list.\n");
    }
    if (rdr2.present && evaluated == 0) {
        WNDCLASSW window_class{}; window_class.lpfnWndProc = DefWindowProcW;
        window_class.hInstance = GetModuleHandleW(nullptr); window_class.lpszClassName = L"RSFRDR2SRHostFixture";
        if (!RegisterClassW(&window_class)) return 1;
        const auto window = CreateWindowExW(0, window_class.lpszClassName, L"RSF fixture", WS_OVERLAPPEDWINDOW,
            0, 0, 640, 360, nullptr, nullptr, window_class.hInstance, nullptr);
        if (!window) return 1;
        DXGI_SWAP_CHAIN_DESC1 swap_desc{};
        swap_desc.Width = 640; swap_desc.Height = 360; swap_desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        swap_desc.SampleDesc.Count = 1; swap_desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
        swap_desc.BufferCount = 2; swap_desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
        ComPtr<IDXGISwapChain1> native_swapchain;
        if (FAILED(factory->CreateSwapChainForHwnd(queue.Get(), window, &swap_desc, nullptr, nullptr, &native_swapchain))) return 1;
        rdr2.present(rdr2.user, native_swapchain.Get(), 0, 0, 1);
        const auto presented = native_swapchain->Present(0, 0);
        rdr2.present(rdr2.user, native_swapchain.Get(), 0, 0, 0);
        if (FAILED(presented)) return 1;
        event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!event || FAILED(queue->Signal(fence.Get(), 3)) || FAILED(fence->SetEventOnCompletion(3, event)) ||
            WaitForSingleObject(event, 30000) != WAIT_OBJECT_0) return 1;
        CloseHandle(event);
        native_swapchain.Reset(); DestroyWindow(window); UnregisterClassW(window_class.lpszClassName, window_class.hInstance);
        std::fprintf(stderr, "PASS: host bookkeeping surrounds an actual DXGI Present.\n");
    }
    if (rdr2.process_upscale && evaluated == 0 && setup.backend == RSF_SR_DLSS && std::getenv("RSF_RDR2_HOST_SWITCHES")) {
        const auto registration = GetModuleHandleW(L"sl.interposer.dll");
        if (!registration) return 1;
        wchar_t configuration[32768]{};
        if (!MultiByteToWideChar(CP_UTF8, 0, host_ini, -1, configuration, 32768)) return 1;
        uint64_t completion = 3;
        for (uint32_t backend : {RSF_SR_FSR3, RSF_SR_DLSS}) {
            const auto requested = std::to_wstring(backend);
            if (!WritePrivateProfileStringW(L"RDR2", L"SRBackend", requested.c_str(), configuration)) return 1;
            Sleep(1100); // Exercise the public live INI poll, not a fixture-only host shortcut.
            if (FAILED(allocator->Reset()) || FAILED(list->Reset(allocator.Get(), nullptr))) return 1;
            barrier(list.Get(), images[3].Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
            ++pass.native_frame;
            if (rdr2.process_upscale(rdr2.user, list.Get(), &pass) != 0 ||
                GetModuleHandleW(L"sl.interposer.dll") != registration) return 1;
            barrier(list.Get(), images[3].Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
            list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
            if (FAILED(list->Close())) return 1;
            ++completion;
            rdr2.submit(rdr2.user, queue.Get(), completion, 1, submitted_pointers, 1);
            queue->ExecuteCommandLists(1, submitted);
            rdr2.submit(rdr2.user, queue.Get(), completion, 1, submitted_pointers, 0);
            event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
            if (!event || FAILED(queue->Signal(fence.Get(), completion)) ||
                FAILED(fence->SetEventOnCompletion(completion, event)) || WaitForSingleObject(event, 30000) != WAIT_OBJECT_0) return 1;
            CloseHandle(event);
            std::fprintf(stderr, "PASS: backend %u accepted after a live switch; the same NGX registration remains loaded.\n", backend);
        }
    }
    void* pixels = nullptr; if (FAILED(readback->Map(0, nullptr, &pixels))) return 1;
    auto* center = reinterpret_cast<uint16_t*>(static_cast<unsigned char*>(pixels) + footprint.Footprint.RowPitch*180) + 320*4;
    const float r = DirectX::PackedVector::XMConvertHalfToFloat(center[0]);
    const float g = DirectX::PackedVector::XMConvertHalfToFloat(center[1]);
    const float b = DirectX::PackedVector::XMConvertHalfToFloat(center[2]);
    std::fprintf(stderr, "GPU output center RGB %.4f %.4f %.4f\n", r, g, b);
    readback->Unmap(0, nullptr); rsf_sr12_destroy(sr);
    if (shared) {
        for (auto marker : {RSF_LATENCY_RENDER_SUBMIT_END, RSF_LATENCY_PRESENT_START, RSF_LATENCY_PRESENT_END})
            if (rsf_streamline_host_marker(shared, 1, marker, 0) != RSF_BACKEND_OK) return 1;
        rsf_streamline_host_destroy(shared);
    }
    const bool validation_ok = validation(device.Get());
    return validation_ok && evaluated == RSF_BACKEND_OK && std::isfinite(r) && std::isfinite(g) && std::isfinite(b) && r > 0.1f && g > 0.2f && b > 0.3f ? 0 : 1;
}
