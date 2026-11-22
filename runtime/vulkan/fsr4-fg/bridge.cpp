// SPDX-License-Identifier: MIT
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <ffx_framegeneration.h>
#include <dx12/ffx_api_framegeneration_dx12.h>
#include <new>
#include "bridge_platform.h"
#include "../../backends/common/d3d12_helpers.h"

extern "C" void rsf_core_fsr4fg_dxgi_description(DXGI_SWAP_CHAIN_DESC1*, const rsf_fsr4fg_swapchain_desc*, uint32_t);
static_assert(sizeof(DXGI_SWAP_CHAIN_DESC1) == 48 &&
    offsetof(DXGI_SWAP_CHAIN_DESC1, SampleDesc) == 16 &&
    offsetof(DXGI_SWAP_CHAIN_DESC1, SwapEffect) == 36);

extern "C" int32_t rsf_core_fsr4fg_shared_image(void*, uint32_t, uint32_t, uint32_t, void**, void**);
extern "C" int32_t rsf_core_fsr4fg_create_factory(uint32_t, void**);
extern "C" int32_t rsf_core_fsr4fg_find_adapter(void*, uint64_t, void**);
extern "C" int32_t rsf_core_fsr4fg_get_adapter_vendor(void*, uint32_t*);
extern "C" int32_t rsf_core_fsr4fg_create_device(void*, uint32_t, void**);
extern "C" int32_t rsf_core_fsr4fg_create_event(uint32_t, uint32_t, void**);
extern "C" int32_t rsf_core_fsr4fg_create_queue(void*, const rsf_fsr4fg_queue_desc*, void**);
extern "C" int32_t rsf_core_fsr4fg_create_allocator(void*, uint32_t, void**);
extern "C" int32_t rsf_core_fsr4fg_create_list(void*, void*, uint32_t, uint32_t, void**);
extern "C" int32_t rsf_core_fsr4fg_create_completion_fence(void*, uint64_t, uint32_t, void**);
extern "C" int32_t rsf_core_fsr4fg_shared_fence(void*, void**, void**);
extern "C" int32_t rsf_core_fsr4fg_reset_commands(void*, void*);
extern "C" int32_t rsf_core_fsr4fg_close_commands(void*);
extern "C" void rsf_core_fsr4fg_describe_texture(void*, rsf_fsr4fg_texture*);
extern "C" void rsf_core_fsr4fg_shader_read(void*, void*, uint32_t);
extern "C" int32_t rsf_core_fsr4fg_copy_to_backbuffer(void*, void*, void*);
extern "C" void rsf_core_fsr4fg_execute(void*, void*);
extern "C" int32_t rsf_core_fsr4fg_queue_fence(void*, void*, uint64_t, uint32_t);
extern "C" int32_t rsf_core_fsr4fg_present_dxgi(void*, uint32_t);
extern "C" int32_t rsf_core_fsr4fg_present_count(void*, uint32_t*);
extern "C" int32_t rsf_core_fsr4fg_sdk_wait_presents(PfnFfxDispatch, ffxContext*);

static_assert(FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_WAIT_FOR_PRESENTS_DX12 == 0x30007 &&
    sizeof(ffxDispatchDescFrameGenerationSwapChainWaitForPresentsDX12) == sizeof(ffxApiHeader));

extern "C" int32_t rsf_core_fsr4fg_sdk_versions(PfnFfxQuery, void*, rsf_ffx_version*, uint64_t*);
extern "C" int32_t rsf_core_fsr4fg_sdk_provider(PfnFfxQuery, ffxContext*, uint64_t*);
using DescribeResource = void (*)(void*, uint32_t, FfxApiResource*);
extern "C" int32_t rsf_core_fsr4fg_sdk_prepare(const rsf_fsr4fg_frame*, uint64_t, uint32_t,
    void*, ffxContext*, PfnFfxDispatch, DescribeResource);

extern "C" void rsf_core_fsr4fg_sdk_generation(const rsf_fsr4fg_create*, uint64_t, uint32_t, uint32_t,
    ffxApiHeader*, ffxOverrideVersion*, ffxCreateContextDescFrameGenerationVersion*,
    ffxCreateContextDescFrameGeneration*);
extern "C" int32_t rsf_core_fsr4fg_sdk_configure(void*, uint64_t, uint32_t, uint32_t, uint32_t,
    FfxApiFrameGenerationDispatchFunc, void*, ffxContext*, PfnFfxConfigure, void*, DescribeResource);

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

template<class T, class Call, class... Args>
int32_t create_interface(T*& slot, Call call, Args... args) {
    void* object = nullptr;
    const int32_t code = call(args..., &object);
    slot = static_cast<T*>(object);
    return code;
}

