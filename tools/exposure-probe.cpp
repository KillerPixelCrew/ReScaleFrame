// SPDX-License-Identifier: MIT
// SDK-specific leaves for the standalone frozen-input probe in exposure_probe/src/lib.rs.
#include <rescaleframe/dlss.h>
#include <rescaleframe/texture_dump.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
#include <MinHook.h>
#ifdef RSF_PROBE_RENDERDOC
#include <renderdoc_app.h>
#endif
#ifdef RSF_PROBE_NGX_PARAMS
#include <nvsdk_ngx_params.h>
#endif
using Microsoft::WRL::ComPtr;
static_assert(RSF_DLSS_QUALITY_PERFORMANCE == 3);
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

#ifdef RSF_PROBE_RENDERDOC
namespace {
RENDERDOC_API_1_6_0* capture = nullptr;
}
#endif

extern "C" int32_t rsf_probe_capture_open(const char* prefix)
{
#ifdef RSF_PROBE_RENDERDOC
    wchar_t capture_library[1024]{};
    const auto size = GetEnvironmentVariableW(L"RSF_PROBE_RENDERDOC", capture_library, 1024);
    if (size == 0) return 1;
    if (size >= 1024) return 0;
    auto library = LoadLibraryW(capture_library);
    auto get_api = library ? reinterpret_cast<pRENDERDOC_GetAPI>(GetProcAddress(library, "RENDERDOC_GetAPI")) : nullptr;
    if (!get_api || get_api(eRENDERDOC_API_Version_1_6_0, reinterpret_cast<void**>(&capture)) != 1) return 0;
    capture->SetCaptureOptionU32(eRENDERDOC_Option_AllowUnsupportedVendorExtensions, 0x10de);
    capture->SetCaptureFilePathTemplate(prefix);
    capture->SetCaptureKeys(nullptr, 0);
    std::fprintf(stderr, "RenderDoc enabled before D3D11 device creation\n");
#else
    (void)prefix;
#endif
    return 1;
}

extern "C" void rsf_probe_capture_start(void* device)
{
#ifdef RSF_PROBE_RENDERDOC
    if (capture) capture->StartFrameCapture(device, nullptr);
#else
    (void)device;
#endif
}

extern "C" int32_t rsf_probe_capture_end(void* device)
{
#ifdef RSF_PROBE_RENDERDOC
    if (capture && capture->IsFrameCapturing()) {
        const auto saved = capture->EndFrameCapture(device, nullptr);
        std::fprintf(stderr, "RenderDoc capture saved=%u count=%u\n", saved, capture->GetNumCaptures());
        return saved != 0;
    }
#else
    (void)device;
#endif
    return 1;
}

extern "C" int32_t rsf_probe_load(const char* interposer, void* device)
{
    rsf_dlss_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_DLSS_ABI_VERSION;
    setup.interposer_path_utf8 = interposer;
    setup.engine = RSF_DLSS_ENGINE_UNREAL;
    setup.engine_version_utf8 = "4.18";
    setup.project_id_utf8 = "a3ed1f08-3542-4698-b85c-e1a9908e861a";
    setup.log = log_line;
    if (rsf_dlss_load(&setup) != 0) return 0;
    if (rsf_dlss_set_device(device) != 0) {
        rsf_dlss_shutdown();
        return 0;
    }
#ifdef RSF_PROBE_NGX_PARAMS
    install_trace();
#endif
    return 1;
}

extern "C" void rsf_probe_shutdown()
{
#ifdef RSF_PROBE_NGX_PARAMS
    MH_DisableHook(MH_ALL_HOOKS);
    MH_Uninitialize();
#endif
    rsf_dlss_shutdown();
}

extern "C" int32_t rsf_probe_dump(void* device, void* context, void* output, const char* prefix)
{
    rsf_texture_dump_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;
    options.output_prefix_utf8 = prefix;
    return rsf_dump_texture_bytes(device, context, output, &options);
}

extern "C" int32_t rsf_exposure_probe(int32_t argc, const char* const* argv);

int main(int argc, char** argv)
{
    return rsf_exposure_probe(argc, argv);
}
