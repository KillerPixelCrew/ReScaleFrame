// SPDX-License-Identifier: MIT
// Explicit device regression: <scratch runtime root with streamline/>. Hidden window,
// real shared DLSS evaluation, FG Off. Never uses or changes a game installation.
#include <rescaleframe/d3d11_host.h>
#include "d3d11_host_device_fixture.h"
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/d3d11_present_api.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/overlay_api.h>
#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <dxgi1_6.h>
#include <DirectXPackedVector.h>
#include <wrl/client.h>
#include <cstdio>
#include <filesystem>
#include <string>

using Microsoft::WRL::ComPtr;
namespace {
HWND window{};
struct TemporaryIni { std::wstring path; ~TemporaryIni() { DeleteFileW(path.c_str()); } };
constexpr UINT width = 960, height = 540;
void log(void*, const char* message) { std::printf("%s\n", message); }
int accept(void*, void* candidate) { return candidate == window; }
ComPtr<ID3D11Texture2D> texture(ID3D11Device* device, DXGI_FORMAT format, UINT bindings) {
    ComPtr<ID3D11Texture2D> result;
    result.Attach(static_cast<ID3D11Texture2D*>(
        rsf_d3d11_host_fixture_texture(device, width, height, uint32_t(format), bindings)));
    return result;
}

}
int main(int argc, char** argv) {
    const auto failed = [](int line) {
        std::fprintf(stderr,"FAIL shared-SR fixture startup line=%d\n",line);
        return 1;
    };
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
    if (!RegisterClassW(&wc)) return failed(__LINE__);
    window = CreateWindowW(wc.lpszClassName, L"RSF shared SR device check", WS_OVERLAPPED,
        0, 0, int(width), int(height), nullptr, nullptr, module, nullptr);
    if (!window) return failed(__LINE__); // Deliberately never shown or made foreground.
    wchar_t temporary[MAX_PATH]{}, ini[MAX_PATH]{};
    if (!GetTempPathW(MAX_PATH, temporary) || !GetTempFileNameW(temporary, L"rsf", 0, ini)) return failed(__LINE__);
    const TemporaryIni remove_ini{ini};
    if (!WritePrivateProfileSectionW(L"Device", L"SR=1\0SRBackend=1\0SRQuality=1\0FG=0\0Latency=0\0StatusOverlay=0\0", ini)) return failed(__LINE__);
    const auto directory = root.wstring();
    rsf_d3d11_host_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    setup.configuration_path = ini; setup.configuration_section = L"Device"; setup.runtime_directory = directory.c_str();
    setup.identity_utf8 = "shared-sr-device"; setup.executable_sha256 = "synthetic";
    setup.engine_version_utf8 = "1"; setup.project_id_utf8 = "668392a0-ecfe-4d0c-9f58-c2838c5ca308";
    setup.units_to_meters = 1; setup.quality_choices = 0x1f; setup.depth_inverted = 1;
    setup.depth_far_plane = RSF_D3D11_HOST_DEPTH_INFINITE;
    setup.motion.struct_size = sizeof(setup.motion); setup.motion.scale_x = setup.motion.scale_y = 1;
    setup.motion.output_scale_x = setup.motion.output_scale_y = 1; setup.motion.invalid_value = -1000;
    setup.presentation_bridge = 1; setup.accept_window = accept;
    rsf_game_host_services host{}; host.struct_size = sizeof(host); host.abi_version = RSF_GAME_ABI_VERSION;
    host.session_id = 91; host.log = log;
    if (rsf_d3d11_host_create(&setup, &host) != RSF_OK || rsf_d3d11_host_set_active(host.user, 1) != RSF_OK) return failed(__LINE__);
    rsf_d3d11_present_api presentation{sizeof(presentation), RSF_D3D11_PRESENT_API_VERSION};
    if (rsf_get_d3d11_present_api(&presentation) != RSF_OK) return failed(__LINE__);
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
    if (FAILED(created)) return failed(__LINE__);
    // Native startup presents before the first scene. The bridge adopts its chain there.
    const auto first_present = chain->Present(0, 0);
    if (FAILED(first_present) || !presentation.is_owner(chain.Get()) || !presentation.host()) {
        std::printf("Shared presentation not established: 0x%08lx owner=%u host=%p\n",
            static_cast<unsigned long>(first_present), presentation.is_owner(chain.Get()),
            static_cast<void*>(presentation.host())); return failed(__LINE__);
    }
    auto color = texture(device.Get(), DXGI_FORMAT_R11G11B10_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto motion = texture(device.Get(), DXGI_FORMAT_R16G16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto depth = texture(device.Get(), DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE);
    auto output = texture(device.Get(), DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    if (!color || !motion || !depth || !output) return failed(__LINE__);
    ComPtr<ID3D11RenderTargetView> color_view, motion_view, output_view;
    D3D11_DEPTH_STENCIL_VIEW_DESC ds{}; ds.Format = DXGI_FORMAT_D24_UNORM_S8_UINT; ds.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    ComPtr<ID3D11DepthStencilView> depth_view;
    if (FAILED(device->CreateRenderTargetView(color.Get(), nullptr, &color_view)) ||
        FAILED(device->CreateRenderTargetView(motion.Get(), nullptr, &motion_view)) ||
        FAILED(device->CreateRenderTargetView(output.Get(), nullptr, &output_view)) ||
        FAILED(device->CreateDepthStencilView(depth.Get(), &ds, &depth_view))) return failed(__LINE__);
    ComPtr<ID3D11InfoQueue> diagnostics; (void)device.As(&diagnostics);
    std::printf("D3D11 debug queue available=%u\n", diagnostics ? 1u : 0u);
    if (diagnostics) diagnostics->ClearStoredMessages();
    const auto capture_root = root / "captures" / "website";
    rsf_overlay_api overlay{sizeof(overlay),RSF_OVERLAY_API_VERSION};
    if (rsf_get_overlay_api(&overlay) != RSF_OK) return failed(__LINE__);
    struct NativeCalls { const rsf_overlay_api* overlay; const rsf_d3d11_present_api* presentation; };
    NativeCalls native_calls{&overlay, &presentation};
    const rsf_d3d11_fixture_sr_calls calls{
        &native_calls,
        [](void* user, uint64_t source) noexcept -> int32_t {
            try {
                const auto& api = *static_cast<const NativeCalls*>(user)->presentation;
                return rsf_streamline_host_token(api.host(), source) != nullptr;
            } catch (...) { return -1; }
        },
        [](void* user, uint32_t quality) noexcept -> int32_t {
            try {
                const auto& api = *static_cast<const NativeCalls*>(user)->overlay;
                rsf_render_settings settings{sizeof(settings),RSF_RENDER_FRONTEND_ABI_VERSION};
                const auto result = api.settings_get(&settings);
                if (result != RSF_OK) return result;
                settings.sr_quality = quality;
                return api.settings_apply(&settings);
            } catch (...) { return RSF_ERROR_NOT_READY; }
        },
        [](void* user, int32_t* result, uint32_t* backend) noexcept -> int32_t {
            try {
                const auto& api = *static_cast<const NativeCalls*>(user)->overlay;
                rsf_render_status published{sizeof(published),RSF_RENDER_FRONTEND_ABI_VERSION};
                const auto status = api.status(&published);
                *result = published.sr_result;
                *backend = published.sr_effective_backend;
                return status;
            } catch (...) { return RSF_ERROR_NOT_READY; }
        },
        [](uint64_t session) noexcept -> int32_t {
            try { rsf_input_dump_cancel(session); return RSF_OK; }
            catch (...) { return RSF_ERROR_NOT_READY; }
        },
        [](void* receiver) noexcept -> int32_t {
            try { return static_cast<IDXGISwapChain*>(receiver)->Present(0, 0); }
            catch (...) { return E_FAIL; }
        },
        [](void* user) noexcept -> int32_t {
            try {
                const auto& api = *static_cast<const NativeCalls*>(user)->overlay;
                rsf_render_settings changed{sizeof(changed), RSF_RENDER_FRONTEND_ABI_VERSION};
                const auto read = api.settings_get(&changed);
                if (read != RSF_OK) return read;
                changed.sr_enabled = 0;
                return api.settings_apply(&changed);
            } catch (...) { return RSF_ERROR_NOT_READY; }
        },
        [](uint16_t channel) noexcept { return DirectX::PackedVector::XMConvertHalfToFloat(channel); },
    };
    const rsf_d3d11_fixture_resources resources{
        &host, device.Get(), context.Get(), chain.Get(), color.Get(), motion.Get(), depth.Get(), output.Get(),
        color_view.Get(), motion_view.Get(), depth_view.Get(), output_view.Get(), width, height,
        diagnostics.Get(), []() noexcept { DestroyWindow(window); },
    };
    return rsf_d3d11_host_fixture_shared_sr(&resources, &calls, capture_root.c_str());
}
