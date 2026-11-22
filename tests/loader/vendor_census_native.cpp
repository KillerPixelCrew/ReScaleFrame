// SPDX-License-Identifier: MIT
// Native payload, SDK slot and caller-address evidence for the Rust census fixture.
//
// Fake vendor DLLs are called through a direct import, through
// LoadLibrary plus GetProcAddress and through nvapi_QueryInterface, with the census installed. The
// test checks that every call passes through unchanged and that the records, summary and byte
// budget say what was called. A real DXGI and D3D12 (WARP) path covers the swapchain hooks, and a
// fake vtable checks the slot numbers the census relies on against the Windows SDK headers.
//
//   rsf_vendor_census_test functional <output dir>
//   rsf_vendor_census_test budget <output dir>
//   rsf_vendor_census_test vulkan|ffx|dllmain <output dir>
#include <windows.h>

#include <d3d12.h>
#include <dxgi1_6.h>

#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <string>
#include <thread>
#include <utility>
#include <vector>

#include "rescaleframe/vendor_census.h"
#include "rescaleframe/census_d3d12_observer.h"
#include "../vendor_census_ngx_params.h"
#include "census_facts.h"

extern "C" {
int rsf_test_census_main(int argc, char** argv, const rsf_test_census_native* native);
__declspec(dllimport) int ffxFsr2ContextDispatch(void* context, const void* desc);
__declspec(dllimport) int ffxFsr2GetJitterOffset(float* x, float* y, int index, int phase_count);
__declspec(dllimport) float ffxFsr2GetUpscaleRatioFromQualityMode(int mode);
__declspec(dllimport) void fixture_fsr2_seen(void** context, const void** desc, int* calls);
}

namespace {

int g_failures = 0;

#define CHECK(cond)                                                      \
    do {                                                                 \
        if (!(cond)) {                                                   \
            std::printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond);  \
            ++g_failures;                                                \
        }                                                                \
    } while (0)

#define CHECK_NEAR(a, b)                                                                       \
    do {                                                                                       \
        const double va = (a), vb = (b);                                                       \
        if (!(std::fabs(va - vb) <= 1e-5 * (1.0 + std::fabs(vb)))) {                           \
            std::printf("FAIL %s:%d  %s (%g) != %s (%g)\n", __FILE__, __LINE__, #a, va, #b, vb); \
            ++g_failures;                                                                      \
        }                                                                                      \
    } while (0)

std::wstring exe_dir() {
    wchar_t path[MAX_PATH];
    GetModuleFileNameW(nullptr, path, MAX_PATH);
    wchar_t* slash = std::wcsrchr(path, L'\\');
    if (slash) *slash = 0;
    return path;
}

uint32_t image_size() {
    auto* base = reinterpret_cast<const uint8_t*>(GetModuleHandleW(nullptr));
    auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    return nt->OptionalHeader.SizeOfImage;
}

// ---- A fake ID3D12Resource ---------------------------------------------------------------------
class FakeResource final : public ID3D12Resource {
public:
    FakeResource(DXGI_FORMAT format, UINT64 w, UINT h, D3D12_RESOURCE_FLAGS flags = D3D12_RESOURCE_FLAG_NONE) {
        desc_.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        desc_.Width = w;
        desc_.Height = h;
        desc_.DepthOrArraySize = 1;
        desc_.MipLevels = 1;
        desc_.Format = format;
        desc_.SampleDesc.Count = 1;
        desc_.Flags = flags;
    }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID, void**) override { return E_NOINTERFACE; }
    ULONG STDMETHODCALLTYPE AddRef() override { return 2; }
    ULONG STDMETHODCALLTYPE Release() override { return 1; }
    HRESULT STDMETHODCALLTYPE GetPrivateData(REFGUID, UINT*, void*) override { return E_FAIL; }
    HRESULT STDMETHODCALLTYPE SetPrivateData(REFGUID, UINT, const void*) override { return E_FAIL; }
    HRESULT STDMETHODCALLTYPE SetPrivateDataInterface(REFGUID, const IUnknown*) override { return E_FAIL; }
    HRESULT STDMETHODCALLTYPE SetName(LPCWSTR) override { return S_OK; }
    HRESULT STDMETHODCALLTYPE GetDevice(REFIID, void**) override { return E_FAIL; }
    HRESULT STDMETHODCALLTYPE Map(UINT, const D3D12_RANGE*, void**) override { return E_FAIL; }
    void STDMETHODCALLTYPE Unmap(UINT, const D3D12_RANGE*) override {}
    D3D12_RESOURCE_DESC STDMETHODCALLTYPE GetDesc() override { return desc_; }
    D3D12_GPU_VIRTUAL_ADDRESS STDMETHODCALLTYPE GetGPUVirtualAddress() override { return 0; }
    HRESULT STDMETHODCALLTYPE WriteToSubresource(UINT, const D3D12_BOX*, const void*, UINT, UINT) override {
        return E_FAIL;
    }
    HRESULT STDMETHODCALLTYPE ReadFromSubresource(void*, UINT, UINT, UINT, const D3D12_BOX*) override { return E_FAIL; }
    HRESULT STDMETHODCALLTYPE GetHeapProperties(D3D12_HEAP_PROPERTIES*, D3D12_HEAP_FLAGS*) override { return E_FAIL; }

private:
    D3D12_RESOURCE_DESC desc_{};
};

// ---- Vtable slot probe -------------------------------------------------------------------------
size_t g_slot = ~size_t{0};
template <size_t I>
int64_t stub(int64_t first) {
    g_slot = I;
    return first; // a struct returned through a hidden pointer comes back in rax
}
template <size_t... I>
std::array<void*, sizeof...(I)> make_stubs(std::index_sequence<I...>) {
    return {reinterpret_cast<void*>(&stub<I>)...};
}
const auto kStubs = make_stubs(std::make_index_sequence<48>{});
struct FakeObject {
    void* const* vtable;
};
template <class T>
T* fake() {
    static FakeObject object{kStubs.data()};
    return reinterpret_cast<T*>(&object);
}

