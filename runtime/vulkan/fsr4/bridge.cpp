// AMD FSR 4 DX12 upscaler on the Vulkan adapter. Shared D3D12 resources are
// imported by Vulkan; a D3D12 fence orders both queues without CPU readback.
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <vulkan/vulkan.h>
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <memory>
#include <new>
#include <string>
#include <cmath>
#include <limits>
#include "api/include/dx12/ffx_api_dx12.h"
#include "upscalers/include/ffx_upscale.h"
#include "fsr4_compat_core.h"
#define RSF_FSR4_BRIDGE_BUILD
#include "bridge_api.h"

/// @brief Releases a local COM reference and clears its pointer without waiting for GPU work.
template<class T> static void Release(T*& object) {
    if (object) { object->Release(); object = nullptr; }
}

/// @brief Fixed-layout reconstruction frame shared with the managed FSR4 ABI.
/// @details All four resource pointers borrow caller-owned shared images in COMMON state. Vulkan signals readyValue after input copies; DX12 signals doneValue after reconstruction/output handoff.
struct VulkanStoryFsr4Frame {
    ID3D12Resource* color;
    ID3D12Resource* depth;
    ID3D12Resource* motion;
    ID3D12Resource* output;
    uint32_t renderWidth, renderHeight, displayWidth, displayHeight;
    float jitterX, jitterY, deltaMs, nearPlane, farPlane, fov;
    uint32_t reset;
    uint64_t readyValue, doneValue;
};
static_assert(sizeof(VulkanStoryFsr4Frame) == 96 &&
    offsetof(VulkanStoryFsr4Frame, readyValue) == 80);

/// @brief Owns the signed-runtime module, matching DX12 adapter/device, SDK context, queue and allocator-completion tracking.
/// @details Imported image/resource ownership stays with the caller. A checked release failure retains this owner and prevents later destructive cleanup.
/// The NVIDIA/Intel INT8 compatibility lease must outlive the SDK context; it is released after context destruction.
struct VulkanStoryFsr4 {
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
    uint64_t allocatorValues[3]{};
    ID3D12GraphicsCommandList* commands{};
    uint64_t completionValue{};
    uint32_t nextAllocator{};
    ffxContext effect{};
    PfnFfxCreateContext create{};
    PfnFfxDestroyContext destroy{};
    PfnFfxDispatch dispatch{};
    PfnFfxQuery query{};
    int releaseFailure{};
    bool releasePrepared{};
    uint64_t adapterLuid{}, providerVersion{}, lastDoneSignalled{};
    uint32_t vendor{}, maxRenderWidth{}, maxRenderHeight{}, maxOutputWidth{}, maxOutputHeight{};
    bool depthInverted{}, depthInfinite{};
};

/// @brief Whether a PCI vendor is an FSR4 candidate: AMD natively, NVIDIA/Intel through the INT8 compatibility lease.
static bool IsCandidateVendor(uint32_t vendor) {
    return vendor == 0x1002 || vendor == 0x10de || vendor == 0x8086;
}

/// @brief Checks the Vulkan adapter vendor required by this FSR4 bridge.
/// @details The vendor check is preliminary; NVIDIA/Intel compatibility and signed-provider selection are verified during Create.
/// @return Zero for an AMD, NVIDIA or Intel Vulkan adapter, otherwise a negative availability code.
extern "C" __declspec(dllexport) int VulkanStoryFsr4Probe(VkPhysicalDevice physical) {
    if (!physical) return -1;
    HMODULE loader = GetModuleHandleW(L"vulkan-1.dll");
    auto properties = loader ? reinterpret_cast<PFN_vkGetPhysicalDeviceProperties>(
        GetProcAddress(loader, "vkGetPhysicalDeviceProperties")) : nullptr;
    if (!properties) return -2;
    VkPhysicalDeviceProperties info{};
    properties(physical, &info);
    return IsCandidateVendor(info.vendorID) ? 0 : -3;
}

