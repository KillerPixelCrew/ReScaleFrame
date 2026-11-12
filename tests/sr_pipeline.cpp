// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/d3d11_state.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstdio>
#include <DirectXPackedVector.h>
#include <vector>
#include <cstring>
#include <cmath>
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
    const uint32_t sequence[] = {1u, 2u, 3u, 5u, 4u, 1u};
    for (uint32_t backend : sequence) {
        result = rsf_dlss_pipeline_select_backend(backend, nullptr, nullptr);
        std::fprintf(stderr, "select %u: %d\n", backend, result);
        if (backend == 4 && result == RSF_BACKEND_ERROR_NOT_SUPPORTED) continue;
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
    // Exercise the production post pass with deterministic reset frames on FSR3.
    result = rsf_dlss_pipeline_select_backend(3,nullptr,nullptr);
    passed &= result == 0;
    if (result == 0) {
        std::vector<uint16_t> source(512*512*4);
        for (uint32_t y=0;y<512;++y) for (uint32_t x=0;x<512;++x) {
            const float base=0.1f+0.3f*float(x)/512+(x>=256?0.3f:0)+0.01f*std::sin(float(x)*0.7f);
            for (uint32_t c=0;c<4;++c)
                source[(y*512+x)*4+c]=DirectX::PackedVector::XMConvertFloatToHalf(c==3?0.75f:base*(0.9f+float(c)*0.1f));
        }
        context->UpdateSubresource(textures[0].Get(),0,nullptr,source.data(),512*8,0);
        camera.reset=1; camera.jitter_pixels[0]=0.25f;
        auto read_output=[&]() {
            auto* output=static_cast<ID3D11Texture2D*>(rsf_dlss_pipeline_output_texture());
            D3D11_TEXTURE2D_DESC desc{}; output->GetDesc(&desc);
            desc.BindFlags=0; desc.Usage=D3D11_USAGE_STAGING; desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
            ComPtr<ID3D11Texture2D> copy; std::vector<uint16_t> bytes;
            if (FAILED(device->CreateTexture2D(&desc,nullptr,&copy))) return bytes;
            context->CopyResource(copy.Get(),output); D3D11_MAPPED_SUBRESOURCE mapped{};
            if (FAILED(context->Map(copy.Get(),0,D3D11_MAP_READ,0,&mapped))) return bytes;
            bytes.resize(512*512*4);
            for (uint32_t y=0;y<512;++y) std::memcpy(bytes.data()+y*512*4,static_cast<char*>(mapped.pData)+y*mapped.RowPitch,512*8);
            context->Unmap(copy.Get(),0); return bytes;
        };
        auto options=rsf_sharpening_defaults();
        passed &= rsf_dlss_pipeline_set_sharpening(&options)==0;
        passed &= rsf_dlss_pipeline_on_frame(context.Get(),&frame)==0;
        const auto off=read_output();
        options.mode=RSF_SHARPENING_RCAS; options.sharpness=0;
        passed &= rsf_dlss_pipeline_set_sharpening(&options)==0;
        passed &= rsf_dlss_pipeline_on_frame(context.Get(),&frame)==0;
        const auto zero=read_output();
        const bool identical=!off.empty() && off==zero;
        std::fprintf(stderr,"Sharpening Off versus zero strength: %s (byte identical)\n",identical?"PASS":"FAIL");
        passed &= identical;
        options.sharpness=1;
        passed &= rsf_dlss_pipeline_set_sharpening(&options)==0;
        passed &= rsf_dlss_pipeline_on_frame(context.Get(),&frame)==0;
        const auto sharpened=read_output();
        const bool changed=!sharpened.empty() && sharpened!=off;
        std::fprintf(stderr,"Sharpening production on_frame edge: %s (output changed)\n",changed?"PASS":"FAIL");
        passed &= changed;
        for (uint32_t mode : {RSF_SHARPENING_DEPTH_RCAS,RSF_SHARPENING_DEPTH_DAS,RSF_SHARPENING_VENDOR}) {
            options.mode=mode;
            passed &= rsf_dlss_pipeline_set_sharpening(&options)==0;
            passed &= rsf_dlss_pipeline_on_frame(context.Get(),&frame)==0;
            const auto filtered=read_output();
            const bool distinct=!filtered.empty() && filtered!=off;
            std::fprintf(stderr,"Sharpening production mode %u edge: %s\n",mode,distinct?"PASS":"FAIL");
            passed &= distinct;
        }
        options.mode=RSF_SHARPENING_OFF;
        passed &= rsf_dlss_pipeline_set_sharpening(&options)==0;
    }
    result = rsf_dlss_pipeline_stop(); passed &= result == 0;
    return passed ? 0 : 1;
}
