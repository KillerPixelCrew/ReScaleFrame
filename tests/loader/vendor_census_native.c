/* SPDX-License-Identifier: MIT */
/* Native payload, SDK slot and caller-address evidence for the Rust census fixture.

   Fake vendor DLLs are called through a direct import, through LoadLibrary plus GetProcAddress
   and through nvapi_QueryInterface, with the census installed. The test checks that every call
   passes through unchanged and that the records, summary and byte budget say what was called. A
   real DXGI and D3D12 (WARP) path covers the swapchain hooks, and the Windows SDK vtable layouts
   check the slot numbers the census relies on.

     rsf_vendor_census_test functional <output dir>
     rsf_vendor_census_test budget <output dir>
     rsf_vendor_census_test vulkan|ffx|dllmain <output dir> */
#include <windows.h>

#include <d3d12.h>
#include <dxgi1_6.h>

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

#include "rescaleframe/vendor_census.h"
#include "rescaleframe/census_d3d12_observer.h"
#include "census_facts.h"
#include "ngx_param_probe.h"

int rsf_test_census_main(int argc, char** argv, const rsf_test_census_native* native);
HMODULE rsf_test_census_load_fixture(const wchar_t* file);
HMODULE rsf_test_census_loaded_module(const wchar_t* name);
bool rsf_test_census_warp(const void* payload, void (*dispatch)(const void*), uint64_t* calls,
                          int* failures);
__declspec(dllimport) int ffxFsr2ContextDispatch(void* context, const void* desc);
__declspec(dllimport) int ffxFsr2GetJitterOffset(float* x, float* y, int index, int phase_count);
__declspec(dllimport) float ffxFsr2GetUpscaleRatioFromQualityMode(int mode);
__declspec(dllimport) void fixture_fsr2_seen(void** context, const void** desc, int* calls);

static int g_failures = 0;

#define CHECK(cond)                                                     \
    do {                                                                \
        if (!(cond)) {                                                  \
            printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond);      \
            ++g_failures;                                               \
        }                                                               \
    } while (0)

/* A check whose failure leaves the rest of the phase nothing to call. */
#define REQUIRE(cond)                                                   \
    do {                                                                \
        CHECK(cond);                                                    \
        if (!(cond)) return;                                            \
    } while (0)
#define REQUIRE_RET(cond, value)                                        \
    do {                                                                \
        CHECK(cond);                                                    \
        if (!(cond)) return (value);                                    \
    } while (0)

/* Store `value` as `type` at a byte offset of a payload buffer. */
#define PUT(type, base, offset, value)                                           \
    do {                                                                         \
        type put_value_ = (value);                                               \
        memcpy((unsigned char*)(base) + (offset), &put_value_, sizeof(put_value_)); \
    } while (0)

/* Export lookup and pointer reinterpretation between function pointer types. */
#define FN_FROM_PTR(type, pointer) ((type)(uintptr_t)(pointer))
#define PROC(type, module, name) ((type)(uintptr_t)GetProcAddress((module), (name)))

/* ---- A fake ID3D12Resource -------------------------------------------------------------------- */
typedef struct FakeResource {
    const ID3D12ResourceVtbl* lpVtbl;
    D3D12_RESOURCE_DESC desc;
} FakeResource;

/* Only GetDesc answers; the census never reads the other slots, so they refuse. */
#ifdef _MSC_VER
#pragma warning(push)
#pragma warning(disable : 4100)
#else
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-parameter"
#endif
static HRESULT STDMETHODCALLTYPE res_query_interface(ID3D12Resource* self, REFIID iid, void** out)
{
    return E_NOINTERFACE;
}
static ULONG STDMETHODCALLTYPE res_add_ref(ID3D12Resource* self) { return 2; }
static ULONG STDMETHODCALLTYPE res_release(ID3D12Resource* self) { return 1; }
static HRESULT STDMETHODCALLTYPE res_get_private_data(ID3D12Resource* self, REFGUID guid, UINT* size,
                                                      void* data)
{
    return E_FAIL;
}
static HRESULT STDMETHODCALLTYPE res_set_private_data(ID3D12Resource* self, REFGUID guid, UINT size,
                                                      const void* data)
{
    return E_FAIL;
}
static HRESULT STDMETHODCALLTYPE res_set_private_data_interface(ID3D12Resource* self, REFGUID guid,
                                                                const IUnknown* data)
{
    return E_FAIL;
}
static HRESULT STDMETHODCALLTYPE res_set_name(ID3D12Resource* self, LPCWSTR name) { return S_OK; }
static HRESULT STDMETHODCALLTYPE res_get_device(ID3D12Resource* self, REFIID iid, void** out)
{
    return E_FAIL;
}
static HRESULT STDMETHODCALLTYPE res_map(ID3D12Resource* self, UINT subresource,
                                         const D3D12_RANGE* range, void** data)
{
    return E_FAIL;
}
static void STDMETHODCALLTYPE res_unmap(ID3D12Resource* self, UINT subresource,
                                        const D3D12_RANGE* range)
{
}
static D3D12_RESOURCE_DESC* STDMETHODCALLTYPE res_get_desc(ID3D12Resource* self,
                                                           D3D12_RESOURCE_DESC* out)
{
    *out = ((FakeResource*)self)->desc;
    return out;
}
static D3D12_GPU_VIRTUAL_ADDRESS STDMETHODCALLTYPE res_get_gpu_address(ID3D12Resource* self)
{
    return 0;
}
static HRESULT STDMETHODCALLTYPE res_write_to_subresource(ID3D12Resource* self, UINT dst,
                                                          const D3D12_BOX* box, const void* src,
                                                          UINT row_pitch, UINT depth_pitch)
{
    return E_FAIL;
}
static HRESULT STDMETHODCALLTYPE res_read_from_subresource(ID3D12Resource* self, void* dst,
                                                           UINT row_pitch, UINT depth_pitch,
                                                           UINT src, const D3D12_BOX* box)
{
    return E_FAIL;
}
static HRESULT STDMETHODCALLTYPE res_get_heap_properties(ID3D12Resource* self,
                                                         D3D12_HEAP_PROPERTIES* properties,
                                                         D3D12_HEAP_FLAGS* flags)
{
    return E_FAIL;
}
#ifdef _MSC_VER
#pragma warning(pop)
#else
#pragma GCC diagnostic pop
#endif
static const ID3D12ResourceVtbl kResourceVtbl = {
    res_query_interface,
    res_add_ref,
    res_release,
    res_get_private_data,
    res_set_private_data,
    res_set_private_data_interface,
    res_set_name,
    res_get_device,
    res_map,
    res_unmap,
    res_get_desc,
    res_get_gpu_address,
    res_write_to_subresource,
    res_read_from_subresource,
    res_get_heap_properties,
};

