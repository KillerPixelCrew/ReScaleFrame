// AMD FSR 4 DX12 upscaler on the Vulkan adapter. Shared D3D12 resources are
// imported by Vulkan; a D3D12 fence orders both queues without CPU readback.
// The Rust owner (owner.rs) holds the bridge state, provider policy, fence
// sequencing and checked retirement. These leaves keep the typed DXGI, D3D12,
// Vulkan and FidelityFX calls and the INT8 compatibility lease.
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <vulkan/vulkan.h>
#include <cstdint>
#include <cstddef>
#include <memory>
#include <new>
#include "api/include/dx12/ffx_api_dx12.h"
#include "upscalers/include/ffx_upscale.h"
#include "fsr4_compat_core.h"
#if !defined(RSF_FSR4_VULKAN_STATIC)
#define RSF_FSR4_BRIDGE_BUILD
#endif
#include "bridge_platform.h"

extern "C" uint32_t rsf_core_fsr4sr_own_directory(uint16_t*, uint32_t);
extern "C" int32_t rsf_core_fsr4sr_load_provider(const uint16_t*, void**, PfnFfxCreateContext*,
    PfnFfxDestroyContext*, PfnFfxDispatch*, PfnFfxQuery*);
extern "C" void rsf_core_fsr4sr_release_provider_objects(void* const (*)[10], void*, void*);
extern "C" int32_t rsf_core_fsr4sr_sdk_versions(PfnFfxQuery, void*, rsf_ffx_version*, uint64_t*);
extern "C" int32_t rsf_core_fsr4sr_sdk_provider(PfnFfxQuery, ffxContext*, rsf_ffx_version*, uint32_t*);
extern "C" int32_t rsf_core_fsr4sr_sdk_create(PfnFfxCreateContext, ffxContext*, ffxApiHeader*, uint32_t,
    const rsf_fsr4sr_context*);
extern "C" int32_t rsf_core_fsr4sr_sdk_plan(PfnFfxQuery, ffxContext*, uint32_t, uint32_t, uint32_t,
    uint32_t*, uint32_t*);
using DescribeResource = void (*)(void*, uint32_t, FfxApiResource*);
extern "C" int32_t rsf_core_fsr4sr_record(void*, void*, const rsf_fsr4sr_dispatch*, ffxContext*,
    PfnFfxDispatch, DescribeResource);
extern "C" int32_t rsf_core_fsr4sr_shared_image(void*, uint32_t, uint32_t, uint32_t, uint32_t,
    void**, void**);
extern "C" int32_t rsf_core_fsr4sr_find_adapter(uint64_t, void**, void**, uint32_t*);
extern "C" int32_t rsf_core_fsr4sr_create_device(void*, void**, void**);
extern "C" int32_t rsf_core_fsr4sr_create_commands(void*, rsf_fsr4sr_command_objects*);
extern "C" uint64_t rsf_core_fsr4sr_completed(void*);
extern "C" int32_t rsf_core_fsr4sr_wait_event(void*, void*, uint64_t, uint32_t);
extern "C" int32_t rsf_core_fsr4sr_shared_fence(void*, void**, void**);
extern "C" void rsf_core_fsr4sr_describe(void*, rsf_fsr4sr_texture*);
extern "C" void rsf_core_fsr4sr_release_image(void*);
extern "C" void rsf_core_fsr4sr_execute(void*, void*);
extern "C" int32_t rsf_core_fsr4sr_queue_fence(void*, void*, uint64_t, uint32_t);

static_assert(sizeof(VkPhysicalDeviceIDProperties) == 64 &&
    offsetof(VkPhysicalDeviceIDProperties, deviceLUID) == 48 &&
    offsetof(VkPhysicalDeviceIDProperties, deviceLUIDValid) == 60);
static_assert(VK_FORMAT_R16G16B16A16_SFLOAT == 97 && VK_FORMAT_R16G16_SFLOAT == 83 &&
    VK_FORMAT_R32_SFLOAT == 100 && VK_FORMAT_D32_SFLOAT == 126);

static_assert(FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE == 0x10000 &&
    FFX_API_DISPATCH_DESC_TYPE_UPSCALE == 0x10001 &&
    FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE == 0x10003 &&
    FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE_VERSION == 0x1000b);
