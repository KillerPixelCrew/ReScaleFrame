// SPDX-License-Identifier: GPL-3.0-only
// Vendor call census test.
//
// Fake vendor DLLs (tests/vendor_census_fixture.cpp) are called through a direct import, through
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
#include <fstream>
#include <sstream>
#include <string>
#include <thread>
#include <utility>
#include <vector>

#include "rescaleframe/vendor_census.h"
#include "rescaleframe/census_d3d12_observer.h"
#include "vendor_census_ngx_params.h"

extern "C" {
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

// ---- A small JSON reader ----------------------------------------------------------------------
struct Json {
    enum class Type { Null, Bool, Num, Str, Arr, Obj } type = Type::Null;
    bool b = false;
    double n = 0;
    std::string s;
    std::vector<Json> a;
    std::vector<std::pair<std::string, Json>> o;

    const Json* get(const char* key) const {
        for (const auto& kv : o) {
            if (kv.first == key) return &kv.second;
        }
        return nullptr;
    }
    const Json* at(std::initializer_list<const char*> path) const {
        const Json* cur = this;
        for (const char* k : path) {
            if (!cur) return nullptr;
            cur = cur->get(k);
        }
        return cur;
    }
    bool is(const char* v) const { return type == Type::Str && s == v; }
    bool has_string(const char* v) const {
        for (const Json& e : a) {
            if (e.is(v)) return true;
        }
        return false;
    }
};

struct Parser {
    const char* p;
    const char* end;
    bool ok = true;

    void ws() {
        while (p < end && (*p == ' ' || *p == '\n' || *p == '\r' || *p == '\t')) ++p;
    }
    bool lit(const char* t) {
        size_t n = std::strlen(t);
        if (static_cast<size_t>(end - p) >= n && !std::memcmp(p, t, n)) {
            p += n;
            return true;
        }
        return false;
    }
    std::string str() {
        std::string r;
        ++p; // opening quote
        while (p < end && *p != '"') {
            if (*p == '\\' && p + 1 < end) {
                ++p;
                switch (*p) {
                case 'n': r += '\n'; break;
                case 't': r += '\t'; break;
                case 'u': {
                    unsigned v = 0;
                    std::sscanf(p + 1, "%4x", &v);
                    r += static_cast<char>(v);
                    p += 4;
                    break;
                }
                default: r += *p; break;
                }
                ++p;
            } else {
                r += *p++;
            }
        }
        if (p < end) ++p;
        return r;
    }
    Json value() {
        Json j;
        ws();
        if (p >= end) {
            ok = false;
            return j;
        }
        if (*p == '{') {
            j.type = Json::Type::Obj;
            ++p;
            ws();
            if (*p == '}') {
                ++p;
                return j;
            }
            for (;;) {
                ws();
                if (*p != '"') {
                    ok = false;
                    return j;
                }
                std::string k = str();
                ws();
                if (*p != ':') {
                    ok = false;
                    return j;
                }
                ++p;
                j.o.emplace_back(std::move(k), value());
                ws();
                if (*p == ',') {
                    ++p;
                    continue;
                }
                if (*p == '}') {
                    ++p;
                    return j;
                }
                ok = false;
                return j;
            }
        }
        if (*p == '[') {
            j.type = Json::Type::Arr;
            ++p;
            ws();
            if (*p == ']') {
                ++p;
                return j;
            }
            for (;;) {
                j.a.push_back(value());
                ws();
                if (*p == ',') {
                    ++p;
                    continue;
                }
                if (*p == ']') {
                    ++p;
                    return j;
                }
                ok = false;
                return j;
            }
        }
        if (*p == '"') {
            j.type = Json::Type::Str;
            j.s = str();
            return j;
        }
        if (lit("true")) {
            j.type = Json::Type::Bool;
            j.b = true;
            return j;
        }
        if (lit("false")) {
            j.type = Json::Type::Bool;
            return j;
        }
        if (lit("null")) return j;
        char* e = nullptr;
        j.n = std::strtod(p, &e);
        if (e == p) {
            ok = false;
            return j;
        }
        j.type = Json::Type::Num;
        p = e;
        return j;
    }
};

bool parse_json(const std::string& text, Json& out) {
    Parser ps{text.data(), text.data() + text.size()};
    out = ps.value();
    ps.ws();
    return ps.ok && ps.p == ps.end;
}

std::string read_file(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    std::stringstream ss;
    ss << f.rdbuf();
    return ss.str();
}

std::vector<Json> read_records(const std::string& path, int& bad_lines) {
    std::vector<Json> r;
    std::ifstream f(path, std::ios::binary);
    std::string line;
    bad_lines = 0;
    while (std::getline(f, line)) {
        if (line.empty()) continue;
        Json j;
        if (parse_json(line, j)) {
            r.push_back(std::move(j));
        } else {
            ++bad_lines;
        }
    }
    return r;
}

std::vector<const Json*> by_fn(const std::vector<Json>& all, const char* fn) {
    std::vector<const Json*> r;
    for (const Json& j : all) {
        const Json* f = j.get("fn");
        if (f && f->is(fn)) r.push_back(&j);
    }
    return r;
}

std::string hexs(const void* p) {
    char buf[32];
    std::snprintf(buf, sizeof(buf), "0x%llx", static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(p)));
    return buf;
}

std::string exe_basename() {
    char path[MAX_PATH];
    GetModuleFileNameA(nullptr, path, MAX_PATH);
    const char* slash = std::strrchr(path, '\\');
    return slash ? slash + 1 : path;
}

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

