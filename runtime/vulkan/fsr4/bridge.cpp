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
#include <cstring>
#include <memory>
#include <new>
#include "api/include/dx12/ffx_api_dx12.h"
#include "upscalers/include/ffx_upscale.h"
#include "fsr4_compat_core.h"
#if !defined(RSF_FSR4_VULKAN_STATIC)
#define RSF_FSR4_BRIDGE_BUILD
#endif
#include "bridge_platform.h"

extern "C" int32_t rsf_core_fsr4sr_sdk_versions(PfnFfxQuery, void*, rsf_ffx_version*, uint64_t*);
extern "C" int32_t rsf_core_fsr4sr_sdk_provider(PfnFfxQuery, ffxContext*, uint64_t*, const char**);
extern "C" int32_t rsf_core_fsr4sr_sdk_create(PfnFfxCreateContext, ffxContext*, ffxApiHeader*, uint32_t,
    const rsf_fsr4sr_context*);
extern "C" int32_t rsf_core_fsr4sr_sdk_plan(PfnFfxQuery, ffxContext*, uint32_t, uint32_t, uint32_t,
    uint32_t*, uint32_t*);
using DescribeResource = void (*)(void*, uint32_t, FfxApiResource*);
extern "C" int32_t rsf_core_fsr4sr_record(void*, void*, const rsf_fsr4sr_dispatch*, ffxContext*,
    PfnFfxDispatch, DescribeResource);
extern "C" int32_t rsf_core_fsr4sr_shared_image(void*, uint32_t, uint32_t, uint32_t, uint32_t,
    void**, void**);
extern "C" void rsf_core_fsr4sr_execute(void*, void*);
extern "C" int32_t rsf_core_fsr4sr_queue_fence(void*, void*, uint64_t, uint32_t);

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

/// @brief Releases a local COM reference and clears its pointer without waiting for GPU work.
template<class T> static void Release(T*& object) {
    if (object) { object->Release(); object = nullptr; }
}

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

int32_t Result(HRESULT hr) { return FAILED(hr) ? static_cast<int32_t>(hr) : 0; }

rsf_fsr4sr_native* Allocate() { return new (std::nothrow) rsf_fsr4sr_native(); }

/// @brief Frees COM owners, the event, the module and the capsule in the previous checked-destruction order.
void ReleaseNative(rsf_fsr4sr_native* native) {
    if (!native) return;
    native->compatibility.reset();
    Release(native->commands);
    for (auto*& allocator : native->allocators) Release(allocator);
    Release(native->completionFence);
    Release(native->sharedFence);
    Release(native->queue);
    Release(native->device);
    Release(native->adapter);
    Release(native->factory);
    if (native->completionEvent) CloseHandle(native->completionEvent);
    if (native->module) FreeLibrary(native->module);
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
    HRESULT hr = CreateDXGIFactory2(0, IID_PPV_ARGS(&native->factory));
    if (FAILED(hr)) return static_cast<int32_t>(hr);
    for (UINT index = 0; ; ++index) {
        IDXGIAdapter1* candidate{};
        hr = native->factory->EnumAdapters1(index, &candidate);
        if (hr == DXGI_ERROR_NOT_FOUND) return -5;
        if (FAILED(hr)) return static_cast<int32_t>(hr);
        DXGI_ADAPTER_DESC1 description{};
        hr = candidate->GetDesc1(&description);
        if (FAILED(hr)) { Release(candidate); return static_cast<int32_t>(hr); }
        uint64_t candidateLuid{};
        std::memcpy(&candidateLuid, &description.AdapterLuid, sizeof(candidateLuid));
        if (candidateLuid == luid) {
            native->adapter = candidate;
            *vendor = description.VendorId;
            return 0;
        }
        candidate->Release();
    }
}

int32_t CreateDevice(rsf_fsr4sr_native* native) {
    HRESULT hr = D3D12CreateDevice(native->adapter, D3D_FEATURE_LEVEL_12_0,
        IID_PPV_ARGS(&native->device));
    if (FAILED(hr)) return static_cast<int32_t>(hr);
    D3D12_COMMAND_QUEUE_DESC queue{};
    queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    return Result(native->device->CreateCommandQueue(&queue, IID_PPV_ARGS(&native->queue)));
}

/// @brief Writes the directory of the module containing this bridge, with its trailing separator.
/// @return The UTF-16 length written, or zero when it cannot be resolved.
uint32_t OwnDirectory(uint16_t* path, uint32_t capacity) {
    HMODULE own{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(&OwnDirectory), &own)) return 0;
    wchar_t module[MAX_PATH]{};
    DWORD length = GetModuleFileNameW(own, module, MAX_PATH);
    if (!length || length == MAX_PATH) return 0;
    while (length && module[length - 1] != L'\\' && module[length - 1] != L'/') --length;
    if (!length || length > capacity) return 0;
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    std::memcpy(path, module, length * sizeof(uint16_t));
    return length;
}