static_assert(sizeof(ffxApiHeader) == 16 && sizeof(ffxQueryDescGetVersions) == 56 &&
    sizeof(ffxQueryGetProviderVersion) == 32 && sizeof(ffxOverrideVersion) == 24 &&
    sizeof(ffxCreateContextDescUpscale) == 48 && sizeof(ffxCreateContextDescUpscaleVersion) == 24 &&
    sizeof(ffxQueryDescUpscaleGetRenderResolutionFromQualityMode) == 48 &&
    sizeof(ffxDispatchDescUpscale) == 432 && offsetof(ffxDispatchDescUpscale, color) == 24 &&
    offsetof(ffxDispatchDescUpscale, motionVectors) == 120 &&
    offsetof(ffxDispatchDescUpscale, output) == 312);

// owner.rs repeats these values.
static_assert(DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && DXGI_FORMAT_R16G16_FLOAT == 34 &&
    DXGI_FORMAT_R32_FLOAT == 41);
static_assert(static_cast<uint32_t>(E_UNEXPECTED) == 0x8000ffffu &&
    static_cast<uint32_t>(DXGI_ERROR_DEVICE_REMOVED) == 0x887a0005u);

/// @brief Native capsule: signed-runtime module, matching DX12 adapter/device, SDK context, queue and command objects.
/// @details Imported image/resource ownership stays with the caller. Only the Rust owner releases the capsule.
/// The NVIDIA/Intel INT8 compatibility lease must outlive the SDK context; Rust drops it after context destruction.
struct rsf_fsr4sr_native {
    std::shared_ptr<Fsr4Compatibility> compatibility;
    HMODULE module{};
    IDXGIFactory4* factory{};
    IDXGIAdapter1* adapter{};
    ID3D12Device* device{};
    ID3D12CommandQueue* queue{};
    ID3D12Fence* sharedFence{};
    ID3D12Fence* completionFence{};
    HANDLE completionEvent{};
    ID3D12CommandAllocator* allocators[3]{};
    ID3D12GraphicsCommandList* commands{};
    ffxContext effect{};
    PfnFfxCreateContext create{};
    PfnFfxDestroyContext destroy{};
    PfnFfxDispatch dispatch{};
    PfnFfxQuery query{};
};