void check_vtable_slots() {
    // The numbers below are the ones the census uses; this derives them again from the SDK headers.
    fake<IDXGIFactory2>()->CreateSwapChain(nullptr, nullptr, nullptr);
    CHECK(g_slot == 10);
    fake<IDXGIFactory2>()->CreateSwapChainForHwnd(nullptr, nullptr, nullptr, nullptr, nullptr, nullptr);
    CHECK(g_slot == 15);
    fake<IDXGIFactory2>()->CreateSwapChainForCoreWindow(nullptr, nullptr, nullptr, nullptr, nullptr);
    CHECK(g_slot == 16);
    fake<IDXGISwapChain3>()->Present(0, 0);
    CHECK(g_slot == 8);
    fake<IDXGISwapChain3>()->SetFullscreenState(FALSE, nullptr);
    CHECK(g_slot == 10);
    fake<IDXGISwapChain3>()->GetDesc(nullptr);
    CHECK(g_slot == 12);
    fake<IDXGISwapChain3>()->ResizeBuffers(0, 0, 0, DXGI_FORMAT_UNKNOWN, 0);
    CHECK(g_slot == 13);
    fake<IDXGISwapChain3>()->Present1(0, 0, nullptr);
    CHECK(g_slot == 22);
    fake<IDXGISwapChain3>()->GetFrameLatencyWaitableObject();
    CHECK(g_slot == 33);
    fake<IDXGISwapChain3>()->SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
    CHECK(g_slot == 38);
    fake<IDXGISwapChain3>()->ResizeBuffers1(0, 0, 0, DXGI_FORMAT_UNKNOWN, 0, nullptr, nullptr);
    CHECK(g_slot == 39);
    fake<ID3D12Device>()->CreateCommandQueue(nullptr, __uuidof(ID3D12CommandQueue), nullptr);
    CHECK(g_slot == 8);
    fake<ID3D12CommandQueue>()->ExecuteCommandLists(0, nullptr);
    CHECK(g_slot == 10);
    D3D12_COMMAND_QUEUE_DESC qd = fake<ID3D12CommandQueue>()->GetDesc();
    (void)qd;
    CHECK(g_slot == 18);
    D3D12_RESOURCE_DESC rd = fake<ID3D12Resource>()->GetDesc();
    (void)rd;
    CHECK(g_slot == 10);

    // NVSDK_NGX_Parameter: MSVC groups each overload set and reverses the declaration order.
    fake<NgxParam>()->Set("x", static_cast<void*>(nullptr));
    CHECK(g_slot == 0);
    fake<NgxParam>()->Set("x", static_cast<ID3D12Resource*>(nullptr));
    CHECK(g_slot == 1);
    fake<NgxParam>()->Set("x", static_cast<ID3D11Resource*>(nullptr));
    CHECK(g_slot == 2);
    fake<NgxParam>()->Set("x", 0);
    CHECK(g_slot == 3);
    fake<NgxParam>()->Set("x", 0u);
    CHECK(g_slot == 4);
    fake<NgxParam>()->Set("x", 0.0);
    CHECK(g_slot == 5);
    fake<NgxParam>()->Set("x", 0.0f);
    CHECK(g_slot == 6);
    fake<NgxParam>()->Set("x", 0ull);
    CHECK(g_slot == 7);
    fake<NgxParam>()->Get("x", static_cast<void**>(nullptr));
    CHECK(g_slot == 8);
    fake<NgxParam>()->Get("x", static_cast<ID3D12Resource**>(nullptr));
    CHECK(g_slot == 9);
    fake<NgxParam>()->Get("x", static_cast<ID3D11Resource**>(nullptr));
    CHECK(g_slot == 10);
    fake<NgxParam>()->Get("x", static_cast<int*>(nullptr));
    CHECK(g_slot == 11);
    fake<NgxParam>()->Get("x", static_cast<unsigned int*>(nullptr));
    CHECK(g_slot == 12);
    fake<NgxParam>()->Get("x", static_cast<double*>(nullptr));
    CHECK(g_slot == 13);
    fake<NgxParam>()->Get("x", static_cast<float*>(nullptr));
    CHECK(g_slot == 14);
    fake<NgxParam>()->Get("x", static_cast<unsigned long long*>(nullptr));
    CHECK(g_slot == 15);
    fake<NgxParam>()->Reset();
    CHECK(g_slot == 16);
}

// ---- FSR 2.1.2 dispatch description, restated for the test ------------------------------------
struct Fsr2ResourceDesc {
    uint32_t type, format, width, height, depth, mip_count, flags;
};
struct Fsr2Resource {
    void* resource;
    wchar_t name[64];
    Fsr2ResourceDesc description;
    uint32_t state;
    bool is_depth;
    uint64_t descriptor_data;
};
struct Fsr2Dispatch {
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
};
static_assert(sizeof(Fsr2Dispatch) == 1352);

void fill_resource(Fsr2Resource& r, void* ptr, const wchar_t* name, uint32_t format, uint32_t w, uint32_t h,
                   bool depth = false) {
    std::memset(&r, 0, sizeof(r));
    r.resource = ptr;
    if (name) std::wcsncpy(r.name, name, 63);
    if (ptr) {
        r.description = {2, format, w, h, 1, 1, 0};
        r.state = 0x2;
        r.is_depth = depth;
    }
}

// Each helper stores the result after the call so the compiler cannot turn the call into a tail
// jump; a tail call would make the caller the helper's caller and merge the call sites.
volatile int g_sink;
__declspec(noinline) int dispatch_site(const Fsr2Dispatch* d) {
    const int r = ffxFsr2ContextDispatch(const_cast<Fsr2Resource*>(&d->color), d);
    g_sink = r;
    return r;
}
__declspec(noinline) int dispatch_via_pointer(int (*fn)(void*, const void*), const Fsr2Dispatch* d) {
    const int r = fn(const_cast<Fsr2Resource*>(&d->color), d);
    g_sink = r;
    return r;
}
__declspec(noinline) int jitter_noise(int index) {
    float x, y;
    const int r = ffxFsr2GetJitterOffset(&x, &y, index, 8);
    g_sink = r;
    return r;
}