// Ported from ReScaleFrame runtime/backends/fsr/src/ffx_version.h (same author, GPL-3.0-only).
/// @brief Picks the newest provider of one FidelityFX major version out of an ffxQuery GetVersions answer.
/// @details IDs are opaque; the family comes from the SDK's paired "major.minor.patch" display name, never from bit masks.
/// @return False when nothing matches.
static bool SelectNewestVersion(const uint64_t* ids, const char* const* names, uint64_t count,
    unsigned major, uint64_t& id) {
    bool found = false;
    unsigned bestMinor = 0, bestPatch = 0;
    for (uint64_t i = 0; i < count; ++i) {
        unsigned foundMajor = 0, minor = 0, patch = 0;
        if (!names[i] || std::sscanf(names[i], "%u.%u.%u", &foundMajor, &minor, &patch) != 3 ||
            foundMajor != major) continue;
        if (!found || minor > bestMinor || (minor == bestMinor && patch > bestPatch)) {
            found = true; id = ids[i]; bestMinor = minor; bestPatch = patch;
        }
    }
    return found;
}

/// @brief Whether a provider display name identifies FSR4 ("4.x.y" in SDK 2.x, or an "FSR4"/"FSR 4" label).
static bool IsFsr4Name(const char* name) {
    if (!name) return false;
    unsigned major = 0, minor = 0, patch = 0;
    if (std::sscanf(name, "%u.%u.%u", &major, &minor, &patch) == 3) return major == 4;
    return std::strstr(name, "FSR4") || std::strstr(name, "FSR 4");
}

/// @brief Locates the bridge directory used for its sibling signed FSR4 runtime.
static std::wstring OwnDirectory() {
    HMODULE own{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(&OwnDirectory), &own)) return {};
    wchar_t path[MAX_PATH]{};
    DWORD length = GetModuleFileNameW(own, path, MAX_PATH);
    if (!length || length == MAX_PATH) return {};
    std::wstring result(path, length);
    auto slash = result.find_last_of(L"\\/");
    return slash == std::wstring::npos ? std::wstring{} : result.substr(0, slash + 1);
}

/// @brief Waits up to ten seconds for a tracked local DX12 completion-fence value.
/// @details Device removal is a failure, not completion; a retained release failure is returned before any new wait.
static int WaitForCompletion(VulkanStoryFsr4* value, uint64_t target) {
    if (value->releaseFailure != 0) return value->releaseFailure;
    if (!target) return 0;
    if (!value->completionFence) return value->releaseFailure = static_cast<int>(E_UNEXPECTED);
    uint64_t completed = value->completionFence->GetCompletedValue();
    if (completed == UINT64_MAX)
        return value->releaseFailure = static_cast<int>(DXGI_ERROR_DEVICE_REMOVED);
    if (completed >= target) return 0;
    if (!value->completionEvent) return -1;
    HRESULT hr = value->completionFence->SetEventOnCompletion(
        target, value->completionEvent);
    if (FAILED(hr)) return static_cast<int>(hr);
    if (WaitForSingleObject(value->completionEvent, 10000) != WAIT_OBJECT_0) return -2;
    completed = value->completionFence->GetCompletedValue();
    if (completed == UINT64_MAX)
        return value->releaseFailure = static_cast<int>(DXGI_ERROR_DEVICE_REMOVED);
    return completed >= target ? 0 : -2;
}

/// @brief Waits for the most recent tracked submission on this DX12 queue.
/// @return Zero when complete or value is null; nonzero on completion failure.
extern "C" __declspec(dllexport) int VulkanStoryFsr4WaitIdle(VulkanStoryFsr4* value) {
    return value ? WaitForCompletion(value, value->completionValue) : 0;
}

