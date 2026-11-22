// SPDX-License-Identifier: MIT
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <ffx_framegeneration.h>
#include <dx12/ffx_api_framegeneration_dx12.h>
#include <cstring>
#include <new>
#include "bridge_platform.h"
#include "../../backends/common/d3d12_helpers.h"

extern "C" int32_t rsf_core_fsr4fg_sdk_versions(PfnFfxQuery, void*, rsf_ffx_version*, uint64_t*);
extern "C" int32_t rsf_core_fsr4fg_sdk_provider(PfnFfxQuery, ffxContext*, uint64_t*);
extern "C" void rsf_core_fsr4fg_sdk_prepare(const rsf_fsr4fg_frame*, uint64_t, uint32_t,
    ffxDispatchDescFrameGenerationPrepareV2*);

extern "C" void rsf_core_fsr4fg_sdk_generation(const rsf_fsr4fg_create*, uint64_t, uint32_t, uint32_t,
    ffxApiHeader*, ffxOverrideVersion*, ffxCreateContextDescFrameGenerationVersion*,
    ffxCreateContextDescFrameGeneration*);
extern "C" void rsf_core_fsr4fg_sdk_configure(void*, uint64_t, uint32_t, uint32_t, uint32_t,
    FfxApiFrameGenerationDispatchFunc, void*, ffxConfigureDescFrameGeneration*);

static_assert(FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION == 0x20001 &&
    FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION == 0x20002 &&
    FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2 == 0x2000c &&
    FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_VERSION == 0x2000e);
static_assert(sizeof(ffxOverrideVersion) == 24 && sizeof(ffxCreateContextDescFrameGenerationVersion) == 24 &&
    sizeof(ffxCreateContextDescFrameGeneration) == 40 && sizeof(ffxConfigureDescFrameGeneration) == 144 &&
    offsetof(ffxConfigureDescFrameGeneration, HUDLessColor) == 64);
static_assert(sizeof(ffxQueryDescGetVersions) == 56 && sizeof(ffxQueryGetProviderVersion) == 32);
static_assert(sizeof(ffxDispatchDescFrameGenerationPrepareV2) == 232 &&
    offsetof(ffxDispatchDescFrameGenerationPrepareV2, commandList) == 32 &&
    offsetof(ffxDispatchDescFrameGenerationPrepareV2, depth) == 88 &&
    offsetof(ffxDispatchDescFrameGenerationPrepareV2, cameraPosition) == 184);

struct rsf_fsr4fg_native {
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
    PfnFfxCreateContext create{};
    PfnFfxDestroyContext destroy{};
    PfnFfxConfigure configure{};
    PfnFfxDispatch dispatch{};
    PfnFfxQuery query{};
    // The SDK retains create descriptors until their contexts are destroyed.
    ffxOverrideVersion override_version{};
    ffxCreateBackendDX12Desc backend{};
    ffxCreateContextDescFrameGenerationVersion api_version{};
    ffxCreateContextDescFrameGeneration description{};
    ffxCreateContextDescFrameGenerationSwapChainVersionDX12 chain_version{};
    ffxCreateContextDescFrameGenerationSwapChainForHwndDX12 chain_description{};
    DXGI_SWAP_CHAIN_DESC1 dxgi_description{};
    void* telemetry{};
    rsf_fsr4fg_generated_fn generated{};
};

