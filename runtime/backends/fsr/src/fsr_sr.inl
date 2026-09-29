// SPDX-License-Identifier: GPL-3.0-only
// Included inside the backend's private namespace, with SDK headers included at file scope.
struct FsrSession {
    HMODULE module = nullptr;
    ffxContext context = nullptr;
    PfnFfxCreateContext create = nullptr;
    PfnFfxDestroyContext destroy = nullptr;
    PfnFfxQuery query = nullptr;
    PfnFfxDispatch dispatch = nullptr;
    rsf_sr_open_desc open{};
    ffxCreateContextDescUpscale description{};
    ffxCreateBackendDX12Desc device{};
    ffxOverrideVersion override_version{};
    uint64_t version = 0;
    char name[128]{};
};
void sr_close(void* pointer)
{
    auto* session = static_cast<FsrSession*>(pointer);
    if (!session) return;
    if (session->context) session->destroy(&session->context, nullptr);
    if (session->module) FreeLibrary(session->module);
    delete session;
}
rsf_backend_result sr_open(const rsf_sr_open_desc* desc, void** out)
{
    auto result = rsf::validate_open(desc, out);
    if (result != RSF_BACKEND_OK) return result;
    if (desc->fsr_major < 2 || desc->fsr_major > 4) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* session = new (std::nothrow) FsrSession;
    if (!session) return RSF_BACKEND_ERROR_INIT_FAILED;
    session->open = *desc;
    session->module = rsf::load_runtime(*desc, L"amd_fidelityfx_upscaler_dx12.dll");
    if (!session->module) { sr_close(session); return RSF_BACKEND_ERROR_LOAD_FAILED; }
    session->create = rsf::entry<PfnFfxCreateContext>(session->module, "ffxCreateContext");
    session->destroy = rsf::entry<PfnFfxDestroyContext>(session->module, "ffxDestroyContext");
    session->query = rsf::entry<PfnFfxQuery>(session->module, "ffxQuery");
    session->dispatch = rsf::entry<PfnFfxDispatch>(session->module, "ffxDispatch");
    if (!session->create || !session->destroy || !session->query || !session->dispatch) {
        sr_close(session); return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    }
    uint64_t count = 32;
    uint64_t ids[32]{};
    const char* names[32]{};
    ffxQueryDescGetVersions versions{};
    versions.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    versions.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    versions.device = desc->device;
    versions.outputCount = &count;
    versions.versionIds = ids;
    versions.versionNames = names;
    if (session->query(nullptr, &versions.header) != FFX_API_RETURN_OK || count > 32) {
        sr_close(session); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    // IDs are opaque. Family identification uses the SDK's paired display version, never bit masks.
    unsigned best_minor = 0, best_patch = 0;
    for (uint64_t i = 0; i < count; ++i) {
        unsigned major = 0, minor = 0, patch = 0;
        if (!names[i] || std::sscanf(names[i], "%u.%u.%u", &major, &minor, &patch) != 3 ||
            major != desc->fsr_major || (desc->version_id && ids[i] != desc->version_id)) continue;
        if (!session->version || minor > best_minor || (minor == best_minor && patch > best_patch)) {
            session->version = ids[i];
            best_minor = minor; best_patch = patch;
            std::snprintf(session->name, sizeof(session->name), "%s", names[i]);
        }
    }
    if (!session->version) { sr_close(session); return RSF_BACKEND_ERROR_NOT_SUPPORTED; }
    session->device.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
    session->device.device = static_cast<ID3D12Device*>(desc->device);
    session->override_version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION;
    session->override_version.versionId = session->version;
    session->device.header.pNext = &session->override_version.header;
    auto& create = session->description;
    create.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    create.header.pNext = &session->device.header;
    create.maxRenderSize = {desc->output_width, desc->output_height};
    create.maxUpscaleSize = create.maxRenderSize;
    create.flags = (desc->hdr ? FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE : 0u) |
        (desc->inverted_depth ? FFX_UPSCALE_ENABLE_DEPTH_INVERTED : 0u) |
        (desc->depth_infinite ? FFX_UPSCALE_ENABLE_DEPTH_INFINITE : 0u) |
        (desc->dynamic_resolution ? FFX_UPSCALE_ENABLE_DYNAMIC_RESOLUTION : 0u) |
        (desc->motion_jittered ? FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION : 0u) |
        (desc->auto_exposure ? FFX_UPSCALE_ENABLE_AUTO_EXPOSURE : 0u);
    if (session->create(&session->context, &create.header, nullptr) != FFX_API_RETURN_OK) {
        sr_close(session); return RSF_BACKEND_ERROR_INIT_FAILED;
    }
    ffxQueryGetProviderVersion effective{};
    effective.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (session->query(&session->context, &effective.header) != FFX_API_RETURN_OK ||
        effective.versionId != session->version) {
        sr_close(session); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    if (desc->log) desc->log(desc->log_user, session->name);
    *out = session;
    return RSF_BACKEND_OK;
}
rsf_backend_result sr_plan(void* pointer, rsf_quality quality, uint32_t* width, uint32_t* height)
{
    auto* session = static_cast<FsrSession*>(pointer);
    if (!session || !width || !height || quality > RSF_QUALITY_ULTRA_PERFORMANCE)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    // FSR2's quality-query enum starts at QUALITY. A 1:1 dispatch is legal, but querying
    // NATIVEAA returns an SDK error. Explicit native sizing keeps this distinct from a ratio.
    if (quality == RSF_QUALITY_NATIVE && session->open.fsr_major == 2) {
        *width = session->open.output_width; *height = session->open.output_height;
        return RSF_BACKEND_OK;
    }
    ffxQueryDescUpscaleGetRenderResolutionFromQualityMode query{};
    query.header.type = FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE;
    query.displayWidth = session->open.output_width;
    query.displayHeight = session->open.output_height;
    query.qualityMode = quality;
    query.pOutRenderWidth = width; query.pOutRenderHeight = height;
    return session->query(&session->context, &query.header) == FFX_API_RETURN_OK ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
rsf_backend_result sr_evaluate(void* pointer, void* context, const rsf_sr_frame* frame)
{
    auto result = rsf::validate_frame(frame);
    if (result != RSF_BACKEND_OK) return result;
    if (!pointer || !context) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* session = static_cast<FsrSession*>(pointer);
    result = rsf::validate_d3d12_resources(*frame, session->open.device);
    if (result != RSF_BACKEND_OK) return result;
    const auto& record = *frame->record;
    ffxDispatchDescUpscale dispatch{};
    dispatch.header.type = FFX_API_DISPATCH_DESC_TYPE_UPSCALE;
    dispatch.commandList = context;
    dispatch.color = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->color.resource));
    dispatch.depth = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->depth.resource));
    dispatch.motionVectors = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->motion.resource));
    dispatch.exposure = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->exposure.resource));
    dispatch.output = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->output.resource),
                                          FFX_API_RESOURCE_STATE_UNORDERED_ACCESS);
    dispatch.jitterOffset = {frame->jitter_x, frame->jitter_y};
    dispatch.motionVectorScale = {frame->motion_scale_x, frame->motion_scale_y};
    dispatch.renderSize = {record.render_width, record.render_height};
    dispatch.upscaleSize = {record.output_width, record.output_height};
    dispatch.frameTimeDelta = record.frame_time_ms;
    dispatch.preExposure = frame->pre_exposure;
    dispatch.reset = frame->reset != 0;
    dispatch.cameraNear = record.camera.near_plane;
    dispatch.cameraFar = record.camera.far_plane > 0 ? record.camera.far_plane : FLT_MAX;
    dispatch.cameraFovAngleVertical = record.camera.vertical_fov_radians;
    dispatch.viewSpaceToMetersFactor = frame->view_space_to_meters;
    const auto code = session->dispatch(&session->context, &dispatch.header);
    return code == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
rsf_backend_result sr_release(void* pointer)
{
    // No independent release operation in FFX API. Closing requires GPU completion by the owner.
    return pointer ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
rsf_backend_result sr_version(void* pointer, uint64_t* id, const char** name)
{
    if (!pointer || !id || !name) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* session = static_cast<FsrSession*>(pointer);
    *id = session->version; *name = session->name;
    return RSF_BACKEND_OK;
}