/// @brief Drains tracked DX12 work and destroys the SDK consumer before imported resources are retired.
/// @details Failure is persisted as terminal releaseFailure; native ownership is retained.
/// @return Zero when prepared, or the retained drain/SDK destruction error.
extern "C" __declspec(dllexport) int VulkanStoryFsr4PrepareDestroy(VulkanStoryFsr4* value) {
    if (!value) return 0;
    if (value->releaseFailure != 0) return value->releaseFailure;
    if (value->releasePrepared) return 0;
    int idle = VulkanStoryFsr4WaitIdle(value);
    if (idle != 0) return value->releaseFailure = idle;
    if (value->effect) {
        if (!value->destroy) return value->releaseFailure = -1;
        ffxContext owned = value->effect;
        int result = static_cast<int>(value->destroy(&owned, nullptr));
        if (result != 0) return value->releaseFailure = result;
        value->effect = nullptr;
    }
    value->releasePrepared = true;
    return 0;
}
/// @brief Performs checked release preparation then frees native/COM owners and the context allocation.
/// @details The caller must first drain Vulkan users and dispose imported shared images/fence imports before destroying their owning DX12 context. Failure leaves the owner attached.
/// @return Zero after successful destruction; a nonzero preparation error preserves the value pointer.
extern "C" __declspec(dllexport) int VulkanStoryFsr4DestroyChecked(VulkanStoryFsr4* value) {
    if (!value) return 0;
    int prepared = VulkanStoryFsr4PrepareDestroy(value);
    if (prepared != 0) return prepared;
    // The SDK context is destroyed; dropping the lease restores native capability answers.
    value->compatibility.reset();
    Release(value->commands);
    for (auto*& allocator : value->allocators) Release(allocator);
    Release(value->completionFence);
    Release(value->sharedFence);
    Release(value->queue);
    Release(value->device);
    Release(value->adapter);
    Release(value->factory);
    if (value->completionEvent) CloseHandle(value->completionEvent);
    if (value->module) FreeLibrary(value->module);
    delete value;
    return 0;
}
/// @brief Legacy void destruction entry point that preserves ownership when checked preparation fails.
/// @details New callers should use DestroyChecked to observe release failure.
extern "C" __declspec(dllexport) void VulkanStoryFsr4Destroy(VulkanStoryFsr4* value) {
    VulkanStoryFsr4DestroyChecked(value);
}

