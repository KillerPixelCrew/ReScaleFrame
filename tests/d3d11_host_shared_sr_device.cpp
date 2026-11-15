// SPDX-License-Identifier: GPL-3.0-only
// Explicit device regression: <scratch runtime root with streamline/>. Hidden window,
// real shared DLSS evaluation, FG Off. Never uses or changes a game installation.
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/d3d11_present_api.h>
#include <rescaleframe/native_fg.h>
#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <dxgi1_6.h>
#include <DirectXPackedVector.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <vector>

using Microsoft::WRL::ComPtr;
namespace {
HWND window{};
struct TemporaryIni { std::wstring path; ~TemporaryIni() { DeleteFileW(path.c_str()); } };
constexpr UINT width = 960, height = 540;
void log(void*, const char* message) { std::printf("%s\n", message); }
int accept(void*, void* candidate) { return candidate == window; }
ComPtr<ID3D11Texture2D> texture(ID3D11Device* device, DXGI_FORMAT format, UINT bindings) {
    D3D11_TEXTURE2D_DESC desc{width, height, 1, 1, format, {1, 0}, D3D11_USAGE_DEFAULT, bindings, 0, 0};
    ComPtr<ID3D11Texture2D> result;
    (void)device->CreateTexture2D(&desc, nullptr, &result);
    return result;
}
void identity(float* matrix) { matrix[0] = matrix[5] = matrix[10] = matrix[15] = 1; }
void cpu(const rsf_game_host_services& host, uint64_t source, uint32_t stage) {
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event{}; event.struct_size = sizeof(event); event.event_version = RSF_GAME_CPU_EVENT_VERSION;
    event.stage = stage; event.session_id = host.session_id; event.source_frame_id = source;
    event.timestamp_qpc = uint64_t(now.QuadPart); event.qpc_frequency = uint64_t(frequency.QuadPart);
    host.cpu_event(host.user, &event);
}
}
int main(int argc, char** argv) {
    if (argc != 2) { std::puts("Usage: rsf_d3d11_host_shared_sr_device <scratch runtime root>"); return 77; }
    const std::filesystem::path root(argv[1]);
    if (!std::filesystem::exists(root / "streamline/sl.interposer.dll")) return 77;
    ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return 77;
    ComPtr<IDXGIAdapter1> adapter;
    for (UINT i = 0; ; ++i) {
        if (FAILED(factory->EnumAdapters1(i, &adapter))) return 77;
        DXGI_ADAPTER_DESC1 desc{}; adapter->GetDesc1(&desc);
        if (desc.VendorId == 0x10de && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) {
            std::printf("Adapter: %ls\n", desc.Description); break;
        }
        adapter.Reset();
    }
    const auto module = GetModuleHandleW(nullptr);
    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = module; wc.lpszClassName = L"RSFSharedSRDevice";
    if (!RegisterClassW(&wc)) return 1;
    window = CreateWindowW(wc.lpszClassName, L"RSF shared SR device check", WS_OVERLAPPED,
        0, 0, int(width), int(height), nullptr, nullptr, module, nullptr);
    if (!window) return 1; // Deliberately never shown or made foreground.
    wchar_t temporary[MAX_PATH]{}, ini[MAX_PATH]{};
    if (!GetTempPathW(MAX_PATH, temporary) || !GetTempFileNameW(temporary, L"rsf", 0, ini)) return 1;
    const TemporaryIni remove_ini{ini};
    if (!WritePrivateProfileSectionW(L"Device", L"SR=1\0SRBackend=1\0SRQuality=1\0FG=0\0Latency=0\0StatusOverlay=0\0", ini)) return 1;
    const auto directory = root.wstring();
    rsf_d3d11_host_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    setup.configuration_path = ini; setup.configuration_section = L"Device"; setup.runtime_directory = directory.c_str();
    setup.identity_utf8 = "shared-sr-device"; setup.executable_sha256 = "synthetic";
    setup.engine_version_utf8 = "1"; setup.project_id_utf8 = "668392a0-ecfe-4d0c-9f58-c2838c5ca308";
    setup.units_to_meters = 1; setup.quality_choices = 0x1f; setup.depth_inverted = 1;
    setup.motion.struct_size = sizeof(setup.motion); setup.motion.scale_x = setup.motion.scale_y = 1;
    setup.motion.output_scale_x = setup.motion.output_scale_y = 1; setup.motion.invalid_value = -1000;
    setup.presentation_bridge = 1; setup.accept_window = accept;
    rsf_game_host_services host{}; host.struct_size = sizeof(host); host.abi_version = RSF_GAME_ABI_VERSION;
    host.session_id = 91; host.log = log;
    if (rsf_d3d11_host_create(&setup, &host) != RSF_OK || rsf_d3d11_host_set_active(host.user, 1) != RSF_OK) return 1;
    rsf_d3d11_present_api presentation{sizeof(presentation), RSF_D3D11_PRESENT_API_VERSION};
    if (rsf_get_d3d11_present_api(&presentation) != RSF_OK) return 1;
    DXGI_SWAP_CHAIN_DESC chain_desc{}; chain_desc.BufferDesc.Width = width; chain_desc.BufferDesc.Height = height;
    chain_desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; chain_desc.SampleDesc.Count = 1;
    chain_desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; chain_desc.BufferCount = 2;
    chain_desc.OutputWindow = window; chain_desc.Windowed = TRUE;
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context; ComPtr<IDXGISwapChain> chain;
    const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_1;
    auto created = D3D11CreateDeviceAndSwapChain(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
        D3D11_CREATE_DEVICE_DEBUG, &level, 1, D3D11_SDK_VERSION, &chain_desc, &chain, &device, nullptr, &context);
    if (created == DXGI_ERROR_SDK_COMPONENT_MISSING)
        created = D3D11CreateDeviceAndSwapChain(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
            0, &level, 1, D3D11_SDK_VERSION, &chain_desc, &chain, &device, nullptr, &context);
    if (FAILED(created)) return 1;
    // Native startup presents before the first scene. The bridge adopts its chain there.
    const auto first_present = chain->Present(0, 0);
    if (FAILED(first_present) || !presentation.is_owner(chain.Get()) || !presentation.host()) {
        std::printf("Shared presentation not established: 0x%08lx owner=%u host=%p\n",
            static_cast<unsigned long>(first_present), presentation.is_owner(chain.Get()),
            static_cast<void*>(presentation.host())); return 1;
    }
    auto color = texture(device.Get(), DXGI_FORMAT_R11G11B10_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto motion = texture(device.Get(), DXGI_FORMAT_R16G16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto depth = texture(device.Get(), DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE);
    auto output = texture(device.Get(), DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    if (!color || !motion || !depth || !output) return 1;
    ComPtr<ID3D11RenderTargetView> color_view, motion_view, output_view;
    D3D11_DEPTH_STENCIL_VIEW_DESC ds{}; ds.Format = DXGI_FORMAT_D24_UNORM_S8_UINT; ds.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    ComPtr<ID3D11DepthStencilView> depth_view;
    if (FAILED(device->CreateRenderTargetView(color.Get(), nullptr, &color_view)) ||
        FAILED(device->CreateRenderTargetView(motion.Get(), nullptr, &motion_view)) ||
        FAILED(device->CreateRenderTargetView(output.Get(), nullptr, &output_view)) ||
        FAILED(device->CreateDepthStencilView(depth.Get(), &ds, &depth_view))) return 1;
    ComPtr<ID3D11InfoQueue> diagnostics; (void)device.As(&diagnostics);
    std::printf("D3D11 debug queue available=%u\n", diagnostics ? 1u : 0u);
    if (diagnostics) diagnostics->ClearStoredMessages();
    for (uint64_t source = 1; source <= 8; ++source) {
        cpu(host, source, RSF_GAME_CPU_FRAME_BEGIN);
        const float scene_color[]{0.25f, 0.5f, 0.75f, 1}, zero[4]{}, unwritten[]{3, 0, 3, 1};
        context->ClearRenderTargetView(color_view.Get(), scene_color); context->ClearRenderTargetView(motion_view.Get(), zero);
        context->ClearRenderTargetView(output_view.Get(), unwritten); context->ClearDepthStencilView(depth_view.Get(), D3D11_CLEAR_DEPTH, 0.5f, 0);
        rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
        pass.session_id = host.session_id; pass.source_frame_id = pass.submission_id = pass.scope_id = source;
        pass.family_key = pass.view_key = pass.history_key = pass.viewport_key = pass.window_key = pass.rhi_viewport_key = 1;
        pass.native_frame = source; pass.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_NO_FG;
        pass.swapchain = chain.Get(); pass.color_input = color.Get(); pass.color_output = output.Get();
        pass.depth = depth.Get(); pass.motion = motion.Get(); pass.camera_valid = pass.motion_camera_included = 1;
        pass.motion_to_uv[0] = pass.motion_to_uv[1] = -1;
        auto& camera = pass.camera; camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        identity(camera.view_to_clip); identity(camera.clip_to_view); identity(camera.world_to_view);
        identity(camera.view_to_world); identity(camera.clip_to_previous_clip); identity(pass.previous_clip_to_clip);
        camera.view_to_clip[0] = float(height) / float(width); camera.clip_to_view[0] = float(width) / float(height);
        camera.near_plane = 0.1f; camera.far_plane = 1000; camera.vertical_fov_radians = 1.2f;
        camera.frame_time_seconds = 1.0f / 60; camera.depth_inverted = 1;
        camera.render_width = 640; camera.render_height = 360; camera.output_width = width; camera.output_height = height;
        camera.jitter_pixels[0] = source % 2 ? 0.25f : -0.25f;
        pass.render_rect[2] = 640; pass.render_rect[3] = 360; pass.output_rect[2] = int32_t(width); pass.output_rect[3] = int32_t(height);
        const auto result = host.process_upscale(host.user, context.Get(), &pass);
        std::printf("Shared SR source=%llu result=%d\n", static_cast<unsigned long long>(source), result);
        if (result != RSF_OK || FAILED(chain->Present(0, 0))) return 1;
        cpu(host, source, RSF_GAME_CPU_FRAME_END);
    }
    rsf_dlss_pipeline_status status{}; status.struct_size = sizeof(status);
    if (rsf_dlss_pipeline_get_status(&status) != RSF_DLSS_PIPELINE_OK || status.frames_evaluated != 8 || status.frames_refused) return 1;
    rsf_native_fg_status generation{}; generation.struct_size = sizeof(generation);
    if (!rsf_native_fg_status_get(&generation) || generation.requested.mode != RSF_FG_OFF ||
        generation.vendor.effective_mode != RSF_FG_OFF || generation.tagged_frames) return 1;
    std::printf("FG requested=%u effective=%u tagged=%llu\n", generation.requested.mode,
        generation.vendor.effective_mode, static_cast<unsigned long long>(generation.tagged_frames));
    D3D11_TEXTURE2D_DESC readback_desc{}; output->GetDesc(&readback_desc);
    readback_desc.BindFlags = 0; readback_desc.Usage = D3D11_USAGE_STAGING; readback_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> readback;
    if (FAILED(device->CreateTexture2D(&readback_desc, nullptr, &readback))) return 1;
    context->CopyResource(readback.Get(), output.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(readback.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return 1;
    const auto* pixel = reinterpret_cast<const uint16_t*>(static_cast<const unsigned char*>(mapped.pData) + mapped.RowPitch * (height / 2)) + (width / 2) * 4;
    bool valid = true; const float expected[]{0.25f, 0.5f, 0.75f};
    for (unsigned i = 0; i < 3; ++i) {
        const auto value = DirectX::PackedVector::XMConvertHalfToFloat(pixel[i]);
        std::printf("Output channel%u=%g\n", i, value); valid &= std::isfinite(value) && std::fabs(value - expected[i]) < 0.15f;
    }
    context->Unmap(readback.Get(), 0);
    if (diagnostics) for (UINT64 i = 0; i < diagnostics->GetNumStoredMessagesAllowedByRetrievalFilter(); ++i) {
        SIZE_T size{}; (void)diagnostics->GetMessage(i, nullptr, &size); std::vector<unsigned char> storage(size);
        auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
        if (SUCCEEDED(diagnostics->GetMessage(i, message, &size)) && message->Severity <= D3D11_MESSAGE_SEVERITY_ERROR) {
            std::printf("D3D11 error: %s\n", message->pDescription); valid = false;
        }
    }
    (void)rsf_d3d11_host_set_active(host.user, 0); DestroyWindow(window);
    std::puts(valid ? "Shared SR device check passed; FG remained Off" : "Shared SR device check FAILED");
    return valid ? 0 : 1;
}