uint64_t parse_hex(const Json* j) {
    if (!j || j->type != Json::Type::Str) return ~0ull;
    return std::strtoull(j->s.c_str(), nullptr, 16);
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

// ---- Assertions on the output ------------------------------------------------------------------
void check_fsr2_records(const std::vector<Json>& all, const std::string& exe) {
    auto recs = by_fn(all, "ffxFsr2ContextDispatch");
    CHECK(recs.size() >= 7);
    const uint32_t size = image_size();
    int ok_records = 0, failed_records = 0, named_thread = 0;
    for (const Json* r : recs) {
        const Json* module = r->get("module");
        CHECK(module && module->is(exe.c_str()));
        const uint64_t rva = parse_hex(r->get("rva"));
        CHECK(rva > 0x1000 && rva < size);
        CHECK(r->get("api") && r->get("api")->is("fsr2"));
        CHECK(r->get("tid") && r->get("tid")->n > 0);
        CHECK(parse_hex(r->get("ret")) == 0x1234ABCD);
        const Json* thread = r->get("thread");
        if (thread && thread->is("census-worker")) ++named_thread;
        const Json* check = r->at({"args", "layout_check"});
        if (check && check->is("ok")) {
            ++ok_records;
            const Json* res = r->at({"args", "resources"});
            CHECK(res != nullptr);
            CHECK(res->at({"color", "name"}) && res->at({"color", "name"})->is("FSR2_InputColor"));
            CHECK(res->at({"depth", "name"}) && res->at({"depth", "name"})->is("FSR2_InputDepth"));
            CHECK(res->at({"output", "name"}) && res->at({"output", "name"})->is("FSR2_OutputUpscaledColor"));
            CHECK(res->at({"color", "width"}) && res->at({"color", "width"})->n == 1280);
            CHECK(res->at({"output", "height"}) && res->at({"output", "height"})->n == 1080);
            CHECK(res->at({"depth", "is_depth"}) && res->at({"depth", "is_depth"})->b);
            CHECK(res->at({"color", "d3d12", "format_name"}) &&
                  res->at({"color", "d3d12", "format_name"})->is("R16G16B16A16_FLOAT"));
            CHECK(res->at({"depth", "d3d12", "format_name"}) && res->at({"depth", "d3d12", "format_name"})->is("D32_FLOAT"));
            CHECK(res->at({"output", "d3d12", "width"}) && res->at({"output", "d3d12", "width"})->n == 1920);
            CHECK(res->at({"reactive", "ptr"}) && res->at({"reactive", "ptr"})->type == Json::Type::Null);
            const Json* sc = r->at({"args", "scalars"});
            CHECK(sc != nullptr);
            CHECK_NEAR(sc->get("jitter_x")->n, 0.25);
            CHECK_NEAR(sc->get("jitter_y")->n, -0.375);
            CHECK_NEAR(sc->get("mv_scale_x")->n, 1280.0);
            CHECK(sc->get("render_width")->n == 1280 && sc->get("render_height")->n == 720);
            CHECK(sc->get("enable_sharpening")->b);
            CHECK_NEAR(sc->get("sharpness")->n, 0.5);
            CHECK_NEAR(sc->get("frame_time_delta_ms")->n, 16.6);
            CHECK_NEAR(sc->get("pre_exposure")->n, 1.0);
            CHECK(!sc->get("reset")->b);
            CHECK_NEAR(sc->get("camera_far")->n, 5000.0);
            CHECK_NEAR(sc->get("camera_fov_vertical")->n, 1.0);
        } else if (check && check->is("failed")) {
            ++failed_records;
            CHECK(r->at({"args", "raw_bytes_len"}) && r->at({"args", "raw_bytes_len"})->n == 1352);
            CHECK(r->at({"args", "resources"}) == nullptr);
        }
    }
    CHECK(ok_records >= 4);
    CHECK(failed_records == 1);
    CHECK(named_thread == 1);
}

void check_ngx_records(const std::vector<Json>& all) {
    auto creates = by_fn(all, "NVSDK_NGX_D3D12_CreateFeature");
    CHECK(creates.size() == 1);
    if (creates.size() == 1) {
        const Json* a = creates[0]->get("args");
        CHECK(a && a->get("feature") && a->get("feature")->is("SuperSampling"));
        const Json* c = a->get("create");
        CHECK(c != nullptr);
        CHECK(c->get("Width")->n == 1280 && c->get("Height")->n == 720);
        CHECK(c->get("OutWidth")->n == 1920 && c->get("OutHeight")->n == 1080);
        CHECK(c->get("PerfQualityValue")->n == 2 && c->get("quality")->is("MaxQuality"));
        const Json* names = c->get("create_flag_names");
        CHECK(names && names->has_string("IsHDR") && names->has_string("MVLowRes") && names->has_string("MVJittered") &&
              names->has_string("DepthInverted") && names->has_string("AutoExposure"));
        CHECK(!names->has_string("AlphaUpscaling"));
        CHECK(a->get("handle") && a->get("handle")->type == Json::Type::Str);
        CHECK(parse_hex(a->get("command_list_or_context")) == 0x1111);
    }
    auto evals = by_fn(all, "NVSDK_NGX_D3D12_EvaluateFeature");
    CHECK(evals.size() == 2);
    if (!evals.empty()) {
        const Json* a = evals[0]->get("args");
        CHECK(parse_hex(a->get("command_list_or_context")) == 0x2222);
        const Json* res = a->get("resources");
        CHECK(res != nullptr);
        CHECK(res->at({"Color", "format_name"})->is("R16G16B16A16_FLOAT"));
        CHECK(res->at({"Color", "width"})->n == 1280 && res->at({"Color", "height"})->n == 720);
        CHECK(res->at({"Output", "width"})->n == 1920);
        CHECK(res->at({"Depth", "format_name"})->is("D32_FLOAT"));
        CHECK(res->at({"MotionVectors", "format_name"})->is("R16G16_FLOAT"));
        CHECK(res->at({"ExposureTexture", "format_name"})->is("R32_FLOAT"));
        CHECK(res->get("TransparencyMask") == nullptr);
        const Json* sc = a->get("scalars");
        CHECK(sc != nullptr);
        CHECK_NEAR(sc->get("Jitter.Offset.X")->n, -0.125);
        CHECK_NEAR(sc->get("Jitter.Offset.Y")->n, 0.0625);
        CHECK_NEAR(sc->get("MV.Scale.X")->n, 1280.0);
        CHECK(sc->get("Reset")->n == 1);
        CHECK_NEAR(sc->get("Sharpness")->n, 0.35);
        CHECK(sc->get("DLSS.Render.Subrect.Dimensions.Width")->n == 1280);
        CHECK_NEAR(sc->get("DLSS.Pre.Exposure")->n, 1.5);
        CHECK_NEAR(sc->get("FrameTimeDeltaInMsec")->n, 16.6);
        CHECK(sc->get("DLSS.Input.Color.Subrect.Base.X")->n == 16);
        CHECK(evals[0]->get("module")->is(exe_basename().c_str()));
    }
}

void check_other_records(const std::vector<Json>& all) {
    auto qi = by_fn(all, "nvapi_QueryInterface");
    CHECK(qi.size() >= 3);
    bool saw_marker_id = false;
    for (const Json* r : qi) {
        if (r->at({"args", "id"}) && r->at({"args", "id"})->is("0xd9984c05")) {
            saw_marker_id = true;
            CHECK(r->at({"args", "reflex_entry"})->is("NvapiSetLatencyMarker"));
        }
    }
    CHECK(saw_marker_id);
    auto marker = by_fn(all, "NvapiSetLatencyMarker");
    CHECK(marker.size() == 1);
    if (marker.size() == 1) {
        const Json* a = marker[0]->get("args");
        CHECK(a->get("frame_id")->n == 77);
        CHECK(a->get("marker_type")->n == 4 && a->get("marker")->is("PRESENT_START"));
        CHECK(parse_hex(a->get("device_or_queue")) == 0xD3D3);
        CHECK(parse_hex(marker[0]->get("ret")) == 0x5151);
    }
    auto sleep_mode = by_fn(all, "NvapiSetSleepMode");
    CHECK(sleep_mode.size() == 1);
    if (sleep_mode.size() == 1) {
        const Json* a = sleep_mode[0]->get("args");
        CHECK(a->get("low_latency_mode")->b && a->get("low_latency_boost")->b);
        CHECK(a->get("minimum_interval_us")->n == 8333);
    }
    auto jitter = by_fn(all, "xessSetJitterScale");
    CHECK(jitter.size() == 1);
    if (jitter.size() == 1) {
        CHECK_NEAR(jitter[0]->at({"args", "x"})->n, 0.25);
        CHECK_NEAR(jitter[0]->at({"args", "y"})->n, -0.5);
    }
    auto exec = by_fn(all, "xessD3D12Execute");
    CHECK(exec.size() == 2);
    if (!exec.empty()) {
        const Json* a = exec[0]->get("args");
        CHECK(a->at({"resources", "color", "format_name"})->is("R16G16B16A16_FLOAT"));
        CHECK(a->at({"resources", "velocity", "format_name"})->is("R16G16_FLOAT"));
        CHECK(a->at({"resources", "output", "width"})->n == 1920);
        CHECK(a->at({"resources", "exposure_scale", "ptr"}) == nullptr ||
              a->at({"resources", "exposure_scale"})->type == Json::Type::Null);
        CHECK_NEAR(a->at({"scalars", "jitter_x"})->n, 0.125);
        CHECK(a->at({"scalars", "input_width"})->n == 1280);
        CHECK(parse_hex(exec[0]->get("ret")) == 0x77);
    }
}

bool entry_modified(const Json& summary, const char* fn) {
    const Json* hooks = summary.get("hooks");
    if (!hooks) return false;
    for (const Json& hk : hooks->a) {
        const Json* m = hk.get("entry_modified");
        if (hk.get("function")->is(fn) && m && m->b) return true;
    }
    return false;
}

void check_dxgi_records(const std::vector<Json>& all, const Json& summary) {
    auto factory = by_fn(all, "CreateDXGIFactory1");
    CHECK(factory.size() == 1);
    CHECK(by_fn(all, "D3D12CreateDevice").size() == 1);
    auto queues = by_fn(all, "D3d12DeviceCreateCommandQueue");
    CHECK(queues.size() == 1);
    if (!queues.empty()) CHECK(queues[0]->at({"args", "type_name"})->is("DIRECT"));
    auto hwnd = by_fn(all, "DxgiFactoryCreateSwapChainForHwnd");
    CHECK(hwnd.size() == 1);
    if (!hwnd.empty()) {
        const Json* a = hwnd[0]->get("args");
        CHECK(a->at({"desc", "width"})->n == 640 && a->at({"desc", "height"})->n == 360);
        CHECK(a->at({"desc", "format_name"})->is("B8G8R8A8_UNORM"));
        CHECK(a->at({"desc", "buffer_count"})->n == 2);
        CHECK(a->at({"desc", "swap_effect"})->n == static_cast<double>(DXGI_SWAP_EFFECT_FLIP_DISCARD));
    }
    // An overlay (RTSS, Steam, Discord) that hooks Present after the census can overwrite the
    // census' detour at the function entry. The census reports that instead of hiding it, so a
    // missing record is only acceptable when the summary flags the entry as modified.
    const bool present_modified = entry_modified(summary, "DxgiSwapChainPresent");
    auto p0 = by_fn(all, "DxgiSwapChainPresent");
    auto p1 = by_fn(all, "DxgiSwapChainPresent1");
    if (present_modified) {
        std::printf("NOTE: another hook layer overwrote the Present detour (overlay in process); "
                    "present-count assertions relaxed, entry_modified reported\n");
        CHECK(!p0.empty() || !p1.empty());
    } else {
        CHECK(p0.size() == 3); // two frames plus the test present
        CHECK(p1.size() == 1);
        if (p0.size() == 3) {
            CHECK(p0[0]->get("present")->n == 1);
            CHECK(p0[0]->at({"args", "swapchain_desc", "width"})->n == 640);
            CHECK(p0[0]->at({"args", "swapchain_desc", "format_name"})->is("B8G8R8A8_UNORM"));
            CHECK(p0[0]->at({"args", "present_flags"}) != nullptr);
            CHECK(p0[0]->at({"args", "last_color_space_set"}) != nullptr);
            CHECK(p0[1]->get("present")->n == 3);
            CHECK(p0[2]->get("present")->n == 3); // the test present is not counted
            CHECK(p0[2]->at({"args", "present_flag_names"})->has_string("TEST"));
        }
        if (!p1.empty()) CHECK(p1[0]->get("present")->n == 2);
    }
    auto cs = by_fn(all, "DxgiSwapChainSetColorSpace1");
    CHECK(cs.size() == 1);
    CHECK(by_fn(all, "DxgiSwapChainResizeBuffers").size() == 1);
    auto exec = by_fn(all, "D3d12QueueExecuteCommandLists");
    CHECK(exec.size() >= 4); // the caller may be an overlay hook, so several calls can share a site

    // Queue statistics and the calls-per-present histogram.
    const Json* qs = summary.get("queues");
    CHECK(qs && qs->a.size() == 1);
    if (qs && qs->a.size() == 1) {
        const Json& q = qs->a[0];
        CHECK(q.get("kind")->is("d3d12"));
        CHECK(q.get("type")->n == 0);
        CHECK(q.get("submit_calls")->n == 6);
        if (!present_modified) {
            const Json* h = q.at({"calls_per_present", "2"});
            CHECK(h && h->n == 3);
        }
    }
}

void check_summary(const Json& summary, const std::string& exe, uint64_t dispatch_calls, bool had_dxgi) {
    const Json* sites = summary.get("sites");
    CHECK(sites && sites->type == Json::Type::Arr);
    if (!sites) return;
    double dispatch_total = 0, helper_site = 0, pointer_site = 0;
    size_t helper_threads = 0;
    bool saw_ret_in_exe = false;
    for (const Json& s : sites->a) {
        const Json* fn = s.get("function");
        if (!fn || !fn->is("ffxFsr2ContextDispatch")) continue;
        CHECK(s.get("module")->is(exe.c_str()));
        CHECK(s.get("api")->is("fsr2"));
        const double count = s.get("count")->n;
        dispatch_total += count;
        saw_ret_in_exe = true;
        if (s.get("threads")->a.size() == 2) {
            helper_site = count;
            helper_threads = 2;
        } else {
            pointer_site = count;
        }
        CHECK(s.get("first_present")->n <= s.get("last_present")->n);
    }
    CHECK(saw_ret_in_exe);
    CHECK(dispatch_total == static_cast<double>(dispatch_calls));
    CHECK(helper_threads == 2);
    CHECK(pointer_site == 2);
    CHECK(helper_site == static_cast<double>(dispatch_calls) - 2);

    // Histogram: the calls made between presents.
    if (had_dxgi && !entry_modified(summary, "DxgiSwapChainPresent")) {
        const Json* hs = summary.get("histograms");
        CHECK(hs != nullptr);
        for (const Json& h : hs->a) {
            if (!h.get("function")->is("ffxFsr2ContextDispatch")) continue;
            CHECK(h.get("calls")->n == static_cast<double>(dispatch_calls));
            const Json* by = h.get("calls_per_present");
            CHECK(by && by->get("2") && by->get("2")->n == 2);
            char first[16];
            std::snprintf(first, sizeof(first), "%llu", static_cast<unsigned long long>(dispatch_calls - 6 + 2));
            CHECK(by->get(first) && by->get(first)->n == 1);
        }
        CHECK(summary.get("present_index")->n == 3);
    }

    // Threads and hooks.
    const Json* threads = summary.get("threads");
    bool named = false;
    if (threads) {
        for (const Json& t : threads->a) {
            if (t.get("name") && t.get("name")->is("census-worker")) named = true;
        }
    }
    CHECK(named);
    CHECK(summary.get("hooks") && summary.get("hooks")->a.size() > 5);
    const Json* mods = summary.get("modules_hooked");
    CHECK(mods && mods->has_string("ffx_fsr2_api_x64.dll") && mods->has_string("_nvngx.dll") &&
          mods->has_string("libxess.dll"));
    // The nvapi fixture is not on the detour list, so it was bound by a GetProcAddress thunk.
    bool thunk = false;
    for (const Json& h : summary.get("hooks")->a) {
        if (h.get("function")->is("nvapi_QueryInterface") && h.get("binding")->is("getprocaddress_thunk")) thunk = true;
    }
    CHECK(thunk);
}

void check_static_findings(const Json& summary) {
    const Json* f = summary.get("static_findings");
    CHECK(f != nullptr);
    if (!f) return;
    bool xref = false;
    const uint64_t base = reinterpret_cast<uint64_t>(GetModuleHandleW(nullptr));
    const uint64_t anchor = reinterpret_cast<uint64_t>(&static_anchor) - base;
    for (const Json& e : f->a) {
        if (e.get("vendor")->is("fsr2") && e.get("what")->s.rfind("xref to FSR2_InputColor", 0) == 0) {
            // The anchor function is a leaf, so its start comes from the padding before it.
            if (parse_hex(e.get("function_rva")) == anchor && parse_hex(e.get("rva")) >= anchor &&
                parse_hex(e.get("rva")) < anchor + 0x40) {
                xref = true;
            }
        }
    }
    CHECK(xref);
}

int functional(const std::string& dir) {
    check_vtable_slots();

    rsf_census_options opt{};
    opt.struct_size = sizeof(opt);
    opt.abi_version = RSF_CENSUS_ABI_VERSION;
    opt.output_dir_utf8 = dir.c_str();
    opt.byte_budget = 8u << 20;
    opt.full_record_cap = 4;
    opt.sample_interval = 10;
    opt.summary_period_ms = 50;

    rsf_census_options bad = opt;
    bad.abi_version = 99;
    CHECK(rsf_census_install(&bad) == RSF_CENSUS_ERROR_ABI_MISMATCH);
    bad = opt;
    bad.struct_size = 4;
    CHECK(rsf_census_install(&bad) == RSF_CENSUS_ERROR_INVALID_ARGUMENT);

    CHECK(rsf_census_install(&opt) == RSF_CENSUS_OK);
    CHECK(rsf_census_install(&opt) == RSF_CENSUS_ERROR_ALREADY_INSTALLED);
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

    // The static scan runs on the writer thread; wait for it.
    rsf_census_counters counters{sizeof(counters), RSF_CENSUS_ABI_VERSION};
    for (int i = 0; i < 400; ++i) {
        rsf_census_get_counters(&counters);
        if (counters.static_findings > 0) break;
        Sleep(50);
    }
    CHECK(rsf_census_flush() == RSF_CENSUS_OK);
    rsf_census_get_counters(&counters);

    const std::string exe = exe_basename();
    int bad_lines = 0;
    const std::vector<Json> all = read_records(dir + "\\census.jsonl", bad_lines);
    CHECK(bad_lines == 0);
    CHECK(!all.empty() && all[0].get("type") && all[0].get("type")->is("census_start"));
    Json summary;
    CHECK(parse_json(read_file(dir + "\\census-summary.json"), summary));

    check_fsr2_records(all, exe);
    check_ngx_records(all);
    check_other_records(all);
    check_summary(summary, exe, dispatch_calls, had_dxgi);
    if (had_dxgi) check_dxgi_records(all, summary);
    check_static_findings(summary);

    // Sampling: calls 0..3 in full, then calls 4, 14, ..., 94.
    CHECK(by_fn(all, "ffxFsr2GetJitterOffset").size() == 1 + 14); // one direct call earlier, 100 at the noise site
    // Sequence numbers are dense and increase.
    double last_seq = -1;
    for (const Json& j : all) {
        if (const Json* s = j.get("seq")) {
            CHECK(s->n == last_seq + 1);
            last_seq = s->n;
        }
    }
    CHECK(counters.records_dropped_queue == 0 && counters.records_dropped_budget == 0 && counters.records_dropped_size == 0);
    CHECK(counters.records_written == all.size() - 1);
    CHECK(counters.calls_seen > 100);
    CHECK(counters.hooks_installed > 10);
    CHECK(counters.sites > 10);
    CHECK(counters.static_findings > 0);

    CHECK(rsf_census_shutdown() == RSF_CENSUS_OK);
    // After shutdown the hooks are off and calls pass straight through.
    CHECK(dispatch_site(nullptr) == 0x1234ABCD);

    std::printf("vendor_census functional: %d failure(s)\n", g_failures);
    return g_failures ? 1 : 0;
}

int budget(const std::string& dir) {
    rsf_census_options opt{};
    opt.struct_size = sizeof(opt);
    opt.abi_version = RSF_CENSUS_ABI_VERSION;
    opt.output_dir_utf8 = dir.c_str();
    opt.byte_budget = 40000;
    opt.full_record_cap = 1000000;
    opt.sample_interval = 1;
    opt.summary_period_ms = 50;
    opt.flags = RSF_CENSUS_FLAG_NO_STATIC_SCAN | RSF_CENSUS_FLAG_NO_DXGI | RSF_CENSUS_FLAG_NO_VULKAN;
    CHECK(rsf_census_install(&opt) == RSF_CENSUS_OK);

    for (int i = 0; i < 5000; ++i) {
        CHECK(jitter_noise(i) == 0);
    }
    CHECK(rsf_census_flush() == RSF_CENSUS_OK);

    rsf_census_counters c{sizeof(c), RSF_CENSUS_ABI_VERSION};
    CHECK(rsf_census_get_counters(&c) == RSF_CENSUS_OK);
    const std::string text = read_file(dir + "\\census.jsonl");
    CHECK(text.size() <= 40000);
    CHECK(c.bytes_written == text.size());
    CHECK(c.bytes_written > 30000); // the budget was actually used
    CHECK(c.calls_seen >= 5000);
    CHECK(c.records_dropped_budget + c.records_dropped_queue > 0);
    CHECK(c.records_dropped_size == 0);
    CHECK(c.records_written + c.records_dropped_budget == c.records_queued);

    int bad_lines = 0;
    const std::vector<Json> all = read_records(dir + "\\census.jsonl", bad_lines);
    CHECK(bad_lines == 0);

    // The summary still counts every call although most records were dropped.
    Json summary;
    CHECK(parse_json(read_file(dir + "\\census-summary.json"), summary));
    double counted = 0;
    for (const Json& s : summary.get("sites")->a) {
        if (s.get("function")->is("ffxFsr2GetJitterOffset")) counted += s.get("count")->n;
    }
    CHECK(counted == 5000);
    CHECK(summary.at({"budget", "exhausted"}) && summary.at({"budget", "exhausted"})->b);
    CHECK(summary.at({"budget", "bytes_written"})->n <= 40000);

    CHECK(rsf_census_shutdown() == RSF_CENSUS_OK);
    std::printf("vendor_census budget: %d failure(s), %llu bytes written, %llu records dropped\n", g_failures,
                static_cast<unsigned long long>(c.bytes_written),
                static_cast<unsigned long long>(c.records_dropped_budget + c.records_dropped_queue));
    return g_failures ? 1 : 0;
}

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

int vulkan_mode(const std::string& dir) {
    rsf_census_options opt{};
    opt.struct_size = sizeof(opt);
    opt.abi_version = RSF_CENSUS_ABI_VERSION;
    opt.output_dir_utf8 = dir.c_str();
    opt.full_record_cap = 8;
    opt.summary_period_ms = 50;
    opt.flags = RSF_CENSUS_FLAG_NO_STATIC_SCAN;
    CHECK(rsf_census_install(&opt) == RSF_CENSUS_OK);

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
    unsigned char options[96] = {};
    put<uint32_t>(options, 32, 1);
    put<uint32_t>(options, 36, 2);
    put<uint32_t>(options, 64, 1920);
    put<uint32_t>(options, 68, 1080);
    CHECK(reinterpret_cast<int (*)(const void*, const void*)>(dlssg_options)(viewport, options) == 0x33);

    CHECK(rsf_census_flush() == RSF_CENSUS_OK);
    int bad_lines = 0;
    const std::vector<Json> all = read_records(dir + "\\census.jsonl", bad_lines);
    CHECK(bad_lines == 0);
    Json summary;
    CHECK(parse_json(read_file(dir + "\\census-summary.json"), summary));

    auto one = [&](const char* fn) -> const Json* {
        auto r = by_fn(all, fn);
        CHECK(r.size() >= 1);
        return r.empty() ? nullptr : r[0]->get("args");
    };
    if (const Json* a = one("vkCreateInstance")) {
        CHECK(a->get("application")->is("CensusGame") && a->get("engine")->is("CensusEngine"));
        CHECK(a->get("api_major")->n == 1 && a->get("api_minor")->n == 3);
        CHECK(a->get("extensions")->has_string("VK_KHR_win32_surface"));
        CHECK(parse_hex(a->get("instance")) == 0x1001);
    }
    if (const Json* a = one("vkCreateDevice")) {
        CHECK(a->get("queue_families")->a.size() == 2);
        CHECK(a->get("queue_families")->a[1].get("family")->n == 1 && a->get("queue_families")->a[1].get("count")->n == 2);
        CHECK(a->get("extensions")->has_string("VK_KHR_swapchain"));
    }
    if (const Json* a = one("vkCreateSwapchainKHR")) {
        CHECK(a->get("width")->n == 2560 && a->get("height")->n == 1440);
        CHECK(a->get("image_format")->n == 50 && a->get("min_image_count")->n == 3);
        CHECK(a->get("present_mode_name")->is("MAILBOX"));
        CHECK(parse_hex(a->get("swapchain")) == 0x4004);
    }
    auto presents_rec = by_fn(all, "vkQueuePresentKHR");
    CHECK(presents_rec.size() == 3);
    for (size_t i = 0; i < presents_rec.size(); ++i) {
        CHECK(presents_rec[i]->get("present")->n == static_cast<double>(i + 1));
        CHECK(presents_rec[i]->at({"args", "swapchains"})->a.size() == 1);
        CHECK(presents_rec[i]->at({"args", "image_indices"})->a[0].n == 1);
        CHECK(presents_rec[i]->get("module")->is(exe_basename().c_str()));
    }
    auto submits_rec = by_fn(all, "vkQueueSubmit");
    CHECK(submits_rec.size() >= 2);
    if (!submits_rec.empty()) {
        CHECK(submits_rec[0]->at({"args", "batches"})->n == 2);
        CHECK(submits_rec[0]->at({"args", "command_buffers_in_first_16"})->n == 4);
    }
    if (const Json* a = one("vkQueueSubmit2")) CHECK(a->get("command_buffers_in_first_16")->n == 2);

    const Json* queues = summary.get("queues");
    CHECK(queues && queues->a.size() == 2);
    int seen_queues = 0;
    if (queues) {
        for (const Json& q : queues->a) {
            CHECK(q.get("kind")->is("vulkan"));
            if (parse_hex(q.get("queue")) == 0x3000) {
                ++seen_queues;
                CHECK(q.get("family")->n == 0 && q.get("index")->n == 0);
                CHECK(q.get("submit_calls")->n == 6 && q.get("lists_or_batches")->n == 9);
                CHECK(q.at({"calls_per_present", "2"}) && q.at({"calls_per_present", "2"})->n == 3);
            } else if (parse_hex(q.get("queue")) == 0x3011) {
                ++seen_queues;
                CHECK(q.get("family")->n == 1 && q.get("index")->n == 1);
                CHECK(q.get("submit_calls")->n == 3);
                CHECK(q.at({"calls_per_present", "1"}) && q.at({"calls_per_present", "1"})->n == 3);
            }
        }
    }
    CHECK(seen_queues == 2);
    CHECK(summary.get("present_index")->n == 3);

    if (const Json* a = one("slInit")) {
        CHECK(a->get("application_id")->n == 0x1234 && a->get("engine")->n == 1);
        CHECK(a->get("engine_version")->is("5.4") && a->get("project_id")->is("census-project"));
        CHECK(a->get("log_level")->n == 2 && a->get("show_console")->b);
        CHECK(a->get("features_to_load")->a.size() == 2 && a->get("features_to_load")->a[1].n == 1000);
        CHECK(a->get("path_to_logs_and_data")->is("C:\\census\\logs"));
    }
    if (const Json* a = one("slReflexSleep")) CHECK(a->get("frame")->n == 42);
    if (const Json* a = one("slPCLSetMarker")) {
        CHECK(a->get("marker")->n == 4 && a->get("marker_name")->is("PresentStart") && a->get("frame")->n == 42);
    }
    if (const Json* a = one("slDLSSGSetOptions")) {
        CHECK(a->get("viewport")->n == 7 && a->get("mode_name")->is("On"));
        CHECK(a->get("num_frames_to_generate")->n == 2);
        CHECK(a->get("color_width")->n == 1920 && a->get("color_height")->n == 1080);
    }
    // slGetFeatureFunction swapped the entry points for thunks and the summary says so.
    int sl_thunks = 0;
    for (const Json& hk : summary.get("hooks")->a) {
        if (hk.get("api")->is("streamline") && hk.get("binding")->is("getprocaddress_thunk")) ++sl_thunks;
    }
    CHECK(sl_thunks == 3);

    CHECK(rsf_census_shutdown() == RSF_CENSUS_OK);
    std::printf("vendor_census vulkan_streamline: %d failure(s)\n", g_failures);
    return g_failures ? 1 : 0;
}

// FidelityFX API (FSR 3.1 and later): context creation with a descriptor chain, dispatch,
// configure. Descriptor offsets are the ones tests/vendor_census_layouts.cpp checks.
int ffx_mode(const std::string& dir) {
    rsf_census_options opt{};
    opt.struct_size = sizeof(opt);
    opt.abi_version = RSF_CENSUS_ABI_VERSION;
    opt.output_dir_utf8 = dir.c_str();
    opt.full_record_cap = 8;
    opt.summary_period_ms = 50;
    opt.flags = RSF_CENSUS_FLAG_NO_STATIC_SCAN;
    CHECK(rsf_census_install(&opt) == RSF_CENSUS_OK);

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

    CHECK(rsf_census_flush() == RSF_CENSUS_OK);
    int bad_lines = 0;
    const std::vector<Json> all = read_records(dir + "\\census.jsonl", bad_lines);
    CHECK(bad_lines == 0);

    auto creates = by_fn(all, "ffxCreateContext");
    CHECK(creates.size() == 2);
    if (creates.size() == 2) {
        const Json* chain = creates[0]->at({"args", "chain"});
        CHECK(chain && chain->a.size() == 2);
        if (chain && chain->a.size() == 2) {
            CHECK(chain->a[0].get("name")->is("create_upscale"));
            CHECK(chain->a[0].get("max_render_width")->n == 1280 && chain->a[0].get("max_upscale_height")->n == 1080);
            CHECK(chain->a[1].get("name")->is("create_backend_dx12"));
            CHECK(parse_hex(chain->a[1].get("device")) == 0xDE71CE);
        }
        CHECK(parse_hex(creates[0]->at({"args", "context"})) == 0xF00D);
        const Json* sc = creates[1]->at({"args", "chain"});
        CHECK(sc && sc->a.size() == 1 && sc->a[0].get("name")->is("create_swapchain_for_hwnd_dx12"));
        if (sc && sc->a.size() == 1) {
            CHECK(sc->a[0].at({"swapchain_desc1", "width"})->n == 3840);
            CHECK(sc->a[0].at({"swapchain_desc1", "format_name"})->is("R10G10B10A2_UNORM"));
            CHECK(sc->a[0].at({"swapchain_desc1", "buffer_count"})->n == 3);
            CHECK(parse_hex(sc->a[0].get("game_queue")) == 0x9E3E);
        }
    }
    auto dispatches = by_fn(all, "ffxDispatch");
    CHECK(dispatches.size() == 2);
    if (dispatches.size() == 2) {
        CHECK(parse_hex(dispatches[0]->get("ret")) == 0x40);
        const Json* node = dispatches[0]->at({"args", "chain"});
        CHECK(node && node->a.size() == 1 && node->a[0].get("name")->is("dispatch_upscale"));
        if (node && node->a.size() == 1) {
            const Json& d = node->a[0];
            CHECK(d.at({"resources", "color", "d3d12", "width"})->n == 1280);
            CHECK(d.at({"resources", "depth", "d3d12", "format_name"})->is("D32_FLOAT"));
            CHECK(d.at({"resources", "output", "d3d12", "width"})->n == 1920);
            CHECK(d.at({"resources", "output", "state"})->is("0x4"));
            CHECK_NEAR(d.at({"scalars", "jitter_x"})->n, 0.5);
            CHECK_NEAR(d.at({"scalars", "jitter_y"})->n, -0.25);
            CHECK(d.at({"scalars", "render_width"})->n == 1280 && d.at({"scalars", "upscale_height"})->n == 1080);
            CHECK(d.at({"scalars", "enable_sharpening"})->b && d.at({"scalars", "reset"})->b);
            CHECK_NEAR(d.at({"scalars", "frame_time_delta_ms"})->n, 8.3);
            CHECK_NEAR(d.at({"scalars", "camera_far"})->n, 10000.0);
            CHECK_NEAR(d.at({"scalars", "camera_fov_vertical"})->n, 0.9);
            CHECK(d.at({"scalars", "flags"})->is("0x2"));
        }
        const Json* fgnode = dispatches[1]->at({"args", "chain"});
        CHECK(fgnode && fgnode->a.size() == 1 && fgnode->a[0].get("name")->is("dispatch_framegeneration"));
        if (fgnode && fgnode->a.size() == 1) {
            const Json& d = fgnode->a[0];
            CHECK(d.get("num_generated_frames")->n == 1 && d.get("reset")->b);
            CHECK(d.get("backbuffer_transfer_function")->n == 2);
            CHECK(d.get("frame_id")->n == 1234);
            CHECK_NEAR(d.get("max_luminance")->n, 1000.0);
        }
    }
    auto configs = by_fn(all, "ffxConfigure");
    CHECK(configs.size() == 1);
    if (configs.size() == 1) {
        const Json& n = configs[0]->at({"args", "chain"})->a[0];
        CHECK(n.get("name")->is("configure_framegeneration"));
        CHECK(parse_hex(n.get("swap_chain")) == 0x5C5C && n.get("frame_generation_enabled")->b);
        CHECK(n.get("frame_id")->n == 99);
    }
    CHECK(by_fn(all, "ffxQuery").size() == 1);
    CHECK(by_fn(all, "ffxDestroyContext").size() == 1);

    CHECK(rsf_census_shutdown() == RSF_CENSUS_OK);
    std::printf("vendor_census fidelityfx_api: %d failure(s)\n", g_failures);
    return g_failures ? 1 : 0;
}

int dllmain_mode(const std::string& dir) {
    SetEnvironmentVariableA("RSF_CENSUS_OUT", dir.c_str());
    // The fsr2 fixture is a static import of this executable, so it is already mapped when the
    // plugin installs the census from DllMain: the already-loaded-module scan runs under the lock.
    HMODULE asi = load_fixture(L"rsf_census_asi.dll");
    CHECK(asi != nullptr);
    if (!asi) return 1;
    auto result = proc<int (*)()>(asi, "asi_install_result");
    auto flush = proc<int (*)()>(asi, "asi_flush");
    auto counters = proc<int (*)(rsf_census_counters*)>(asi, "asi_counters");
    auto shutdown = proc<int (*)()>(asi, "asi_shutdown");
    CHECK(result && flush && counters && shutdown);
    if (!(result && flush && counters && shutdown)) return 1;
    CHECK(result() == RSF_CENSUS_OK);

    Frame frame;
    Fsr2Dispatch d;
    make_dispatch(d, frame);
    CHECK(dispatch_site(&d) == 0x1234ABCD);
    CHECK(dispatch_site(&d) == 0x1234ABCD);
    CHECK(flush() == RSF_CENSUS_OK);

    rsf_census_counters c{sizeof(c), RSF_CENSUS_ABI_VERSION};
    CHECK(counters(&c) == RSF_CENSUS_OK);
    CHECK(c.hooks_installed > 0 && c.modules_hooked > 0);
    int bad_lines = 0;
    const std::vector<Json> all = read_records(dir + "\\census.jsonl", bad_lines);
    CHECK(bad_lines == 0);
    auto recs = by_fn(all, "ffxFsr2ContextDispatch");
    CHECK(recs.size() == 2);
    if (!recs.empty()) {
        CHECK(recs[0]->at({"args", "layout_check"})->is("ok"));
        CHECK(recs[0]->get("module")->is(exe_basename().c_str()));
    }
    Json summary;
    CHECK(parse_json(read_file(dir + "\\census-summary.json"), summary));
    const Json* mods = summary.get("modules_hooked");
    CHECK(mods && mods->has_string("ffx_fsr2_api_x64.dll"));
    CHECK(shutdown() == RSF_CENSUS_OK);
    std::printf("vendor_census dllmain: %d failure(s)\n", g_failures);
    return g_failures ? 1 : 0;
}

} // namespace

int main(int argc, char** argv) {
    if (argc < 3) {
        std::printf("usage: %s functional|budget <output dir>\n", argv[0]);
        return 2;
    }
    const std::string mode = argv[1];
    if (mode == "functional") return functional(argv[2]);
    if (mode == "budget") return budget(argv[2]);
    if (mode == "vulkan") return vulkan_mode(argv[2]);
    if (mode == "ffx") return ffx_mode(argv[2]);
    if (mode == "dllmain") return dllmain_mode(argv[2]);
    return 2;
}