int32_t create_factory(Native* self, uint32_t flags) {
    return create_interface(self->factory, rsf_core_fsr4fg_create_factory, flags);
}

int32_t find_adapter(Native* self, uint64_t adapter_luid) {
    return create_interface(self->adapter, rsf_core_fsr4fg_find_adapter, self->factory, adapter_luid);
}

int32_t get_adapter_vendor(Native* self, uint32_t* vendor) {
    return rsf_core_fsr4fg_get_adapter_vendor(self->adapter, vendor);
}

int32_t create_device(Native* self, uint32_t feature_level) {
    return create_interface(self->device, rsf_core_fsr4fg_create_device, self->adapter, feature_level);
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
    return create_interface(self->queue, rsf_core_fsr4fg_create_queue, self->device, setup);
}

int32_t create_allocator(Native* self, uint32_t list_type) {
    return create_interface(self->allocator, rsf_core_fsr4fg_create_allocator, self->device, list_type);
}

int32_t create_command_list(Native* self, uint32_t node_mask, uint32_t list_type) {
    return create_interface(self->commands, rsf_core_fsr4fg_create_list, self->device, self->allocator, node_mask,
        list_type);
}

int32_t create_completion_fence(Native* self, uint64_t initial_value, uint32_t flags) {
    return create_interface(self->completion, rsf_core_fsr4fg_create_completion_fence, self->device, initial_value,
        flags);
}

int32_t create_event(Native* self, uint32_t manual_reset, uint32_t initial_state) {
    return rsf_core_fsr4fg_create_event(manual_reset, initial_state, &self->event);
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
    rsf_core_fsr4fg_dxgi_description(&desc, setup, first_attempt);
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
    return rsf_core_fsr4fg_shared_image(self->device, width, height, format, handle, resource);
}

int32_t create_fence(Native* self, void** handle) {
    void* fence = self->shared;
    const int32_t result = rsf_core_fsr4fg_shared_fence(self->device, &fence, handle);
    self->shared = static_cast<ID3D12Fence*>(fence);
    return result;
}

int32_t reset_commands(Native* self) {
    return rsf_core_fsr4fg_reset_commands(self->commands, self->allocator);
}

int32_t close_commands(Native* self) { return rsf_core_fsr4fg_close_commands(self->commands); }

ffxReturnCode_t generate(ffxDispatchDescFrameGeneration* desc, void* user) {
    auto* self = static_cast<Native*>(user);
    const auto code = self->dispatch(&self->generation, &desc->header);
    const auto count = code == FFX_API_RETURN_OK ? desc->numGeneratedFrames : 0u;
    self->generated(self->telemetry, static_cast<int32_t>(code), count);
    return code;
}

void describe_resource(void* resource, uint32_t state, FfxApiResource* out) {
    *out = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(resource), state);
}

int32_t configure(Native* self, uint64_t sequence, uint32_t width, uint32_t height, uint32_t enabled, void* hudless) {
    return rsf_core_fsr4fg_sdk_configure(self->chain, sequence, width, height, enabled, generate, self,
        &self->generation, self->configure, hudless, describe_resource);
}

void shader_read(Native* self, void* resource, uint32_t enter) {
    rsf_core_fsr4fg_shader_read(self->commands, resource, enter);
}

int32_t prepare(Native* self, const rsf_fsr4fg_frame* frame, uint64_t sequence, uint32_t reset) {
    return rsf_core_fsr4fg_sdk_prepare(frame, sequence, reset, self->commands, &self->generation,
        self->dispatch, describe_resource);
}

int32_t copy_to_backbuffer(Native* self, void* color) {
    return rsf_core_fsr4fg_copy_to_backbuffer(self->commands, self->chain, color);
}

int32_t wait_shared(Native* self, uint64_t value) { return rsf_core_fsr4fg_queue_fence(self->queue, self->shared, value, 0); }

void execute(Native* self) {
    rsf_core_fsr4fg_execute(self->queue, self->commands);
}

int32_t present(Native* self, uint32_t vsync) { return rsf_core_fsr4fg_present_dxgi(self->chain, vsync); }

int32_t wait_presents(Native* self) {
    return rsf_core_fsr4fg_sdk_wait_presents(self->dispatch, &self->swapchain);
}

int32_t signal(Native* self, uint64_t value, uint32_t shared) {
    return rsf_core_fsr4fg_queue_fence(self->queue, shared ? self->shared : self->completion, value, 1);
}

int32_t wait_local(Native* self, uint64_t value) {
    return rsf::wait_fence(self->completion, value, self->event);
}

int32_t get_present_count(Native* self, uint32_t* count) { return rsf_core_fsr4fg_present_count(self->chain, count); }

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
    create_image, create_fence, rsf_core_fsr4fg_describe_texture, reset_commands,
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