namespace {
using Native = rsf_fsr4fg_native;

static_assert(D3D12_RESOURCE_DIMENSION_TEXTURE2D == 3 && DXGI_FORMAT_B8G8R8A8_UNORM == 87 &&
    DXGI_FORMAT_R32_FLOAT == 41 && DXGI_FORMAT_R16G16_FLOAT == 34);
static_assert(sizeof(LUID) == sizeof(uint64_t));
static_assert(D3D_FEATURE_LEVEL_12_0 == 0xc000 && D3D12_COMMAND_LIST_TYPE_DIRECT == 0 &&
    D3D12_COMMAND_QUEUE_PRIORITY_NORMAL == 0 && D3D12_COMMAND_QUEUE_FLAG_NONE == 0 && D3D12_FENCE_FLAG_NONE == 0);
static_assert(DXGI_USAGE_RENDER_TARGET_OUTPUT == 0x20 && DXGI_SCALING_STRETCH == 0 &&
    DXGI_SWAP_EFFECT_FLIP_DISCARD == 4 && DXGI_ALPHA_MODE_UNSPECIFIED == 0);

int32_t result(HRESULT hr) { return FAILED(hr) ? hr : 0; }

template<class T> void release(T*& value) {
    if (value) {
        value->Release();
        value = nullptr;
    }
}

Native* allocate(void* telemetry, rsf_fsr4fg_generated_fn generated) {
    auto* self = new (std::nothrow) Native{};
    if (self) {
        self->telemetry = telemetry;
        self->generated = generated;
    }
    return self;
}

void release_native(Native* self) {
    release(self->commands);
    release(self->allocator);
    release(self->completion);
    release(self->shared);
    release(self->queue);
    release(self->device);
    release(self->adapter);
    release(self->factory);
    if (self->event) CloseHandle(self->event);
    if (self->module) FreeLibrary(self->module);
    delete self;
}

int32_t create_factory(Native* self, uint32_t flags) {
    return result(CreateDXGIFactory2(flags, IID_PPV_ARGS(&self->factory)));
}

int32_t find_adapter(Native* self, uint64_t adapter_luid) {
    LUID luid{};
    std::memcpy(&luid, &adapter_luid, sizeof(luid));
    return result(self->factory->EnumAdapterByLuid(luid, IID_PPV_ARGS(&self->adapter)));
}

int32_t get_adapter_vendor(Native* self, uint32_t* vendor) {
    DXGI_ADAPTER_DESC1 desc{};
    const HRESULT hr = self->adapter->GetDesc1(&desc);
    if (SUCCEEDED(hr)) *vendor = desc.VendorId;
    return result(hr);
}

int32_t create_device(Native* self, uint32_t feature_level) {
    return result(D3D12CreateDevice(self->adapter, static_cast<D3D_FEATURE_LEVEL>(feature_level),
        IID_PPV_ARGS(&self->device)));
}

int32_t load(Native* self, const uint16_t* path) {
    self->module = LoadLibraryExW(reinterpret_cast<const wchar_t*>(path), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!self->module) return HRESULT_FROM_WIN32(GetLastError());
    self->create = reinterpret_cast<PfnFfxCreateContext>(GetProcAddress(self->module, "ffxCreateContext"));
    self->destroy = reinterpret_cast<PfnFfxDestroyContext>(GetProcAddress(self->module, "ffxDestroyContext"));
    self->configure = reinterpret_cast<PfnFfxConfigure>(GetProcAddress(self->module, "ffxConfigure"));
    self->dispatch = reinterpret_cast<PfnFfxDispatch>(GetProcAddress(self->module, "ffxDispatch"));
    self->query = reinterpret_cast<PfnFfxQuery>(GetProcAddress(self->module, "ffxQuery"));
    return self->create && self->destroy && self->configure && self->dispatch && self->query ? 0 : E_NOINTERFACE;
}

int32_t versions(Native* self, rsf_ffx_version* out, uint64_t* count) {
    return rsf_core_fsr4fg_sdk_versions(self->query, self->device, out, count);
}

int32_t create_generation(Native* self, const rsf_fsr4fg_create* setup, uint64_t provider) {
    self->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
    self->backend.device = self->device;
    rsf_core_fsr4fg_sdk_generation(setup, provider, FFX_FRAMEGENERATION_VERSION,
        ffxApiGetSurfaceFormatDX12(DXGI_FORMAT_B8G8R8A8_UNORM), &self->backend.header,
        &self->override_version, &self->api_version, &self->description);
    return static_cast<int32_t>(self->create(&self->generation, &self->description.header, nullptr));
}

int32_t get_provider(Native* self, uint64_t* actual) {
    return rsf_core_fsr4fg_sdk_provider(self->query, &self->generation, actual);
}

int32_t create_queue(Native* self, const rsf_fsr4fg_queue_desc* setup) {
    D3D12_COMMAND_QUEUE_DESC queue{};
    queue.Type = static_cast<D3D12_COMMAND_LIST_TYPE>(setup->list_type);
    queue.Priority = setup->priority;
    queue.Flags = static_cast<D3D12_COMMAND_QUEUE_FLAGS>(setup->flags);
    queue.NodeMask = setup->node_mask;
    return result(self->device->CreateCommandQueue(&queue, IID_PPV_ARGS(&self->queue)));
}

int32_t create_allocator(Native* self, uint32_t list_type) {
    return result(self->device->CreateCommandAllocator(static_cast<D3D12_COMMAND_LIST_TYPE>(list_type),
        IID_PPV_ARGS(&self->allocator)));
}

int32_t create_command_list(Native* self, uint32_t node_mask, uint32_t list_type) {
    return result(self->device->CreateCommandList(node_mask, static_cast<D3D12_COMMAND_LIST_TYPE>(list_type),
        self->allocator, nullptr, IID_PPV_ARGS(&self->commands)));
}

int32_t create_completion_fence(Native* self, uint64_t initial_value, uint32_t flags) {
    return result(self->device->CreateFence(initial_value, static_cast<D3D12_FENCE_FLAGS>(flags),
        IID_PPV_ARGS(&self->completion)));
}

int32_t create_event(Native* self, uint32_t manual_reset, uint32_t initial_state) {
    self->event = CreateEventW(nullptr, manual_reset != 0, initial_state != 0, nullptr);
    return self->event ? 0 : HRESULT_FROM_WIN32(GetLastError());
}

uint32_t objects(Native* self) {
    uint32_t found = 0;
    if (self->device) found |= RSF_FSR4FG_DEVICE;
    if (self->generation) found |= RSF_FSR4FG_GENERATION;
    if (self->chain) found |= RSF_FSR4FG_CHAIN;
    if (self->swapchain) found |= RSF_FSR4FG_SWAPCHAIN;
    if (self->queue && self->completion && self->event) found |= RSF_FSR4FG_LOCAL_FENCE;
    if (self->shared) found |= RSF_FSR4FG_SHARED_FENCE;
    return found;
}

int32_t create_swapchain(Native* self, void* window, const rsf_fsr4fg_swapchain_desc* setup, uint32_t first_attempt) {
    auto& desc = self->dxgi_description;
    if (first_attempt) {
        desc.Stereo = setup->stereo != 0;
        desc.SampleDesc.Quality = setup->sample_quality;
        desc.Scaling = static_cast<DXGI_SCALING>(setup->scaling);
        desc.AlphaMode = static_cast<DXGI_ALPHA_MODE>(setup->alpha_mode);
        desc.Flags = setup->flags;
    }
    desc.Width = setup->width;
    desc.Height = setup->height;
    desc.Format = static_cast<DXGI_FORMAT>(setup->format);
    desc.SampleDesc.Count = setup->sample_count;
    desc.BufferUsage = setup->buffer_usage;
    desc.BufferCount = setup->buffer_count;
    desc.SwapEffect = static_cast<DXGI_SWAP_EFFECT>(setup->swap_effect);
    self->chain_version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_VERSION_DX12;
    self->chain_version.version = FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION;
    auto& create = self->chain_description;
    create.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12;
    create.header.pNext = &self->chain_version.header;
    create.swapchain = &self->chain;
    create.hwnd = static_cast<HWND>(window);
    create.desc = &desc;
    create.dxgiFactory = self->factory;
    create.gameQueue = self->queue;
    return static_cast<int32_t>(self->create(&self->swapchain, &create.header, nullptr));
}

int32_t create_image(Native* self, uint32_t width, uint32_t height, uint32_t format, void** handle, void** resource) {
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width;
    desc.Height = height;
    desc.DepthOrArraySize = desc.MipLevels = 1;
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.SampleDesc.Count = 1;
    ID3D12Resource* image{};
    HRESULT hr = self->device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED, &desc,
        D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&image));
    if (FAILED(hr)) return hr;
    hr = self->device->CreateSharedHandle(image, nullptr, GENERIC_ALL, nullptr, handle);
    if (FAILED(hr)) {
        image->Release();
        return hr;
    }
    *resource = image;
    return 0;
}

