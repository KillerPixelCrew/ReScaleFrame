// SPDX-License-Identifier: GPL-3.0-only
// Frozen-input precision experiment. This is not ordered game-frame replay.
#include <rescaleframe/dlss.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/colour_fidelity.h>
#include <rescaleframe/colour_transport.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <vector>
#include <MinHook.h>
#ifdef RSF_PROBE_RENDERDOC
#include <renderdoc_app.h>
#endif
#ifdef RSF_PROBE_NGX_PARAMS
#include <nvsdk_ngx_params.h>
#endif
using Microsoft::WRL::ComPtr;
void log_line(void*, const char* message) { std::fprintf(stderr, "%s\n", message); }
#ifdef RSF_PROBE_NGX_PARAMS
// Resource/flag tracing and an optional pre-exposure experiment in the standalone probe.
// The component may expose a three-argument
// entry point while the frontend adds a feature ID. Forward all registers unchanged.
using CreateFn = uint32_t(*)(void*, uintptr_t, const void*, void*);
CreateFn original_create = nullptr;
using EvaluateFn = uint32_t(*)(void*, void*, NVSDK_NGX_Parameter*, void*);
EvaluateFn original_evaluate = nullptr;
uint32_t trace_evaluate(void* context, void* handle, NVSDK_NGX_Parameter* params, void* callback)
{
    static bool reported = false;
    char override_value[64]{};
    if (GetEnvironmentVariableA("RSF_PROBE_PRE_EXPOSURE", override_value, sizeof(override_value))) {
        const float value = std::strtof(override_value, nullptr);
        if (value > 0) params->Set(NVSDK_NGX_Parameter_DLSS_Pre_Exposure, value);
    }
    if (GetEnvironmentVariableA("RSF_PROBE_EXPOSURE_SCALE", override_value, sizeof(override_value))) {
        const float value = std::strtof(override_value, nullptr);
        if (value > 0) params->Set(NVSDK_NGX_Parameter_DLSS_Exposure_Scale, value);
    }
    if (!reported) {
        float pre = 0, scale = 0;
        params->Get(NVSDK_NGX_Parameter_DLSS_Pre_Exposure, &pre);
        params->Get(NVSDK_NGX_Parameter_DLSS_Exposure_Scale, &scale);
        std::fprintf(stderr, "NGX_EVALUATE preExposure=%g exposureScale=%g\n", double(pre), double(scale));
        const char* names[] = {NVSDK_NGX_Parameter_Color, NVSDK_NGX_Parameter_Output};
        for (const auto* name : names) {
            ID3D11Resource* resource = nullptr;
            if (params->Get(name, &resource) == NVSDK_NGX_Result_Success && resource) {
                ComPtr<ID3D11Texture2D> texture;
                if (SUCCEEDED(resource->QueryInterface(IID_PPV_ARGS(&texture)))) {
                    D3D11_TEXTURE2D_DESC description{}; texture->GetDesc(&description);
                    std::fprintf(stderr, "NGX_RESOURCE %s %ux%u format=%u bind=%x\n", name,
                                 description.Width, description.Height, unsigned(description.Format), description.BindFlags);
                }
            }
        }
        reported = true;
    }
    return original_evaluate(context, handle, params, callback);
}
uint32_t trace_create(void* context, uintptr_t second, const void* third, void* fourth)
{
    const auto* params = static_cast<const NVSDK_NGX_Parameter*>(second < 16 ? third : reinterpret_cast<const void*>(second));
    int flags = 0;
    const auto result = params->Get(NVSDK_NGX_Parameter_DLSS_Feature_Create_Flags, &flags);
    char ldr[8]{};
    if (GetEnvironmentVariableA("RSF_PROBE_LDR", ldr, sizeof(ldr)) && ldr[0] == '1') {
        flags &= ~int(NVSDK_NGX_DLSS_Feature_Flags_IsHDR);
        const_cast<NVSDK_NGX_Parameter*>(params)->Set(NVSDK_NGX_Parameter_DLSS_Feature_Create_Flags, flags);
    }
    std::fprintf(stderr, "NGX_CREATE flags_get=%x flags=%x HDR=%u AutoExposure=%u DepthInverted=%u\n",
                 unsigned(result), unsigned(flags), (unsigned(flags) & 1u) != 0,
                 (unsigned(flags) & 64u) != 0, (unsigned(flags) & 8u) != 0);
    return original_create(context, second, third, fourth);
}
void install_trace()
{
    auto module = GetModuleHandleW(L"nvngx_dlss.dll");
    auto target = module ? GetProcAddress(module, "NVSDK_NGX_D3D11_CreateFeature") : nullptr;
    if (!target) { std::fprintf(stderr, "NGX trace unavailable: component not loaded\n"); return; }
    if (MH_Initialize() == MH_OK &&
        MH_CreateHook(reinterpret_cast<void*>(target), reinterpret_cast<void*>(&trace_create),
                      reinterpret_cast<void**>(&original_create)) == MH_OK &&
        MH_EnableHook(reinterpret_cast<void*>(target)) == MH_OK)
        std::fprintf(stderr, "NGX creation trace installed in standalone probe\n");
    target = GetProcAddress(module, "NVSDK_NGX_D3D11_EvaluateFeature");
    if (target && MH_CreateHook(reinterpret_cast<void*>(target), reinterpret_cast<void*>(&trace_evaluate),
                               reinterpret_cast<void**>(&original_evaluate)) == MH_OK)
        MH_EnableHook(reinterpret_cast<void*>(target));
}
#endif
int main(int argc, char** argv)
{
    if (argc < 5 || argc > 10 || argc == 9) { std::puts("usage: exposure-probe interposer input.bin output-prefix exposure (0=auto) [quality: 0=DLAA,1=Quality,3=Performance,4=Ultra] [input DXGI format: 26=R11,10=FP16,2=FP32] [output format:10=FP16,2=FP32] [render width height]"); return 1; }
    const uint32_t render_width = argc == 10 ? uint32_t(std::strtoul(argv[8], nullptr, 10)) : 800;
    const uint32_t render_height = argc == 10 ? uint32_t(std::strtoul(argv[9], nullptr, 10)) : 452;
    if (!render_width || !render_height || render_width > 1600 || render_height > 900) return 1;
    const auto input_format = argc >= 7 ? static_cast<DXGI_FORMAT>(std::strtoul(argv[6], nullptr, 10)) : DXGI_FORMAT_R16G16B16A16_FLOAT;
    const auto output_format = argc >= 8 ? static_cast<DXGI_FORMAT>(std::strtoul(argv[7], nullptr, 10)) : DXGI_FORMAT_R16G16B16A16_FLOAT;
    if ((input_format != DXGI_FORMAT_R11G11B10_FLOAT && input_format != DXGI_FORMAT_R16G16B16A16_FLOAT && input_format != DXGI_FORMAT_R32G32B32A32_FLOAT) ||
        (output_format != DXGI_FORMAT_R16G16B16A16_FLOAT && output_format != DXGI_FORMAT_R32G32B32A32_FLOAT)) return 1;
    const uint32_t pixel_bytes = input_format == DXGI_FORMAT_R11G11B10_FLOAT ? 4u : input_format == DXGI_FORMAT_R16G16B16A16_FLOAT ? 8u : 16u;
    std::ifstream input(argv[2], std::ios::binary);
    std::vector<char> pixels(size_t(render_width) * render_height * pixel_bytes);
    if (!input.read(pixels.data(), std::streamsize(pixels.size()))) return 1;
#ifdef RSF_PROBE_RENDERDOC
    RENDERDOC_API_1_6_0* capture = nullptr;
    wchar_t capture_library[1024]{};
    if (GetEnvironmentVariableW(L"RSF_PROBE_RENDERDOC", capture_library, 1024)) {
        auto library = LoadLibraryW(capture_library);
        auto get_api = library ? reinterpret_cast<pRENDERDOC_GetAPI>(GetProcAddress(library, "RENDERDOC_GetAPI")) : nullptr;
        if (!get_api || get_api(eRENDERDOC_API_Version_1_6_0, reinterpret_cast<void**>(&capture)) != 1) return 1;
        capture->SetCaptureOptionU32(eRENDERDOC_Option_AllowUnsupportedVendorExtensions, 0x10de);
        capture->SetCaptureFilePathTemplate(argv[3]);
        capture->SetCaptureKeys(nullptr, 0);
        std::fprintf(stderr, "RenderDoc enabled before D3D11 device creation\n");
    }
#endif
    ComPtr<IDXGIFactory> factory; ComPtr<IDXGIAdapter> adapter;
    if (FAILED(CreateDXGIFactory(IID_PPV_ARGS(&factory)))) return 1;
    for (uint32_t i = 0; ; ++i) {
        if (FAILED(factory->EnumAdapters(i, &adapter))) return 1;
        DXGI_ADAPTER_DESC desc{}; adapter->GetDesc(&desc);
        if (desc.VendorId == 0x10de) break;
        adapter.Reset();
    }
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    if (FAILED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, 0, nullptr, 0,
                                D3D11_SDK_VERSION, &device, nullptr, &context))) return 1;
    rsf_dlss_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_DLSS_ABI_VERSION;
    setup.interposer_path_utf8 = argv[1]; setup.engine = RSF_DLSS_ENGINE_UNREAL;
    setup.engine_version_utf8 = "4.18"; setup.project_id_utf8 = "a3ed1f08-3542-4698-b85c-e1a9908e861a";
    setup.log = log_line;
    if (rsf_dlss_load(&setup) != 0 || rsf_dlss_set_device(device.Get()) != 0) return 1;
