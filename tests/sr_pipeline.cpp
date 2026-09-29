// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/d3d11_state.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstdio>
using Microsoft::WRL::ComPtr;
void log_line(void*, const char* message) { std::fprintf(stderr, "%s\n", message); }
int main(int argc, char** argv)
{
    if (argc != 4) { std::puts("provide Streamline, FFX and XeSS runtime directories for the hardware fixture"); return 77; }
    ComPtr<IDXGIFactory> factory; ComPtr<IDXGIAdapter> adapter;
    if (FAILED(CreateDXGIFactory(IID_PPV_ARGS(&factory)))) return 77;
    for (uint32_t i = 0; ; ++i) {
        if (FAILED(factory->EnumAdapters(i, &adapter))) return 77;
        DXGI_ADAPTER_DESC desc{}; adapter->GetDesc(&desc);
        if (desc.VendorId == 0x10de) break;
        adapter.Reset();
    }
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    if (FAILED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, 0, nullptr, 0,
                                 D3D11_SDK_VERSION, &device, nullptr, &context))) return 77;
    rsf_dlss_pipeline_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    setup.streamline_directory_utf8 = argv[1]; setup.output_width = setup.output_height = 512;
    setup.quality = RSF_DLSS_QUALITY_NATIVE;
    setup.motion.struct_size = sizeof(setup.motion);
    setup.motion.scale_x = setup.motion.scale_y = 1;
    setup.motion.output_scale_x = setup.motion.output_scale_y = 1;
    setup.motion.invalid_value = -1000; setup.motion.zero_means_unwritten = 1;
    setup.engine = RSF_DLSS_ENGINE_CUSTOM; setup.engine_version_utf8 = "ReScaleFrame synthetic fixture";
    setup.project_id_utf8 = "a3ed1f08-3542-4698-b85c-e1a9908e861a";
    setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = argv[2];
    setup.xess_directory_utf8 = argv[3]; setup.view_space_to_meters = 1; setup.log = log_line;
    auto result = rsf_dlss_pipeline_start(device.Get(), &setup);
    if (result != 0) { std::fprintf(stderr, "pipeline start refused: %d\n", result); return 1; }
    ComPtr<ID3D11Texture2D> textures[3];
    for (uint32_t i = 0; i < 3; ++i) {
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = desc.Height = 512; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        desc.Format = i == 0 ? DXGI_FORMAT_R16G16B16A16_FLOAT : i == 1 ? DXGI_FORMAT_R32_FLOAT : DXGI_FORMAT_R16G16_FLOAT;
        desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
        if (FAILED(device->CreateTexture2D(&desc, nullptr, &textures[i]))) return 1;
        ComPtr<ID3D11UnorderedAccessView> view;
        if (FAILED(device->CreateUnorderedAccessView(textures[i].Get(), nullptr, &view))) return 1;
        const FLOAT values[] = {i == 0 ? 0.25f : i == 1 ? 0.5f : 0.0f, 0, 0, 1};
        context->ClearUnorderedAccessViewFloat(view.Get(), values);
    }
    rsf_pipeline_camera_frame camera{};
    camera.struct_size = sizeof(camera); camera.abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION;
    camera.view_to_clip[0] = camera.view_to_clip[5] = 2; camera.view_to_clip[11] = 1; camera.view_to_clip[14] = 0.1f;
    camera.clip_to_view[0] = camera.clip_to_view[5] = 0.5f; camera.clip_to_view[11] = 10; camera.clip_to_view[14] = 1;
    camera.clip_to_prev_clip[0] = camera.clip_to_prev_clip[5] = camera.clip_to_prev_clip[10] = camera.clip_to_prev_clip[15] = 1;
    camera.prev_clip_to_clip[0] = camera.prev_clip_to_clip[5] = camera.prev_clip_to_clip[10] = camera.prev_clip_to_clip[15] = 1;
    camera.camera_forward[2] = camera.camera_up[1] = camera.camera_right[0] = 1;
    camera.near_plane = 0.1f; camera.vertical_fov = 1; camera.aspect_ratio = 1;
    camera.depth_inverted = camera.has_jitter = 1; camera.jitter_pixels[0] = 0.25f;
    rsf_dlss_pipeline_frame frame{};
    frame.struct_size = sizeof(frame); frame.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    frame.scene_color = textures[0].Get(); frame.depth = textures[1].Get(); frame.game_motion = textures[2].Get();
    frame.render_width = frame.render_height = 512; frame.camera = &camera;
    bool passed = true;
    const uint32_t sequence[] = {1u, 2u, 3u, 5u, 1u};
    for (uint32_t backend : sequence) {
        result = rsf_dlss_pipeline_select_backend(backend, nullptr, nullptr);
        std::fprintf(stderr, "select %u: %d\n", backend, result);
        if (result != 0) { passed = false; continue; }
        for (uint32_t i = 0; i < 3; ++i) {
            camera.jitter_pixels[0] = i % 2 ? -0.25f : 0.25f;
            rsf_d3d11_state saved{}; rsf_d3d11_state_save(context.Get(), &saved);
            result = rsf_dlss_pipeline_on_frame(context.Get(), &frame);
            rsf_d3d11_state_restore(context.Get(), &saved);
            std::fprintf(stderr, "evaluate %u: %d\n", backend, result);
            passed &= result == 0;
        }
    }
    result = rsf_dlss_pipeline_stop(); passed &= result == 0;
    return passed ? 0 : 1;
}