// A function that references an FSR resource name, for the static scan to find.
const wchar_t kAnchorName[] = L"FSR2_InputColor";
__declspec(noinline) const wchar_t* static_anchor() {
    volatile const wchar_t* p = kAnchorName;
    return const_cast<const wchar_t*>(p);
}

template <class T>
T proc(HMODULE m, const char* name) {
    return reinterpret_cast<T>(GetProcAddress(m, name));
}

HMODULE load_fixture(const wchar_t* file) {
    std::wstring path = exe_dir() + L"\\" + file;
    HMODULE m = LoadLibraryW(path.c_str());
    if (!m) std::printf("cannot load fixture %ls (error %lu)\n", file, GetLastError());
    return m;
}

// ---- Phases ------------------------------------------------------------------------------------
struct Frame {
    FakeResource color{DXGI_FORMAT_R16G16B16A16_FLOAT, 1280, 720};
    FakeResource depth{DXGI_FORMAT_D32_FLOAT, 1280, 720, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL};
    FakeResource mv{DXGI_FORMAT_R16G16_FLOAT, 1280, 720};
    FakeResource output{DXGI_FORMAT_R16G16B16A16_FLOAT, 1920, 1080, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS};
    FakeResource exposure{DXGI_FORMAT_R32_FLOAT, 1, 1};
};

void make_dispatch(Fsr2Dispatch& d, Frame& f) {
    std::memset(&d, 0, sizeof(d));
    d.command_list = reinterpret_cast<void*>(static_cast<uintptr_t>(0xC0DE1000));
    fill_resource(d.color, &f.color, L"FSR2_InputColor", 10, 1280, 720);
    fill_resource(d.depth, &f.depth, L"FSR2_InputDepth", 40, 1280, 720, true);
    fill_resource(d.motion_vectors, &f.mv, L"FSR2_InputMotionVectors", 34, 1280, 720);
    fill_resource(d.exposure, &f.exposure, L"FSR2_InputExposure", 41, 1, 1);
    fill_resource(d.reactive, nullptr, nullptr, 0, 0, 0);
    fill_resource(d.transparency, nullptr, nullptr, 0, 0, 0);
    fill_resource(d.output, &f.output, L"FSR2_OutputUpscaledColor", 10, 1920, 1080);
    d.jitter[0] = 0.25f;
    d.jitter[1] = -0.375f;
    d.mv_scale[0] = 1280.0f;
    d.mv_scale[1] = 720.0f;
    d.render_size[0] = 1280;
    d.render_size[1] = 720;
    d.enable_sharpening = true;
    d.sharpness = 0.5f;
    d.frame_time_delta = 16.6f;
    d.pre_exposure = 1.0f;
    d.reset = false;
    d.camera_near = 0.1f;
    d.camera_far = 5000.0f;
    d.camera_fov = 1.0f;
}

int run_fsr2(Frame& frame, uint64_t& dispatch_calls) {
    Fsr2Dispatch d;
    make_dispatch(d, frame);

    // Direct import: three calls on the main thread.
    for (int i = 0; i < 3; ++i) {
        CHECK(dispatch_site(&d) == 0x1234ABCD);
        ++dispatch_calls;
    }
    void* seen_ctx = nullptr;
    const void* seen_desc = nullptr;
    int seen_calls = 0;
    fixture_fsr2_seen(&seen_ctx, &seen_desc, &seen_calls);
    CHECK(seen_ctx == &d.color);
    CHECK(seen_desc == &d);

    // One call from a named thread: same call site, second thread.
    std::thread worker([&] {
        SetThreadDescription(GetCurrentThread(), L"census-worker");
        CHECK(dispatch_site(&d) == 0x1234ABCD);
    });
    worker.join();
    ++dispatch_calls;

    // A description whose layout check must fail.
    Fsr2Dispatch junk;
    std::memset(&junk, 0xFF, sizeof(junk));
    CHECK(dispatch_site(&junk) == 0x1234ABCD);
    ++dispatch_calls;

    // Through GetProcAddress: the pointer is the detoured export.
    HMODULE m = GetModuleHandleW(L"ffx_fsr2_api_x64.dll");
    CHECK(m != nullptr);
    auto via = proc<int (*)(void*, const void*)>(m, "ffxFsr2ContextDispatch");
    CHECK(via != nullptr);
    if (via) {
        CHECK(dispatch_via_pointer(via, &d) == 0x1234ABCD);
        CHECK(dispatch_via_pointer(via, &d) == 0x1234ABCD);
        dispatch_calls += 2;
    }

    // A float return value must survive the wrapper bit for bit.
    CHECK(ffxFsr2GetUpscaleRatioFromQualityMode(2) == 1.7f);
    CHECK(ffxFsr2GetUpscaleRatioFromQualityMode(1) == 1.5f);

    float x = 0, y = 0;
    CHECK(ffxFsr2GetJitterOffset(&x, &y, 3, 8) == 0);
    CHECK(x == 1.5f && y == -0.75f);
    return 0;
}

