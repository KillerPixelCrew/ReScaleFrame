// SPDX-License-Identifier: GPL-3.0-only
// Fake vendor DLLs for the census test. One source builds four modules, selected by definition,
// each named like the vendor DLL the census hooks. Every function does a little real work, stores
// what it received for the test to compare, and returns a value the test knows.
#include <windows.h>

#include "vendor_census_ngx_params.h"

#define FIXTURE_EXPORT extern "C" __declspec(dllexport)

#if defined(FIXTURE_FSR2)

struct Seen {
    void* context;
    const void* desc;
    int calls;
};
static Seen g_seen;

FIXTURE_EXPORT int ffxFsr2ContextCreate(void* context, const void* desc) {
    g_seen.context = context;
    g_seen.desc = desc;
    ++g_seen.calls;
    return 0x2001;
}
FIXTURE_EXPORT int ffxFsr2ContextDispatch(void* context, const void* desc) {
    g_seen.context = context;
    g_seen.desc = desc;
    ++g_seen.calls;
    return 0x1234ABCD;
}
FIXTURE_EXPORT int ffxFsr2GetJitterOffset(float* x, float* y, int index, int phase_count) {
    *x = 0.5f * static_cast<float>(index % phase_count);
    *y = -0.25f * static_cast<float>(index);
    ++g_seen.calls;
    return 0;
}
FIXTURE_EXPORT float ffxFsr2GetUpscaleRatioFromQualityMode(int mode) {
    ++g_seen.calls;
    return mode == 2 ? 1.7f : 1.5f;
}
FIXTURE_EXPORT void fixture_fsr2_seen(void** context, const void** desc, int* calls) {
    *context = g_seen.context;
    *desc = g_seen.desc;
    *calls = g_seen.calls;
}

#elif defined(FIXTURE_NGX)

struct NgxSeen {
    void* cmd;
    void* handle;
    void* params;
    unsigned feature;
    int creates;
    int evaluates;
};
static NgxSeen g_ngx;
static FakeNgxParams g_params;
static int g_handle_storage;

FIXTURE_EXPORT int NVSDK_NGX_D3D12_GetParameters(NgxParam** out) {
    *out = &g_params;
    return 1;
}
FIXTURE_EXPORT int NVSDK_NGX_D3D12_CreateFeature(void* cmd, unsigned feature, NgxParam* params, void** handle) {
    g_ngx.cmd = cmd;
    g_ngx.feature = feature;
    g_ngx.params = params;
    ++g_ngx.creates;
    *handle = &g_handle_storage;
    return 1;
}
FIXTURE_EXPORT int NVSDK_NGX_D3D12_EvaluateFeature(void* cmd, void* handle, NgxParam* params, void* cb) {
    g_ngx.cmd = cmd;
    g_ngx.handle = handle;
    g_ngx.params = params;
    ++g_ngx.evaluates;
    return cb ? 0 : 1;
}
FIXTURE_EXPORT int NVSDK_NGX_D3D12_ReleaseFeature(void* handle) {
    g_ngx.handle = handle;
    return 1;
}
FIXTURE_EXPORT void fixture_ngx_seen(void** cmd, void** handle, void** params, unsigned* feature, int* creates,
                                     int* evaluates) {
    *cmd = g_ngx.cmd;
    *handle = g_ngx.handle;
    *params = g_ngx.params;
    *feature = g_ngx.feature;
    *creates = g_ngx.creates;
    *evaluates = g_ngx.evaluates;
}

#elif defined(FIXTURE_NVAPI)

struct NvSeen {
    void* device;
    void* params;
    int calls;
};
static NvSeen g_nv;

static int nv_marker(void* device, void* params) {
    g_nv.device = device;
    g_nv.params = params;
    ++g_nv.calls;
    return 0x5151;
}
static int nv_sleep_mode(void* device, void* params) {
    g_nv.device = device;
    g_nv.params = params;
    g_nv.calls += 10;
    return 0x5252;
}
FIXTURE_EXPORT void* nvapi_QueryInterface(unsigned id) {
    switch (id) {
    case 0xD9984C05: return reinterpret_cast<void*>(&nv_marker);
    case 0xAC1CA9E0: return reinterpret_cast<void*>(&nv_sleep_mode);
    default: return nullptr;
    }
}
FIXTURE_EXPORT void* fixture_nvapi_raw_marker() { return reinterpret_cast<void*>(&nv_marker); }
FIXTURE_EXPORT void fixture_nvapi_seen(void** device, void** params, int* calls) {
    *device = g_nv.device;
    *params = g_nv.params;
    *calls = g_nv.calls;
}