static void init_resource(FakeResource* r, DXGI_FORMAT format, UINT64 w, UINT h,
                          D3D12_RESOURCE_FLAGS flags)
{
    memset(r, 0, sizeof(*r));
    r->lpVtbl = &kResourceVtbl;
    r->desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    r->desc.Width = w;
    r->desc.Height = h;
    r->desc.DepthOrArraySize = 1;
    r->desc.MipLevels = 1;
    r->desc.Format = format;
    r->desc.SampleDesc.Count = 1;
    r->desc.Flags = flags;
}

/* ---- Vtable slots ----------------------------------------------------------------------------- */
/* The numbers below are the ones the census uses; the SDK vtable structures give them again. The
   NVSDK_NGX_Parameter slots depend on C++ overload ordering and are derived in
   vendor_census_ngx_platform.cpp. */
#define SLOT(type, member) (offsetof(type, member) / sizeof(void*))

static void expect_slot(size_t actual, size_t expected, const char* name)
{
    if (actual != expected) {
        printf("FAIL %s is vtable slot %zu, the census expects %zu\n", name, actual, expected);
        ++g_failures;
    }
}

#define EXPECT_SLOT(type, member, expected) expect_slot(SLOT(type, member), (expected), #type "::" #member)

static void check_vtable_slots(void)
{
    EXPECT_SLOT(IDXGIFactory2Vtbl, CreateSwapChain, 10);
    EXPECT_SLOT(IDXGIFactory2Vtbl, CreateSwapChainForHwnd, 15);
    EXPECT_SLOT(IDXGIFactory2Vtbl, CreateSwapChainForCoreWindow, 16);
    EXPECT_SLOT(IDXGISwapChain3Vtbl, Present, 8);
    EXPECT_SLOT(IDXGISwapChain3Vtbl, SetFullscreenState, 10);
    EXPECT_SLOT(IDXGISwapChain3Vtbl, GetDesc, 12);
    EXPECT_SLOT(IDXGISwapChain3Vtbl, ResizeBuffers, 13);
    EXPECT_SLOT(IDXGISwapChain3Vtbl, Present1, 22);
    EXPECT_SLOT(IDXGISwapChain3Vtbl, GetFrameLatencyWaitableObject, 33);
    EXPECT_SLOT(IDXGISwapChain3Vtbl, SetColorSpace1, 38);
    EXPECT_SLOT(IDXGISwapChain3Vtbl, ResizeBuffers1, 39);
    EXPECT_SLOT(ID3D12DeviceVtbl, CreateCommandQueue, 8);
    EXPECT_SLOT(ID3D12CommandQueueVtbl, ExecuteCommandLists, 10);
    EXPECT_SLOT(ID3D12CommandQueueVtbl, GetDesc, 18);
    EXPECT_SLOT(ID3D12ResourceVtbl, GetDesc, 10);
    g_failures += rsf_test_ngx_check_slots();
}

/* ---- FSR 2.1.2 dispatch description, restated for the test ------------------------------------ */
typedef struct Fsr2ResourceDesc {
    uint32_t type, format, width, height, depth, mip_count, flags;
} Fsr2ResourceDesc;

typedef struct Fsr2Resource {
    void* resource;
    wchar_t name[64];
    Fsr2ResourceDesc description;
    uint32_t state;
    bool is_depth;
    uint64_t descriptor_data;
} Fsr2Resource;

typedef struct Fsr2Dispatch {
    void* command_list;
    Fsr2Resource color, depth, motion_vectors, exposure, reactive, transparency, output;
    float jitter[2];
    float mv_scale[2];
    uint32_t render_size[2];
    bool enable_sharpening;
    float sharpness;
    float frame_time_delta;
    float pre_exposure;
    bool reset;
    float camera_near, camera_far, camera_fov;
} Fsr2Dispatch;
_Static_assert(sizeof(Fsr2Dispatch) == 1352, "FSR 2.1.2 dispatch description size");

static void fill_resource(Fsr2Resource* r, void* ptr, const wchar_t* name, uint32_t format,
                          uint32_t w, uint32_t h, bool depth)
{
    memset(r, 0, sizeof(*r));
    r->resource = ptr;
    if (name) wcsncpy(r->name, name, 63);
    if (ptr) {
        r->description = (Fsr2ResourceDesc){2, format, w, h, 1, 1, 0};
        r->state = 0x2;
        r->is_depth = depth;
    }
}

/* Each helper stores the result after the call so the compiler cannot turn the call into a tail
   jump; a tail call would make the caller the helper's caller and merge the call sites. */
static volatile int g_sink;

__declspec(noinline) static int dispatch_site(const Fsr2Dispatch* d)
{
    const int r = ffxFsr2ContextDispatch((void*)&d->color, d);
    g_sink = r;
    return r;
}

__declspec(noinline) static int dispatch_via_pointer(int (*fn)(void*, const void*),
                                                     const Fsr2Dispatch* d)
{
    const int r = fn((void*)&d->color, d);
    g_sink = r;
    return r;
}