using NgxSeen = void (*)(void**, void**, void**, unsigned*, int*, int*);
NgxSeen observer_seen{};
std::vector<int> observer_evaluations;
unsigned observer_creates{}, observer_releases{};
void observe_ngx_create(void* handle,const void* params,uint32_t feature) {
    CHECK(handle && params && feature==1); ++observer_creates;
}
void observe_ngx_evaluate(void* list,void* handle,const void* params,uint32_t phase,uint64_t result) {
    CHECK(list && handle && params);
    void *seen_list{},*seen_handle{},*seen_params{};unsigned feature{};int creates{},evaluations{};
    observer_seen(&seen_list,&seen_handle,&seen_params,&feature,&creates,&evaluations);
    observer_evaluations.push_back(evaluations);
    if(phase)CHECK(result==(evaluations==1?1u:0u));
}
void observe_ngx_release(void* handle) { CHECK(handle); ++observer_releases; }
void run_ngx(Frame& frame) {
    HMODULE m = load_fixture(L"_nvngx.dll");
    if (!m) {
        ++g_failures;
        return;
    }
    auto get_params = proc<int (*)(NgxParam**)>(m, "NVSDK_NGX_D3D12_GetParameters");
    auto create = proc<int (*)(void*, unsigned, NgxParam*, void**)>(m, "NVSDK_NGX_D3D12_CreateFeature");
    auto evaluate = proc<int (*)(void*, void*, NgxParam*, void*)>(m, "NVSDK_NGX_D3D12_EvaluateFeature");
    auto release = proc<int (*)(void*)>(m, "NVSDK_NGX_D3D12_ReleaseFeature");
    auto seen = proc<void (*)(void**, void**, void**, unsigned*, int*, int*)>(m, "fixture_ngx_seen");
    CHECK(get_params && create && evaluate && release && seen);
    if (!(get_params && create && evaluate && release && seen)) return;
    observer_seen=seen;
    rsf_census_observe_ngx_d3d12(observe_ngx_create,observe_ngx_evaluate,observe_ngx_release);

    NgxParam* params = nullptr;
    CHECK(get_params(&params) == 1);
    CHECK(params != nullptr);

    params->Set("Width", 1280u);
    params->Set("Height", 720u);
    params->Set("OutWidth", 1920u);
    params->Set("OutHeight", 1080u);
    params->Set("PerfQualityValue", 2);
    params->Set("DLSS.Feature.Create.Flags", 1 | 2 | 4 | 8 | 64);
    void* handle = nullptr;
    CHECK(create(reinterpret_cast<void*>(0x1111), 1, params, &handle) == 1);
    CHECK(handle != nullptr);

    params->Set("Color", static_cast<ID3D12Resource*>(&frame.color));
    params->Set("Output", static_cast<ID3D12Resource*>(&frame.output));
    params->Set("Depth", static_cast<ID3D12Resource*>(&frame.depth));
    params->Set("MotionVectors", static_cast<ID3D12Resource*>(&frame.mv));
    params->Set("ExposureTexture", static_cast<ID3D12Resource*>(&frame.exposure));
    params->Set("Jitter.Offset.X", -0.125f);
    params->Set("Jitter.Offset.Y", 0.0625f);
    params->Set("MV.Scale.X", 1280.0f);
    params->Set("MV.Scale.Y", 720.0f);
    params->Set("Reset", 1);
    params->Set("Sharpness", 0.35f);
    params->Set("DLSS.Render.Subrect.Dimensions.Width", 1280u);
    params->Set("DLSS.Render.Subrect.Dimensions.Height", 720u);
    params->Set("DLSS.Pre.Exposure", 1.5f);
    params->Set("FrameTimeDeltaInMsec", 16.6f);
    params->Set("DLSS.Input.Color.Subrect.Base.X", 16);
    CHECK(evaluate(reinterpret_cast<void*>(0x2222), handle, params, nullptr) == 1);
    CHECK(evaluate(reinterpret_cast<void*>(0x2223), handle, params, reinterpret_cast<void*>(0x1)) == 0);
    CHECK(release(handle) == 1);

    void *cmd, *h, *p;
    unsigned feature;
    int creates, evaluates;
    seen(&cmd, &h, &p, &feature, &creates, &evaluates);
    CHECK(cmd == reinterpret_cast<void*>(0x2223));
    CHECK(h == handle);
    CHECK(p == params);
    CHECK(feature == 1 && creates == 1 && evaluates == 2);
    CHECK(observer_creates==1 && observer_releases==1);
    CHECK(observer_evaluations==std::vector<int>({0,1,1,2}));
    rsf_census_observe_ngx_d3d12(nullptr,nullptr,nullptr);
}

void run_nvapi(HMODULE* out_module) {
    HMODULE m = load_fixture(L"rsf_fixture_nvapi.dll");
    *out_module = m;
    if (!m) {
        ++g_failures;
        return;
    }
    auto qi = proc<void* (*)(unsigned)>(m, "nvapi_QueryInterface");
    auto raw = proc<void* (*)()>(m, "fixture_nvapi_raw_marker");
    auto seen = proc<void (*)(void**, void**, int*)>(m, "fixture_nvapi_seen");
    CHECK(qi && raw && seen);
    if (!(qi && raw && seen)) return;
    // This module is not on the census' detour list, so the pointer GetProcAddress returned is a
    // recording thunk and differs from the fixture's own function.
    CHECK(qi(0x12345678) == nullptr);
    void* marker = qi(0xD9984C05);
    CHECK(marker != nullptr && marker != raw());
    unsigned char params[72] = {};
    const uint32_t version = 72 | (1u << 16);
    const uint64_t frame_id = 77;
    const uint32_t marker_type = 4;
    std::memcpy(params, &version, 4);
    std::memcpy(params + 8, &frame_id, 8);
    std::memcpy(params + 16, &marker_type, 4);
    void* device = reinterpret_cast<void*>(0xD3D3);
    CHECK(reinterpret_cast<int (*)(void*, void*)>(marker)(device, params) == 0x5151);
    void* sd;
    void* sp;
    int calls;
    seen(&sd, &sp, &calls);
    CHECK(sd == device && sp == params && calls == 1);

    void* sleep_mode = qi(0xAC1CA9E0);
    CHECK(sleep_mode != nullptr);
    unsigned char mode[40] = {};
    mode[4] = 1;
    mode[5] = 1;
    const uint32_t interval = 8333;
    std::memcpy(mode + 8, &interval, 4);
    CHECK(reinterpret_cast<int (*)(void*, void*)>(sleep_mode)(device, mode) == 0x5252);
}

struct XessExecute {
    void *color, *velocity, *depth, *exposure_scale, *responsive_mask, *output;
    float jitter_x, jitter_y, exposure;
    uint32_t reset_history;
    uint32_t input_width, input_height;
    uint32_t bases[12];
    void* descriptor_heap;
    uint32_t descriptor_heap_offset;
};