int32_t create_fence(Native* self, void** handle) {
    const HRESULT hr = self->device->CreateFence(0, D3D12_FENCE_FLAG_SHARED, IID_PPV_ARGS(&self->shared));
    if (FAILED(hr)) return hr;
    return self->device->CreateSharedHandle(self->shared, nullptr, GENERIC_ALL, nullptr, handle);
}

void describe_texture(void* resource, rsf_fsr4fg_texture* out) {
    const auto desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    *out = {desc.Width, desc.Height, static_cast<uint32_t>(desc.Format), static_cast<uint32_t>(desc.Dimension),
        desc.DepthOrArraySize, desc.SampleDesc.Count};
}

int32_t reset_commands(Native* self) {
    const HRESULT hr = self->allocator->Reset();
    return FAILED(hr) ? hr : result(self->commands->Reset(self->allocator, nullptr));
}

int32_t close_commands(Native* self) { return result(self->commands->Close()); }

ffxReturnCode_t generate(ffxDispatchDescFrameGeneration* desc, void* user) {
    auto* self = static_cast<Native*>(user);
    const auto code = self->dispatch(&self->generation, &desc->header);
    const auto count = code == FFX_API_RETURN_OK ? desc->numGeneratedFrames : 0u;
    self->generated(self->telemetry, static_cast<int32_t>(code), count);
    return code;
}

int32_t configure(Native* self, uint64_t sequence, uint32_t width, uint32_t height, uint32_t enabled, void* hudless) {
    ffxConfigureDescFrameGeneration config{};
    rsf_core_fsr4fg_sdk_configure(self->chain, sequence, width, height, enabled, generate, self, &config);
    if (enabled) {
        config.HUDLessColor = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(hudless), FFX_API_RESOURCE_STATE_COMPUTE_READ);
    }
    return static_cast<int32_t>(self->configure(&self->generation, &config.header));
}

