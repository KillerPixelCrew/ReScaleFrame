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
#include <type_traits>
#include "bridge_platform.h"

extern "C" int32_t rsf_core_fsr4fg_load_provider(const uint16_t*, void**, PfnFfxCreateContext*,
    PfnFfxDestroyContext*, PfnFfxConfigure*, PfnFfxDispatch*, PfnFfxQuery*);

extern "C" void rsf_core_fsr4fg_dxgi_description(DXGI_SWAP_CHAIN_DESC1*, const rsf_fsr4fg_swapchain_desc*, uint32_t);
static_assert(sizeof(DXGI_SWAP_CHAIN_DESC1) == 48 &&
    offsetof(DXGI_SWAP_CHAIN_DESC1, SampleDesc) == 16 &&
    offsetof(DXGI_SWAP_CHAIN_DESC1, SwapEffect) == 36);

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
    rsf_fsr4fg_native_graphics graphics{};
    HMODULE module{};
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

static_assert(std::is_standard_layout_v<rsf_fsr4fg_native>);
static_assert(offsetof(rsf_fsr4fg_native, graphics) == 0);

namespace {
using Native = rsf_fsr4fg_native;

static_assert(D3D12_RESOURCE_DIMENSION_TEXTURE2D == 3 && DXGI_FORMAT_B8G8R8A8_UNORM == 87 &&
    DXGI_FORMAT_R32_FLOAT == 41 && DXGI_FORMAT_R16G16_FLOAT == 34);
static_assert(sizeof(LUID) == sizeof(uint64_t));
static_assert(D3D_FEATURE_LEVEL_12_0 == 0xc000 && D3D12_COMMAND_LIST_TYPE_DIRECT == 0 &&
    D3D12_COMMAND_QUEUE_PRIORITY_NORMAL == 0 && D3D12_COMMAND_QUEUE_FLAG_NONE == 0 && D3D12_FENCE_FLAG_NONE == 0);
static_assert(DXGI_USAGE_RENDER_TARGET_OUTPUT == 0x20 && DXGI_SCALING_STRETCH == 0 &&
    DXGI_SWAP_EFFECT_FLIP_DISCARD == 4 && DXGI_ALPHA_MODE_UNSPECIFIED == 0);

Native* allocate(void* telemetry, rsf_fsr4fg_generated_fn generated) {
    auto* self = new (std::nothrow) Native{};
    if (self) {
        self->telemetry = telemetry;
        self->generated = generated;
    }
    return self;
}

void release_native(Native* self) {
    rsf_core_fsr4fg_release_provider_objects(&self->graphics, self->module);
    delete self;
}

int32_t load(Native* self, const uint16_t* path) {
    void* module = nullptr;
    const int32_t code = rsf_core_fsr4fg_load_provider(path, &module, &self->create, &self->destroy,
        &self->configure, &self->dispatch, &self->query);
    self->module = static_cast<HMODULE>(module);
    return code;
}

int32_t versions(Native* self, rsf_ffx_version* out, uint64_t* count) {
    return rsf_core_fsr4fg_sdk_versions(self->query, self->graphics.device, out, count);
}

int32_t create_generation(Native* self, const rsf_fsr4fg_create* setup, uint64_t provider) {
    self->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
    self->backend.device = static_cast<ID3D12Device*>(self->graphics.device);
    rsf_core_fsr4fg_sdk_generation(setup, provider, FFX_FRAMEGENERATION_VERSION,
        ffxApiGetSurfaceFormatDX12(DXGI_FORMAT_B8G8R8A8_UNORM), &self->backend.header,
        &self->override_version, &self->api_version, &self->description);
    return static_cast<int32_t>(self->create(&self->generation, &self->description.header, nullptr));
}

int32_t get_provider(Native* self, uint64_t* actual) {
    return rsf_core_fsr4fg_sdk_provider(self->query, &self->generation, actual);
}

uint32_t objects(Native* self) {
    uint32_t found = 0;
    if (self->graphics.device) found |= RSF_FSR4FG_DEVICE;
    if (self->generation) found |= RSF_FSR4FG_GENERATION;
    if (self->graphics.chain) found |= RSF_FSR4FG_CHAIN;
    if (self->swapchain) found |= RSF_FSR4FG_SWAPCHAIN;
    if (self->graphics.queue && self->graphics.completion && self->graphics.event) found |= RSF_FSR4FG_LOCAL_FENCE;
    if (self->graphics.shared) found |= RSF_FSR4FG_SHARED_FENCE;
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
    create.swapchain = &self->graphics.chain;
    create.hwnd = static_cast<HWND>(window);
    create.desc = &desc;
    create.dxgiFactory = static_cast<IDXGIFactory4*>(self->graphics.factory);
    create.gameQueue = static_cast<ID3D12CommandQueue*>(self->graphics.queue);
    return static_cast<int32_t>(self->create(&self->swapchain, &create.header, nullptr));
}

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
    return rsf_core_fsr4fg_sdk_configure(self->graphics.chain, sequence, width, height, enabled, generate, self,
        &self->generation, self->configure, hudless, describe_resource);
}

int32_t prepare(Native* self, const rsf_fsr4fg_frame* frame, uint64_t sequence, uint32_t reset) {
    return rsf_core_fsr4fg_sdk_prepare(frame, sequence, reset, self->graphics.commands, &self->generation,
        self->dispatch, describe_resource);
}

int32_t wait_presents(Native* self) {
    return rsf_core_fsr4fg_sdk_wait_presents(self->dispatch, &self->swapchain);
}

int32_t destroy_context(Native* self, ffxContext& context) {
    const auto code = self->destroy(&context, nullptr);
    if (!code) context = nullptr;
    return static_cast<int32_t>(code);
}
int32_t destroy_generation(Native* self) { return destroy_context(self, self->generation); }
int32_t destroy_swapchain(Native* self) { return destroy_context(self, self->swapchain); }

const rsf_fsr4fg_platform platform{
    allocate, release_native, rsf_core_fsr4fg_create_factory, rsf_core_fsr4fg_find_adapter,
    rsf_core_fsr4fg_get_adapter_vendor, rsf_core_fsr4fg_create_device, load, versions, create_generation, get_provider,
    rsf_core_fsr4fg_create_queue, rsf_core_fsr4fg_create_allocator, rsf_core_fsr4fg_create_list,
    rsf_core_fsr4fg_create_completion_fence, rsf_core_fsr4fg_create_event, objects, create_swapchain,
    rsf_core_fsr4fg_shared_image, rsf_core_fsr4fg_shared_fence, rsf_core_fsr4fg_describe_texture,
    rsf_core_fsr4fg_reset_commands, rsf_core_fsr4fg_close_commands, configure, rsf_core_fsr4fg_shader_read, prepare,
    rsf_core_fsr4fg_copy_to_backbuffer, rsf_core_fsr4fg_wait_shared, rsf_core_fsr4fg_execute,
    rsf_core_fsr4fg_present_dxgi, wait_presents, rsf_core_fsr4fg_signal, rsf_core_fsr4fg_wait_local,
    rsf_core_fsr4fg_present_count, destroy_generation, rsf_core_fsr4fg_release_chain, destroy_swapchain,
};
}

extern "C" int32_t RsfFsr4FgProbe(uint64_t luid, const uint16_t* directory, rsf_fsr4fg_info* info) {
    return rsf_core_fsr4fg_probe(&platform, luid, directory, info);
}
extern "C" int32_t RsfFsr4FgCreate(const rsf_fsr4fg_create* setup, rsf_fsr4fg_bridge** output) {
    return rsf_core_fsr4fg_create(&platform, setup, output);
}