void run_xess(Frame& frame) {
    HMODULE m = load_fixture(L"libxess.dll");
    if (!m) {
        ++g_failures;
        return;
    }
    auto jitter = proc<int (*)(void*, float, float)>(m, "xessSetJitterScale");
    auto execute = proc<int (*)(void*, void*, const void*)>(m, "xessD3D12Execute");
    auto seen = proc<void (*)(void**, float*, float*, int*)>(m, "fixture_xess_seen");
    CHECK(jitter && execute && seen);
    if (!(jitter && execute && seen)) return;
    void* ctx = reinterpret_cast<void*>(0x7E55);
    CHECK(jitter(ctx, 0.25f, -0.5f) == 0);
    void* sc;
    float sx, sy;
    int calls;
    seen(&sc, &sx, &sy, &calls);
    CHECK(sc == ctx && sx == 0.25f && sy == -0.5f);

    XessExecute e{};
    e.color = &frame.color;
    e.velocity = &frame.mv;
    e.depth = &frame.depth;
    e.output = &frame.output;
    e.jitter_x = 0.125f;
    e.jitter_y = -0.25f;
    e.exposure = 1.0f;
    e.input_width = 1280;
    e.input_height = 720;
    CHECK(execute(ctx, reinterpret_cast<void*>(0xABCD), &e) == 0x77);
    CHECK(execute(ctx, nullptr, &e) == 0x78);
}

// Real DXGI and D3D12 on the WARP adapter. Returns false when the machine cannot run it.
bool run_dxgi(uint64_t& dispatch_calls, Frame& frame) {
    HMODULE dxgi = LoadLibraryW(L"dxgi.dll");
    HMODULE d3d12 = LoadLibraryW(L"d3d12.dll");
    if (!dxgi || !d3d12) return false;
    auto create_factory = proc<HRESULT(WINAPI*)(REFIID, void**)>(dxgi, "CreateDXGIFactory1");
    auto create_device = proc<PFN_D3D12_CREATE_DEVICE>(d3d12, "D3D12CreateDevice");
    if (!create_factory || !create_device) return false;

    IDXGIFactory4* factory = nullptr;
    if (FAILED(create_factory(__uuidof(IDXGIFactory4), reinterpret_cast<void**>(&factory)))) return false;
    IDXGIAdapter* warp = nullptr;
    if (FAILED(factory->EnumWarpAdapter(__uuidof(IDXGIAdapter), reinterpret_cast<void**>(&warp)))) return false;
    ID3D12Device* device = nullptr;
    if (FAILED(create_device(warp, D3D_FEATURE_LEVEL_11_0, __uuidof(ID3D12Device), reinterpret_cast<void**>(&device)))) {
        return false;
    }
    D3D12_COMMAND_QUEUE_DESC qd{};
    qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ID3D12CommandQueue* queue = nullptr;
    CHECK(SUCCEEDED(device->CreateCommandQueue(&qd, __uuidof(ID3D12CommandQueue), reinterpret_cast<void**>(&queue))));
    if (!queue) return false;
    ID3D12CommandAllocator* alloc = nullptr;
    ID3D12GraphicsCommandList* list = nullptr;
    CHECK(SUCCEEDED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, __uuidof(ID3D12CommandAllocator),
                                                   reinterpret_cast<void**>(&alloc))));
    CHECK(SUCCEEDED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, alloc, nullptr,
                                              __uuidof(ID3D12GraphicsCommandList), reinterpret_cast<void**>(&list))));
    if (!alloc || !list) return false;
    list->Close();

    HWND hwnd = CreateWindowExW(0, L"STATIC", L"census", WS_OVERLAPPED, 0, 0, 640, 360, nullptr, nullptr, nullptr, nullptr);
    if (!hwnd) return false;
    DXGI_SWAP_CHAIN_DESC1 sd{};
    sd.Width = 640;
    sd.Height = 360;
    sd.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    sd.SampleDesc.Count = 1;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.BufferCount = 2;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    IDXGISwapChain1* sc1 = nullptr;
    HRESULT hr = factory->CreateSwapChainForHwnd(queue, hwnd, &sd, nullptr, nullptr, &sc1);
    CHECK(SUCCEEDED(hr));
    if (FAILED(hr)) return false;
    IDXGISwapChain3* sc3 = nullptr;
    sc1->QueryInterface(__uuidof(IDXGISwapChain3), reinterpret_cast<void**>(&sc3));
    if (sc3) sc3->SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);

    Fsr2Dispatch d;
    make_dispatch(d, frame);
    ID3D12CommandList* lists[] = {list};
    for (int f = 0; f < 3; ++f) {
        dispatch_site(&d);
        dispatch_site(&d);
        dispatch_calls += 2;
        queue->ExecuteCommandLists(1, lists);
        queue->ExecuteCommandLists(1, lists);
        if (f == 1) {
            DXGI_PRESENT_PARAMETERS pp{};
            CHECK(SUCCEEDED(sc1->Present1(0, 0, &pp)));
        } else {
            CHECK(SUCCEEDED(sc1->Present(0, 0)));
        }
    }
    // A test present must not advance the present index.
    sc1->Present(0, DXGI_PRESENT_TEST);
    CHECK(SUCCEEDED(sc3 ? sc3->ResizeBuffers(0, 320, 180, DXGI_FORMAT_UNKNOWN, 0) : S_OK));

    if (sc3) sc3->Release();
    sc1->Release();
    DestroyWindow(hwnd);
    list->Release();
    alloc->Release();
    queue->Release();
    device->Release();
    warp->Release();
    factory->Release();
    return true;
}

int probe_vtable() {
    check_vtable_slots();
    const int failures = g_failures;
    g_failures = 0;
    return failures;
}

int probe_functional(rsf_test_census_functional_facts* facts) {
    // Referenced after install so the scan sees it in the loaded image.
    CHECK(static_anchor() == kAnchorName);

    Frame frame;
    uint64_t dispatch_calls = 0;
    run_fsr2(frame, dispatch_calls);
    run_ngx(frame);
    HMODULE nvapi = nullptr;
    run_nvapi(&nvapi);
    run_xess(frame);

    // Sampling: one call site, 100 calls, cap 4, one in ten afterwards.
    for (int i = 0; i < 100; ++i) jitter_noise(i);

    const bool had_dxgi = run_dxgi(dispatch_calls, frame);
    if (!had_dxgi) std::printf("SKIPPED: DXGI/D3D12 WARP path unavailable on this machine\n");
    facts->dispatch_calls = dispatch_calls;
    facts->had_dxgi = had_dxgi ? 1u : 0u;
    facts->image_size = image_size();
    facts->anchor_rva = reinterpret_cast<uintptr_t>(&static_anchor) -
                        reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    return g_failures;
}

