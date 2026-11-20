// SPDX-License-Identifier: MIT
// Presentation-bridge, D3D11, D3D12, shared-surface, SR and frame-dump operations for the Rust D3D11
// frame generation owner in native_fg.rs, and the stable exported entry points that pass this table.
// Each leaf performs one platform call. Resources, ordering, leases, admission, retirement and
// fallback are decided in Rust. Returned COM interfaces carry one reference for the caller.
#include "native_fg_platform.h"
#include <rescaleframe/input_dump.h>
#include <rescaleframe/named_frame_dump.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/shared_surface.h>
#include <rescaleframe/sr_session.h>
#include "../../backends/common/d3d12_helpers.h"
#include <d3d11_4.h>
#include <d3d12.h>
#include <dxgi.h>
#include <wrl/client.h>
using Microsoft::WRL::ComPtr;

static_assert(RSF_NFG_FORMAT_RGBA16_FLOAT == DXGI_FORMAT_R16G16B16A16_FLOAT);
static_assert(RSF_NFG_FORMAT_RG32_FLOAT == DXGI_FORMAT_R32G32_FLOAT);
static_assert(RSF_NFG_FORMAT_RGB10A2_UNORM == DXGI_FORMAT_R10G10B10A2_UNORM);
static_assert(RSF_NFG_FORMAT_RGBA8_UNORM == DXGI_FORMAT_R8G8B8A8_UNORM);
static_assert(RSF_NFG_FORMAT_RGBA8_UNORM_SRGB == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB);
static_assert(RSF_NFG_FORMAT_RG16_FLOAT == DXGI_FORMAT_R16G16_FLOAT);
static_assert(RSF_NFG_FORMAT_R32_FLOAT == DXGI_FORMAT_R32_FLOAT);
static_assert(RSF_NFG_FORMAT_BGRA8_UNORM == DXGI_FORMAT_B8G8R8A8_UNORM);
static_assert(RSF_NFG_FORMAT_BGRA8_UNORM_SRGB == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB);
static_assert(RSF_NFG_STATE_COMMON == D3D12_RESOURCE_STATE_COMMON);
static_assert(RSF_NFG_STATE_UNORDERED_ACCESS == D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
static_assert(RSF_NFG_STATE_NON_PIXEL_SHADER_RESOURCE == D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
static_assert(RSF_NFG_STATE_PRESENT == D3D12_RESOURCE_STATE_PRESENT);
static_assert(RSF_NFG_PHASE_UI_COMPLETE == RSF_PHASE_UI_COMPLETE);
static_assert(RSF_NFG_DUMP_VIEW_RAW == RSF_DUMP_VIEW_RAW && RSF_NFG_DUMP_VIEW_DEPTH == RSF_DUMP_VIEW_DEPTH &&
              RSF_NFG_DUMP_VIEW_DENSE_MOTION == RSF_DUMP_VIEW_DENSE_MOTION);
static_assert(RSF_NFG_SR_DLSS == RSF_SR_DLSS);
static_assert(RSF_NFG_FG_BACKEND_FSR3 == RSF_FG_BACKEND_FSR3 && RSF_NFG_FG_BACKEND_FSR4 == RSF_FG_BACKEND_FSR4);
static_assert(RSF_NFG_STAT_LATENCY_AVAILABLE == RSF_FG_STAT_LATENCY_AVAILABLE);
static_assert(RSF_NFG_PRESENT_TEST == DXGI_PRESENT_TEST);
static_assert(RSF_NFG_RESULT_OK == RSF_BACKEND_OK && RSF_NFG_RESULT_OK == RSF_DLSS_OK);
static_assert(RSF_NFG_RESULT_NOT_READY == RSF_BACKEND_ERROR_NOT_READY &&
              RSF_NFG_RESULT_NOT_READY == RSF_DLSS_ERROR_NOT_READY);
static_assert(RSF_NFG_RESULT_FEATURE_FAILED == RSF_BACKEND_ERROR_FEATURE_FAILED &&
              RSF_NFG_RESULT_FEATURE_FAILED == RSF_DLSS_ERROR_FEATURE_FAILED);

namespace {
template<class T> void* transfer(HRESULT result, T* object)
{
    if (SUCCEEDED(result)) return object;
    if (object) object->Release();
    return nullptr;
}

int32_t present_graphics(void** device12, void** queue12, void** native_device)
{
    rsf_streamline_graphics graphics{};
    graphics.struct_size = sizeof(graphics);
    if (!rsf_d3d11_present_graphics(&graphics)) return 0;
    *device12 = graphics.device;
    *queue12 = graphics.queue;
    *native_device = graphics.native_device;
    return 1;
}
int32_t present_has_owner() { return rsf_d3d11_present_has_owner(); }
int32_t present_is_owner(void* swapchain) { return rsf_d3d11_present_is_owner(swapchain); }
void* present_host() { return rsf_d3d11_present_host(); }
int32_t present_abort(uint64_t id) { return rsf_d3d11_present_abort(id); }
int32_t host_token(void* host, uint64_t id)
{
    return rsf_streamline_host_token(static_cast<rsf_streamline_host*>(host), id) != nullptr;
}
int32_t window_match(void* swapchain, rsf_game_render_pass* out) { return rsf_native_window_match(swapchain, out); }
int32_t scene_for_window(const rsf_game_render_pass* window, rsf_native_scene_identity* out)
{
    return rsf_native_scene_for_window(window, out);
}
int32_t allows_fg(const rsf_frame_record* record) { return rsf_frame_allows_fg(record); }
void* context_device(void* context)
{
    ID3D11Device* device = nullptr;
    static_cast<ID3D11DeviceContext*>(context)->GetDevice(&device);
    return device;
}
void* query_context4(void* context)
{
    ID3D11DeviceContext4* queried = nullptr;
    const HRESULT result = static_cast<ID3D11DeviceContext*>(context)->QueryInterface(IID_PPV_ARGS(&queried));
    return transfer(result, queried);
}
void* query_texture(void* object)
{
    ID3D11Texture2D* texture = nullptr;
    const HRESULT result = static_cast<IUnknown*>(object)->QueryInterface(IID_PPV_ARGS(&texture));
    return transfer(result, texture);
}
int32_t texture_info(void* texture, rsf_nfg_texture_info* out)
{
    D3D11_TEXTURE2D_DESC desc{};
    static_cast<ID3D11Texture2D*>(texture)->GetDesc(&desc);
    *out = {desc.Width, desc.Height, static_cast<uint32_t>(desc.Format), desc.SampleDesc.Count};
    return 1;
}
void* create_event() { return CreateEventW(nullptr, FALSE, FALSE, nullptr); }
void close_event(void* event) { CloseHandle(event); }
void* fence_create(void* device11, void* native_device)
{
    rsf_shared_fence* fence = nullptr;
    return rsf_shared_fence_create(device11, native_device, nullptr, nullptr, &fence) == RSF_SHARED_OK ? fence : nullptr;
}
void* fence_d3d11(void* fence) { return rsf_shared_fence_d3d11(static_cast<rsf_shared_fence*>(fence)); }
void* fence_d3d12(void* fence) { return rsf_shared_fence_d3d12(static_cast<rsf_shared_fence*>(fence)); }
void* surface_create(void* device11, void* native_device, uint32_t width, uint32_t height, uint32_t format)
{
    rsf_shared_surface_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
    setup.width = width; setup.height = height; setup.format = format;
    rsf_shared_surface* surface = nullptr;
    return rsf_shared_surface_create(device11, native_device, &setup, &surface) == RSF_SHARED_OK ? surface : nullptr;
}
void surface_destroy(void* surface) { rsf_shared_surface_destroy(static_cast<rsf_shared_surface*>(surface)); }
void* surface_d3d11(void* surface) { return rsf_shared_surface_d3d11(static_cast<rsf_shared_surface*>(surface)); }
void* surface_d3d12(void* surface) { return rsf_shared_surface_d3d12(static_cast<rsf_shared_surface*>(surface)); }
uint64_t fence_completed(void* fence12) { return static_cast<ID3D12Fence*>(fence12)->GetCompletedValue(); }
int32_t wait_fence(void* fence12, uint64_t value, void* event)
{
    return SUCCEEDED(rsf::wait_fence(static_cast<ID3D12Fence*>(fence12), value, event));
}
void flush(void* context4) { static_cast<ID3D11DeviceContext4*>(context4)->Flush(); }
void copy_resource(void* context4, void* destination, void* source)
{
    static_cast<ID3D11DeviceContext4*>(context4)->CopyResource(static_cast<ID3D11Resource*>(destination),
                                                              static_cast<ID3D11Resource*>(source));
}
int32_t context_signal(void* context4, void* fence11, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D11DeviceContext4*>(context4)->Signal(static_cast<ID3D11Fence*>(fence11), value));
}
int32_t context_wait(void* context4, void* fence11, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D11DeviceContext4*>(context4)->Wait(static_cast<ID3D11Fence*>(fence11), value));
}
int32_t create_commands(void* device12, void** allocator, void** list)
{
    ID3D12CommandAllocator* created_allocator = nullptr;
    ID3D12GraphicsCommandList* created_list = nullptr;
    if (!rsf::create_command_list(static_cast<ID3D12Device*>(device12), D3D12_COMMAND_LIST_TYPE_DIRECT,
                                  &created_allocator, &created_list)) return 0;
    *allocator = created_allocator;
    *list = created_list;
    return 1;
}
void* create_output(void* device12, uint32_t width, uint32_t height, uint32_t format)
{
    ID3D12Resource* output = nullptr;
    return rsf::create_uav_texture(static_cast<ID3D12Device*>(device12), width, height,
                                   static_cast<DXGI_FORMAT>(format), &output) ? output : nullptr;
}
int32_t reset_commands(void* allocator, void* list)
{
    auto* typed = static_cast<ID3D12CommandAllocator*>(allocator);
    return SUCCEEDED(typed->Reset()) &&
        SUCCEEDED(static_cast<ID3D12GraphicsCommandList*>(list)->Reset(typed, nullptr));
}
int32_t close_commands(void* list) { return SUCCEEDED(static_cast<ID3D12GraphicsCommandList*>(list)->Close()); }
void execute(void* queue, void* list)
{
    ID3D12CommandList* lists[]{static_cast<ID3D12GraphicsCommandList*>(list)};
    static_cast<ID3D12CommandQueue*>(queue)->ExecuteCommandLists(1, lists);
}
void transition(void* list, void* resource, uint32_t before, uint32_t after)
{
    rsf::transition(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(resource),
                    static_cast<D3D12_RESOURCE_STATES>(before), static_cast<D3D12_RESOURCE_STATES>(after));
}
void copy_transitioned(void* list, void* destination, uint32_t destination_before, uint32_t destination_after,
                       void* source, uint32_t source_before, uint32_t source_after)
{
    rsf::copy_transitioned(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(destination),
        static_cast<D3D12_RESOURCE_STATES>(destination_before), static_cast<D3D12_RESOURCE_STATES>(destination_after),
        static_cast<ID3D12Resource*>(source), static_cast<D3D12_RESOURCE_STATES>(source_before),
        static_cast<D3D12_RESOURCE_STATES>(source_after));
}
int32_t queue_signal(void* queue, void* fence12, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->Signal(static_cast<ID3D12Fence*>(fence12), value));
}
int32_t queue_wait(void* queue, void* fence12, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->Wait(static_cast<ID3D12Fence*>(fence12), value));
}
int32_t dlss_evaluate(void* context, const rsf_dlss_frame* frame) { return rsf_dlss_evaluate(context, frame); }
int32_t dlss_evaluate_shared(void* list, const rsf_dlss_frame* frame, uint64_t source)
{
    return rsf_dlss_evaluate_shared(list, frame, source);
}
void dump_sr(void* context, const rsf_dlss_frame* frame, uint32_t backend) { rsf_input_dump_sr(context, frame, backend); }
int32_t dump_named(void* context, void* texture, const char* directory, const char* name, uint32_t view, float scale,
                   rsf_backend_log_fn log, void* log_user)
{
    rsf_named_frame_dump request{};
    request.struct_size = sizeof(request);
    request.abi_version = RSF_NAMED_FRAME_DUMP_ABI_VERSION;
    request.directory_utf8 = directory; request.name = name; request.view = view; request.scale = scale;
    request.log = log; request.log_user = log_user;
    return rsf_input_dump_named_texture(context, texture, &request, nullptr);
}