namespace {
constexpr uint32_t kAllocators = 3;

rsf_fsr4sr_native* Allocate() { return new (std::nothrow) rsf_fsr4sr_native(); }

/// @brief Frees COM owners, the event, the module and the capsule in the previous checked-destruction order.
void ReleaseNative(rsf_fsr4sr_native* native) {
    if (!native) return;
    native->compatibility.reset();
    void* const objects[10] = {
        native->commands, native->allocators[0], native->allocators[1], native->allocators[2],
        native->completionFence, native->sharedFence, native->queue, native->device, native->adapter, native->factory,
    };
    rsf_core_fsr4sr_release_provider_objects(&objects, native->completionEvent, native->module);
    delete native;
}

uint32_t Objects(const rsf_fsr4sr_native* native) {
    uint32_t objects = 0;
    if (native->device) objects |= RSF_FSR4SR_DEVICE;
    if (native->queue) objects |= RSF_FSR4SR_QUEUE;
    if (native->effect) objects |= RSF_FSR4SR_EFFECT;
    if (native->completionFence) objects |= RSF_FSR4SR_LOCAL_FENCE;
    if (native->completionEvent) objects |= RSF_FSR4SR_EVENT;
    if (native->sharedFence) objects |= RSF_FSR4SR_SHARED_FENCE;
    return objects;
}

/// @brief Creates the DXGI factory and retains the adapter whose LUID matches the Vulkan device.
/// @return Zero with its PCI vendor, -5 when no adapter matches, or an HRESULT.
int32_t FindAdapter(rsf_fsr4sr_native* native, uint64_t luid, uint32_t* vendor) {
    void* factory = nullptr;
    void* adapter = nullptr;
    const int32_t code = rsf_core_fsr4sr_find_adapter(luid, &factory, &adapter, vendor);
    native->factory = static_cast<IDXGIFactory4*>(factory);
    native->adapter = static_cast<IDXGIAdapter1*>(adapter);
    return code;
}

int32_t CreateDevice(rsf_fsr4sr_native* native) {
    void* device = nullptr;
    void* queue = nullptr;
    const int32_t code = rsf_core_fsr4sr_create_device(native->adapter, &device, &queue);
    native->device = static_cast<ID3D12Device*>(device);
    native->queue = static_cast<ID3D12CommandQueue*>(queue);
    return code;
}

/// @brief Loads the signed upscaler from a full NUL-terminated path and resolves its four exports.
/// @return Zero, -7 when the module does not load, or -8 when an export is missing.
int32_t Load(rsf_fsr4sr_native* native, const uint16_t* path) {
    void* module = nullptr;
    const int32_t code = rsf_core_fsr4sr_load_provider(path, &module, &native->create, &native->destroy,
        &native->dispatch, &native->query);
    native->module = static_cast<HMODULE>(module);
    return code;
}

/// @brief Takes the pinned-SDK INT8 capability lease for NVIDIA/Intel.
/// @details It must be active before version enumeration, which filters providers by device support.
uint32_t AcquireCompatibility(rsf_fsr4sr_native* native) {
    try {
        native->compatibility = rsf_fsr4_enable_int8_adapter(native->module, native->adapter, native->device, nullptr, nullptr);
    } catch (...) {
        native->compatibility.reset();
    }
    return native->compatibility ? 1u : 0u;
}

void ReleaseCompatibility(rsf_fsr4sr_native* native) { native->compatibility.reset(); }

/// @brief Lists upscaler providers for this device, parsed by the shared FidelityFX version helper.
/// @param count In: capacity of out (at most 32). Out: the SDK's provider count.
int32_t Versions(rsf_fsr4sr_native* native, rsf_ffx_version* out, uint64_t* count) {
    return rsf_core_fsr4sr_sdk_versions(native->query, native->device, out, count);
}

/// @brief Creates the upscaler context with the provider forced through ffxOverrideVersion.
int32_t CreateContext(rsf_fsr4sr_native* native, const rsf_fsr4sr_context* setup) {
    ffxCreateBackendDX12Desc backend{};
    backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
    backend.device = native->device;
    return rsf_core_fsr4sr_sdk_create(native->create, &native->effect, &backend.header, FFX_UPSCALER_VERSION, setup);
}

/// @brief Reports the effective provider ID, its parsed display name and whether the name parsed.
/// @details The shared parse reports family 0 both for "0.x.y" and for labels without that shape, so
/// `parsed` carries the previous IsFsr4Name sscanf result separately.
int32_t Provider(rsf_fsr4sr_native* native, rsf_ffx_version* out, uint32_t* parsed) {
    return rsf_core_fsr4sr_sdk_provider(native->query, &native->effect, out, parsed);
}

/// @brief Creates the local completion fence, its event (-10 on failure), three allocators and a closed list.
int32_t CreateCommands(rsf_fsr4sr_native* native) {
    rsf_fsr4sr_command_objects objects{};
    const int32_t result = rsf_core_fsr4sr_create_commands(native->device, &objects);
    native->completionFence = static_cast<ID3D12Fence*>(objects.completion);
    native->completionEvent = objects.event;
    for (uint32_t slot = 0; slot < kAllocators; ++slot)
        native->allocators[slot] = static_cast<ID3D12CommandAllocator*>(objects.allocators[slot]);
    native->commands = static_cast<ID3D12GraphicsCommandList*>(objects.list);
    return result;
}

int32_t Plan(rsf_fsr4sr_native* native, uint32_t quality, uint32_t displayWidth,
    uint32_t displayHeight, uint32_t* width, uint32_t* height) {
    return rsf_core_fsr4sr_sdk_plan(native->query, &native->effect, quality, displayWidth, displayHeight, width, height);
}

/// @brief Creates a committed shared DX12 texture and NT handle for dedicated Vulkan import.
/// @details Rust has validated the arguments and cleared both outputs. The caller owns the handle and the resource.
int32_t CreateImage(rsf_fsr4sr_native* native, uint32_t width, uint32_t height, uint32_t vkFormat,
    uint32_t writable, void** handle, void** resource) {
    return rsf_core_fsr4sr_shared_image(native->device, width, height, vkFormat, writable, handle, resource);
}

/// @brief Creates or reuses the capsule-owned shared fence and exports a new NT handle.
int32_t CreateFence(rsf_fsr4sr_native* native, void** handle) {
    void* fence = native->sharedFence;
    const int32_t result = rsf_core_fsr4sr_shared_fence(native->device, &fence, handle);
    native->sharedFence = static_cast<ID3D12Fence*>(fence);
    return result;
}

uint64_t Completed(rsf_fsr4sr_native* native) { return rsf_core_fsr4sr_completed(native->completionFence); }

/// @return Zero when the event fired, an HRESULT when it could not be armed, or -2 on timeout.
int32_t WaitEvent(rsf_fsr4sr_native* native, uint64_t value, uint32_t milliseconds) {
    return rsf_core_fsr4sr_wait_event(native->completionFence, native->completionEvent, value, milliseconds);
}

void DescribeResourceForSdk(void* resource, uint32_t state, FfxApiResource* out) {
    *out = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(resource), state);
}