int probe_after_shutdown() { return dispatch_site(nullptr); }

// Vulkan entry points reached through vkGetInstanceProcAddr and vkGetDeviceProcAddr, and the
// Streamline feature functions handed out by slGetFeatureFunction. Structures are built from the
// Vulkan and Streamline offsets that tests/vendor_census_layouts.cpp checks against the headers.
struct FakeToken {
    virtual uint32_t index() const { return 42; }
};

template <class T>
void put(unsigned char* base, size_t offset, T value) {
    std::memcpy(base + offset, &value, sizeof(T));
}

int probe_vulkan() {
    HMODULE vk = load_fixture(L"rsf_fixture_vulkan.dll");
    HMODULE sl = load_fixture(L"sl.interposer.dll");
    if (!vk || !sl) return 1;
    auto gipa = proc<void* (*)(void*, const char*)>(vk, "vkGetInstanceProcAddr");
    auto gdpa = proc<void* (*)(void*, const char*)>(vk, "vkGetDeviceProcAddr");
    auto raw = proc<void* (*)(const char*)>(vk, "fixture_vk_raw");
    auto seen = proc<void (*)(void**, unsigned*, unsigned*, const void**)>(vk, "fixture_vk_seen");
    CHECK(gipa && gdpa && raw && seen);
    if (!(gipa && gdpa && raw && seen)) return 1;

    CHECK(gipa(nullptr, "vkNoSuchFunction") == nullptr);
    using CreateInstance = int (*)(const void*, const void*, void**);
    using CreateDevice = int (*)(void*, const void*, const void*, void**);
    using GetDeviceQueue = void (*)(void*, unsigned, unsigned, void**);
    using Submit = int (*)(void*, unsigned, const void*, unsigned long long);
    using Present = int (*)(void*, const void*);
    using CreateSwapchain = int (*)(void*, const void*, const void*, unsigned long long*);
    auto create_instance = reinterpret_cast<CreateInstance>(gipa(nullptr, "vkCreateInstance"));
    auto create_device = reinterpret_cast<CreateDevice>(gipa(nullptr, "vkCreateDevice"));
    auto get_queue = reinterpret_cast<GetDeviceQueue>(gdpa(nullptr, "vkGetDeviceQueue"));
    auto submit = reinterpret_cast<Submit>(gipa(nullptr, "vkQueueSubmit"));
    auto submit2 = reinterpret_cast<Submit>(gdpa(nullptr, "vkQueueSubmit2"));
    auto present = reinterpret_cast<Present>(gipa(nullptr, "vkQueuePresentKHR"));
    auto create_swapchain = reinterpret_cast<CreateSwapchain>(gdpa(nullptr, "vkCreateSwapchainKHR"));
    CHECK(create_instance && create_device && get_queue && submit && submit2 && present && create_swapchain);
    if (!(create_instance && create_device && get_queue && submit && submit2 && present && create_swapchain)) return 1;
    // The pointers handed out are recording thunks, not the fixture's functions.
    CHECK(reinterpret_cast<void*>(create_instance) != raw("vkCreateInstance"));
    CHECK(reinterpret_cast<void*>(submit) != raw("vkQueueSubmit"));
    // A second lookup of the same function (device level) returns the same thunk.
    CHECK(gdpa(nullptr, "vkQueueSubmit") == reinterpret_cast<void*>(submit));

    // vkCreateInstance
    const char app_name[] = "CensusGame";
    const char engine_name[] = "CensusEngine";
    unsigned char app_info[48] = {};
    put<const char*>(app_info, 16, app_name);
    put<uint32_t>(app_info, 24, 0x00010002);
    put<const char*>(app_info, 32, engine_name);
    put<uint32_t>(app_info, 40, 0x00020001);
    put<uint32_t>(app_info, 44, (1u << 22) | (3u << 12));
    const char* instance_ext[] = {"VK_KHR_surface", "VK_KHR_win32_surface"};
    unsigned char instance_info[64] = {};
    put<void*>(instance_info, 24, app_info);
    put<uint32_t>(instance_info, 48, 2);
    put<const char* const*>(instance_info, 56, instance_ext);
    void* instance = nullptr;
    CHECK(create_instance(instance_info, nullptr, &instance) == 0);
    CHECK(instance == reinterpret_cast<void*>(0x1001));

    // vkCreateDevice with two queue families
    unsigned char queue_info[2][40] = {};
    put<uint32_t>(queue_info[0], 20, 0);
    put<uint32_t>(queue_info[0], 24, 1);
    put<uint32_t>(queue_info[1], 20, 1);
    put<uint32_t>(queue_info[1], 24, 2);
    const char* device_ext[] = {"VK_KHR_swapchain"};
    unsigned char device_info[72] = {};
    put<uint32_t>(device_info, 20, 2);
    put<void*>(device_info, 24, queue_info);
    put<uint32_t>(device_info, 48, 1);
    put<const char* const*>(device_info, 56, device_ext);
    void* device = nullptr;
    CHECK(create_device(reinterpret_cast<void*>(0x5005), device_info, nullptr, &device) == 0);
    CHECK(device == reinterpret_cast<void*>(0x2002));

    void *graphics = nullptr, *compute = nullptr;
    get_queue(device, 0, 0, &graphics);
    get_queue(device, 1, 1, &compute);
    CHECK(graphics == reinterpret_cast<void*>(0x3000) && compute == reinterpret_cast<void*>(0x3011));

    unsigned long long swapchain = 0;
    unsigned char swapchain_info[104] = {};
    put<uint64_t>(swapchain_info, 24, 0x5001);
    put<uint32_t>(swapchain_info, 32, 3);
    put<uint32_t>(swapchain_info, 36, 50); // VK_FORMAT_B8G8R8A8_SRGB
    put<uint32_t>(swapchain_info, 44, 2560);
    put<uint32_t>(swapchain_info, 48, 1440);
    put<uint32_t>(swapchain_info, 56, 0x10);
    put<uint32_t>(swapchain_info, 88, 1); // MAILBOX
    CHECK(create_swapchain(device, swapchain_info, nullptr, &swapchain) == 0);
    CHECK(swapchain == 0x4004);

    unsigned char submit_infos[2][72] = {};
    put<uint32_t>(submit_infos[0], 40, 3);
    put<uint32_t>(submit_infos[1], 40, 1);
    unsigned char submit2_infos[1][64] = {};
    put<uint32_t>(submit2_infos[0], 32, 2);
    uint32_t image_index = 1;
    unsigned long long chain = swapchain;
    unsigned char present_info[64] = {};
    put<uint32_t>(present_info, 32, 1);
    put<unsigned long long*>(present_info, 40, &chain);
    put<uint32_t*>(present_info, 48, &image_index);
    for (int f = 0; f < 3; ++f) {
        CHECK(submit(graphics, 2, submit_infos, 0) == 0x10);
        CHECK(submit(graphics, 1, submit_infos, 0) == 0x10);
        CHECK(submit2(compute, 1, submit2_infos, 0) == 0x11);
        CHECK(present(graphics, present_info) == 0);
    }
    void* last_queue;
    unsigned submits, presents;
    const void* last_info;
    seen(&last_queue, &submits, &presents, &last_info);
    CHECK(last_queue == graphics && submits == 3 * (2 + 1 + 1) && presents == 3 && last_info == present_info);

    // Streamline
    auto sl_init = proc<int (*)(const void*, unsigned long long)>(sl, "slInit");
    auto sl_get = proc<int (*)(unsigned, const char*, void*&)>(sl, "slGetFeatureFunction");
    auto sl_raw = proc<void* (*)(const char*)>(sl, "fixture_sl_raw");
    CHECK(sl_init && sl_get && sl_raw);
    if (!(sl_init && sl_get && sl_raw)) return 1;
    const wchar_t log_path[] = L"C:\\census\\logs";
    const uint32_t features[] = {0, 1000};
    unsigned char prefs[144] = {};
    put<uint8_t>(prefs, 32, 1);
    put<uint32_t>(prefs, 36, 2);
    put<const wchar_t*>(prefs, 56, log_path);
    put<const uint32_t*>(prefs, 96, features);
    put<uint32_t>(prefs, 104, 2);
    put<uint32_t>(prefs, 108, 0x1234);
    put<uint32_t>(prefs, 112, 1);
    put<const char*>(prefs, 120, "5.4");
    put<const char*>(prefs, 128, "census-project");
    CHECK(sl_init(prefs, 0xABCD) == 0);

    void* missing = reinterpret_cast<void*>(0x1);
    CHECK(sl_get(3, "slNotAFunction", missing) == 7 && missing == reinterpret_cast<void*>(0x1));
    void *reflex_sleep = nullptr, *pcl_marker = nullptr, *dlssg_options = nullptr;
    CHECK(sl_get(3, "slReflexSleep", reflex_sleep) == 0);
    CHECK(sl_get(4, "slPCLSetMarker", pcl_marker) == 0);
    CHECK(sl_get(1000, "slDLSSGSetOptions", dlssg_options) == 0);
    CHECK(reflex_sleep && reflex_sleep != sl_raw("slReflexSleep"));
    CHECK(pcl_marker && pcl_marker != sl_raw("slPCLSetMarker"));
    CHECK(dlssg_options && dlssg_options != sl_raw("slDLSSGSetOptions"));
    FakeToken token;
    CHECK(reinterpret_cast<int (*)(const void*)>(reflex_sleep)(&token) == 0x31);
    CHECK(reinterpret_cast<int (*)(unsigned, const void*)>(pcl_marker)(4, &token) == 0x32);
    unsigned char viewport[40] = {};
    put<uint32_t>(viewport, 32, 7);
    auto sl_constants = proc<int (*)(const void*, const void*, const void*)>(sl, "slSetConstants");
    CHECK(sl_constants);
    // Exercise the v1-sized prefix, matrix order and camera axes. Deliberately
    // distinct entries catch offset/transpose mistakes in the recorded arrays.
    unsigned char constants[456] = {};
    put<uint32_t>(constants, 24, 1);
    for (size_t i = 0; i < 80; ++i) put<float>(constants, 32 + i * 4, static_cast<float>(i) - 17.25f);
    for (size_t i = 0; i < 18; ++i) put<float>(constants, 352 + i * 4, static_cast<float>(i) + 0.125f);
    put<float>(constants, 424, 0.1f);
    put<float>(constants, 428, 10000.f);
    put<float>(constants, 432, 1.1f);
    put<uint8_t>(constants, 444, 1);
    if (sl_constants) CHECK(sl_constants(constants, &token, viewport) == 0x34);
    unsigned char options[96] = {};
    put<uint32_t>(options, 32, 1);
    put<uint32_t>(options, 36, 2);
    put<uint32_t>(options, 64, 1920);
    put<uint32_t>(options, 68, 1080);
    CHECK(reinterpret_cast<int (*)(const void*, const void*)>(dlssg_options)(viewport, options) == 0x33);

    return g_failures ? 1 : 0;
}