/// @brief Loads the signed upscaler from a full NUL-terminated path and resolves its four exports.
/// @return Zero, -7 when the module does not load, or -8 when an export is missing.
int32_t Load(rsf_fsr4sr_native* native, const uint16_t* path) {
    native->module = LoadLibraryExW(reinterpret_cast<const wchar_t*>(path), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!native->module) return -7;
    native->create = reinterpret_cast<PfnFfxCreateContext>(GetProcAddress(native->module, "ffxCreateContext"));
    native->destroy = reinterpret_cast<PfnFfxDestroyContext>(GetProcAddress(native->module, "ffxDestroyContext"));
    native->dispatch = reinterpret_cast<PfnFfxDispatch>(GetProcAddress(native->module, "ffxDispatch"));
    native->query = reinterpret_cast<PfnFfxQuery>(GetProcAddress(native->module, "ffxQuery"));
    return native->create && native->destroy && native->dispatch && native->query ? 0 : -8;
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
    uint64_t id{};
    const char* name{};
    const auto result = rsf_core_fsr4sr_sdk_provider(native->query, &native->effect, &id, &name);
    if (result) return result;
    bool numeric = false;
    *out = rsf::ffx_version(id, name, &numeric);
    *parsed = numeric ? 1u : 0u;
    return 0;
}

/// @brief Creates the local completion fence, its event (-10 on failure), three allocators and a closed list.
int32_t CreateCommands(rsf_fsr4sr_native* native) {
    HRESULT hr = native->device->CreateFence(0, D3D12_FENCE_FLAG_NONE,
        IID_PPV_ARGS(&native->completionFence));
    if (FAILED(hr)) return static_cast<int32_t>(hr);
    native->completionEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!native->completionEvent) return -10;
    for (auto*& allocator : native->allocators) {
        hr = native->device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
            IID_PPV_ARGS(&allocator));
        if (FAILED(hr)) return static_cast<int32_t>(hr);
    }
    hr = native->device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
        native->allocators[0], nullptr, IID_PPV_ARGS(&native->commands));
    if (FAILED(hr)) return static_cast<int32_t>(hr);
    return Result(native->commands->Close());
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
    if (!native->sharedFence) {
        HRESULT hr = native->device->CreateFence(0, D3D12_FENCE_FLAG_SHARED,
            IID_PPV_ARGS(&native->sharedFence));
        if (FAILED(hr)) return static_cast<int32_t>(hr);
    }
    HANDLE shared{};
    const int32_t result = Result(native->device->CreateSharedHandle(native->sharedFence,
        nullptr, GENERIC_ALL, nullptr, &shared));
    *handle = shared;
    return result;
}

void Describe(void* resource, rsf_fsr4sr_texture* texture) {
    const auto desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    texture->width = desc.Width;
    texture->height = desc.Height;
    texture->format = static_cast<uint32_t>(desc.Format);
}

uint64_t Completed(rsf_fsr4sr_native* native) { return native->completionFence->GetCompletedValue(); }

/// @return Zero when the event fired, an HRESULT when it could not be armed, or -2 on timeout.
int32_t WaitEvent(rsf_fsr4sr_native* native, uint64_t value, uint32_t milliseconds) {
    HRESULT hr = native->completionFence->SetEventOnCompletion(value, native->completionEvent);
    if (FAILED(hr)) return static_cast<int32_t>(hr);
    return WaitForSingleObject(native->completionEvent, milliseconds) == WAIT_OBJECT_0 ? 0 : -2;
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
    Allocate, ReleaseNative, Objects, FindAdapter, CreateDevice, OwnDirectory, Load,
    AcquireCompatibility, ReleaseCompatibility, Versions, CreateContext, Provider, CreateCommands,
    Plan, CreateImage, CreateFence, Describe, Completed, WaitEvent, Record, WaitShared, Execute,
    Signal, DestroyContext,
};
}

/// @brief Checks the Vulkan adapter vendor required by this FSR4 bridge.
/// @details The vendor check is preliminary; NVIDIA/Intel compatibility and signed-provider selection are verified during Create.
/// @return Zero for an AMD, NVIDIA or Intel Vulkan adapter, otherwise a negative availability code.
extern "C" RSF_FSR4_BRIDGE_API int VulkanStoryFsr4Probe(VkPhysicalDevice physical) {
    if (!physical) return -1;
    HMODULE loader = GetModuleHandleW(L"vulkan-1.dll");
    auto properties = loader ? reinterpret_cast<PFN_vkGetPhysicalDeviceProperties>(
        GetProcAddress(loader, "vkGetPhysicalDeviceProperties")) : nullptr;
    if (!properties) return -2;
    VkPhysicalDeviceProperties info{};
    properties(physical, &info);
    return rsf_core_fsr4sr_candidate_vendor(info.vendorID) ? 0 : -3;
}

/// @brief Waits for the most recent tracked submission on this DX12 queue.
/// @return Zero when complete or value is null; nonzero on completion failure.
extern "C" RSF_FSR4_BRIDGE_API int VulkanStoryFsr4WaitIdle(VulkanStoryFsr4* value) {
    return rsf_core_fsr4sr_wait_idle(value);
}