int32_t Record(rsf_fsr4sr_native* native, const rsf_fsr4sr_dispatch* frame) {
    if (frame->slot >= kAllocators) return -1;
    return rsf_core_fsr4sr_record(native->commands, native->allocators[frame->slot], frame,
        &native->effect, native->dispatch, DescribeResourceForSdk);
}

int32_t WaitShared(rsf_fsr4sr_native* native, uint64_t value) {
    return rsf_core_fsr4sr_queue_fence(native->queue, native->sharedFence, value, 0);
}

void Execute(rsf_fsr4sr_native* native) {
    rsf_core_fsr4sr_execute(native->queue, native->commands);
}

int32_t Signal(rsf_fsr4sr_native* native, uint64_t value, uint32_t shared) {
    return rsf_core_fsr4sr_queue_fence(native->queue, shared ? native->sharedFence : native->completionFence, value, 1);
}

/// @brief Destroys the SDK consumer; -1 when the context exists without its destroy export.
int32_t DestroyContext(rsf_fsr4sr_native* native) {
    if (!native->effect) return 0;
    if (!native->destroy) return -1;
    ffxContext owned = native->effect;
    const auto result = static_cast<int32_t>(native->destroy(&owned, nullptr));
    if (result != 0) return result;
    native->effect = nullptr;
    return 0;
}

constexpr rsf_fsr4sr_platform kPlatform{
    sizeof(rsf_fsr4sr_platform), RSF_FSR4SR_PLATFORM_ABI_VERSION,
    Allocate, ReleaseNative, Objects, FindAdapter, CreateDevice, rsf_core_fsr4sr_own_directory, Load,
    AcquireCompatibility, ReleaseCompatibility, Versions, CreateContext, Provider, CreateCommands,
    Plan, CreateImage, CreateFence, rsf_core_fsr4sr_describe, Completed, WaitEvent, Record, WaitShared, Execute,
    Signal, DestroyContext,
};
}

/// @brief Checks the Vulkan adapter vendor required by this FSR4 bridge.
/// @details The vendor check is preliminary; NVIDIA/Intel compatibility and signed-provider selection are verified during Create.
/// @return Zero for an AMD, NVIDIA or Intel Vulkan adapter, otherwise a negative availability code.
extern "C" RSF_FSR4_BRIDGE_API int rsf_vk_fsr4_probe(VkPhysicalDevice physical) {
    return rsf_core_fsr4sr_probe_physical(physical);
}

/// @brief Waits for the most recent tracked submission on this DX12 queue.
/// @return Zero when complete or value is null; nonzero on completion failure.
extern "C" RSF_FSR4_BRIDGE_API int rsf_vk_fsr4_wait_idle(rsf_vk_fsr4* value) {
    return rsf_core_fsr4sr_wait_idle(value);
}

/// @brief Drains tracked DX12 work and destroys the SDK consumer before imported resources are retired.
/// @details Failure is persisted as terminal releaseFailure; native ownership is retained.
/// @return Zero when prepared, or the retained drain/SDK destruction error.
extern "C" RSF_FSR4_BRIDGE_API int rsf_vk_fsr4_prepare_destroy(rsf_vk_fsr4* value) {
    return rsf_core_fsr4sr_prepare_destroy(value);
}

/// @brief Performs checked release preparation then frees native/COM owners and the context allocation.
/// @details The caller must first drain Vulkan users and dispose imported shared images/fence imports before destroying their owning DX12 context. Failure leaves the owner attached.
/// @return Zero after successful destruction; a nonzero preparation error preserves the value pointer.
extern "C" RSF_FSR4_BRIDGE_API int rsf_vk_fsr4_destroy_checked(rsf_vk_fsr4* value) {
    return rsf_core_fsr4sr_destroy(value);
}

/// @brief Legacy void destruction entry point that preserves ownership when checked preparation fails.
/// @details New callers should use DestroyChecked to observe release failure.
extern "C" RSF_FSR4_BRIDGE_API void rsf_vk_fsr4_destroy(rsf_vk_fsr4* value) {
    rsf_core_fsr4sr_destroy(value);
}