// FidelityFX API (FSR 3.1 and later): context creation with a descriptor chain, dispatch,
// configure. Descriptor offsets are the ones tests/vendor_census_layouts.cpp checks.
int probe_ffx() {
    HMODULE m = load_fixture(L"amd_fidelityfx_dx12.dll");
    if (!m) return 1;
    auto create = proc<unsigned (*)(void**, void*, const void*)>(m, "ffxCreateContext");
    auto destroy = proc<unsigned (*)(void**, const void*)>(m, "ffxDestroyContext");
    auto configure = proc<unsigned (*)(void**, const void*)>(m, "ffxConfigure");
    auto query = proc<unsigned (*)(void**, void*)>(m, "ffxQuery");
    auto dispatch = proc<unsigned (*)(void**, const void*)>(m, "ffxDispatch");
    CHECK(create && destroy && configure && query && dispatch);
    if (!(create && destroy && configure && query && dispatch)) return 1;

    Frame frame;
    void* context = nullptr;

    // Upscaler context: create-context header, then the DX12 backend descriptor.
    unsigned char backend[24] = {};
    put<uint64_t>(backend, 0, 0x00000002);
    put<void*>(backend, 16, reinterpret_cast<void*>(0xDE71CE));
    unsigned char upscale_create[40] = {};
    put<uint64_t>(upscale_create, 0, 0x00010000);
    put<void*>(upscale_create, 8, backend);
    put<uint32_t>(upscale_create, 16, 0x5);
    put<uint32_t>(upscale_create, 20, 1280);
    put<uint32_t>(upscale_create, 24, 720);
    put<uint32_t>(upscale_create, 28, 1920);
    put<uint32_t>(upscale_create, 32, 1080);
    CHECK(create(&context, upscale_create, nullptr) == 0);
    CHECK(context == reinterpret_cast<void*>(0xF00D));

    // Frame generation swapchain creation for an HWND.
    unsigned char swapchain_desc[sizeof(DXGI_SWAP_CHAIN_DESC1)] = {};
    put<uint32_t>(swapchain_desc, 0, 3840);
    put<uint32_t>(swapchain_desc, 4, 2160);
    put<uint32_t>(swapchain_desc, 8, DXGI_FORMAT_R10G10B10A2_UNORM);
    put<uint32_t>(swapchain_desc, 28, 3); // buffer count
    put<uint32_t>(swapchain_desc, 36, 4); // flip discard
    void* hwnd_swapchain = nullptr;
    unsigned char swapchain_create[64] = {};
    put<uint64_t>(swapchain_create, 0, 0x00030006);
    put<void**>(swapchain_create, 16, &hwnd_swapchain);
    put<void*>(swapchain_create, 24, reinterpret_cast<void*>(0x4E4D));
    put<void*>(swapchain_create, 32, swapchain_desc);
    put<void*>(swapchain_create, 48, reinterpret_cast<void*>(0xFAC7));
    put<void*>(swapchain_create, 56, reinterpret_cast<void*>(0x9E3E));
    void* fg_context = nullptr;
    CHECK(create(&fg_context, swapchain_create, nullptr) == 0);

    // Upscale dispatch.
    unsigned char up[432] = {};
    put<uint64_t>(up, 0, 0x00010001);
    put<void*>(up, 16, reinterpret_cast<void*>(0xC0DE));
    auto resource = [&](size_t offset, FakeResource* r, uint32_t format, uint32_t w, uint32_t h) {
        put<void*>(up, offset, r);
        put<uint32_t>(up, offset + 8, 2);
        put<uint32_t>(up, offset + 12, format);
        put<uint32_t>(up, offset + 16, w);
        put<uint32_t>(up, offset + 20, h);
        put<uint32_t>(up, offset + 40, 0x4);
    };
    resource(24, &frame.color, 4, 1280, 720);
    resource(72, &frame.depth, 28, 1280, 720);
    resource(120, &frame.mv, 18, 1280, 720);
    resource(312, &frame.output, 4, 1920, 1080);
    put<float>(up, 360, 0.5f);
    put<float>(up, 364, -0.25f);
    put<float>(up, 368, 1280.0f);
    put<float>(up, 372, 720.0f);
    put<uint32_t>(up, 376, 1280);
    put<uint32_t>(up, 380, 720);
    put<uint32_t>(up, 384, 1920);
    put<uint32_t>(up, 388, 1080);
    put<uint8_t>(up, 392, 1);
    put<float>(up, 396, 0.8f);
    put<float>(up, 400, 8.3f);
    put<float>(up, 404, 2.0f);
    put<uint8_t>(up, 408, 1);
    put<float>(up, 412, 0.1f);
    put<float>(up, 416, 10000.0f);
    put<float>(up, 420, 0.9f);
    put<uint32_t>(up, 428, 0x2);
    CHECK(dispatch(&context, up) == 0x40);

    // Frame generation dispatch and configure.
    unsigned char fg[312] = {};
    put<uint64_t>(fg, 0, 0x00020003);
    put<void*>(fg, 16, reinterpret_cast<void*>(0xC0DF));
    put<void*>(fg, 24, &frame.output);
    put<uint32_t>(fg, 36, 4);
    put<uint32_t>(fg, 264, 1);
    put<uint8_t>(fg, 268, 1);
    put<uint32_t>(fg, 272, 2);
    put<float>(fg, 276, 0.5f);
    put<float>(fg, 280, 1000.0f);
    put<uint64_t>(fg, 304, 1234);
    CHECK(dispatch(&fg_context, fg) == 0x40);
    unsigned char cfg[144] = {};
    put<uint64_t>(cfg, 0, 0x00020002);
    put<void*>(cfg, 16, reinterpret_cast<void*>(0x5C5C));
    put<uint8_t>(cfg, 56, 1);
    put<uint64_t>(cfg, 136, 99);
    CHECK(configure(&fg_context, cfg) == 0x41);
    unsigned char qry[16] = {};
    put<uint64_t>(qry, 0, 0x00010003);
    CHECK(query(&context, qry) == 0x42);
    CHECK(destroy(&context, nullptr) == 0x43);

    return g_failures ? 1 : 0;
}

// Keep the two calls through the native import and its original noinline caller.
void dispatch_twice(int* results) {
    Frame frame;
    Fsr2Dispatch description;
    make_dispatch(description, frame);
    results[0] = dispatch_site(&description);
    results[1] = dispatch_site(&description);
}

} // namespace

int main(int argc, char** argv) {
    static const rsf_test_census_native native = {
        rsf_census_install, rsf_census_flush, rsf_census_get_counters, rsf_census_shutdown,
        probe_vtable, probe_functional, probe_after_shutdown, jitter_noise, probe_vulkan,
        probe_ffx, dispatch_twice,
    };
    return rsf_test_census_main(argc, argv, &native);
}