__declspec(noinline) static int jitter_noise(int index)
{
    float x, y;
    const int r = ffxFsr2GetJitterOffset(&x, &y, index, 8);
    g_sink = r;
    return r;
}

/* A function that references an FSR resource name, for the static scan to find. */
static const wchar_t kAnchorName[] = L"FSR2_InputColor";
__declspec(noinline) static const wchar_t* static_anchor(void)
{
    const volatile wchar_t* p = kAnchorName;
    return (const wchar_t*)p;
}

static HMODULE load_fixture(const wchar_t* file)
{
    HMODULE m = rsf_test_census_load_fixture(file);
    if (!m) printf("cannot load fixture %ls (error %lu)\n", file, GetLastError());
    return m;
}

/* ---- Phases ----------------------------------------------------------------------------------- */
typedef struct Frame {
    FakeResource color, depth, mv, output, exposure;
} Frame;

static void init_frame(Frame* f)
{
    init_resource(&f->color, DXGI_FORMAT_R16G16B16A16_FLOAT, 1280, 720, D3D12_RESOURCE_FLAG_NONE);
    init_resource(&f->depth, DXGI_FORMAT_D32_FLOAT, 1280, 720,
                  D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);
    init_resource(&f->mv, DXGI_FORMAT_R16G16_FLOAT, 1280, 720, D3D12_RESOURCE_FLAG_NONE);
    init_resource(&f->output, DXGI_FORMAT_R16G16B16A16_FLOAT, 1920, 1080,
                  D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
    init_resource(&f->exposure, DXGI_FORMAT_R32_FLOAT, 1, 1, D3D12_RESOURCE_FLAG_NONE);
}

static void make_dispatch(Fsr2Dispatch* d, Frame* f)
{
    memset(d, 0, sizeof(*d));
    d->command_list = (void*)(uintptr_t)0xC0DE1000;
    fill_resource(&d->color, &f->color, L"FSR2_InputColor", 10, 1280, 720, false);
    fill_resource(&d->depth, &f->depth, L"FSR2_InputDepth", 40, 1280, 720, true);
    fill_resource(&d->motion_vectors, &f->mv, L"FSR2_InputMotionVectors", 34, 1280, 720, false);
    fill_resource(&d->exposure, &f->exposure, L"FSR2_InputExposure", 41, 1, 1, false);
    fill_resource(&d->reactive, NULL, NULL, 0, 0, 0, false);
    fill_resource(&d->transparency, NULL, NULL, 0, 0, 0, false);
    fill_resource(&d->output, &f->output, L"FSR2_OutputUpscaledColor", 10, 1920, 1080, false);
    d->jitter[0] = 0.25f;
    d->jitter[1] = -0.375f;
    d->mv_scale[0] = 1280.0f;
    d->mv_scale[1] = 720.0f;
    d->render_size[0] = 1280;
    d->render_size[1] = 720;
    d->enable_sharpening = true;
    d->sharpness = 0.5f;
    d->frame_time_delta = 16.6f;
    d->pre_exposure = 1.0f;
    d->reset = false;
    d->camera_near = 0.1f;
    d->camera_far = 5000.0f;
    d->camera_fov = 1.0f;
}

static DWORD WINAPI dispatch_worker(void* argument)
{
    SetThreadDescription(GetCurrentThread(), L"census-worker");
    CHECK(dispatch_site((const Fsr2Dispatch*)argument) == 0x1234ABCD);
    return 0;
}

static int run_fsr2(Frame* frame, uint64_t* dispatch_calls)
{
    Fsr2Dispatch d;
    make_dispatch(&d, frame);

    /* Direct import: three calls on the main thread. */
    for (int i = 0; i < 3; ++i) {
        CHECK(dispatch_site(&d) == 0x1234ABCD);
        ++*dispatch_calls;
    }
    void* seen_ctx = NULL;
    const void* seen_desc = NULL;
    int seen_calls = 0;
    fixture_fsr2_seen(&seen_ctx, &seen_desc, &seen_calls);
    CHECK(seen_ctx == &d.color);
    CHECK(seen_desc == &d);

    /* One call from a named thread: same call site, second thread. */
    HANDLE worker = CreateThread(NULL, 0, dispatch_worker, &d, 0, NULL);
    CHECK(worker != NULL);
    if (worker) {
        WaitForSingleObject(worker, INFINITE);
        CloseHandle(worker);
    }
    ++*dispatch_calls;

    /* A description whose layout check must fail. */
    Fsr2Dispatch junk;
    memset(&junk, 0xFF, sizeof(junk));
    CHECK(dispatch_site(&junk) == 0x1234ABCD);
    ++*dispatch_calls;

    /* Through GetProcAddress: the pointer is the detoured export. */
    HMODULE m = rsf_test_census_loaded_module(L"ffx_fsr2_api_x64.dll");
    CHECK(m != NULL);
    int (*via)(void*, const void*) =
        PROC(int (*)(void*, const void*), m, "ffxFsr2ContextDispatch");
    CHECK(via != NULL);
    if (via) {
        CHECK(dispatch_via_pointer(via, &d) == 0x1234ABCD);
        CHECK(dispatch_via_pointer(via, &d) == 0x1234ABCD);
        *dispatch_calls += 2;
    }

    /* A float return value must survive the wrapper bit for bit. */
    CHECK(ffxFsr2GetUpscaleRatioFromQualityMode(2) == 1.7f);
    CHECK(ffxFsr2GetUpscaleRatioFromQualityMode(1) == 1.5f);

    float x = 0, y = 0;
    CHECK(ffxFsr2GetJitterOffset(&x, &y, 3, 8) == 0);
    CHECK(x == 1.5f && y == -0.75f);
    return 0;
}

typedef void (*NgxSeen)(void**, void**, void**, unsigned*, int*, int*);
static NgxSeen observer_seen;
static int observer_evaluations[16];
static unsigned observer_evaluation_count;
static unsigned observer_creates, observer_releases;

static void observe_ngx_create(void* handle, const void* params, uint32_t feature)
{
    CHECK(handle && params && feature == 1);
    ++observer_creates;
}

static void observe_ngx_evaluate(void* list, void* handle, const void* params, uint32_t phase,
                                 uint64_t result)
{
    CHECK(list && handle && params);
    void *seen_list = NULL, *seen_handle = NULL, *seen_params = NULL;
    unsigned feature = 0;
    int creates = 0, evaluations = 0;
    observer_seen(&seen_list, &seen_handle, &seen_params, &feature, &creates, &evaluations);
    if (observer_evaluation_count < sizeof(observer_evaluations) / sizeof(observer_evaluations[0])) {
        observer_evaluations[observer_evaluation_count++] = evaluations;
    }
    if (phase) CHECK(result == (evaluations == 1 ? 1u : 0u));
}

static void observe_ngx_release(void* handle)
{
    CHECK(handle);
    ++observer_releases;
}

static void run_ngx(Frame* frame)
{
    HMODULE m = load_fixture(L"_nvngx.dll");
    REQUIRE(m);
    int (*get_params)(void**) = PROC(int (*)(void**), m, "NVSDK_NGX_D3D12_GetParameters");
    int (*create)(void*, unsigned, void*, void**) =
        PROC(int (*)(void*, unsigned, void*, void**), m, "NVSDK_NGX_D3D12_CreateFeature");
    int (*evaluate)(void*, void*, void*, void*) =
        PROC(int (*)(void*, void*, void*, void*), m, "NVSDK_NGX_D3D12_EvaluateFeature");
    int (*release)(void*) = PROC(int (*)(void*), m, "NVSDK_NGX_D3D12_ReleaseFeature");
    NgxSeen seen = PROC(NgxSeen, m, "fixture_ngx_seen");
    REQUIRE(get_params && create && evaluate && release && seen);
    observer_seen = seen;
    rsf_census_observe_ngx_d3d12(observe_ngx_create, observe_ngx_evaluate, observe_ngx_release);

    void* params = NULL;
    CHECK(get_params(&params) == 1);
    CHECK(params != NULL);

    rsf_test_ngx_set_u32(params, "Width", 1280u);
    rsf_test_ngx_set_u32(params, "Height", 720u);
    rsf_test_ngx_set_u32(params, "OutWidth", 1920u);
    rsf_test_ngx_set_u32(params, "OutHeight", 1080u);
    rsf_test_ngx_set_i32(params, "PerfQualityValue", 2);
    rsf_test_ngx_set_i32(params, "DLSS.Feature.Create.Flags", 1 | 2 | 4 | 8 | 64);
    void* handle = NULL;
    CHECK(create((void*)(uintptr_t)0x1111, 1, params, &handle) == 1);
    CHECK(handle != NULL);

    rsf_test_ngx_set_resource12(params, "Color", &frame->color);
    rsf_test_ngx_set_resource12(params, "Output", &frame->output);
    rsf_test_ngx_set_resource12(params, "Depth", &frame->depth);
    rsf_test_ngx_set_resource12(params, "MotionVectors", &frame->mv);
    rsf_test_ngx_set_resource12(params, "ExposureTexture", &frame->exposure);
    rsf_test_ngx_set_f32(params, "Jitter.Offset.X", -0.125f);
    rsf_test_ngx_set_f32(params, "Jitter.Offset.Y", 0.0625f);
    rsf_test_ngx_set_f32(params, "MV.Scale.X", 1280.0f);
    rsf_test_ngx_set_f32(params, "MV.Scale.Y", 720.0f);
    rsf_test_ngx_set_i32(params, "Reset", 1);
    rsf_test_ngx_set_f32(params, "Sharpness", 0.35f);
    rsf_test_ngx_set_u32(params, "DLSS.Render.Subrect.Dimensions.Width", 1280u);
    rsf_test_ngx_set_u32(params, "DLSS.Render.Subrect.Dimensions.Height", 720u);
    rsf_test_ngx_set_f32(params, "DLSS.Pre.Exposure", 1.5f);
    rsf_test_ngx_set_f32(params, "FrameTimeDeltaInMsec", 16.6f);
    rsf_test_ngx_set_i32(params, "DLSS.Input.Color.Subrect.Base.X", 16);
    CHECK(evaluate((void*)(uintptr_t)0x2222, handle, params, NULL) == 1);
    CHECK(evaluate((void*)(uintptr_t)0x2223, handle, params, (void*)(uintptr_t)0x1) == 0);
    CHECK(release(handle) == 1);

    void *cmd = NULL, *h = NULL, *p = NULL;
    unsigned feature = 0;
    int creates = 0, evaluates = 0;
    seen(&cmd, &h, &p, &feature, &creates, &evaluates);
    CHECK(cmd == (void*)(uintptr_t)0x2223);
    CHECK(h == handle);
    CHECK(p == params);
    CHECK(feature == 1 && creates == 1 && evaluates == 2);
    CHECK(observer_creates == 1 && observer_releases == 1);
    static const int expected_evaluations[] = {0, 1, 1, 2};
    CHECK(observer_evaluation_count == 4 &&
          memcmp(observer_evaluations, expected_evaluations, sizeof(expected_evaluations)) == 0);
    rsf_census_observe_ngx_d3d12(NULL, NULL, NULL);
}

static void run_nvapi(HMODULE* out_module)
{
    HMODULE m = load_fixture(L"rsf_fixture_nvapi.dll");
    *out_module = m;
    REQUIRE(m);
    void* (*qi)(unsigned) = PROC(void* (*)(unsigned), m, "nvapi_QueryInterface");
    void* (*raw)(void) = PROC(void* (*)(void), m, "fixture_nvapi_raw_marker");
    void (*seen)(void**, void**, int*) = PROC(void (*)(void**, void**, int*), m, "fixture_nvapi_seen");
    REQUIRE(qi && raw && seen);
    /* This module is not on the census' detour list, so the pointer GetProcAddress returned is a
       recording thunk and differs from the fixture's own function. */
    CHECK(qi(0x12345678) == NULL);
    void* marker = qi(0xD9984C05);
    CHECK(marker != NULL && marker != raw());
    unsigned char params[72] = {0};
    const uint32_t version = 72 | (1u << 16);
    const uint64_t frame_id = 77;
    const uint32_t marker_type = 4;
    memcpy(params, &version, 4);
    memcpy(params + 8, &frame_id, 8);
    memcpy(params + 16, &marker_type, 4);
    void* device = (void*)(uintptr_t)0xD3D3;
    CHECK(FN_FROM_PTR(int (*)(void*, void*), marker)(device, params) == 0x5151);
    void* sd = NULL;
    void* sp = NULL;
    int calls = 0;
    seen(&sd, &sp, &calls);
    CHECK(sd == device && sp == params && calls == 1);

    void* sleep_mode = qi(0xAC1CA9E0);
    CHECK(sleep_mode != NULL);
    unsigned char mode[40] = {0};
    mode[4] = 1;
    mode[5] = 1;
    const uint32_t interval = 8333;
    memcpy(mode + 8, &interval, 4);
    CHECK(FN_FROM_PTR(int (*)(void*, void*), sleep_mode)(device, mode) == 0x5252);
}

typedef struct XessExecute {
    void *color, *velocity, *depth, *exposure_scale, *responsive_mask, *output;
    float jitter_x, jitter_y, exposure;
    uint32_t reset_history;
    uint32_t input_width, input_height;
    uint32_t bases[12];
    void* descriptor_heap;
    uint32_t descriptor_heap_offset;
} XessExecute;

static void run_xess(Frame* frame)
{
    HMODULE m = load_fixture(L"libxess.dll");
    REQUIRE(m);
    int (*jitter)(void*, float, float) = PROC(int (*)(void*, float, float), m, "xessSetJitterScale");
    int (*execute)(void*, void*, const void*) =
        PROC(int (*)(void*, void*, const void*), m, "xessD3D12Execute");
    void (*seen)(void**, float*, float*, int*) =
        PROC(void (*)(void**, float*, float*, int*), m, "fixture_xess_seen");
    REQUIRE(jitter && execute && seen);
    void* ctx = (void*)(uintptr_t)0x7E55;
    CHECK(jitter(ctx, 0.25f, -0.5f) == 0);
    void* sc = NULL;
    float sx = 0, sy = 0;
    int calls = 0;
    seen(&sc, &sx, &sy, &calls);
    CHECK(sc == ctx && sx == 0.25f && sy == -0.5f);

    XessExecute e;
    memset(&e, 0, sizeof(e));
    e.color = &frame->color;
    e.velocity = &frame->mv;
    e.depth = &frame->depth;
    e.output = &frame->output;
    e.jitter_x = 0.125f;
    e.jitter_y = -0.25f;
    e.exposure = 1.0f;
    e.input_width = 1280;
    e.input_height = 720;
    CHECK(execute(ctx, (void*)(uintptr_t)0xABCD, &e) == 0x77);
    CHECK(execute(ctx, NULL, &e) == 0x78);
}

/* The Rust WARP driver borrows this independent native input and caller probe. */
static void dispatch_warp(const void* description)
{
    dispatch_site((const Fsr2Dispatch*)description);
}

static bool run_dxgi(uint64_t* dispatch_calls, Frame* frame)
{
    Fsr2Dispatch description;
    make_dispatch(&description, frame);
    return rsf_test_census_warp(&description, dispatch_warp, dispatch_calls, &g_failures);
}

static int probe_vtable(void)
{
    check_vtable_slots();
    const int failures = g_failures;
    g_failures = 0;
    return failures;
}

static int probe_functional(rsf_test_census_functional_facts* facts)
{
    /* Referenced after install so the scan sees it in the loaded image. */
    CHECK(static_anchor() == kAnchorName);

    Frame frame;
    init_frame(&frame);
    uint64_t dispatch_calls = 0;
    run_fsr2(&frame, &dispatch_calls);
    run_ngx(&frame);
    HMODULE nvapi = NULL;
    run_nvapi(&nvapi);
    run_xess(&frame);

    /* Sampling: one call site, 100 calls, cap 4, one in ten afterwards. */
    for (int i = 0; i < 100; ++i) jitter_noise(i);

    const bool had_dxgi = run_dxgi(&dispatch_calls, &frame);
    if (!had_dxgi) printf("SKIPPED: DXGI/D3D12 WARP path unavailable on this machine\n");
    facts->dispatch_calls = dispatch_calls;
    facts->had_dxgi = had_dxgi ? 1u : 0u;
    facts->anchor = (const void*)(uintptr_t)&static_anchor;
    return g_failures;
}

static int probe_after_shutdown(void) { return dispatch_site(NULL); }

/* Vulkan entry points reached through vkGetInstanceProcAddr and vkGetDeviceProcAddr, and the
   Streamline feature functions handed out by slGetFeatureFunction. Structures are built from the
   Vulkan and Streamline offsets that tests/loader/vendor_layout_facts compares with the headers. */
static uint32_t token_index(const void* self)
{
    (void)self;
    return 42;
}

static void* const kTokenVtable[] = {(void*)(uintptr_t)token_index};

typedef struct FakeToken {
    void* const* vtable;
} FakeToken;

static int probe_vulkan(void)
{
    HMODULE vk = load_fixture(L"rsf_fixture_vulkan.dll");
    HMODULE sl = load_fixture(L"sl.interposer.dll");
    if (!vk || !sl) return 1;
    typedef void* (*GetProcAddr)(void*, const char*);
    GetProcAddr gipa = PROC(GetProcAddr, vk, "vkGetInstanceProcAddr");
    GetProcAddr gdpa = PROC(GetProcAddr, vk, "vkGetDeviceProcAddr");
    void* (*raw)(const char*) = PROC(void* (*)(const char*), vk, "fixture_vk_raw");
    void (*seen)(void**, unsigned*, unsigned*, const void**) =
        PROC(void (*)(void**, unsigned*, unsigned*, const void**), vk, "fixture_vk_seen");
    REQUIRE_RET(gipa && gdpa && raw && seen, 1);

    CHECK(gipa(NULL, "vkNoSuchFunction") == NULL);
    typedef int (*CreateInstance)(const void*, const void*, void**);
    typedef int (*CreateDevice)(void*, const void*, const void*, void**);
    typedef void (*GetDeviceQueue)(void*, unsigned, unsigned, void**);
    typedef int (*Submit)(void*, unsigned, const void*, unsigned long long);
    typedef int (*Present)(void*, const void*);
    typedef int (*CreateSwapchain)(void*, const void*, const void*, unsigned long long*);
    CreateInstance create_instance = FN_FROM_PTR(CreateInstance, gipa(NULL, "vkCreateInstance"));
    CreateDevice create_device = FN_FROM_PTR(CreateDevice, gipa(NULL, "vkCreateDevice"));
    GetDeviceQueue get_queue = FN_FROM_PTR(GetDeviceQueue, gdpa(NULL, "vkGetDeviceQueue"));
    Submit submit = FN_FROM_PTR(Submit, gipa(NULL, "vkQueueSubmit"));
    Submit submit2 = FN_FROM_PTR(Submit, gdpa(NULL, "vkQueueSubmit2"));
    Present present = FN_FROM_PTR(Present, gipa(NULL, "vkQueuePresentKHR"));
    CreateSwapchain create_swapchain =
        FN_FROM_PTR(CreateSwapchain, gdpa(NULL, "vkCreateSwapchainKHR"));
    REQUIRE_RET(create_instance && create_device && get_queue && submit && submit2 && present &&
                    create_swapchain,
                1);
    /* The pointers handed out are recording thunks, not the fixture's functions. */
    CHECK((void*)(uintptr_t)create_instance != raw("vkCreateInstance"));
    CHECK((void*)(uintptr_t)submit != raw("vkQueueSubmit"));
    /* A second lookup of the same function (device level) returns the same thunk. */
    CHECK(gdpa(NULL, "vkQueueSubmit") == (void*)(uintptr_t)submit);

    /* vkCreateInstance */
    const char app_name[] = "CensusGame";
    const char engine_name[] = "CensusEngine";
    unsigned char app_info[48] = {0};
    PUT(const char*, app_info, 16, app_name);
    PUT(uint32_t, app_info, 24, 0x00010002);
    PUT(const char*, app_info, 32, engine_name);
    PUT(uint32_t, app_info, 40, 0x00020001);
    PUT(uint32_t, app_info, 44, (1u << 22) | (3u << 12));
    const char* instance_ext[] = {"VK_KHR_surface", "VK_KHR_win32_surface"};
    unsigned char instance_info[64] = {0};
    PUT(void*, instance_info, 24, app_info);
    PUT(uint32_t, instance_info, 48, 2);
    PUT(const char* const*, instance_info, 56, instance_ext);
    void* instance = NULL;
    CHECK(create_instance(instance_info, NULL, &instance) == 0);
    CHECK(instance == (void*)(uintptr_t)0x1001);

    /* vkCreateDevice with two queue families */
    unsigned char queue_info[2][40] = {{0}};
    PUT(uint32_t, queue_info[0], 20, 0);
    PUT(uint32_t, queue_info[0], 24, 1);
    PUT(uint32_t, queue_info[1], 20, 1);
    PUT(uint32_t, queue_info[1], 24, 2);
    const char* device_ext[] = {"VK_KHR_swapchain"};
    unsigned char device_info[72] = {0};
    PUT(uint32_t, device_info, 20, 2);
    PUT(void*, device_info, 24, queue_info);
    PUT(uint32_t, device_info, 48, 1);
    PUT(const char* const*, device_info, 56, device_ext);
    void* device = NULL;
    CHECK(create_device((void*)(uintptr_t)0x5005, device_info, NULL, &device) == 0);
    CHECK(device == (void*)(uintptr_t)0x2002);

    void *graphics = NULL, *compute = NULL;
    get_queue(device, 0, 0, &graphics);
    get_queue(device, 1, 1, &compute);
    CHECK(graphics == (void*)(uintptr_t)0x3000 && compute == (void*)(uintptr_t)0x3011);

    unsigned long long swapchain = 0;
    unsigned char swapchain_info[104] = {0};
    PUT(uint64_t, swapchain_info, 24, 0x5001);
    PUT(uint32_t, swapchain_info, 32, 3);
    PUT(uint32_t, swapchain_info, 36, 50); /* VK_FORMAT_B8G8R8A8_SRGB */
    PUT(uint32_t, swapchain_info, 44, 2560);
    PUT(uint32_t, swapchain_info, 48, 1440);
    PUT(uint32_t, swapchain_info, 56, 0x10);
    PUT(uint32_t, swapchain_info, 88, 1); /* MAILBOX */
    CHECK(create_swapchain(device, swapchain_info, NULL, &swapchain) == 0);
    CHECK(swapchain == 0x4004);

    unsigned char submit_infos[2][72] = {{0}};
    PUT(uint32_t, submit_infos[0], 40, 3);
    PUT(uint32_t, submit_infos[1], 40, 1);
    unsigned char submit2_infos[1][64] = {{0}};
    PUT(uint32_t, submit2_infos[0], 32, 2);
    uint32_t image_index = 1;
    unsigned long long chain = swapchain;
    unsigned char present_info[64] = {0};
    PUT(uint32_t, present_info, 32, 1);
    PUT(unsigned long long*, present_info, 40, &chain);
    PUT(uint32_t*, present_info, 48, &image_index);
    for (int f = 0; f < 3; ++f) {
        CHECK(submit(graphics, 2, submit_infos, 0) == 0x10);
        CHECK(submit(graphics, 1, submit_infos, 0) == 0x10);
        CHECK(submit2(compute, 1, submit2_infos, 0) == 0x11);
        CHECK(present(graphics, present_info) == 0);
    }
    void* last_queue = NULL;
    unsigned submits = 0, presents = 0;
    const void* last_info = NULL;
    seen(&last_queue, &submits, &presents, &last_info);
    CHECK(last_queue == graphics && submits == 3 * (2 + 1 + 1) && presents == 3 &&
          last_info == present_info);

    /* Streamline */
    int (*sl_init)(const void*, unsigned long long) =
        PROC(int (*)(const void*, unsigned long long), sl, "slInit");
    int (*sl_get)(unsigned, const char*, void**) =
        PROC(int (*)(unsigned, const char*, void**), sl, "slGetFeatureFunction");
    void* (*sl_raw)(const char*) = PROC(void* (*)(const char*), sl, "fixture_sl_raw");
    REQUIRE_RET(sl_init && sl_get && sl_raw, 1);
    const wchar_t log_path[] = L"C:\\census\\logs";
    const uint32_t features[] = {0, 1000};
    unsigned char prefs[144] = {0};
    PUT(uint8_t, prefs, 32, 1);
    PUT(uint32_t, prefs, 36, 2);
    PUT(const wchar_t*, prefs, 56, log_path);
    PUT(const uint32_t*, prefs, 96, features);
    PUT(uint32_t, prefs, 104, 2);
    PUT(uint32_t, prefs, 108, 0x1234);
    PUT(uint32_t, prefs, 112, 1);
    PUT(const char*, prefs, 120, "5.4");
    PUT(const char*, prefs, 128, "census-project");
    CHECK(sl_init(prefs, 0xABCD) == 0);

    void* missing = (void*)(uintptr_t)0x1;
    CHECK(sl_get(3, "slNotAFunction", &missing) == 7 && missing == (void*)(uintptr_t)0x1);
    void *reflex_sleep = NULL, *pcl_marker = NULL, *dlssg_options = NULL;
    CHECK(sl_get(3, "slReflexSleep", &reflex_sleep) == 0);
    CHECK(sl_get(4, "slPCLSetMarker", &pcl_marker) == 0);
    CHECK(sl_get(1000, "slDLSSGSetOptions", &dlssg_options) == 0);
    CHECK(reflex_sleep && reflex_sleep != sl_raw("slReflexSleep"));
    CHECK(pcl_marker && pcl_marker != sl_raw("slPCLSetMarker"));
    CHECK(dlssg_options && dlssg_options != sl_raw("slDLSSGSetOptions"));
    FakeToken token = {kTokenVtable};
    CHECK(FN_FROM_PTR(int (*)(const void*), reflex_sleep)(&token) == 0x31);
    CHECK(FN_FROM_PTR(int (*)(unsigned, const void*), pcl_marker)(4, &token) == 0x32);
    unsigned char viewport[40] = {0};
    PUT(uint32_t, viewport, 32, 7);
    int (*sl_constants)(const void*, const void*, const void*) =
        PROC(int (*)(const void*, const void*, const void*), sl, "slSetConstants");
    CHECK(sl_constants);
    /* Exercise the v1-sized prefix, matrix order and camera axes. Deliberately distinct entries
       catch offset and transpose mistakes in the recorded arrays. */
    unsigned char constants[456] = {0};
    PUT(uint32_t, constants, 24, 1);
    for (size_t i = 0; i < 80; ++i) PUT(float, constants, 32 + i * 4, (float)i - 17.25f);
    for (size_t i = 0; i < 18; ++i) PUT(float, constants, 352 + i * 4, (float)i + 0.125f);
    PUT(float, constants, 424, 0.1f);
    PUT(float, constants, 428, 10000.f);
    PUT(float, constants, 432, 1.1f);
    PUT(uint8_t, constants, 444, 1);
    if (sl_constants) CHECK(sl_constants(constants, &token, viewport) == 0x34);
    unsigned char options[96] = {0};
    PUT(uint32_t, options, 32, 1);
    PUT(uint32_t, options, 36, 2);
    PUT(uint32_t, options, 64, 1920);
    PUT(uint32_t, options, 68, 1080);
    CHECK(FN_FROM_PTR(int (*)(const void*, const void*), dlssg_options)(viewport, options) == 0x33);

    return g_failures ? 1 : 0;
}

/* One upscale dispatch resource: pointer, type, format, extent and state at the offsets
   tests/loader/vendor_layout_facts compares with the FidelityFX headers. */
static void ffx_resource(unsigned char* up, size_t offset, const FakeResource* r, uint32_t format,
                         uint32_t w, uint32_t h)
{
    PUT(const void*, up, offset, r);
    PUT(uint32_t, up, offset + 8, 2);
    PUT(uint32_t, up, offset + 12, format);
    PUT(uint32_t, up, offset + 16, w);
    PUT(uint32_t, up, offset + 20, h);
    PUT(uint32_t, up, offset + 40, 0x4);
}

/* FidelityFX API (FSR 3.1 and later): context creation with a descriptor chain, dispatch,
   configure. Descriptor offsets are the ones tests/loader/vendor_layout_facts checks. */
static int probe_ffx(void)
{
    HMODULE m = load_fixture(L"amd_fidelityfx_dx12.dll");
    if (!m) return 1;
    unsigned (*create)(void**, void*, const void*) =
        PROC(unsigned (*)(void**, void*, const void*), m, "ffxCreateContext");
    unsigned (*destroy)(void**, const void*) =
        PROC(unsigned (*)(void**, const void*), m, "ffxDestroyContext");
    unsigned (*configure)(void**, const void*) =
        PROC(unsigned (*)(void**, const void*), m, "ffxConfigure");
    unsigned (*query)(void**, void*) = PROC(unsigned (*)(void**, void*), m, "ffxQuery");
    unsigned (*dispatch)(void**, const void*) =
        PROC(unsigned (*)(void**, const void*), m, "ffxDispatch");
    REQUIRE_RET(create && destroy && configure && query && dispatch, 1);

    Frame frame;
    init_frame(&frame);
    void* context = NULL;

    /* Upscaler context: create-context header, then the DX12 backend descriptor. */
    unsigned char backend[24] = {0};
    PUT(uint64_t, backend, 0, 0x00000002);
    PUT(void*, backend, 16, (void*)(uintptr_t)0xDE71CE);
    unsigned char upscale_create[40] = {0};
    PUT(uint64_t, upscale_create, 0, 0x00010000);
    PUT(void*, upscale_create, 8, backend);
    PUT(uint32_t, upscale_create, 16, 0x5);
    PUT(uint32_t, upscale_create, 20, 1280);
    PUT(uint32_t, upscale_create, 24, 720);
    PUT(uint32_t, upscale_create, 28, 1920);
    PUT(uint32_t, upscale_create, 32, 1080);
    CHECK(create(&context, upscale_create, NULL) == 0);
    CHECK(context == (void*)(uintptr_t)0xF00D);

    /* Frame generation swapchain creation for an HWND. */
    unsigned char swapchain_desc[sizeof(DXGI_SWAP_CHAIN_DESC1)] = {0};
    PUT(uint32_t, swapchain_desc, 0, 3840);
    PUT(uint32_t, swapchain_desc, 4, 2160);
    PUT(uint32_t, swapchain_desc, 8, DXGI_FORMAT_R10G10B10A2_UNORM);
    PUT(uint32_t, swapchain_desc, 28, 3); /* buffer count */
    PUT(uint32_t, swapchain_desc, 36, 4); /* flip discard */
    void* hwnd_swapchain = NULL;
    unsigned char swapchain_create[64] = {0};
    PUT(uint64_t, swapchain_create, 0, 0x00030006);
    PUT(void**, swapchain_create, 16, &hwnd_swapchain);
    PUT(void*, swapchain_create, 24, (void*)(uintptr_t)0x4E4D);
    PUT(void*, swapchain_create, 32, swapchain_desc);
    PUT(void*, swapchain_create, 48, (void*)(uintptr_t)0xFAC7);
    PUT(void*, swapchain_create, 56, (void*)(uintptr_t)0x9E3E);
    void* fg_context = NULL;
    CHECK(create(&fg_context, swapchain_create, NULL) == 0);

    /* Upscale dispatch. */
    unsigned char up[432] = {0};
    PUT(uint64_t, up, 0, 0x00010001);
    PUT(void*, up, 16, (void*)(uintptr_t)0xC0DE);
    ffx_resource(up, 24, &frame.color, 4, 1280, 720);
    ffx_resource(up, 72, &frame.depth, 28, 1280, 720);
    ffx_resource(up, 120, &frame.mv, 18, 1280, 720);
    ffx_resource(up, 312, &frame.output, 4, 1920, 1080);
    PUT(float, up, 360, 0.5f);
    PUT(float, up, 364, -0.25f);
    PUT(float, up, 368, 1280.0f);
    PUT(float, up, 372, 720.0f);
    PUT(uint32_t, up, 376, 1280);
    PUT(uint32_t, up, 380, 720);
    PUT(uint32_t, up, 384, 1920);
    PUT(uint32_t, up, 388, 1080);
    PUT(uint8_t, up, 392, 1);
    PUT(float, up, 396, 0.8f);
    PUT(float, up, 400, 8.3f);
    PUT(float, up, 404, 2.0f);
    PUT(uint8_t, up, 408, 1);
    PUT(float, up, 412, 0.1f);
    PUT(float, up, 416, 10000.0f);
    PUT(float, up, 420, 0.9f);
    PUT(uint32_t, up, 428, 0x2);
    CHECK(dispatch(&context, up) == 0x40);

    /* Frame generation dispatch and configure. */
    unsigned char fg[312] = {0};
    PUT(uint64_t, fg, 0, 0x00020003);
    PUT(void*, fg, 16, (void*)(uintptr_t)0xC0DF);
    PUT(void*, fg, 24, &frame.output);
    PUT(uint32_t, fg, 36, 4);
    PUT(uint32_t, fg, 264, 1);
    PUT(uint8_t, fg, 268, 1);
    PUT(uint32_t, fg, 272, 2);
    PUT(float, fg, 276, 0.5f);
    PUT(float, fg, 280, 1000.0f);
    PUT(uint64_t, fg, 304, 1234);
    CHECK(dispatch(&fg_context, fg) == 0x40);
    unsigned char cfg[144] = {0};
    PUT(uint64_t, cfg, 0, 0x00020002);
    PUT(void*, cfg, 16, (void*)(uintptr_t)0x5C5C);
    PUT(uint8_t, cfg, 56, 1);
    PUT(uint64_t, cfg, 136, 99);
    CHECK(configure(&fg_context, cfg) == 0x41);
    unsigned char qry[16] = {0};
    PUT(uint64_t, qry, 0, 0x00010003);
    CHECK(query(&context, qry) == 0x42);
    CHECK(destroy(&context, NULL) == 0x43);

    return g_failures ? 1 : 0;
}

/* Keep the two calls through the native import and its original noinline caller. */
static void dispatch_twice(int* results)
{
    Frame frame;
    init_frame(&frame);
    Fsr2Dispatch description;
    make_dispatch(&description, &frame);
    results[0] = dispatch_site(&description);
    results[1] = dispatch_site(&description);
}

int main(int argc, char** argv)
{
    static const rsf_test_census_native native = {
        rsf_census_install, rsf_census_flush,   rsf_census_get_counters, rsf_census_shutdown,
        probe_vtable,       probe_functional,   probe_after_shutdown,    jitter_noise,
        probe_vulkan,       probe_ffx,          dispatch_twice,
    };
    return rsf_test_census_main(argc, argv, &native);
}