/// @brief Creates the signed FSR4 DX12 context on the adapter matching the Vulkan device LUID.
/// @details NVIDIA/Intel first acquire the INT8 compatibility lease (-12 when refused). The newest 4.x provider is forced with ffxOverrideVersion and the effective provider must match it (-9 otherwise; -11 for an unsupported vendor). On initialization failure a failed checked cleanup can return a retained owner through output.
/// @param physical Borrowed Vulkan physical device with a valid Windows LUID.
/// @param renderWidth Maximum input width.
/// @param renderHeight Maximum input height.
/// @param displayWidth Maximum output width.
/// @param displayHeight Maximum output height.
/// @param output Required owner slot; release an attached owner only with checked destruction.
/// @return Zero on success; local, HRESULT or FidelityFX failure code otherwise.
static int CreateContext(uint64_t luid, const wchar_t* runtimeDirectory,
    uint32_t renderWidth, uint32_t renderHeight, uint32_t displayWidth,
    uint32_t displayHeight, uint32_t flags, VulkanStoryFsr4** output) {
    if (!luid || !output || !renderWidth || !renderHeight ||
        !displayWidth || !displayHeight) return -1;
    *output = nullptr;
    auto* value = new (std::nothrow) VulkanStoryFsr4();
    if (!value) return -2;
    int error = 0;
    do {
        uint32_t vendor{};
        HRESULT hr = CreateDXGIFactory2(0, IID_PPV_ARGS(&value->factory));
        if (FAILED(hr)) { error = static_cast<int>(hr); break; }
        for (UINT index = 0; ; ++index) {
            IDXGIAdapter1* candidate{};
            hr = value->factory->EnumAdapters1(index, &candidate);
            if (hr == DXGI_ERROR_NOT_FOUND) break;
            if (FAILED(hr)) { error = static_cast<int>(hr); break; }
            DXGI_ADAPTER_DESC1 description{};
            hr = candidate->GetDesc1(&description);
            if (FAILED(hr)) { Release(candidate); error = static_cast<int>(hr); break; }
            uint64_t candidateLuid{};
            std::memcpy(&candidateLuid, &description.AdapterLuid, sizeof(candidateLuid));
            if (candidateLuid == luid) {
                value->adapter = candidate;
                vendor = description.VendorId;
                break;
            }
            candidate->Release();
        }
        if (error != 0) break;
        if (!value->adapter) { error = -5; break; }
        if (!IsCandidateVendor(vendor)) { error = -11; break; }
        hr = D3D12CreateDevice(value->adapter, D3D_FEATURE_LEVEL_12_0,
            IID_PPV_ARGS(&value->device));
        if (FAILED(hr)) { error = static_cast<int>(hr); break; }
        D3D12_COMMAND_QUEUE_DESC queue{};
        queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        hr = value->device->CreateCommandQueue(&queue, IID_PPV_ARGS(&value->queue));
        if (FAILED(hr)) { error = static_cast<int>(hr); break; }
        std::wstring base = runtimeDirectory ? runtimeDirectory : OwnDirectory();
        if (base.empty()) { error = -6; break; }
        if (base.back() != L'\\' && base.back() != L'/') base += L'\\';
        value->module = LoadLibraryExW((base + L"amd_fidelityfx_upscaler_dx12.dll").c_str(),
            nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        if (!value->module) { error = -7; break; }
        value->create = reinterpret_cast<PfnFfxCreateContext>(GetProcAddress(value->module, "ffxCreateContext"));
        value->destroy = reinterpret_cast<PfnFfxDestroyContext>(GetProcAddress(value->module, "ffxDestroyContext"));
        value->dispatch = reinterpret_cast<PfnFfxDispatch>(GetProcAddress(value->module, "ffxDispatch"));
        value->query = reinterpret_cast<PfnFfxQuery>(GetProcAddress(value->module, "ffxQuery"));
        if (!value->create || !value->destroy || !value->dispatch || !value->query) {
            error = -8; break;
        }
        // NVIDIA/Intel run the INT8 provider through the pinned-SDK capability lease. It must be
        // active before version enumeration, which filters providers by device support. A refusal
        // is terminal; there is no fallback to an older provider.
        if (vendor == 0x10de || vendor == 0x8086) {
            value->compatibility = rsf_fsr4_enable_int8_adapter(value->module, value->adapter, value->device, nullptr, nullptr);
            if (!value->compatibility) { error = -12; break; }
        }
        // Ported from ReScaleFrame fsr_sr.inl: force the newest 4.x provider by display name.
        uint64_t count = 32;
        uint64_t ids[32]{};
        const char* names[32]{};
        ffxQueryDescGetVersions versions{};
        versions.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
        versions.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
        versions.device = value->device;
        versions.outputCount = &count;
        versions.versionIds = ids;
        versions.versionNames = names;
        error = static_cast<int>(value->query(nullptr, &versions.header));
        if (error) break;
        uint64_t selected{};
        if (count > 32 || !SelectNewestVersion(ids, names, count, 4, selected)) { error = -9; break; }
        ffxCreateContextDescUpscale upscale{};
        upscale.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
        upscale.flags = FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE | FFX_UPSCALE_ENABLE_AUTO_EXPOSURE;
        upscale.flags |= flags;
        upscale.maxRenderSize = {renderWidth, renderHeight};
        upscale.maxUpscaleSize = {displayWidth, displayHeight};
        ffxCreateBackendDX12Desc backend{};
        backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12;
        backend.device = value->device;
        ffxCreateContextDescUpscaleVersion version{};
        version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE_VERSION;
        version.version = FFX_UPSCALER_VERSION;
        ffxOverrideVersion overrideVersion{};
        overrideVersion.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION;
        overrideVersion.versionId = selected;
        upscale.header.pNext = &backend.header;
        backend.header.pNext = &version.header;
        version.header.pNext = &overrideVersion.header;
        error = static_cast<int>(value->create(&value->effect, &upscale.header, nullptr));
        if (error) break;
        ffxQueryGetProviderVersion provider{};
        provider.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
        error = static_cast<int>(value->query(&value->effect, &provider.header));
        if (error) break;
        // The signed loader can select a legacy FSR provider on older GPUs.
        // This option promises the ML upscaler, so reject that substitution.
        if (provider.versionId != selected || !IsFsr4Name(provider.versionName)) { error = -9; break; }
        value->adapterLuid = luid; value->providerVersion = provider.versionId; value->vendor = vendor;
        value->maxRenderWidth = renderWidth; value->maxRenderHeight = renderHeight;
        value->maxOutputWidth = displayWidth; value->maxOutputHeight = displayHeight;
        value->depthInverted = (flags & FFX_UPSCALE_ENABLE_DEPTH_INVERTED) != 0;
        value->depthInfinite = (flags & FFX_UPSCALE_ENABLE_DEPTH_INFINITE) != 0;
        hr = value->device->CreateFence(0, D3D12_FENCE_FLAG_NONE,
            IID_PPV_ARGS(&value->completionFence));
        if (FAILED(hr)) { error = static_cast<int>(hr); break; }
        value->completionEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!value->completionEvent) { error = -10; break; }
        for (auto*& allocator : value->allocators) {
            hr = value->device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
                IID_PPV_ARGS(&allocator));
            if (FAILED(hr)) { error = static_cast<int>(hr); break; }
        }
        if (error) break;
        hr = value->device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
            value->allocators[0], nullptr, IID_PPV_ARGS(&value->commands));
        if (FAILED(hr)) { error = static_cast<int>(hr); break; }
        hr = value->commands->Close();
        if (FAILED(hr)) { error = static_cast<int>(hr); break; }
    } while (false);
    if (error) {
        if (VulkanStoryFsr4DestroyChecked(value) != 0) *output = value;
        return error;
    }
    *output = value;
    return 0;
}