#elif defined(FIXTURE_XESS)

struct XessSeen {
    void* context;
    float x, y;
    int calls;
    const void* params;
};
static XessSeen g_xess;

FIXTURE_EXPORT int xessSetJitterScale(void* context, float x, float y) {
    g_xess.context = context;
    g_xess.x = x;
    g_xess.y = y;
    ++g_xess.calls;
    return 0;
}
FIXTURE_EXPORT int xessD3D12Execute(void* context, void* cmd, const void* params) {
    g_xess.context = context;
    g_xess.params = params;
    ++g_xess.calls;
    return cmd ? 0x77 : 0x78;
}
FIXTURE_EXPORT void fixture_xess_seen(void** context, float* x, float* y, int* calls) {
    *context = g_xess.context;
    *x = g_xess.x;
    *y = g_xess.y;
    *calls = g_xess.calls;
}

#elif defined(FIXTURE_VK)

// Vulkan handles are opaque integers here; the functions record what reached them.
struct VkSeen {
    void* queue;
    unsigned submits;
    unsigned presents;
    const void* present_info;
};
static VkSeen g_vk;

static int vk_create_instance(const void*, const void*, void** out) {
    *out = reinterpret_cast<void*>(0x1001);
    return 0;
}
static int vk_create_device(void*, const void*, const void*, void** out) {
    *out = reinterpret_cast<void*>(0x2002);
    return 0;
}
static void vk_get_device_queue(void*, unsigned family, unsigned index, void** out) {
    *out = reinterpret_cast<void*>(static_cast<uintptr_t>(0x3000 + family * 16 + index));
}
static int vk_queue_submit(void* queue, unsigned count, const void*, unsigned long long) {
    g_vk.queue = queue;
    g_vk.submits += count;
    return 0x10;
}
static int vk_queue_submit2(void* queue, unsigned count, const void*, unsigned long long) {
    g_vk.queue = queue;
    g_vk.submits += count;
    return 0x11;
}
static int vk_queue_present(void* queue, const void* info) {
    g_vk.queue = queue;
    g_vk.present_info = info;
    ++g_vk.presents;
    return 0;
}
static int vk_create_swapchain(void*, const void*, const void*, unsigned long long* out) {
    *out = 0x4004;
    return 0;
}

static void* vk_lookup(const char* name) {
    struct Entry {
        const char* name;
        void* fn;
    };
    static const Entry kTable[] = {
        {"vkCreateInstance", reinterpret_cast<void*>(&vk_create_instance)},
        {"vkCreateDevice", reinterpret_cast<void*>(&vk_create_device)},
        {"vkGetDeviceQueue", reinterpret_cast<void*>(&vk_get_device_queue)},
        {"vkQueueSubmit", reinterpret_cast<void*>(&vk_queue_submit)},
        {"vkQueueSubmit2", reinterpret_cast<void*>(&vk_queue_submit2)},
        {"vkQueuePresentKHR", reinterpret_cast<void*>(&vk_queue_present)},
        {"vkCreateSwapchainKHR", reinterpret_cast<void*>(&vk_create_swapchain)},
    };
    for (const Entry& e : kTable) {
        if (!std::strcmp(e.name, name)) return e.fn;
    }
    return nullptr;
}

FIXTURE_EXPORT void* vkGetInstanceProcAddr(void*, const char* name) { return vk_lookup(name); }
FIXTURE_EXPORT void* vkGetDeviceProcAddr(void*, const char* name) { return vk_lookup(name); }
FIXTURE_EXPORT void* fixture_vk_raw(const char* name) { return vk_lookup(name); }
FIXTURE_EXPORT void fixture_vk_seen(void** queue, unsigned* submits, unsigned* presents, const void** present_info) {
    *queue = g_vk.queue;
    *submits = g_vk.submits;
    *presents = g_vk.presents;
    *present_info = g_vk.present_info;
}

#elif defined(FIXTURE_CREATION)

// Resolve graphics exports from DllMain, before the census LoadLibrary wrapper
// gets its post_load callback. Overlay installers can do exactly this.
// Export aliases avoid redeclaring the Windows SDK's imported functions.
#pragma comment(linker, "/export:CreateDXGIFactory=fixture_CreateDXGIFactory")
#pragma comment(linker, "/export:CreateDXGIFactory1=fixture_CreateDXGIFactory1")
#pragma comment(linker, "/export:CreateDXGIFactory2=fixture_CreateDXGIFactory2")
#pragma comment(linker, "/export:D3D12CreateDevice=fixture_D3D12CreateDevice")
FIXTURE_EXPORT int fixture_CreateDXGIFactory(const void*, void**) { return 0x51; }
FIXTURE_EXPORT int fixture_CreateDXGIFactory1(const void*, void**) { return 0x52; }
FIXTURE_EXPORT int fixture_CreateDXGIFactory2(unsigned, const void*, void**) { return 0x53; }
FIXTURE_EXPORT int fixture_D3D12CreateDevice(void*, int, const void*, void**) { return 0x54; }
static FARPROC early_creation[4]{};
static const char* const creation_names[] = {
    "CreateDXGIFactory", "CreateDXGIFactory1", "CreateDXGIFactory2", "D3D12CreateDevice"};