#ifdef RSF_PROBE_NGX_PARAMS
    install_trace();
#endif
    ComPtr<ID3D11Texture2D> color, output, depth, motion, exposure;
    D3D11_TEXTURE2D_DESC desc{}; desc.Width = render_width; desc.Height = render_height;
    desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1; desc.Format = input_format;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA initial{}; initial.pSysMem = pixels.data(); initial.SysMemPitch = render_width * pixel_bytes;
    if (FAILED(device->CreateTexture2D(&desc, &initial, &color))) return 1;
    desc.Width = 1600; desc.Height = 900; desc.Format = output_format;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &output))) return 1;
    desc.Width = render_width; desc.Height = render_height; desc.Format = DXGI_FORMAT_R32_FLOAT;
    std::vector<float> depths(size_t(render_width) * render_height, 0.5f); initial.pSysMem = depths.data(); initial.SysMemPitch = render_width * 4;
    char depth_path[1024]{};
    if (GetEnvironmentVariableA("RSF_PROBE_DEPTH_FILE",depth_path,sizeof(depth_path))) {
        std::ifstream depth_input(depth_path,std::ios::binary);
        if (!depth_input.read(reinterpret_cast<char*>(depths.data()),std::streamsize(depths.size()*sizeof(float)))) return 1;
    }
    if (FAILED(device->CreateTexture2D(&desc, &initial, &depth))) return 1;
    desc.Format = DXGI_FORMAT_R16G16_FLOAT;
    std::vector<uint32_t> motions(size_t(render_width) * render_height, 0); initial.pSysMem = motions.data();
    if (FAILED(device->CreateTexture2D(&desc, &initial, &motion))) return 1;
    const float multiplier = std::strtof(argv[4], nullptr);
    if (multiplier > 0) {
        desc.Width = desc.Height = 1; desc.Format = DXGI_FORMAT_R32_FLOAT;
        initial.pSysMem = &multiplier; initial.SysMemPitch = 4;
        if (FAILED(device->CreateTexture2D(&desc, &initial, &exposure))) return 1;
    }
    rsf_dlss_frame frame{}; frame.struct_size = sizeof(frame); frame.abi_version = RSF_DLSS_ABI_VERSION;
    frame.color_in = color.Get(); frame.color_out = output.Get(); frame.depth = depth.Get(); frame.motion = motion.Get();
    frame.exposure = exposure.Get(); frame.render_width = render_width; frame.render_height = render_height;
    frame.output_width = 1600; frame.output_height = 900; frame.quality = argc >= 6 ? static_cast<rsf_dlss_quality>(std::strtoul(argv[5], nullptr, 10)) : RSF_DLSS_QUALITY_PERFORMANCE;
    frame.motion_scale_x = frame.motion_scale_y = 1; frame.camera_motion_included = 1; frame.depth_inverted = 1;
    frame.near_plane = 0.1f; frame.far_plane = 10000; frame.vertical_fov = 1; frame.aspect_ratio = 1600.0f / 900;
    frame.camera_up[1] = frame.camera_right[0] = frame.camera_forward[2] = 1;
    for (uint32_t i = 0; i < 4; ++i) {
        frame.camera_view_to_clip[i * 5] = frame.clip_to_camera_view[i * 5] = 1;
        frame.clip_to_prev_clip[i * 5] = frame.prev_clip_to_clip[i * 5] = 1;
    }
    bool ok = true;
    // RSF_PROBE_TRANSPORT=1: encode with the bounded colour transport, run DLSS with HDR input off,
    // decode the output. The exposure argument becomes the transport's E (0 means 1).
    char transport_flag[8]{};
    const bool transported = GetEnvironmentVariableA("RSF_PROBE_TRANSPORT", transport_flag, sizeof(transport_flag)) && transport_flag[0] == '1';
    rsf_colour_transport* transport = nullptr;
    if (transported) {
        void* encoded = nullptr;
        ok = rsf_colour_transport_create(device.Get(), &transport) &&
            rsf_colour_transport_encode(transport, context.Get(), color.Get(), exposure.Get(), render_width, render_height, &encoded);
        frame.color_in = encoded; frame.color_encoded = 1; frame.exposure = nullptr;
        std::fprintf(stderr, "colour transport: encoded input, DLSS HDR input off\n");
    }
    for (uint32_t i = 0; ok && i < 60; ++i) {
#ifdef RSF_PROBE_RENDERDOC
        if (capture && i == 59) capture->StartFrameCapture(device.Get(), nullptr);
#endif
        frame.reset = i == 0; frame.frame_index = i + 1;
        const auto result = rsf_dlss_evaluate(context.Get(), &frame);
        if (result != 0) { std::fprintf(stderr, "evaluate refused %d\n", result); ok = false; break; }
    }