extern "C" __declspec(dllexport) int VulkanStoryFsr4Create(
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
    return CreateContext(luid, nullptr, renderWidth, renderHeight, displayWidth, displayHeight, 0, output);
}

extern "C" __declspec(dllexport) int32_t RsfFsr4BridgeCreate(const rsf_fsr4_bridge_create* setup,
    rsf_fsr4_bridge** output) {
    if (!output) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    *output = nullptr;
    if (!setup) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    int32_t check = rsf_fsr4_bridge_check(setup->struct_size, setup->abi_version, sizeof(*setup));
    if (check) return check;
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    const auto* directory = reinterpret_cast<const wchar_t*>(setup->runtime_directory);
    if (!directory || !directory[0] || !((directory[1] == L':' && (directory[2] == L'\\' || directory[2] == L'/')) ||
        (directory[0] == L'\\' && directory[1] == L'\\')) || setup->reserved || setup->depth_inverted > 1 ||
        setup->depth_infinite > 1 || setup->motion_jittered > 1) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    uint32_t flags = setup->depth_inverted ? FFX_UPSCALE_ENABLE_DEPTH_INVERTED : 0;
    if (setup->depth_infinite) flags |= FFX_UPSCALE_ENABLE_DEPTH_INFINITE;
    if (setup->motion_jittered) flags |= FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION;
    return CreateContext(setup->adapter_luid, directory, setup->render_width, setup->render_height,
        setup->output_width, setup->output_height, flags, output);
}

extern "C" __declspec(dllexport) int32_t RsfFsr4BridgeInfo(const rsf_fsr4_bridge* value,
    rsf_fsr4_bridge_info* info) {
    if (!value || !info) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    int32_t check = rsf_fsr4_bridge_check(info->struct_size, info->abi_version, sizeof(*info));
    if (check) return check;
    info->adapter_luid = value->adapterLuid; info->provider_version = value->providerVersion;
    info->vendor_id = value->vendor; info->int8_compatibility = value->compatibility ? 1u : 0u;
    return 0;
}

extern "C" __declspec(dllexport) int32_t RsfFsr4BridgePlan(rsf_fsr4_bridge* value,
    uint32_t quality, uint32_t* width, uint32_t* height) {
    if (!value || !width || !height || quality > 4) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    *width = *height = 0;
    ffxQueryDescUpscaleGetRenderResolutionFromQualityMode plan{};
    plan.header.type = FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE;
    plan.displayWidth = value->maxOutputWidth; plan.displayHeight = value->maxOutputHeight;
    plan.qualityMode = quality; plan.pOutRenderWidth = width; plan.pOutRenderHeight = height;
    return static_cast<int>(value->query(&value->effect, &plan.header));
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
extern "C" __declspec(dllexport) int VulkanStoryFsr4CreateSharedImage(
    VulkanStoryFsr4* value, uint32_t width, uint32_t height, uint32_t vkFormat,
    uint32_t writable, HANDLE* handle, ID3D12Resource** resource) {
    if (!value || !width || !height || !handle || !resource) return -1;
    *handle = nullptr; *resource = nullptr;
    DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN;
    D3D12_RESOURCE_FLAGS flags = D3D12_RESOURCE_FLAG_NONE;
    switch (static_cast<VkFormat>(vkFormat)) {
        case VK_FORMAT_R16G16B16A16_SFLOAT: format = DXGI_FORMAT_R16G16B16A16_FLOAT; break;
        case VK_FORMAT_R16G16_SFLOAT: format = DXGI_FORMAT_R16G16_FLOAT; break;
        case VK_FORMAT_R32_SFLOAT: format = DXGI_FORMAT_R32_FLOAT; break;
        case VK_FORMAT_D32_SFLOAT:
            format = DXGI_FORMAT_D32_FLOAT;
            flags = D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
            break;
        default: return -2;
    }
    if (writable) {
        if (flags != D3D12_RESOURCE_FLAG_NONE) return -3;
        flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    }
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width; desc.Height = height;
    desc.DepthOrArraySize = 1; desc.MipLevels = 1;
    desc.Format = format; desc.SampleDesc.Count = 1;
    desc.Flags = flags;
    HRESULT hr = value->device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED,
        &desc, D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(resource));
    if (FAILED(hr)) return static_cast<int>(hr);
    hr = value->device->CreateSharedHandle(*resource, nullptr, GENERIC_ALL,
        nullptr, handle);
    if (FAILED(hr)) { Release(*resource); return static_cast<int>(hr); }
    return 0;
}

