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
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <vector>
#include <algorithm>
#include <fstream>
#include <iterator>
#include <string>
#include <thread>
#include <cstring>

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
std::vector<unsigned char> contents(ID3D11Device* device, ID3D11DeviceContext* context,
    ID3D11Texture2D* source, UINT stride) {
    D3D11_TEXTURE2D_DESC descriptor{}; source->GetDesc(&descriptor);
    const size_t row_bytes = size_t(descriptor.Width) * stride;
    std::vector<unsigned char> bytes(row_bytes * descriptor.Height);
    if (!rsf_d3d11_host_fixture_read_texture(device, context, source, stride, bytes.data(), bytes.size()))
        return {};
    return bytes;
}
void identity(float* matrix) { matrix[0] = matrix[5] = matrix[10] = matrix[15] = 1; }
void cpu(const rsf_game_host_services& host, uint64_t source, uint32_t stage) {
    rsf_d3d11_host_fixture_cpu(&host, source, stage, 0);
}
}
int main(int argc, char** argv) {
    uint64_t diagnostic_source{};
    const char* diagnostic_phase="startup";
    const auto failed = [&](int line) {
        std::fprintf(stderr,"FAIL shared-SR fixture source=%llu phase=%s line=%d\n",
            static_cast<unsigned long long>(diagnostic_source),diagnostic_phase,line);
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
    const auto bundles = [&] {
        std::vector<std::filesystem::path> found;
        if (std::filesystem::exists(capture_root)) for (const auto& entry : std::filesystem::directory_iterator(capture_root))
            if (entry.is_directory()) found.push_back(entry.path());
        std::sort(found.begin(), found.end());
        return found;
    };
    rsf_overlay_api overlay{sizeof(overlay),RSF_OVERLAY_API_VERSION};
    if (rsf_get_overlay_api(&overlay) != RSF_OK) return failed(__LINE__);
    // Source1 must refuse without its pre-configuration CPU begin/token; Present then
    // completes cold setup. Sources2..10 drive real owned plan/evaluate pairs.
    // Successful providers supply sr_constants.json through the existing assembled-frame dump.
    rsf_game_render_pass last_pass{};
    for (uint64_t source = 1; source <= 10; ++source) {
        diagnostic_source=source; diagnostic_phase="planning";
        cpu(host, source, RSF_GAME_CPU_FRAME_BEGIN);
        rsf_native_cpu_frame cpu_facts{}; cpu_facts.struct_size = sizeof(cpu_facts);
        const bool cpu_owned = rsf_native_cpu_read(host.session_id,source,&cpu_facts) != 0;
        const bool token_owned = rsf_streamline_host_token(presentation.host(),source) != nullptr;
        if (source == 1 ? (cpu_owned || token_owned) : (!cpu_owned || cpu_facts.stage_mask != 1u || !token_owned)) {
            std::printf("FAIL: source=%llu cold/owned CPU-token admission cpu=%u token=%u mask=%u\n",
                static_cast<unsigned long long>(source),unsigned(cpu_owned),unsigned(token_owned),cpu_facts.stage_mask); return failed(__LINE__);
        }
        const float scene_color[]{0.25f, 0.5f, 0.75f, 1}, zero[4]{}, unwritten[]{3, 0, 3, 1};
        context->ClearRenderTargetView(color_view.Get(), scene_color); context->ClearRenderTargetView(motion_view.Get(), zero);
        context->ClearRenderTargetView(output_view.Get(), unwritten); context->ClearDepthStencilView(depth_view.Get(), D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 0.5f, 0);
        rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
        pass.session_id = host.session_id; pass.source_frame_id = pass.submission_id = pass.scope_id = source;
        pass.family_key = pass.view_key = pass.history_key = pass.viewport_key = pass.window_key = pass.rhi_viewport_key = 1;
        pass.native_frame = source; pass.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_NO_FG;
        pass.swapchain = chain.Get(); pass.color_input = color.Get(); pass.color_output = output.Get();
        pass.depth = depth.Get(); pass.motion = motion.Get(); pass.camera_valid = pass.motion_camera_included = 1;
        pass.motion_to_uv[0] = pass.motion_to_uv[1] = -1;
        auto& camera = pass.camera; camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        identity(camera.view_to_clip); identity(camera.clip_to_view); identity(camera.world_to_view);
        identity(camera.view_to_world); identity(camera.clip_to_previous_clip); identity(pass.previous_clip_to_clip);
        camera.view_to_clip[0] = float(height) / float(width); camera.clip_to_view[0] = float(width) / float(height);
        camera.near_plane = 0.1f; camera.far_plane = 1000; camera.vertical_fov_radians = 1.2f;
        camera.frame_time_seconds = 1.0f / 60; camera.depth_inverted = 1;
        camera.render_width = 640; camera.render_height = 360; camera.output_width = width; camera.output_height = height;
        camera.jitter_pixels[0] = source % 2 ? 0.25f : -0.25f;
        pass.render_rect[2] = 640; pass.render_rect[3] = 360; pass.output_rect[2] = int32_t(width); pass.output_rect[3] = int32_t(height);
        if (source == 4) {
            rsf_render_settings settings{sizeof(settings),RSF_RENDER_FRONTEND_ABI_VERSION};
            if (overlay.settings_get(&settings) != RSF_OK) return failed(__LINE__);
            settings.sr_quality = 0; // Real Quality -> native change through the public setting.
            if (overlay.settings_apply(&settings) != RSF_OK) return failed(__LINE__);
        }
        const bool fail_evaluation = source == 6;
        const auto before = bundles();
        // Planning must leave a pending real-render capture untouched, even when
        // the plan's input extent differs from the subsequently selected raster.
        if (!fail_evaluation && rsf_d3d11_host_request_capture(host.user) != RSF_OK) return failed(__LINE__);
        auto planning = pass; planning.flags |= RSF_GAME_RENDER_RECONFIGURE;
        if (host.process_upscale(host.user, context.Get(), &planning) != RSF_ERROR_NOT_READY) return failed(__LINE__);
        // A second unchanged plan must neither manufacture nor clear a pending reset.
        if (host.process_upscale(host.user, context.Get(), &planning) != RSF_ERROR_NOT_READY) return failed(__LINE__);
        if (source > 1) {
            auto no_camera = planning; no_camera.camera_valid = 0;
            if (host.process_upscale(host.user, context.Get(), &no_camera) != RSF_ERROR_NOT_READY) return failed(__LINE__);
            rsf_game_render_config config{sizeof(config)};
            if (!host.render_config(host.user,&config) || !config.enabled) return failed(__LINE__);
            pass.camera.render_width = config.render_width; pass.camera.render_height = config.render_height;
            pass.render_rect[2] = int32_t(config.render_width); pass.render_rect[3] = int32_t(config.render_height);
        }
        if (bundles() != before) { std::puts("FAIL: planning consumed the render capture request"); return failed(__LINE__); }
        if (fail_evaluation) pass.depth = nullptr; // Real SR refusal, never a fabricated provider result.
        if (source == 9) pass.flags |= RSF_GAME_RENDER_RESET;
        // Cold planning cannot initialize a scene or reserve reduced recovery.
        // Source1 must not authorize native reduction, then exercises its real
        // no-token evaluation refusal and successful bootstrap Present.
        const bool ticketed = source == 2 || source == 6;
        auto raster = pass; raster.camera_valid=0; raster.depth_domains={};
        rsf_d3d11_auxiliary_surface raw{sizeof(raw),RSF_D3D11_AUXILIARY_COLOUR,motion.Get()};
        for (unsigned j=0;j<4;++j) { raw.source_rect[j]=float(pass.render_rect[j]); raw.output_rect[j]=pass.output_rect[j]; }
        rsf_d3d11_reduced_frame recovery{sizeof(recovery),RSF_D3D11_REDUCED_FRAME_ABI_VERSION,
            RSF_D3D11_REDUCED_RESERVE,0,0,&raster,1,0,&raw};
        if (source==1) {
            diagnostic_phase="cold-reserve-refusal";
            const auto snapshot = [&] {
                std::vector<unsigned char> all;
                ID3D11Texture2D* resources[]{color.Get(),motion.Get(),depth.Get(),output.Get()};
                constexpr UINT strides[]{4,4,4,8};
                for (unsigned i=0;i<4;++i) {
                    const auto bytes=contents(device.Get(),context.Get(),resources[i],strides[i]);
                    if (bytes.empty()) return std::vector<unsigned char>{};
                    all.insert(all.end(),bytes.begin(),bytes.end());
                }
                return all;
            };
            const auto original_contents=snapshot();
            const auto cold_reserve=rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery);
            if (original_contents.empty() || cold_reserve!=RSF_ERROR_NOT_READY || recovery.token || recovery.result_flags ||
                rsf_native_cpu_read(host.session_id,source,&cpu_facts) ||
                rsf_streamline_host_token(presentation.host(),source) || snapshot()!=original_contents) {
                std::printf("FAIL cold reserve result=%d token=%llu flags=%u unchanged-content preflight expected\n",
                    cold_reserve,static_cast<unsigned long long>(recovery.token),recovery.result_flags); return failed(__LINE__);
            }
            std::puts("PASS: cold RESERVE refused without token or native-content mutation; no reduction committed");
        }
        if (ticketed) {
            diagnostic_phase="ticket-reserve-and-seal";
            if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK || !recovery.token) return failed(__LINE__);
            auto bad=recovery; bad.operation=RSF_D3D11_REDUCED_SEAL; ++bad.token;
            if (rsf_d3d11_host_reduced_frame(host.user,reinterpret_cast<void*>(uintptr_t(1)),&bad)!=RSF_ERROR_NOT_READY) return failed(__LINE__);
            recovery.operation=RSF_D3D11_REDUCED_SEAL;
            if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK) return failed(__LINE__);
            auto cancel=recovery; cancel.operation=RSF_D3D11_REDUCED_CANCEL;
            if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&cancel)!=RSF_ERROR_NOT_READY) return failed(__LINE__);
        }
        auto actual_aux=raw; actual_aux.jitter_pixels[0]=pass.camera.jitter_pixels[0];
        const rsf_d3d11_auxiliary_pass temporal{sizeof(temporal),RSF_D3D11_HOST_ABI_VERSION,&pass,1,&actual_aux};
        diagnostic_phase="actual-temporal-evaluation";
        const auto result = ticketed ? rsf_d3d11_host_upscale_auxiliaries(host.user,context.Get(),&temporal) :
            host.process_upscale(host.user, context.Get(), &pass);
        if (ticketed) {
            diagnostic_phase="ticket-finish-and-retire";
            recovery.operation=RSF_D3D11_REDUCED_FINISH;
            if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK ||
                !(recovery.result_flags&RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED) ||
                recovery.temporal_result!=result || !(recovery.result_flags&RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED) ||
                !(recovery.result_flags&(result ? RSF_D3D11_REDUCED_SPATIAL_OUTPUT : RSF_D3D11_REDUCED_TEMPORAL_OUTPUT)) ||
                (recovery.result_flags&RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED)) return failed(__LINE__);
            if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK) return failed(__LINE__);
            recovery.operation=RSF_D3D11_REDUCED_RETIRE;
            if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK ||
                !(recovery.result_flags&RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED) ||
                rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK) return failed(__LINE__);
            std::printf("PASS recovery source=%llu actual temporal result=%d flags=%u\n",
                static_cast<unsigned long long>(source),result,recovery.result_flags);
        }
        diagnostic_phase="evaluation-capture-and-present";
        last_pass=pass;
        std::printf("Shared SR source=%llu result=%d\n", static_cast<unsigned long long>(source), result);
        std::filesystem::path selected;
        for (const auto& candidate : bundles())
            if (std::find(before.begin(),before.end(),candidate) == before.end()) {
                if (!selected.empty()) return failed(__LINE__);
                selected = candidate;
            }
        if (source == 1) {
            // Host::cpu correctly discarded FRAME_BEGIN before bridge_bound. The first
            // real scene configures the host but cannot retroactively acquire its token.
            rsf_dlss_pipeline_status cold{sizeof(cold)};
            if (result != RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED ||
                rsf_dlss_pipeline_get_status(&cold) != RSF_DLSS_PIPELINE_OK || cold.frames_evaluated || cold.frames_refused != 1 ||
                rsf_native_cpu_read(host.session_id,source,&cpu_facts) || rsf_streamline_host_token(presentation.host(),source) ||
                selected.empty() || std::filesystem::exists(selected / "sr_constants.json")) {
                std::puts("FAIL: cold source must refuse before assembled backend submission"); return failed(__LINE__);
            }
            rsf_input_dump_cancel(host.session_id);
            std::puts("PASS: cold source refused without retained CPU begin/token or backend constants; completing Present");
        } else if (fail_evaluation) {
            if (result != RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT || !selected.empty()) return failed(__LINE__);
        } else {
            if (result != RSF_OK) return failed(__LINE__);
            if (selected.empty()) { std::puts("FAIL: assembled backend constants capture absent"); return failed(__LINE__); }
            std::ifstream status_input(selected / "sr_status.json",std::ios::binary);
            const std::string capture_status{std::istreambuf_iterator<char>(status_input),{}};
            if (capture_status.find("\"source_frame_id\":" + std::to_string(source) + ",") == std::string::npos ||
                capture_status.find("\"result\":0") == std::string::npos) {
                std::puts("FAIL: captured SR evaluation did not match the requested source"); return failed(__LINE__);
            }
            std::ifstream frame_input(selected / "frame.json",std::ios::binary);
            const std::string captured_frame{std::istreambuf_iterator<char>(frame_input),{}};
            const auto captured_raster = "\"render\":[" + std::to_string(pass.camera.render_width) + "," +
                std::to_string(pass.camera.render_height) + "]";
            if (captured_frame.find(captured_raster) == std::string::npos) {
                std::puts("FAIL: capture retained planning dimensions instead of the evaluated raster"); return failed(__LINE__);
            }
            std::ifstream input(selected / "sr_constants.json",std::ios::binary);
            const std::string constants{std::istreambuf_iterator<char>(input),{}};
            const bool expected_reset = source == 2 || source == 4 || source == 7 || source == 9;
            if (constants.find(expected_reset ? "\"reset\":1" : "\"reset\":0") == std::string::npos) {
                std::printf("FAIL: source=%llu actual provider reset expected=%u constants=%s\n",
                    static_cast<unsigned long long>(source),unsigned(expected_reset),constants.c_str()); return failed(__LINE__);
            }
            std::printf("Plan/evaluate source=%llu actual backend reset=%u verified\n",
                static_cast<unsigned long long>(source),unsigned(expected_reset));
            rsf_input_dump_cancel(host.session_id); // Read only this fixture's assembled constants, no Present/capture claim.
        }
        // Present samples the public SR report. Plans after an actual success or
        // refusal must preserve that result, not publish a fictitious evaluation.
        if (source > 1) {
            planning = pass; planning.flags |= RSF_GAME_RENDER_RECONFIGURE;
            if (host.process_upscale(host.user, context.Get(), &planning) != RSF_ERROR_NOT_READY ||
                host.process_upscale(host.user, context.Get(), &planning) != RSF_ERROR_NOT_READY) return failed(__LINE__);
        }
        const auto presented = chain->Present(0, 0);
        if (FAILED(presented)) return failed(__LINE__);
        rsf_render_status published{sizeof(published),RSF_RENDER_FRONTEND_ABI_VERSION};
        if (overlay.status(&published) != RSF_OK || published.sr_result != result ||
            published.sr_effective_backend != (result == RSF_OK ? uint32_t(RSF_SR_DLSS) : 0u)) {
            std::puts("FAIL: planning replaced the actual cold/success/refusal evaluation status"); return failed(__LINE__);
        }
        if (source == 1) std::printf("PASS: cold source Present returned 0x%08lx; next source must own CPU/token\n",
            static_cast<unsigned long>(presented));
        cpu(host, source, RSF_GAME_CPU_FRAME_END);
        if (source == 10) {
            // A genuine new plan invalidates the old output report until evaluated.
            rsf_render_settings changed{sizeof(changed),RSF_RENDER_FRONTEND_ABI_VERSION};
            if (overlay.settings_get(&changed) != RSF_OK) return failed(__LINE__);
            changed.sr_quality = 1;
            if (overlay.settings_apply(&changed) != RSF_OK ||
                host.process_upscale(host.user, context.Get(), &planning) != RSF_ERROR_NOT_READY ||
                host.process_upscale(host.user, context.Get(), &planning) != RSF_ERROR_NOT_READY ||
                FAILED(chain->Present(0, 0)) || overlay.status(&published) != RSF_OK ||
                published.sr_result != RSF_ERROR_NOT_READY || published.sr_effective_backend) {
                std::puts("FAIL: changed planning retained stale successful output status"); return failed(__LINE__);
            }
        }
    }
    rsf_dlss_pipeline_status status{}; status.struct_size = sizeof(status);
    if (rsf_dlss_pipeline_get_status(&status) != RSF_DLSS_PIPELINE_OK || status.frames_evaluated != 8 || status.frames_refused != 1) return failed(__LINE__);
    // Resource-only recovery and requested-setting changes do not manufacture a
    // temporal camera or retroactively reconfigure a committed reduced source.
    for (uint64_t source=11;source<=12;++source) {
        diagnostic_source=source; diagnostic_phase="resource-only-reserve-and-owner-negatives";
        cpu(host,source,RSF_GAME_CPU_FRAME_BEGIN);
        auto raster=last_pass; raster.native_frame=raster.source_frame_id=raster.submission_id=raster.scope_id=source;
        raster.camera_valid=0; raster.depth_domains={};
        // Reactivation changes requested_generation. Like a real producer, apply
        // that generation through resource-only planning before querying/reserving.
        diagnostic_phase="resource-only-planning";
        auto resource_plan=raster; resource_plan.flags|=RSF_GAME_RENDER_RECONFIGURE;
        if (host.process_upscale(host.user,context.Get(),&resource_plan)!=RSF_ERROR_NOT_READY) return failed(__LINE__);
        diagnostic_phase="resource-only-reserve-and-owner-negatives";
        rsf_game_render_config plan{sizeof(plan)};
        if (!host.render_config(host.user,&plan) || !plan.enabled) return failed(__LINE__);
        raster.render_rect[2]=int32_t(plan.render_width); raster.render_rect[3]=int32_t(plan.render_height);
        raster.camera.render_width=plan.render_width; raster.camera.render_height=plan.render_height;
        rsf_d3d11_auxiliary_surface raw{sizeof(raw),RSF_D3D11_AUXILIARY_COLOUR,motion.Get()};
        for (unsigned j=0;j<4;++j) { raw.source_rect[j]=float(raster.render_rect[j]); raw.output_rect[j]=raster.output_rect[j]; }
        rsf_d3d11_reduced_frame recovery{sizeof(recovery),RSF_D3D11_REDUCED_FRAME_ABI_VERSION,
            RSF_D3D11_REDUCED_RESERVE,0,0,&raster,1,0,&raw};
        if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK) return failed(__LINE__);
        auto wrong_session=raster; ++wrong_session.session_id;
        auto refused=recovery; refused.owner=&wrong_session; refused.operation=RSF_D3D11_REDUCED_SEAL;
        if (rsf_d3d11_host_reduced_frame(host.user,reinterpret_cast<void*>(uintptr_t(1)),&refused)!=RSF_ERROR_NOT_READY) return failed(__LINE__);
        auto resized=raster; --resized.output_rect[2];
        refused=recovery; refused.owner=&resized; refused.operation=RSF_D3D11_REDUCED_SEAL;
        if (rsf_d3d11_host_reduced_frame(host.user,reinterpret_cast<void*>(uintptr_t(1)),&refused)!=RSF_ERROR_NOT_READY) return failed(__LINE__);
        auto changed_owner=raster; ++changed_owner.resource_generation;
        refused.owner=&changed_owner;
        if (rsf_d3d11_host_reduced_frame(host.user,reinterpret_cast<void*>(uintptr_t(1)),&refused)!=RSF_ERROR_NOT_READY) return failed(__LINE__);
        ++changed_owner.view_key; refused.owner=&changed_owner;
        if (rsf_d3d11_host_reduced_frame(host.user,reinterpret_cast<void*>(uintptr_t(1)),&refused)!=RSF_ERROR_NOT_READY) return failed(__LINE__);
        int32_t thread_result{};
        std::thread other([&] { auto request=recovery; request.operation=RSF_D3D11_REDUCED_SEAL;
            thread_result=rsf_d3d11_host_reduced_frame(host.user,reinterpret_cast<void*>(uintptr_t(1)),&request); });
        other.join(); if (thread_result!=RSF_ERROR_NOT_READY) return failed(__LINE__);
        diagnostic_phase="resource-only-cancel-seal-skip";
        if (source==12) {
            recovery.operation=RSF_D3D11_REDUCED_CANCEL;
            if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK ||
                !(recovery.result_flags&RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED) ||
                (recovery.result_flags&RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED)) return failed(__LINE__);
            recovery.operation=RSF_D3D11_REDUCED_RESERVE; recovery.token=0;
            if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK) return failed(__LINE__);
        }
        recovery.operation=RSF_D3D11_REDUCED_SEAL;
        if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK) return failed(__LINE__);
        if (source==11) {
            if (host.process_upscale(host.user,context.Get(),&raster)!=RSF_ERROR_INVALID_ARGUMENT) return failed(__LINE__);
        } else {
            rsf_render_settings changed{sizeof(changed),RSF_RENDER_FRONTEND_ABI_VERSION};
            if (overlay.settings_get(&changed)!=RSF_OK) return failed(__LINE__);
            changed.sr_enabled=0;
            if (overlay.settings_apply(&changed)!=RSF_OK) return failed(__LINE__);
            auto temporal=raster; temporal.camera_valid=1;
            auto a=raw; a.jitter_pixels[0]=temporal.camera.jitter_pixels[0];
            const rsf_d3d11_auxiliary_pass request{sizeof(request),RSF_D3D11_HOST_ABI_VERSION,&temporal,1,&a};
            if (rsf_d3d11_host_upscale_auxiliaries(host.user,context.Get(),&request)!=RSF_ERROR_NOT_READY) return failed(__LINE__);
            auto planning=temporal; planning.flags|=RSF_GAME_RENDER_RECONFIGURE;
            if (host.process_upscale(host.user,context.Get(),&planning)!=RSF_ERROR_BUSY) return failed(__LINE__);
        }
        diagnostic_phase="resource-only-finish-and-suspended-retire";
        recovery.operation=RSF_D3D11_REDUCED_FINISH;
        recovery.temporal_result=RSF_OK; recovery.result_flags=0xffffffffu; // Never caller success claims.
        if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK ||
            !(recovery.result_flags&RSF_D3D11_REDUCED_SPATIAL_OUTPUT) ||
            (recovery.result_flags&RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED) || recovery.temporal_result!=RSF_ERROR_NOT_READY) return failed(__LINE__);
        if (rsf_d3d11_host_set_active(host.user,0)!=RSF_OK) return failed(__LINE__);
        recovery.operation=RSF_D3D11_REDUCED_RETIRE;
        if (rsf_d3d11_host_reduced_frame(host.user,context.Get(),&recovery)!=RSF_OK ||
            !(recovery.result_flags&RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED) ||
            rsf_d3d11_host_set_active(host.user,1)!=RSF_OK) return failed(__LINE__);
        if (FAILED(chain->Present(0,0))) return failed(__LINE__);
        cpu(host,source,RSF_GAME_CPU_FRAME_END);
        std::printf("PASS no-camera/frozen configuration recovery source=%llu\n",static_cast<unsigned long long>(source));
    }
    diagnostic_phase="final-counters-and-readback";
    rsf_dlss_pipeline_status after_recovery{sizeof(after_recovery)};
    if (rsf_dlss_pipeline_get_status(&after_recovery)!=RSF_DLSS_PIPELINE_OK ||
        after_recovery.frames_evaluated!=status.frames_evaluated || after_recovery.frames_refused!=status.frames_refused) return failed(__LINE__);
    rsf_native_fg_status generation{}; generation.struct_size = sizeof(generation);
    if (!rsf_native_fg_status_get(&generation) || generation.requested.mode != RSF_FG_OFF ||
        generation.vendor.effective_mode != RSF_FG_OFF || generation.tagged_frames) return failed(__LINE__);
    std::printf("FG requested=%u effective=%u tagged=%llu\n", generation.requested.mode,
        generation.vendor.effective_mode, static_cast<unsigned long long>(generation.tagged_frames));
    D3D11_TEXTURE2D_DESC readback_desc{}; output->GetDesc(&readback_desc);
    readback_desc.BindFlags = 0; readback_desc.Usage = D3D11_USAGE_STAGING; readback_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> readback;
    if (FAILED(device->CreateTexture2D(&readback_desc, nullptr, &readback))) return failed(__LINE__);
    context->CopyResource(readback.Get(), output.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(readback.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return failed(__LINE__);
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
