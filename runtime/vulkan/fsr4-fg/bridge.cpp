// SPDX-License-Identifier: GPL-3.0-only
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <ffx_framegeneration.h>
#include <dx12/ffx_api_framegeneration_dx12.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <new>
#include "bridge_api.h"
#include "bridge_contract.h"
#include "../../backends/common/d3d12_helpers.h"

struct rsf_fsr4fg_bridge {
    HMODULE module{};
    IDXGIFactory4* factory{};
    IDXGIAdapter1* adapter{};
    ID3D12Device* device{};
    ID3D12CommandQueue* queue{};
    ID3D12CommandAllocator* allocator{};
    ID3D12GraphicsCommandList* commands{};
    ID3D12Fence* completion{};
    ID3D12Fence* shared{};
    IDXGISwapChain4* chain{};
    HANDLE event{};
    ffxContext generation{}, swapchain{};
    PfnFfxCreateContext create{}; PfnFfxDestroyContext destroy{};
    PfnFfxConfigure configure{}; PfnFfxDispatch dispatch{}; PfnFfxQuery query{};
    // SDK contract retains all create-descriptor pointers through destruction.
    ffxOverrideVersion override_version{};
    ffxCreateBackendDX12Desc backend{};
    ffxCreateContextDescFrameGenerationVersion api_version{};
    ffxCreateContextDescFrameGeneration description{};
    ffxCreateContextDescFrameGenerationSwapChainVersionDX12 chain_version{};
    ffxCreateContextDescFrameGenerationSwapChainForHwndDX12 chain_description{};
    DXGI_SWAP_CHAIN_DESC1 dxgi_description{};
    uint64_t luid{}, provider{}, sequence{}, completed{}, done{}, last_source{};
    uint32_t vendor{}, width{}, height{}, last_present_count{};
    int32_t terminal{};
    bool history{}, prepared_destroy{}, configured{}, enabled{}, admitted{};
    std::atomic<uint64_t> generated{0};
    std::atomic<int32_t> generated_result{0};
};
namespace {
template<class T> void release(T*& value) { if (value) { value->Release(); value = nullptr; } }
bool absolute(const uint16_t* path) {
    return path && path[0] && ((path[1] == ':' && (path[2] == '\\' || path[2] == '/')) ||
        (path[0] == '\\' && path[1] == '\\'));
}
int wait_local(rsf_fsr4fg_bridge& self) {
    if (!self.completed) return 0;
    const uint64_t done = self.completion->GetCompletedValue();
    if (done == UINT64_MAX) return static_cast<int32_t>(DXGI_ERROR_DEVICE_REMOVED);
    if (done >= self.completed) return 0;
    HRESULT hr = self.completion->SetEventOnCompletion(self.completed, self.event);
    if (FAILED(hr)) return hr;
    return WaitForSingleObject(self.event, 10000) == WAIT_OBJECT_0 ? 0 : static_cast<int32_t>(HRESULT_FROM_WIN32(WAIT_TIMEOUT));
}
int signal_local(rsf_fsr4fg_bridge& self) {
    const auto value = self.completed + 1;
    HRESULT hr = self.queue->Signal(self.completion, value);
    if (FAILED(hr)) return self.terminal = hr;
    self.completed = value;
    return 0;
}
int wait_presents(rsf_fsr4fg_bridge& self) {
    if (!self.swapchain) return 0;
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsDX12 wait{};
    wait.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_WAIT_FOR_PRESENTS_DX12;
    return static_cast<int32_t>(self.dispatch(&self.swapchain, &wait.header));
}
ffxReturnCode_t generate(ffxDispatchDescFrameGeneration* desc, void* user) {
    auto& self = *static_cast<rsf_fsr4fg_bridge*>(user);
    const auto result = self.dispatch(&self.generation, &desc->header);
    self.generated_result = static_cast<int32_t>(result);
    if (result == FFX_API_RETURN_OK) self.generated += desc->numGeneratedFrames;
    return result;
}
int configure(rsf_fsr4fg_bridge& self, bool enabled, ID3D12Resource* hudless) {
    ffxConfigureDescFrameGeneration config{};
    config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = self.chain; config.frameID = ++self.sequence;
    config.frameGenerationEnabled = enabled;
    config.frameGenerationCallback = generate; config.frameGenerationCallbackUserContext = &self;
    config.generationRect = {0, 0, static_cast<int32_t>(self.width), static_cast<int32_t>(self.height)};
    if (enabled) config.HUDLessColor = ffxApiGetResourceDX12(hudless, FFX_API_RESOURCE_STATE_COMPUTE_READ);
    const auto code = self.configure(&self.generation, &config.header);
    if (!code) {
        self.configured = true; self.enabled = enabled;
        if (!enabled) self.generated_result = 0;
    }
    return static_cast<int32_t>(code);
}
int handoff(rsf_fsr4fg_bridge& self, const rsf_fsr4fg_frame& frame, int result) {
    if (!frame.ready_value || frame.done_value <= frame.ready_value || frame.done_value <= self.done || !self.shared) return result;
    HRESULT hr = self.queue->Wait(self.shared, frame.ready_value);
    if (FAILED(hr)) return self.terminal = hr;
    hr = self.queue->Signal(self.shared, frame.done_value);
    if (FAILED(hr)) return self.terminal = hr;
    self.done = frame.done_value;
    const int completion = signal_local(self);
    return completion ? completion : result;
}
bool resource(void* pointer, DXGI_FORMAT format, uint32_t width, uint32_t height) {
    if (!pointer) return false;
    const auto desc = static_cast<ID3D12Resource*>(pointer)->GetDesc();
    return desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D && desc.Format == format &&
        desc.Width == width && desc.Height == height && desc.DepthOrArraySize == 1 && desc.SampleDesc.Count == 1;
}
bool temporal(const rsf_fsr4fg_bridge& self, const rsf_fsr4fg_frame& frame) {
    if (!frame.record || frame.record->struct_size < sizeof(*frame.record) ||
        frame.record->abi_version != RSF_GAME_FRAME_ABI_VERSION) return false;
    const auto& record = *frame.record; const auto& camera = record.camera;
    if (camera.struct_size < sizeof(camera) || camera.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        !record.session_id || !record.frame_id || record.output_width != self.width || record.output_height != self.height ||
        !record.render_width || !record.render_height || record.render_width > self.width || record.render_height > self.height ||
        !std::isfinite(record.frame_time_ms) || record.frame_time_ms <= 0 ||
        !std::isfinite(frame.motion_scale_x) || !std::isfinite(frame.motion_scale_y) || !frame.motion_scale_x || !frame.motion_scale_y ||
        !std::isfinite(camera.near_plane) || camera.near_plane <= 0 || !std::isfinite(camera.far_plane) || camera.far_plane < 0 ||
        (!(self.description.flags & FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE) && camera.far_plane <= camera.near_plane) ||
        !std::isfinite(camera.vertical_fov_radians) || camera.vertical_fov_radians <= 0 || camera.vertical_fov_radians >= 3.141593f ||
        !std::isfinite(camera.jitter_pixels[0]) || !std::isfinite(camera.jitter_pixels[1]) ||
        bool(camera.depth_inverted) != bool(self.description.flags & FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED) ||
        bool(camera.motion_jittered) != bool(self.description.flags & FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION)) return false;
    for (uint32_t i = 0; i < 16; ++i)
        if (!std::isfinite(camera.view_to_world[i]) || !std::isfinite(camera.view_to_clip[i])) return false;
    return resource(frame.depth, DXGI_FORMAT_R32_FLOAT, record.render_width, record.render_height) &&
        resource(frame.motion, DXGI_FORMAT_R16G16_FLOAT, record.render_width, record.render_height) &&
        resource(frame.hudless, DXGI_FORMAT_B8G8R8A8_UNORM, self.width, self.height) &&
        frame.depth != frame.motion && frame.depth != frame.hudless && frame.motion != frame.hudless &&
        frame.color != frame.depth && frame.color != frame.motion && frame.color != frame.hudless;
}
int admit(rsf_fsr4fg_bridge& owner, const uint16_t* directory) {
    auto* self = &owner;
        HRESULT hr = CreateDXGIFactory2(0, IID_PPV_ARGS(&self->factory));
        if (FAILED(hr)) { return hr; }
        LUID luid{}; std::memcpy(&luid, &self->luid, sizeof(luid));
        hr = self->factory->EnumAdapterByLuid(luid, IID_PPV_ARGS(&self->adapter));
        if (FAILED(hr)) { return hr; }
        DXGI_ADAPTER_DESC1 desc{}; hr = self->adapter->GetDesc1(&desc);
        if (FAILED(hr)) { return hr; } self->vendor = desc.VendorId;
        hr = D3D12CreateDevice(self->adapter, D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&self->device));
        if (FAILED(hr)) { return hr; }
        wchar_t path[32768]{};
        const auto* source = reinterpret_cast<const wchar_t*>(directory);
        const size_t length = wcsnlen(source, 32700);
        if (!length || length >= 32700) return RSF_FSR4FG_INVALID_ARGUMENT;
        std::memcpy(path, source, length * sizeof(wchar_t));
        size_t end = length;
        if (path[end - 1] != L'/' && path[end - 1] != L'\\') path[end++] = L'\\';
        constexpr wchar_t filename[] = L"amd_fidelityfx_framegeneration_dx12.dll";
        std::memcpy(path + end, filename, sizeof(filename));
        self->module = LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        if (!self->module) { return static_cast<int32_t>(HRESULT_FROM_WIN32(GetLastError())); }
        self->create = reinterpret_cast<PfnFfxCreateContext>(GetProcAddress(self->module, "ffxCreateContext"));
        self->destroy = reinterpret_cast<PfnFfxDestroyContext>(GetProcAddress(self->module, "ffxDestroyContext"));
        self->configure = reinterpret_cast<PfnFfxConfigure>(GetProcAddress(self->module, "ffxConfigure"));
        self->dispatch = reinterpret_cast<PfnFfxDispatch>(GetProcAddress(self->module, "ffxDispatch"));
        self->query = reinterpret_cast<PfnFfxQuery>(GetProcAddress(self->module, "ffxQuery"));
        if (!self->create || !self->destroy || !self->configure || !self->dispatch || !self->query) { return static_cast<int32_t>(E_NOINTERFACE); }
    return rsf::fsr4fg::discover(self->query, self->device, self->provider);
}

}