void shader_read(Native* self, void* resource, uint32_t enter) {
    constexpr auto read = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    constexpr auto common = D3D12_RESOURCE_STATE_COMMON;
    rsf::transition(self->commands, static_cast<ID3D12Resource*>(resource), enter ? common : read, enter ? read : common);
}

int32_t prepare(Native* self, const rsf_fsr4fg_frame* frame, uint64_t sequence, uint32_t reset) {
    ffxDispatchDescFrameGenerationPrepareV2 desc{};
    rsf_core_fsr4fg_sdk_prepare(frame, sequence, reset, &desc);
    desc.commandList = self->commands;
    desc.depth = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->depth), FFX_API_RESOURCE_STATE_COMPUTE_READ);
    desc.motionVectors = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->motion), FFX_API_RESOURCE_STATE_COMPUTE_READ);
    return static_cast<int32_t>(self->dispatch(&self->generation, &desc.header));
}

int32_t copy_to_backbuffer(Native* self, void* color) {
    ID3D12Resource* backbuffer{};
    const HRESULT hr = self->chain->GetBuffer(self->chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&backbuffer));
    if (FAILED(hr)) return hr;
    rsf::copy_transitioned(self->commands, backbuffer, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_PRESENT,
        static_cast<ID3D12Resource*>(color), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COMMON);
    backbuffer->Release();
    return 0;
}

int32_t wait_shared(Native* self, uint64_t value) { return result(self->queue->Wait(self->shared, value)); }

void execute(Native* self) {
    ID3D12CommandList* lists[] = {self->commands};
    self->queue->ExecuteCommandLists(1, lists);
}

int32_t present(Native* self, uint32_t vsync) { return result(self->chain->Present(vsync, 0)); }

int32_t wait_presents(Native* self) {
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsDX12 wait{};
    wait.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_WAIT_FOR_PRESENTS_DX12;
    return static_cast<int32_t>(self->dispatch(&self->swapchain, &wait.header));
}

int32_t signal(Native* self, uint64_t value, uint32_t shared) {
    return result(self->queue->Signal(shared ? self->shared : self->completion, value));
}

int32_t wait_local(Native* self, uint64_t value) {
    uint64_t completed = self->completion->GetCompletedValue();
    if (completed == UINT64_MAX) return DXGI_ERROR_DEVICE_REMOVED;
    if (completed >= value) return 0;
    const HRESULT hr = self->completion->SetEventOnCompletion(value, self->event);
    if (FAILED(hr)) return hr;
    if (WaitForSingleObject(self->event, 10000) != WAIT_OBJECT_0) return HRESULT_FROM_WIN32(WAIT_TIMEOUT);
    // A retry can consume an event from an earlier wait that timed out.
    completed = self->completion->GetCompletedValue();
    if (completed == UINT64_MAX) return DXGI_ERROR_DEVICE_REMOVED;
    return completed >= value ? 0 : HRESULT_FROM_WIN32(WAIT_TIMEOUT);
}

int32_t get_present_count(Native* self, uint32_t* count) { return result(self->chain->GetLastPresentCount(count)); }

int32_t destroy_context(Native* self, ffxContext& context) {
    const auto code = self->destroy(&context, nullptr);
    if (!code) context = nullptr;
    return static_cast<int32_t>(code);
}
int32_t destroy_generation(Native* self) { return destroy_context(self, self->generation); }
void release_chain(Native* self) { release(self->chain); }
int32_t destroy_swapchain(Native* self) { return destroy_context(self, self->swapchain); }

const rsf_fsr4fg_platform platform{
    allocate, release_native, create_factory, find_adapter, get_adapter_vendor, create_device,
    load, versions, create_generation, get_provider, create_queue, create_allocator,
    create_command_list, create_completion_fence, create_event, objects, create_swapchain,
    create_image, create_fence, describe_texture, reset_commands,
    close_commands, configure, shader_read, prepare, copy_to_backbuffer, wait_shared, execute, present,
    wait_presents, signal, wait_local, get_present_count, destroy_generation, release_chain, destroy_swapchain,
};
}

extern "C" int32_t RsfFsr4FgProbe(uint64_t luid, const uint16_t* directory, rsf_fsr4fg_info* info) {
    return rsf_core_fsr4fg_probe(&platform, luid, directory, info);
}
extern "C" int32_t RsfFsr4FgCreate(const rsf_fsr4fg_create* setup, rsf_fsr4fg_bridge** output) {
    return rsf_core_fsr4fg_create(&platform, setup, output);
}
extern "C" void RsfFsr4FgReleaseImage(void* resource) {
    if (resource) static_cast<ID3D12Resource*>(resource)->Release();
}