static FARPROC raw_creation[] = {
    reinterpret_cast<FARPROC>(&fixture_CreateDXGIFactory), reinterpret_cast<FARPROC>(&fixture_CreateDXGIFactory1),
    reinterpret_cast<FARPROC>(&fixture_CreateDXGIFactory2), reinterpret_cast<FARPROC>(&fixture_D3D12CreateDevice)};
BOOL WINAPI DllMain(HINSTANCE module, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH)
        for (unsigned i = 0; i < 4; ++i) early_creation[i] = GetProcAddress(module, creation_names[i]);
    return TRUE;
}
FIXTURE_EXPORT unsigned fixture_creation_identity() {
    unsigned matched = 0;
    for (unsigned i = 0; i < 4; ++i)
        if (early_creation[i] == raw_creation[i]) matched |= 1u << i;
    return matched;
}
FIXTURE_EXPORT void* fixture_creation_raw(unsigned i) {
    return i < 4 ? reinterpret_cast<void*>(raw_creation[i]) : nullptr;
}

#elif defined(FIXTURE_SL)

static int sl_reflex_sleep(const void*) { return 0x31; }
static int sl_pcl_marker(unsigned, const void*) { return 0x32; }
static int sl_dlssg_options(const void*, const void*) { return 0x33; }

FIXTURE_EXPORT int slInit(const void*, unsigned long long) { return 0; }
FIXTURE_EXPORT int slSetConstants(const void*, const void*, const void*) { return 0x34; }
FIXTURE_EXPORT int slGetFeatureFunction(unsigned, const char* name, void*& function) {
    if (!std::strcmp(name, "slReflexSleep")) {
        function = reinterpret_cast<void*>(&sl_reflex_sleep);
    } else if (!std::strcmp(name, "slPCLSetMarker")) {
        function = reinterpret_cast<void*>(&sl_pcl_marker);
    } else if (!std::strcmp(name, "slDLSSGSetOptions")) {
        function = reinterpret_cast<void*>(&sl_dlssg_options);
    } else {
        return 7; // eErrorMissingInputParameter
    }
    return 0;
}
FIXTURE_EXPORT void* fixture_sl_raw(const char* name) {
    if (!std::strcmp(name, "slReflexSleep")) return reinterpret_cast<void*>(&sl_reflex_sleep);
    if (!std::strcmp(name, "slPCLSetMarker")) return reinterpret_cast<void*>(&sl_pcl_marker);
    return reinterpret_cast<void*>(&sl_dlssg_options);
}

#elif defined(FIXTURE_FFX)

struct FfxSeen {
    void* context;
    const void* desc;
    int calls;
};
static FfxSeen g_ffx;

FIXTURE_EXPORT unsigned ffxCreateContext(void** context, void* desc, const void*) {
    g_ffx.desc = desc;
    ++g_ffx.calls;
    *context = reinterpret_cast<void*>(0xF00D);
    return 0;
}
FIXTURE_EXPORT unsigned ffxDestroyContext(void** context, const void*) {
    g_ffx.context = *context;
    ++g_ffx.calls;
    return 0x43;
}
FIXTURE_EXPORT unsigned ffxConfigure(void** context, const void* desc) {
    g_ffx.context = *context;
    g_ffx.desc = desc;
    ++g_ffx.calls;
    return 0x41;
}
FIXTURE_EXPORT unsigned ffxQuery(void** context, void* desc) {
    g_ffx.context = *context;
    g_ffx.desc = desc;
    ++g_ffx.calls;
    return 0x42;
}
FIXTURE_EXPORT unsigned ffxDispatch(void** context, const void* desc) {
    g_ffx.context = *context;
    g_ffx.desc = desc;
    ++g_ffx.calls;
    return 0x40;
}
FIXTURE_EXPORT void fixture_ffx_seen(void** context, const void** desc, int* calls) {
    *context = g_ffx.context;
    *desc = g_ffx.desc;
    *calls = g_ffx.calls;
}

#else
#error "select a fixture"
#endif