extern "C" int32_t RsfFsr4FgProbe(uint64_t luid, const uint16_t* directory, rsf_fsr4fg_info* info) {
    if (!info) return RSF_FSR4FG_INVALID_ARGUMENT;
    const int check = rsf_fsr4fg_check(info->struct_size, info->abi_version, sizeof(*info));
    if (check) return check;
    info->adapter_luid = info->provider_version = 0; info->vendor_id = info->max_generated = 0;
    if (!luid || !absolute(directory)) return RSF_FSR4FG_INVALID_ARGUMENT;
    auto* owner = new (std::nothrow) rsf_fsr4fg_bridge{};
    if (!owner) return E_OUTOFMEMORY;
    owner->luid = luid;
    int result = admit(*owner, directory);
    if (!result) result = RsfFsr4FgInfo(owner, info);
    const int cleanup = RsfFsr4FgDestroy(owner);
    return cleanup ? cleanup : result;
}
extern "C" int32_t RsfFsr4FgCreate(const rsf_fsr4fg_create* setup, rsf_fsr4fg_bridge** output) {
    if (!output) return RSF_FSR4FG_INVALID_ARGUMENT;
    *output = nullptr;
    if (!setup) return RSF_FSR4FG_INVALID_ARGUMENT;
    const int32_t check = rsf_fsr4fg_check(setup->struct_size, setup->abi_version, sizeof(*setup));
    if (check) return check;
    if (!absolute(setup->runtime_directory) || !setup->adapter_luid || !setup->width || !setup->height ||
        setup->width > INT32_MAX || setup->height > INT32_MAX || setup->generated_count != 1 ||
        setup->depth_inverted > 1 || setup->depth_infinite > 1 || setup->motion_jittered > 1) return RSF_FSR4FG_INVALID_ARGUMENT;
    auto* self = new (std::nothrow) rsf_fsr4fg_bridge{};
    if (!self) return E_OUTOFMEMORY;
    self->luid = setup->adapter_luid; self->width = setup->width; self->height = setup->height;
    int result = 0;
    do {
        result = admit(*self, setup->runtime_directory);
        if (result) break;
        HRESULT hr;
        self->override_version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION; self->override_version.versionId = self->provider;
        self->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
        self->backend.header.pNext = &self->override_version.header; self->backend.device = self->device;
        self->api_version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_VERSION;
        self->api_version.header.pNext = &self->backend.header; self->api_version.version = FFX_FRAMEGENERATION_VERSION;
        self->description.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
        self->description.header.pNext = &self->api_version.header;
        self->description.displaySize = self->description.maxRenderSize = {self->width, self->height};
        self->description.backBufferFormat = ffxApiGetSurfaceFormatDX12(DXGI_FORMAT_B8G8R8A8_UNORM);
        self->description.flags = (setup->depth_inverted ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED : 0u) |
            (setup->depth_infinite ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE : 0u) |
            (setup->motion_jittered ? FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION : 0u);
        result = static_cast<int32_t>(self->create(&self->generation, &self->description.header, nullptr));
        if (result) break;
        if (!rsf::fsr4fg::verify(self->query, &self->generation, self->provider)) { result = RSF_FSR4FG_NOT_SUPPORTED; break; }
        self->admitted = true;
        D3D12_COMMAND_QUEUE_DESC queue{}; queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        hr = self->device->CreateCommandQueue(&queue, IID_PPV_ARGS(&self->queue));
        if (FAILED(hr)) { result = hr; break; }
        hr = self->device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&self->allocator));
        if (FAILED(hr)) { result = hr; break; }
        hr = self->device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, self->allocator, nullptr, IID_PPV_ARGS(&self->commands));
        if (FAILED(hr)) { result = hr; break; }
        hr = self->commands->Close(); if (FAILED(hr)) { result = hr; break; }
        hr = self->device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&self->completion));
        if (FAILED(hr)) { result = hr; break; }
        self->event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!self->event) { result = HRESULT_FROM_WIN32(GetLastError()); break; }
    } while (false);
    if (result) { if (RsfFsr4FgDestroy(self)) *output = self; return result; }
    *output = self; return 0;
}
extern "C" int32_t RsfFsr4FgInfo(const rsf_fsr4fg_bridge* self, rsf_fsr4fg_info* out) {
    if (!self || !out) return RSF_FSR4FG_INVALID_ARGUMENT;
    const int check = rsf_fsr4fg_check(out->struct_size, out->abi_version, sizeof(*out)); if (check) return check;
    out->adapter_luid = self->luid; out->provider_version = self->provider; out->vendor_id = self->vendor; out->max_generated = 1;
    return 0;
}
extern "C" int32_t RsfFsr4FgStart(rsf_fsr4fg_bridge* self, void* window) {
    if (!self || !window || !self->generation || !self->admitted || self->chain || self->swapchain || self->terminal || self->prepared_destroy) return RSF_FSR4FG_INVALID_ARGUMENT;
    auto& desc = self->dxgi_description;
    desc.Width = self->width; desc.Height = self->height; desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 3; desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    self->chain_version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_VERSION_DX12;
    self->chain_version.version = FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION;
    auto& create = self->chain_description;
    create.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12;
    create.header.pNext = &self->chain_version.header;
    create.swapchain = &self->chain; create.hwnd = static_cast<HWND>(window);
    create.desc = &desc; create.dxgiFactory = self->factory; create.gameQueue = self->queue;
    const int code = static_cast<int32_t>(self->create(&self->swapchain, &create.header, nullptr));
    return code ? code : self->chain ? 0 : static_cast<int32_t>(E_UNEXPECTED);
}
extern "C" int32_t RsfFsr4FgCreateImage(void* user, uint32_t width, uint32_t height, uint32_t format, void** handle, void** resource) {
    if (!handle || !resource) return RSF_FSR4FG_INVALID_ARGUMENT;
    *handle = nullptr; *resource = nullptr;
    auto* self = static_cast<rsf_fsr4fg_bridge*>(user);
    if (!self || !self->device || !width || !height || self->prepared_destroy) return RSF_FSR4FG_INVALID_ARGUMENT;
    const auto dxgi = format == 44 ? DXGI_FORMAT_B8G8R8A8_UNORM : format == 100 ? DXGI_FORMAT_R32_FLOAT :
        format == 83 ? DXGI_FORMAT_R16G16_FLOAT : DXGI_FORMAT_UNKNOWN;
    if (dxgi == DXGI_FORMAT_UNKNOWN) return RSF_FSR4FG_INVALID_ARGUMENT;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width; desc.Height = height; desc.DepthOrArraySize = desc.MipLevels = 1;
    desc.Format = dxgi; desc.SampleDesc.Count = 1;
    ID3D12Resource* image{};
    HRESULT hr = self->device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED, &desc, D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&image));
    if (FAILED(hr)) return hr;
    hr = self->device->CreateSharedHandle(image, nullptr, GENERIC_ALL, nullptr, handle);
    if (FAILED(hr)) { image->Release(); return hr; }
    *resource = image; return 0;
}
extern "C" void RsfFsr4FgReleaseImage(void* resource) { if (resource) static_cast<ID3D12Resource*>(resource)->Release(); }
extern "C" int32_t RsfFsr4FgCreateFence(void* user, void** handle) {
    if (!handle) return RSF_FSR4FG_INVALID_ARGUMENT; *handle = nullptr;
    auto* self = static_cast<rsf_fsr4fg_bridge*>(user);
    if (!self || self->shared || self->prepared_destroy) return RSF_FSR4FG_INVALID_ARGUMENT;
    HRESULT hr = self->device->CreateFence(0, D3D12_FENCE_FLAG_SHARED, IID_PPV_ARGS(&self->shared));
    if (FAILED(hr)) return hr;
    return self->device->CreateSharedHandle(self->shared, nullptr, GENERIC_ALL, nullptr, handle);
}
extern "C" int32_t RsfFsr4FgPresent(rsf_fsr4fg_bridge* self, const rsf_fsr4fg_frame* frame, rsf_fsr4fg_present_status* status) {
    if (!self || !frame || !status) return RSF_FSR4FG_INVALID_ARGUMENT;
    int code = rsf_fsr4fg_check(frame->struct_size, frame->abi_version, sizeof(*frame)); if (code) return code;
    code = rsf_fsr4fg_check(status->struct_size, status->abi_version, sizeof(*status)); if (code) return code;
    struct TerminalStatus {
        rsf_fsr4fg_bridge& owner; rsf_fsr4fg_present_status& status;
        ~TerminalStatus() { status.terminal = owner.terminal ? 1u : 0u; }
    } terminal_status{*self, *status};
    status->frames_presented = status->fg_enabled = 0; status->result = 0; status->generated_callbacks = self->generated.load();
    if (self->terminal) return self->terminal;
    if (!self->chain || !self->shared || self->prepared_destroy || frame->enabled > 1 || frame->vsync > 1 ||
        !frame->ready_value || frame->ready_value <= self->done || frame->done_value <= frame->ready_value ||
        !resource(frame->color, DXGI_FORMAT_B8G8R8A8_UNORM, self->width, self->height) ||
        (frame->enabled && !temporal(*self, *frame))) return handoff(*self, *frame, RSF_FSR4FG_INVALID_ARGUMENT);
    code = wait_local(*self); if (code) return self->terminal = code;
    HRESULT hr = self->allocator->Reset(); if (FAILED(hr)) return handoff(*self, *frame, hr);
    hr = self->commands->Reset(self->allocator, nullptr); if (FAILED(hr)) return handoff(*self, *frame, hr);
    auto* color = static_cast<ID3D12Resource*>(frame->color);
    auto* depth = static_cast<ID3D12Resource*>(frame->depth);
    auto* motion = static_cast<ID3D12Resource*>(frame->motion);
    auto* hudless = static_cast<ID3D12Resource*>(frame->hudless);
    constexpr auto read = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    const bool enabled = frame->enabled != 0;
    // Every frame, including resets, preserves the SDK-required configure order.
    code = configure(*self, enabled, hudless);
    if (code) { self->commands->Close(); return handoff(*self, *frame, code); }
    if (enabled) {
        for (auto* input : {depth, motion, hudless}) rsf::transition(self->commands, input, D3D12_RESOURCE_STATE_COMMON, read);
        ffxDispatchDescFrameGenerationPrepareV2 prepare{};
        rsf::fsr4fg::prepare_constants(*frame, self->sequence, self->history, self->last_source, prepare);
        prepare.commandList = self->commands;
        prepare.depth = ffxApiGetResourceDX12(depth, FFX_API_RESOURCE_STATE_COMPUTE_READ);
        prepare.motionVectors = ffxApiGetResourceDX12(motion, FFX_API_RESOURCE_STATE_COMPUTE_READ);
        code = static_cast<int32_t>(self->dispatch(&self->generation, &prepare.header));
        if (code) { self->commands->Close(); self->history = false; return handoff(*self, *frame, code); }
        for (auto* input : {depth, motion}) rsf::transition(self->commands, input, read, D3D12_RESOURCE_STATE_COMMON);
    }
    ID3D12Resource* backbuffer{};
    hr = self->chain->GetBuffer(self->chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&backbuffer));
    if (FAILED(hr)) { self->commands->Close(); return handoff(*self, *frame, hr); }
    rsf::copy_transitioned(self->commands, backbuffer, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_PRESENT,
        color, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COMMON);
    backbuffer->Release(); hr = self->commands->Close(); if (FAILED(hr)) return handoff(*self, *frame, hr);
    hr = self->queue->Wait(self->shared, frame->ready_value); if (FAILED(hr)) return self->terminal = hr;
    ID3D12CommandList* lists[] = {self->commands}; self->queue->ExecuteCommandLists(1, lists);
    const HRESULT presented = self->chain->Present(frame->vsync ? 1 : 0, 0);
    code = wait_presents(*self);
    if (!code) code = signal_local(*self);
    if (!code) code = wait_local(*self);
    if (code) return self->terminal = code; // SDK may still borrow shared HUD-less.
    if (enabled) {
        hr = self->allocator->Reset(); if (SUCCEEDED(hr)) hr = self->commands->Reset(self->allocator, nullptr);
        if (FAILED(hr)) return self->terminal = hr;
        rsf::transition(self->commands, hudless, read, D3D12_RESOURCE_STATE_COMMON);
        hr = self->commands->Close(); if (FAILED(hr)) return self->terminal = hr;
        self->queue->ExecuteCommandLists(1, lists);
    }
    self->history = enabled && SUCCEEDED(presented);
    if (frame->record) self->last_source = frame->record->frame_id;
    const int result = handoff(*self, *frame, FAILED(presented) ? presented : self->generated_result.load());
    UINT count = 0;
    if (SUCCEEDED(self->chain->GetLastPresentCount(&count))) { status->frames_presented = count - self->last_present_count; self->last_present_count = count; }
    status->fg_enabled = enabled ? 1u : 0u; status->generated_callbacks = self->generated.load(); status->result = result;
    return result;
}
extern "C" int32_t RsfFsr4FgDrain(rsf_fsr4fg_bridge* self) {
    if (!self) return 0;
    if (self->terminal) return self->terminal;
    int result = wait_presents(*self);
    if (!result && self->queue && self->completion) result = signal_local(*self);
    if (!result) result = wait_local(*self);
    return result;
}
extern "C" int32_t RsfFsr4FgPrepareDestroy(rsf_fsr4fg_bridge* self) {
    if (!self || self->prepared_destroy) return 0;
    if (self->terminal) return self->terminal;
    if (self->chain && self->configured) { const int code = configure(*self, false, nullptr); if (code) return code; }
    int code = RsfFsr4FgDrain(self); if (code) return code;
    if (self->generation) { code = static_cast<int32_t>(self->destroy(&self->generation, nullptr)); if (code) return code; }
    release(self->chain);
    if (self->swapchain) { code = static_cast<int32_t>(self->destroy(&self->swapchain, nullptr)); if (code) return code; }
    self->prepared_destroy = true; return 0;
}
extern "C" int32_t RsfFsr4FgDestroy(rsf_fsr4fg_bridge* self) {
    if (!self) return 0;
    const int code = RsfFsr4FgPrepareDestroy(self); if (code) return code;
    release(self->commands); release(self->allocator); release(self->completion); release(self->shared);
    release(self->queue); release(self->device); release(self->adapter); release(self->factory);
    if (self->event) CloseHandle(self->event);
    if (self->module) FreeLibrary(self->module);
    delete self; return 0;
}