constexpr rsf_nfg_platform platform{
    present_has_owner, present_is_owner, present_host, rsf_d3d11_present_provider, rsf_d3d11_present_session,
    rsf_d3d11_present_backend, rsf_d3d11_present_generation, present_graphics, rsf_d3d11_present_retire,
    present_abort, host_token, window_match, scene_for_window, rsf_native_scene_present_path, allows_fg,
    context_device, query_context4, query_texture, texture_info, create_event, close_event, fence_create,
    fence_d3d11, fence_d3d12, surface_create, surface_destroy, surface_d3d11, surface_d3d12,
    fence_completed, wait_fence, flush, copy_resource, context_signal, context_wait, create_commands, create_output,
    reset_commands, close_commands, execute, transition, copy_transitioned, queue_signal, queue_wait,
    dlss_evaluate, dlss_evaluate_shared, dump_sr, rsf_input_dump_fg, rsf_input_dump_fg_prepare, dump_named,
    &rsf_native_fg_cpu_platform,
};
}

extern "C" RSF_RUNTIME_API void rsf_native_fg_options_set(const rsf_native_fg_options* options)
{
    rsf_core_native_fg_options_set(options);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_set_log(rsf_backend_log_fn sink, void* user)
{
    rsf_core_native_fg_set_log(sink, user);
}
extern "C" RSF_RUNTIME_API int rsf_native_fg_status_get(rsf_native_fg_status* status)
{
    return rsf_core_native_fg_status_get(status, &platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_scene(const rsf_game_render_pass* pass)
{
    rsf_core_native_fg_scene(pass, &platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_final(void* context, const rsf_game_render_pass* pass)
{
    rsf_core_native_fg_final(context, pass, &platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_capture(void* context, const rsf_dlss_frame* frame)
{
    rsf_core_native_fg_capture(context, frame, &platform);
}
extern "C" RSF_RUNTIME_API rsf_dlss_result rsf_native_fg_evaluate(void* context, const rsf_dlss_frame* frame)
{
    return static_cast<rsf_dlss_result>(rsf_core_native_fg_evaluate(context, frame, &platform));
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_prepare(void*, void* chain, void*, void* list, void* buffer,
    rsf_streamline_host*, void* provider, uint32_t sync)
{
    rsf_core_native_fg_prepare(chain, list, buffer, provider, sync, &platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_retire(void*, void* provider)
{
    rsf_core_native_fg_retire(provider, &platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_present(const rsf_observer_present_event* event)
{
    rsf_core_native_fg_present(event, &platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_window_end(const rsf_game_render_pass* pass)
{
    rsf_core_native_fg_window_end(pass, &platform);
}
extern "C" RSF_RUNTIME_API int rsf_native_fg_inputs_ready(uint64_t source)
{
    return rsf_core_native_fg_inputs_ready(source);
}
extern "C" RSF_RUNTIME_API int32_t rsf_native_fg_dump_inputs(void* context, uint64_t source,
    const char* directory, const char* prefix)
{
    return rsf_core_native_fg_dump_inputs(context, source, directory, prefix, &platform);
}