/// @brief Releases the caller-owned DX12 image reference; a null reference is accepted.
/// @details This does not wait. Vulkan import users and DX12 queue users must already be complete.
extern "C" __declspec(dllexport) void VulkanStoryFsr4ReleaseImage(ID3D12Resource* resource) {
    Release(resource);
}

/// @brief Creates or reuses the runtime-owned DX12 shared fence and exports a new NT handle.
/// @details The caller owns and closes the returned handle after permanent Vulkan timeline import; the fence remains owned by value.
/// @return Zero on success, or a local/HRESULT error.
extern "C" __declspec(dllexport) int VulkanStoryFsr4CreateSharedFence(
    VulkanStoryFsr4* value, HANDLE* handle) {
    if (!value || !handle) return -1;
    *handle = nullptr;
    if (!value->sharedFence) {
        HRESULT hr = value->device->CreateFence(0, D3D12_FENCE_FLAG_SHARED,
            IID_PPV_ARGS(&value->sharedFence));
        if (FAILED(hr)) return static_cast<int>(hr);
    }
    HRESULT hr = value->device->CreateSharedHandle(value->sharedFence,
        nullptr, GENERIC_ALL, nullptr, handle);
    return FAILED(hr) ? static_cast<int>(hr) : 0;
}

/// @brief Records an all-subresource DX12 state transition on the borrowed command list.
static void Transition(ID3D12GraphicsCommandList* commands, ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = resource;
    barrier.Transition.StateBefore = before;
    barrier.Transition.StateAfter = after;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    commands->ResourceBarrier(1, &barrier);
}