#ifdef RSF_PROBE_RENDERDOC
    if (capture && capture->IsFrameCapturing()) {
        const auto saved = capture->EndFrameCapture(device.Get(), nullptr);
        std::fprintf(stderr, "RenderDoc capture saved=%u count=%u\n", saved, capture->GetNumCaptures());
        ok = saved != 0 && ok;
    }
#endif
    if (transport) {
        const float no_jitter[2]{};
        ok = rsf_colour_transport_decode(transport, context.Get(), output.Get(), exposure.Get(), 1600, 900,
            color.Get(), render_width, render_height, no_jitter) && ok;
        rsf_colour_transport_destroy(transport);
    }
    rsf_texture_dump_options options{}; options.struct_size = sizeof(options); options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;
    char fidelity[8]{};
    if (GetEnvironmentVariableA("RSF_PROBE_COLOUR_FIDELITY",fidelity,sizeof(fidelity)) && fidelity[0]=='1') {
        rsf_colour_fidelity* pass=nullptr; const float jitter[2]{};
        ComPtr<ID3D11Query> disjoint, begin, end;
        D3D11_QUERY_DESC query{}; query.Query = D3D11_QUERY_TIMESTAMP_DISJOINT;
        bool timing = SUCCEEDED(device->CreateQuery(&query,&disjoint));
        query.Query = D3D11_QUERY_TIMESTAMP;
        timing = timing && SUCCEEDED(device->CreateQuery(&query,&begin)) && SUCCEEDED(device->CreateQuery(&query,&end));
        ok=rsf_colour_fidelity_create(device.Get(),&pass) && ok;
        if (timing) { context->Begin(disjoint.Get()); context->End(begin.Get()); }
        ok=rsf_colour_fidelity_run(pass,context.Get(),color.Get(),output.Get(),depth.Get(),1,jitter) && ok;
        if (timing) {
            context->End(end.Get()); context->End(disjoint.Get()); context->Flush();
            D3D11_QUERY_DATA_TIMESTAMP_DISJOINT times{};
            for (uint32_t n=0;n<1000 && context->GetData(disjoint.Get(),&times,sizeof(times),0)==S_FALSE;++n) Sleep(1);
            UINT64 first=0,last=0;
            if (times.Frequency && !times.Disjoint &&
                context->GetData(begin.Get(),&first,sizeof(first),0)==S_OK &&
                context->GetData(end.Get(),&last,sizeof(last),0)==S_OK)
                std::fprintf(stderr,"colour correction GPU time %.4f ms\n",double(last-first)*1000.0/double(times.Frequency));
        }
        rsf_colour_fidelity_destroy(pass);
    }
    options.output_prefix_utf8 = argv[3];
    ok = rsf_dump_texture_bytes(device.Get(), context.Get(), output.Get(), &options) == 0 && ok;
#ifdef RSF_PROBE_NGX_PARAMS
    MH_DisableHook(MH_ALL_HOOKS); MH_Uninitialize();
#endif
    rsf_dlss_shutdown();
    return ok ? 0 : 1;
}
