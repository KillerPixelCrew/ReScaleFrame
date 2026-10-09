// SPDX-License-Identifier: GPL-3.0-only
struct XessSession {
    HMODULE module = nullptr;
    xess_context_handle_t context = nullptr;
    decltype(&xessD3D12CreateContext) create = nullptr;
    decltype(&xessD3D12Init) init = nullptr;
    decltype(&xessD3D12Execute) execute = nullptr;
    decltype(&xessDestroyContext) destroy = nullptr;
    decltype(&xessGetInputResolution) resolution = nullptr;
    decltype(&xessSetVelocityScale) velocity = nullptr;
    decltype(&xessGetVersion) get_version = nullptr;
    rsf_sr_open_desc open{};
    uint64_t version = 0;
    char name[128]{};
    // XeSS needs a mask on every frame once the responsive flag is set. Frames without one get
    // this zeroed output-size texture, which leaves history weights unchanged.
    Microsoft::WRL::ComPtr<ID3D12Resource> no_response;
};
xess_quality_settings_t xess_quality(rsf_quality quality)
{
    switch (quality) {
    case RSF_QUALITY_NATIVE: return XESS_QUALITY_SETTING_AA;
    case RSF_QUALITY_QUALITY: return XESS_QUALITY_SETTING_QUALITY;
    case RSF_QUALITY_BALANCED: return XESS_QUALITY_SETTING_BALANCED;
    case RSF_QUALITY_PERFORMANCE: return XESS_QUALITY_SETTING_PERFORMANCE;
    case RSF_QUALITY_ULTRA_PERFORMANCE: return XESS_QUALITY_SETTING_ULTRA_PERFORMANCE;
    default: return XESS_QUALITY_SETTING_ULTRA_QUALITY;
    }
}
void sr_close(void* pointer)
{
    auto* session = static_cast<XessSession*>(pointer);
    if (!session) return;
    if (session->context) session->destroy(session->context);
    session->no_response.Reset();
    if (session->module) FreeLibrary(session->module);
    delete session;
}
rsf_backend_result sr_open(const rsf_sr_open_desc* desc, void** out)
{
    auto result = rsf::validate_open(desc, out);
    if (result != RSF_BACKEND_OK) return result;
    auto* session = new (std::nothrow) XessSession;
    if (!session) return RSF_BACKEND_ERROR_INIT_FAILED;
    session->open = *desc;
    session->module = rsf::load_runtime(*desc, L"libxess.dll");
    if (!session->module) { sr_close(session); return RSF_BACKEND_ERROR_LOAD_FAILED; }
#define RSF_XESS_ENTRY(member, function) session->member = rsf::entry<decltype(&function)>(session->module, #function)
    RSF_XESS_ENTRY(create, xessD3D12CreateContext);
    RSF_XESS_ENTRY(init, xessD3D12Init);
    RSF_XESS_ENTRY(execute, xessD3D12Execute);
    RSF_XESS_ENTRY(destroy, xessDestroyContext);
    RSF_XESS_ENTRY(resolution, xessGetInputResolution);
    RSF_XESS_ENTRY(velocity, xessSetVelocityScale);
    RSF_XESS_ENTRY(get_version, xessGetVersion);
#undef RSF_XESS_ENTRY
    if (!session->create || !session->init || !session->execute || !session->destroy ||
        !session->resolution || !session->velocity || !session->get_version) {
        sr_close(session); return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    }
    xess_version_t version{};
    if (session->get_version(&version) != XESS_RESULT_SUCCESS) {
        sr_close(session); return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    session->version = (uint64_t(version.major) << 48) | (uint64_t(version.minor) << 32) |
                          (uint64_t(version.patch) << 16) | version.reserved;
    std::snprintf(session->name, sizeof(session->name), "XeSS %u.%u.%u", version.major, version.minor, version.patch);
    if (desc->version_id && desc->version_id != session->version) {
        sr_close(session); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    if (session->create(static_cast<ID3D12Device*>(desc->device), &session->context) != XESS_RESULT_SUCCESS) {
        sr_close(session); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    xess_d3d12_init_params_t init{};
    init.outputResolution = {desc->output_width, desc->output_height};
    init.qualitySetting = xess_quality(desc->quality);
    init.initFlags = (desc->inverted_depth ? XESS_INIT_FLAG_INVERTED_DEPTH : 0u) |
        (!desc->hdr ? XESS_INIT_FLAG_LDR_INPUT_COLOR : 0u) |
        (desc->motion_jittered ? XESS_INIT_FLAG_JITTERED_MV : 0u) |
        (desc->auto_exposure ? XESS_INIT_FLAG_ENABLE_AUTOEXPOSURE : XESS_INIT_FLAG_EXPOSURE_SCALE_TEXTURE) |
        XESS_INIT_FLAG_RESPONSIVE_PIXEL_MASK;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC zero{};
    zero.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; zero.Width = desc->output_width; zero.Height = desc->output_height;
    zero.DepthOrArraySize = zero.MipLevels = 1; zero.SampleDesc.Count = 1; zero.Format = DXGI_FORMAT_R8_UNORM;
    // Committed resources are zero-filled unless created with CREATE_NOT_ZEROED.
    if (FAILED(static_cast<ID3D12Device*>(desc->device)->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &zero,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, nullptr, IID_PPV_ARGS(session->no_response.GetAddressOf())))) {
        sr_close(session); return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    if (session->init(session->context, &init) != XESS_RESULT_SUCCESS) {
        sr_close(session); return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    *out = session;
    return RSF_BACKEND_OK;
}
rsf_backend_result sr_plan(void* pointer, rsf_quality quality, uint32_t* width, uint32_t* height)
{
    auto* session = static_cast<XessSession*>(pointer);
    if (!session || !width || !height || quality > RSF_QUALITY_ULTRA_QUALITY) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    xess_2d_t output{session->open.output_width, session->open.output_height}, input{};
    if (session->resolution(session->context, &output, xess_quality(quality), &input) != XESS_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    *width = input.x; *height = input.y;
    return RSF_BACKEND_OK;
}
rsf_backend_result sr_evaluate(void* pointer, void* context, const rsf_sr_frame* frame)
{
    auto result = rsf::validate_frame(frame);
    if (result != RSF_BACKEND_OK) return result;
    if (!pointer || !context) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* session = static_cast<XessSession*>(pointer);
    result = rsf::validate_d3d12_resources(*frame, session->open.device);
    if (result != RSF_BACKEND_OK) return result;
    if (!session->open.auto_exposure && !frame->exposure.resource) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (session->velocity(session->context, frame->motion_scale_x, frame->motion_scale_y) != XESS_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    xess_d3d12_execute_params_t params{};
    params.pColorTexture = static_cast<ID3D12Resource*>(frame->color.resource);
    params.pDepthTexture = static_cast<ID3D12Resource*>(frame->depth.resource);
    params.pVelocityTexture = static_cast<ID3D12Resource*>(frame->motion.resource);
    params.pExposureScaleTexture = static_cast<ID3D12Resource*>(frame->exposure.resource);
    params.pResponsivePixelMaskTexture = frame->reactive.resource ?
        static_cast<ID3D12Resource*>(frame->reactive.resource) : session->no_response.Get();
    params.pOutputTexture = static_cast<ID3D12Resource*>(frame->output.resource);
    params.inputWidth = frame->record->render_width;
    params.inputHeight = frame->record->render_height;
    params.jitterOffsetX = frame->jitter_x;
    params.jitterOffsetY = frame->jitter_y;
    params.exposureScale = 1.0f / frame->pre_exposure;
    params.resetHistory = frame->reset;
    const auto code = session->execute(session->context, static_cast<ID3D12GraphicsCommandList*>(context), &params);
    return code == XESS_RESULT_SUCCESS ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