/// @brief Records and submits reconstruction after a GPU wait for the Vulkan ready value.
/// @details Shared images enter and leave COMMON. The local completion value is tracked immediately after submission; the shared done signal follows dispatch. Resources remain live until the appropriate completion/handoff values.
/// @param value Live runtime owner; calls are serialized by the host.
/// @param frame Borrowed fixed-layout resources and temporal constants for this dispatch.
/// @return Zero when submission/signals succeed; local, HRESULT or FidelityFX failure otherwise. Signal failure can retain a terminal release error.
static int EvaluateFrame(VulkanStoryFsr4* value, const VulkanStoryFsr4Frame* frame,
    float motionScaleX, float motionScaleY, float preExposure) {
    if (!value || !frame || !frame->color || !frame->depth || !frame->motion ||
        !frame->output || !frame->readyValue || !frame->doneValue ||
        !value->sharedFence) return -1;
    uint32_t slot = value->nextAllocator++ % 3;
    int idle = WaitForCompletion(value, value->allocatorValues[slot]);
    if (idle != 0) return idle;
    HRESULT hr = value->allocators[slot]->Reset();
    if (FAILED(hr)) return static_cast<int>(hr);
    hr = value->commands->Reset(value->allocators[slot], nullptr);
    if (FAILED(hr)) return static_cast<int>(hr);
    constexpr auto read = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    Transition(value->commands, frame->color, D3D12_RESOURCE_STATE_COMMON, read);
    Transition(value->commands, frame->depth, D3D12_RESOURCE_STATE_COMMON, read);
    Transition(value->commands, frame->motion, D3D12_RESOURCE_STATE_COMMON, read);
    Transition(value->commands, frame->output, D3D12_RESOURCE_STATE_COMMON,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    ffxDispatchDescUpscale upscale{};
    upscale.header.type = FFX_API_DISPATCH_DESC_TYPE_UPSCALE;
    upscale.commandList = value->commands;
    upscale.color = ffxApiGetResourceDX12(frame->color, FFX_API_RESOURCE_STATE_COMPUTE_READ);
    upscale.depth = ffxApiGetResourceDX12(frame->depth, FFX_API_RESOURCE_STATE_COMPUTE_READ);
    upscale.motionVectors = ffxApiGetResourceDX12(frame->motion, FFX_API_RESOURCE_STATE_COMPUTE_READ);
    upscale.output = ffxApiGetResourceDX12(frame->output, FFX_API_RESOURCE_STATE_UNORDERED_ACCESS);
    upscale.jitterOffset = {frame->jitterX, frame->jitterY};
    upscale.motionVectorScale = {motionScaleX, motionScaleY};
    upscale.renderSize = {frame->renderWidth, frame->renderHeight};
    upscale.upscaleSize = {frame->displayWidth, frame->displayHeight};
    upscale.frameTimeDelta = frame->deltaMs > 0.0f ? frame->deltaMs : 16.667f;
    upscale.preExposure = preExposure;
    upscale.reset = frame->reset != 0;
    upscale.cameraNear = frame->nearPlane;
    upscale.cameraFar = frame->farPlane;
    upscale.cameraFovAngleVertical = frame->fov;
    upscale.viewSpaceToMetersFactor = 1.0f;
    int code = static_cast<int>(value->dispatch(&value->effect, &upscale.header));
    if (code != 0) {
        value->commands->Close();
        return code;
    }
    Transition(value->commands, frame->color, read, D3D12_RESOURCE_STATE_COMMON);
    Transition(value->commands, frame->depth, read, D3D12_RESOURCE_STATE_COMMON);
    Transition(value->commands, frame->motion, read, D3D12_RESOURCE_STATE_COMMON);
    Transition(value->commands, frame->output, D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        D3D12_RESOURCE_STATE_COMMON);
    hr = value->commands->Close();
    if (FAILED(hr)) return static_cast<int>(hr);
    hr = value->queue->Wait(value->sharedFence, frame->readyValue);
    if (FAILED(hr)) return static_cast<int>(hr);
    ID3D12CommandList* lists[] = {value->commands};
    value->queue->ExecuteCommandLists(1, lists);
    uint64_t completed = ++value->completionValue;
    value->allocatorValues[slot] = completed;
    HRESULT handoff = value->queue->Signal(value->sharedFence, frame->doneValue);
    if (SUCCEEDED(handoff)) value->lastDoneSignalled = frame->doneValue;
    hr = value->queue->Signal(value->completionFence, completed);
    if (FAILED(hr)) return value->releaseFailure = static_cast<int>(hr);
    if (FAILED(handoff)) return value->releaseFailure = static_cast<int>(handoff);
    return 0;
}

static int FinishHandoff(VulkanStoryFsr4* value, const VulkanStoryFsr4Frame* frame, int result) {
    // The host may have already recorded a continuation waiting for done.
    // Every failure still respects producer readiness before releasing that wait.
    if (value && frame && value->queue && value->sharedFence && frame->readyValue &&
        frame->doneValue > frame->readyValue && frame->doneValue > value->lastDoneSignalled) {
        HRESULT wait = value->queue->Wait(value->sharedFence, frame->readyValue);
        if (FAILED(wait)) return value->releaseFailure = static_cast<int>(wait);
        HRESULT handoff = value->queue->Signal(value->sharedFence, frame->doneValue);
        if (SUCCEEDED(handoff)) value->lastDoneSignalled = frame->doneValue;
        uint64_t completed = ++value->completionValue;
        HRESULT retired = value->queue->Signal(value->completionFence, completed);
        if (FAILED(retired)) return value->releaseFailure = static_cast<int>(retired);
        if (FAILED(handoff)) return value->releaseFailure = static_cast<int>(handoff);
    }
    return result;
}

static int EvaluateChecked(VulkanStoryFsr4* value, const VulkanStoryFsr4Frame* frame,
    float motionScaleX, float motionScaleY, float preExposure) {
    return FinishHandoff(value, frame, EvaluateFrame(value, frame, motionScaleX, motionScaleY, preExposure));
}

extern "C" __declspec(dllexport) int VulkanStoryFsr4Evaluate(
    VulkanStoryFsr4* value, const VulkanStoryFsr4Frame* frame) {
    return EvaluateChecked(value, frame, 1.0f, 1.0f, 1.0f);
}

extern "C" __declspec(dllexport) int32_t RsfFsr4BridgeEvaluate(rsf_fsr4_bridge* value,
    const rsf_fsr4_bridge_frame* frame) {
    if (!value || !frame) return RSF_FSR4_BRIDGE_INVALID_ARGUMENT;
    int32_t check = rsf_fsr4_bridge_check(frame->struct_size, frame->abi_version, sizeof(*frame));
    if (check) return check;
    const auto reject = [&] {
        VulkanStoryFsr4Frame handoff{};
        handoff.readyValue = frame->ready_value; handoff.doneValue = frame->done_value;
        return FinishHandoff(value, &handoff, RSF_FSR4_BRIDGE_INVALID_ARGUMENT);
    };
    if (!frame->color || !frame->depth || !frame->motion || !frame->output || !frame->render_width ||
        !frame->render_height || !frame->output_width || !frame->output_height || frame->reset > 1 ||
        frame->render_width > value->maxRenderWidth || frame->render_height > value->maxRenderHeight ||
        frame->output_width > value->maxOutputWidth || frame->output_height > value->maxOutputHeight ||
        !frame->ready_value || frame->ready_value <= value->lastDoneSignalled || frame->done_value <= frame->ready_value ||
        !std::isfinite(frame->jitter_x) || !std::isfinite(frame->jitter_y) || !std::isfinite(frame->motion_scale_x) ||
        !std::isfinite(frame->motion_scale_y) || !frame->motion_scale_x || !frame->motion_scale_y ||
        !std::isfinite(frame->delta_ms) || frame->delta_ms <= 0 || !std::isfinite(frame->pre_exposure) || frame->pre_exposure <= 0 ||
        !std::isfinite(frame->near_plane) || frame->near_plane <= 0 || !std::isfinite(frame->far_plane) || frame->far_plane < 0 ||
        (!value->depthInfinite && frame->far_plane <= frame->near_plane) || !std::isfinite(frame->vertical_fov) ||
        frame->vertical_fov <= 0 || frame->vertical_fov >= 3.141593f) return reject();
    VulkanStoryFsr4Frame legacy{};
    legacy.color = static_cast<ID3D12Resource*>(frame->color); legacy.depth = static_cast<ID3D12Resource*>(frame->depth);
    legacy.motion = static_cast<ID3D12Resource*>(frame->motion); legacy.output = static_cast<ID3D12Resource*>(frame->output);
    const auto color = legacy.color->GetDesc(), depth = legacy.depth->GetDesc(), motion = legacy.motion->GetDesc(), output = legacy.output->GetDesc();
    if (color.Width != frame->render_width || color.Height != frame->render_height || color.Format != DXGI_FORMAT_R16G16B16A16_FLOAT ||
        depth.Width != color.Width || depth.Height != color.Height || depth.Format != DXGI_FORMAT_R32_FLOAT ||
        motion.Width != color.Width || motion.Height != color.Height || motion.Format != DXGI_FORMAT_R16G16_FLOAT ||
        output.Width != frame->output_width || output.Height != frame->output_height || output.Format != DXGI_FORMAT_R16G16B16A16_FLOAT)
        return reject();
    legacy.renderWidth = frame->render_width; legacy.renderHeight = frame->render_height;
    legacy.displayWidth = frame->output_width; legacy.displayHeight = frame->output_height;
    legacy.jitterX = frame->jitter_x; legacy.jitterY = frame->jitter_y; legacy.deltaMs = frame->delta_ms;
    legacy.nearPlane = frame->near_plane;
    legacy.farPlane = frame->far_plane > 0 ? frame->far_plane : std::numeric_limits<float>::max();
    legacy.fov = frame->vertical_fov; legacy.reset = frame->reset;
    legacy.readyValue = frame->ready_value; legacy.doneValue = frame->done_value;
    return EvaluateChecked(value, &legacy, frame->motion_scale_x, frame->motion_scale_y, frame->pre_exposure);
}