/// @brief Drains tracked DX12 work and destroys the SDK consumer before imported resources are retired.
/// @details Failure is persisted as terminal releaseFailure; native ownership is retained.
/// @return Zero when prepared, or the retained drain/SDK destruction error.
extern "C" RSF_FSR4_BRIDGE_API int VulkanStoryFsr4PrepareDestroy(VulkanStoryFsr4* value) {
    return rsf_core_fsr4sr_prepare_destroy(value);
}

/// @brief Performs checked release preparation then frees native/COM owners and the context allocation.
/// @details The caller must first drain Vulkan users and dispose imported shared images/fence imports before destroying their owning DX12 context. Failure leaves the owner attached.
/// @return Zero after successful destruction; a nonzero preparation error preserves the value pointer.
extern "C" RSF_FSR4_BRIDGE_API int VulkanStoryFsr4DestroyChecked(VulkanStoryFsr4* value) {
    return rsf_core_fsr4sr_destroy(value);
}

/// @brief Legacy void destruction entry point that preserves ownership when checked preparation fails.
/// @details New callers should use DestroyChecked to observe release failure.
extern "C" RSF_FSR4_BRIDGE_API void VulkanStoryFsr4Destroy(VulkanStoryFsr4* value) {
    rsf_core_fsr4sr_destroy(value);
}

/// @brief Creates the signed FSR4 DX12 context on the adapter matching the Vulkan device LUID.
/// @details NVIDIA/Intel first acquire the INT8 compatibility lease (-12 when refused). The newest 4.x provider is forced with ffxOverrideVersion and the effective provider must match it (-9 otherwise; -11 for an unsupported vendor). On initialization failure a failed checked cleanup can return a retained owner through output.
/// @param physical Borrowed Vulkan physical device with a valid Windows LUID.
/// @param output Required owner slot; release an attached owner only with checked destruction.
/// @return Zero on success; local, HRESULT or FidelityFX failure code otherwise.
extern "C" RSF_FSR4_BRIDGE_API int VulkanStoryFsr4Create(
    VkPhysicalDevice physical, uint32_t renderWidth, uint32_t renderHeight,
    uint32_t displayWidth, uint32_t displayHeight, VulkanStoryFsr4** output) {
    if (!output) return -1;
    *output = nullptr;
    if (!physical) return -1;
    HMODULE loader = GetModuleHandleW(L"vulkan-1.dll");
    auto properties = loader ? reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(
        GetProcAddress(loader, "vkGetPhysicalDeviceProperties2")) : nullptr;
    if (!properties) return -3;
    VkPhysicalDeviceIDProperties id{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES};
    VkPhysicalDeviceProperties2 props{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2}; props.pNext = &id;
    properties(physical, &props);
    uint64_t luid = 0; std::memcpy(&luid, id.deviceLUID, sizeof(luid));
    if (!id.deviceLUIDValid) return -4;
    const rsf_fsr4sr_request request{luid, nullptr, renderWidth, renderHeight, displayWidth, displayHeight, 0};
    return rsf_core_fsr4sr_create(&kPlatform, &request, output);
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
extern "C" RSF_FSR4_BRIDGE_API int VulkanStoryFsr4CreateSharedImage(
    VulkanStoryFsr4* value, uint32_t width, uint32_t height, uint32_t vkFormat,
    uint32_t writable, HANDLE* handle, ID3D12Resource** resource) {
    return rsf_core_fsr4sr_create_image(value, width, height, vkFormat, writable, handle,
        reinterpret_cast<void**>(resource));
}

/// @brief Releases the caller-owned DX12 image reference; a null reference is accepted.
/// @details This does not wait. Vulkan import users and DX12 queue users must already be complete.
extern "C" RSF_FSR4_BRIDGE_API void VulkanStoryFsr4ReleaseImage(ID3D12Resource* resource) {
    Release(resource);
}

/// @brief Creates or reuses the runtime-owned DX12 shared fence and exports a new NT handle.
/// @details The caller owns and closes the returned handle after permanent Vulkan timeline import; the fence remains owned by value.
/// @return Zero on success, or a local/HRESULT error.
extern "C" RSF_FSR4_BRIDGE_API int VulkanStoryFsr4CreateSharedFence(
    VulkanStoryFsr4* value, HANDLE* handle) {
    return rsf_core_fsr4sr_create_fence(value, handle);
}

/// @brief Records and submits reconstruction after a GPU wait for the Vulkan ready value.
/// @details Valid ready/done values always receive an ordered handoff, including SDK failures. A prepared owner refuses new work.
extern "C" RSF_FSR4_BRIDGE_API int VulkanStoryFsr4Evaluate(
    VulkanStoryFsr4* value, const VulkanStoryFsr4Frame* frame) {
    return rsf_core_fsr4sr_evaluate_legacy(value, frame);
}

extern "C" RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeEvaluate(rsf_fsr4_bridge* value,
    const rsf_fsr4_bridge_frame* frame) {
    return rsf_core_fsr4sr_evaluate(value, frame);
}

extern "C" RSF_FSR4_BRIDGE_API int32_t RsfFsr4BridgeSharpness(rsf_fsr4_bridge* value, float amount) {
    return rsf_core_fsr4sr_sharpness(value, amount);
}