/// @brief Creates the signed FSR4 DX12 context on the adapter matching the Vulkan device LUID.
/// @details NVIDIA/Intel first acquire the INT8 compatibility lease (-12 when refused). The newest 4.x provider is forced with ffxOverrideVersion and the effective provider must match it (-9 otherwise; -11 for an unsupported vendor). On initialization failure a failed checked cleanup can return a retained owner through output.
/// @param physical Borrowed Vulkan physical device with a valid Windows LUID.
/// @param output Required owner slot; release an attached owner only with checked destruction.
/// @return Zero on success; local, HRESULT or FidelityFX failure code otherwise.
extern "C" RSF_FSR4_BRIDGE_API int rsf_vk_fsr4_create(
    VkPhysicalDevice physical, uint32_t renderWidth, uint32_t renderHeight,
    uint32_t displayWidth, uint32_t displayHeight, rsf_vk_fsr4** output) {
    return rsf_core_fsr4sr_create_physical(&kPlatform, physical, renderWidth, renderHeight, displayWidth,
        displayHeight, output);
}

extern "C" RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeCreate(const rsf_fsr4_bridge_create* setup,
    rsf_fsr4_bridge** output) {
    return rsf_core_fsr4sr_create_public(&kPlatform, setup, output);
}

extern "C" RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeInfo(const rsf_fsr4_bridge* value,
    rsf_fsr4_bridge_info* info) {
    return rsf_core_fsr4sr_info(value, info);
}

extern "C" RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgePlan(rsf_fsr4_bridge* value,
    uint32_t quality, uint32_t* width, uint32_t* height) {
    return rsf_core_fsr4sr_plan(value, quality, width, height);
}

/// @brief Creates a committed DX12 texture and NT handle for dedicated Vulkan import.
/// @param value Borrowed live runtime owner.
/// @param width Nonzero texture width.
/// @param height Nonzero texture height.
/// @param vkFormat Supported Vulkan format translated to DXGI.
/// @param writable Requests unordered-access output; depth writable resources are rejected.
/// @param handle Caller-owned NT handle output; close it after import/use.
/// @param resource Caller-owned COM resource reference; release after both graphics APIs finish.
/// @return Zero on success, or local/HRESULT failure; outputs are cleared before allocation.
extern "C" RSF_FSR4_BRIDGE_API int rsf_vk_fsr4_create_shared_image(
    rsf_vk_fsr4* value, uint32_t width, uint32_t height, uint32_t vkFormat,
    uint32_t writable, HANDLE* handle, ID3D12Resource** resource) {
    return rsf_core_fsr4sr_create_image(value, width, height, vkFormat, writable, handle,
        reinterpret_cast<void**>(resource));
}

/// @brief Releases the caller-owned DX12 image reference; a null reference is accepted.
/// @details This does not wait. Vulkan import users and DX12 queue users must already be complete.
extern "C" RSF_FSR4_BRIDGE_API void rsf_vk_fsr4_release_image(ID3D12Resource* resource) {
    rsf_core_fsr4sr_release_image(resource);
}

/// @brief Creates or reuses the runtime-owned DX12 shared fence and exports a new NT handle.
/// @details The caller owns and closes the returned handle after permanent Vulkan timeline import; the fence remains owned by value.
/// @return Zero on success, or a local/HRESULT error.
extern "C" RSF_FSR4_BRIDGE_API int rsf_vk_fsr4_create_shared_fence(
    rsf_vk_fsr4* value, HANDLE* handle) {
    return rsf_core_fsr4sr_create_fence(value, handle);
}

/// @brief Records and submits reconstruction after a GPU wait for the Vulkan ready value.
/// @details Valid ready/done values always receive an ordered handoff, including SDK failures. A prepared owner refuses new work.
extern "C" RSF_FSR4_BRIDGE_API int rsf_vk_fsr4_evaluate(
    rsf_vk_fsr4* value, const rsf_vk_fsr4_frame* frame) {
    return rsf_core_fsr4sr_evaluate_legacy(value, frame);
}

extern "C" RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeEvaluate(rsf_fsr4_bridge* value,
    const rsf_fsr4_bridge_frame* frame) {
    return rsf_core_fsr4sr_evaluate(value, frame);
}

extern "C" RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeSharpness(rsf_fsr4_bridge* value, float amount) {
    return rsf_core_fsr4sr_sharpness(value, amount);
}
