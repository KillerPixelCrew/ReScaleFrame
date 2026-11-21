// SPDX-License-Identifier: MIT
#include "overlay_hooks.h"
#include "native_hook_ops.h"
#include <d3d11.h>
#include <dxgi1_2.h>
#include <initializer_list>
#include <cstring>

using HookAddress = uintptr_t;
using HookReason = const char*;
using AddHook = bool (*)(void*, HookAddress, HookAddress, uint32_t, HookReason*) noexcept;
using ProbeHooks = bool (*)(void*, AddHook, HookReason*, void**) noexcept;
using InputObserver = void (*)(rsf::anomaly::overlay::DeviceKind) noexcept;
using PresentCallback = void (*)(void*, IDXGISwapChain*) noexcept;

using rsf::anomaly::HookOps;

extern "C" {
bool rsf_anomaly_overlay_install_input(HookAddress, const HookOps*, ProbeHooks, HookReason*) noexcept;
bool rsf_anomaly_overlay_install_present(PresentCallback, PresentCallback, HookAddress,
    const HookOps*, ProbeHooks, HookReason*) noexcept;
HookAddress rsf_anomaly_overlay_original(HookAddress) noexcept;
HookAddress rsf_anomaly_overlay_present_original(uint32_t) noexcept;
HookAddress rsf_anomaly_overlay_input_gate() noexcept;
void rsf_anomaly_overlay_set_input_observer(InputObserver) noexcept;
InputObserver rsf_anomaly_overlay_input_observer() noexcept;
void rsf_anomaly_overlay_before_present(IDXGISwapChain*, uint32_t flags) noexcept;
void rsf_anomaly_overlay_before_resize(IDXGISwapChain*) noexcept;
}

static_assert(DXGI_PRESENT_TEST == 1);
static_assert(sizeof(HookAddress) == sizeof(void*));

namespace rsf::anomaly::overlay {
namespace {
// COM vtable slots. These are fixed by the interface definitions.
constexpr size_t slot_release = 2;
constexpr size_t slot_create_device = 3;        // IDirectInput8
constexpr size_t slot_acquire = 7;              // IDirectInputDevice8
constexpr size_t slot_unacquire = 8;
constexpr size_t slot_get_device_state = 9;
constexpr size_t slot_get_device_data = 10;
constexpr size_t slot_set_cooperative_level = 13;
constexpr size_t slot_present = 8;              // IDXGISwapChain
constexpr size_t slot_resize_buffers = 13;
constexpr size_t slot_resize_target = 14;
constexpr size_t slot_present1 = 22;            // IDXGISwapChain1

// DirectInput GUIDs and IIDs spelled out so the adapter links neither dinput8.lib nor dxguid.lib.
constexpr GUID guid_sys_mouse{0x6F1D2B60, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_keyboard{0x6F1D2B61, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_mouse_em{0x6F1D2B80, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_mouse_em2{0x6F1D2B81, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_keyboard_em{0x6F1D2B82, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_keyboard_em2{0x6F1D2B83, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID iid_direct_input8_a{0xBF798030, 0x483A, 0x4DA2, {0xAA, 0x99, 0x5D, 0x64, 0xED, 0x36, 0x97, 0x00}};
constexpr GUID iid_direct_input8_w{0xBF798031, 0x483A, 0x4DA2, {0xAA, 0x99, 0x5D, 0x64, 0xED, 0x36, 0x97, 0x00}};

enum class Method : uint32_t { create_device, acquire, unacquire, get_state, get_data, cooperative, present,
    present1, resize_buffers, resize_target };

struct Collector {
    void* registry;
    AddHook add;
    bool operator()(void* target, void* detour, Method method, HookReason* reason) const noexcept {
        return add(registry, reinterpret_cast<HookAddress>(target), reinterpret_cast<HookAddress>(detour),
            static_cast<uint32_t>(method), reason);
    }
};

InputGate* active_gate() noexcept {
    return reinterpret_cast<InputGate*>(rsf_anomaly_overlay_input_gate());
}

void** vtable(void* object) noexcept { return object ? *reinterpret_cast<void***>(object) : nullptr; }
void release(void* object) noexcept {
    if (object) reinterpret_cast<ULONG (STDMETHODCALLTYPE*)(void*)>(vtable(object)[slot_release])(object);
}

// ---- DirectInput ---------------------------------------------------------------------------------
// The trampoline of the method the engine called, found from the object's own vtable so Unicode and
// ANSI device classes each reach their own original. An unhooked slot is called directly.
void* real_method(void* self, size_t slot) noexcept {
    const auto target = reinterpret_cast<HookAddress>(vtable(self)[slot]);
    return reinterpret_cast<void*>(rsf_anomaly_overlay_original(target));
}
DeviceCalls calls_for(void* self) noexcept {
    return {reinterpret_cast<decltype(DeviceCalls::acquire)>(real_method(self, slot_acquire)),
        reinterpret_cast<decltype(DeviceCalls::unacquire)>(real_method(self, slot_unacquire)),
        reinterpret_cast<decltype(DeviceCalls::get_state)>(real_method(self, slot_get_device_state)),
        reinterpret_cast<decltype(DeviceCalls::get_data)>(real_method(self, slot_get_device_data))};
}
bool kind_of(const GUID& guid, DeviceKind& kind) noexcept {
    if (IsEqualGUID(guid, guid_sys_mouse) || IsEqualGUID(guid, guid_sys_mouse_em) || IsEqualGUID(guid, guid_sys_mouse_em2)) {
        kind = DeviceKind::Mouse; return true;
    }
    if (IsEqualGUID(guid, guid_sys_keyboard) || IsEqualGUID(guid, guid_sys_keyboard_em) || IsEqualGUID(guid, guid_sys_keyboard_em2)) {
        kind = DeviceKind::Keyboard; return true;
    }
    return false;
}
HRESULT STDMETHODCALLTYPE hook_create_device(void* self, REFGUID guid, void** device, void* outer) {
    const auto original = reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*, REFGUID, void**, void*)>(
        real_method(self, slot_create_device));
    const HRESULT result = original(self, guid, device, outer);
    DeviceKind kind{};
    auto* gate = active_gate();
    if (SUCCEEDED(result) && gate && device && *device && kind_of(guid, kind)) {
        try { gate->add(*device, kind); } catch (...) {}
    }
    return result;
}
HRESULT STDMETHODCALLTYPE hook_acquire(void* self) {
    const auto calls = calls_for(self);
    try { if (auto* gate = active_gate()) return gate->acquire(self, calls); } catch (...) {}
    return calls.acquire(self);
}
HRESULT STDMETHODCALLTYPE hook_unacquire(void* self) {
    const auto calls = calls_for(self);
    try { if (auto* gate = active_gate()) return gate->unacquire(self, calls); } catch (...) {}
    return calls.unacquire(self);
}
HRESULT STDMETHODCALLTYPE hook_get_device_state(void* self, DWORD size, void* data) {
    const auto calls = calls_for(self);
    try { if (auto* gate = active_gate()) return gate->get_state(self, calls, size, data); } catch (...) {}
    return calls.get_state(self, size, data);
}
HRESULT STDMETHODCALLTYPE hook_get_device_data(void* self, DWORD object_size, DIDEVICEOBJECTDATA* data, DWORD* count, DWORD flags) {
    const auto calls = calls_for(self);
    auto* gate = active_gate();
    HRESULT result{}; bool hardware_data{};
    try { result = gate ? gate->get_data(self, calls, object_size, data, count, flags, &hardware_data) :
        calls.get_data(self, object_size, data, count, flags); }
    catch (...) { result = calls.get_data(self, object_size, data, count, flags); }
    if (hardware_data && SUCCEEDED(result) && data && count && *count && !(flags & DIGDD_PEEK) && gate) {
        DeviceKind kind{};
        if (const auto observer = rsf_anomaly_overlay_input_observer(); observer && gate->device_kind(self, kind))
            observer(kind);
    }
    return result;
}
HRESULT STDMETHODCALLTYPE hook_set_cooperative_level(void* self, HWND window, DWORD flags) {
    const auto original = reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*, HWND, DWORD)>(
        real_method(self, slot_set_cooperative_level));
    const HRESULT result = original(self, window, flags);
    if (auto* gate = active_gate(); SUCCEEDED(result) && gate) gate->cooperative_level(self, flags);
    return result;
}

bool probe_input(void* registry, AddHook add, HookReason* reason, void** owned_module) noexcept {
    const Collector collect{registry, add};
    wchar_t path[MAX_PATH]{};
    const UINT length = GetSystemDirectoryW(path, MAX_PATH);
    constexpr wchar_t suffix[] = L"\\dinput8.dll";
    if (!length || length + _countof(suffix) > MAX_PATH) { *reason = "System32 directory unavailable"; return false; }
    std::memcpy(path + length, suffix, sizeof(suffix));
    // The same absolute path the carrier forwards to, so these are the engine's DirectInput classes.
    HMODULE module = LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) { *reason = "system dinput8.dll could not be loaded"; return false; }
    *owned_module = module;
    using Create = HRESULT (WINAPI*)(HINSTANCE, DWORD, REFIID, LPVOID*, LPUNKNOWN);
    const auto create = reinterpret_cast<Create>(reinterpret_cast<void*>(GetProcAddress(module, "DirectInput8Create")));
    if (!create) { *reason = "system DirectInput8Create missing"; return false; }
    size_t devices = 0;
    for (const GUID* iid : {&iid_direct_input8_a, &iid_direct_input8_w}) {
        void* input = nullptr;
        if (FAILED(create(GetModuleHandleW(nullptr), 0x0800, *iid, &input, nullptr)) || !input) continue;
        const bool created = collect(vtable(input)[slot_create_device], reinterpret_cast<void*>(&hook_create_device),
            Method::create_device, reason);
        for (const GUID* guid : {&guid_sys_mouse, &guid_sys_keyboard}) {
            if (!created) break;
            void* device = nullptr;
            const auto create_device = reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*, REFGUID, void**, void*)>(
                vtable(input)[slot_create_device]);
            if (FAILED(create_device(input, *guid, &device, nullptr)) || !device) continue;
            void** table = vtable(device);
            const bool ok =
                collect(table[slot_acquire], reinterpret_cast<void*>(&hook_acquire), Method::acquire, reason) &&
                collect(table[slot_unacquire], reinterpret_cast<void*>(&hook_unacquire), Method::unacquire, reason) &&
                collect(table[slot_get_device_state], reinterpret_cast<void*>(&hook_get_device_state), Method::get_state, reason) &&
                collect(table[slot_get_device_data], reinterpret_cast<void*>(&hook_get_device_data), Method::get_data, reason) &&
                collect(table[slot_set_cooperative_level], reinterpret_cast<void*>(&hook_set_cooperative_level), Method::cooperative, reason);
            release(device);
            if (!ok) { release(input); return false; }
            ++devices;
        }
        release(input);
        if (!created) return false;
    }
    if (!devices) { *reason = "no DirectInput mouse or keyboard device could be probed"; return false; }
    return true;
}

// ---- DXGI ----------------------------------------------------------------------------------------
using PresentFn = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
using Present1Fn = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain1*, UINT, UINT, const DXGI_PRESENT_PARAMETERS*);
using ResizeBuffersFn = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT, UINT, DXGI_FORMAT, UINT);
using ResizeTargetFn = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, const DXGI_MODE_DESC*);

template <typename Function>
Function present_original(Method method) noexcept {
    return reinterpret_cast<Function>(rsf_anomaly_overlay_present_original(static_cast<uint32_t>(method)));
}

HRESULT STDMETHODCALLTYPE hook_present(IDXGISwapChain* self, UINT sync, UINT flags) {
    rsf_anomaly_overlay_before_present(self, flags);
    return present_original<PresentFn>(Method::present)(self, sync, flags);
}
HRESULT STDMETHODCALLTYPE hook_present1(IDXGISwapChain1* self, UINT sync, UINT flags, const DXGI_PRESENT_PARAMETERS* parameters) {
    rsf_anomaly_overlay_before_present(self, flags);
    return present_original<Present1Fn>(Method::present1)(self, sync, flags, parameters);
}
HRESULT STDMETHODCALLTYPE hook_resize_buffers(IDXGISwapChain* self, UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags) {
    rsf_anomaly_overlay_before_resize(self);
    return present_original<ResizeBuffersFn>(Method::resize_buffers)(self, count, width, height, format, flags);
}
HRESULT STDMETHODCALLTYPE hook_resize_target(IDXGISwapChain* self, const DXGI_MODE_DESC* mode) {
    rsf_anomaly_overlay_before_resize(self);
    return present_original<ResizeTargetFn>(Method::resize_target)(self, mode);
}

struct DummyChain {
    HWND window{};
    ID3D11Device* device{};
    ID3D11DeviceContext* context{};
    IDXGISwapChain* chain{};
    ~DummyChain() {
        if (chain) chain->Release();
        if (context) context->Release();
        if (device) device->Release();
        if (window) DestroyWindow(window);
    }
};

bool probe_present(void* registry, AddHook add, HookReason* reason, void** owned_module) noexcept {
    const Collector collect{registry, add};
    HMODULE d3d11 = LoadLibraryExW(L"d3d11.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    *owned_module = d3d11;
    const auto create = d3d11 ? reinterpret_cast<PFN_D3D11_CREATE_DEVICE_AND_SWAP_CHAIN>(
        reinterpret_cast<void*>(GetProcAddress(d3d11, "D3D11CreateDeviceAndSwapChain"))) : nullptr;
    if (!create) { *reason = "d3d11.dll unavailable"; return false; }
    DummyChain dummy;
    dummy.window = CreateWindowExW(0, L"STATIC", L"", WS_POPUP, 0, 0, 8, 8, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
    if (!dummy.window) { *reason = "dummy window creation failed"; return false; }
    DXGI_SWAP_CHAIN_DESC desc{};
    desc.BufferCount = 1; desc.BufferDesc.Width = 8; desc.BufferDesc.Height = 8;
    desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.OutputWindow = dummy.window; desc.SampleDesc.Count = 1; desc.Windowed = TRUE; desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    // WARP only touches the shared DXGI swap-chain class; the engine's adapter is not initialized here.
    if (FAILED(create(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0, D3D11_SDK_VERSION, &desc,
        &dummy.chain, &dummy.device, nullptr, &dummy.context)) || !dummy.chain) {
        *reason = "dummy swap chain creation failed"; return false;
    }
    void** table = vtable(dummy.chain);
    IDXGISwapChain1* chain1 = nullptr;
    void* present1_target = nullptr;
    if (SUCCEEDED(dummy.chain->QueryInterface(__uuidof(IDXGISwapChain1), reinterpret_cast<void**>(&chain1))) && chain1) {
        present1_target = vtable(chain1)[slot_present1];
        chain1->Release();
    }
    bool ok = collect(table[slot_present], reinterpret_cast<void*>(&hook_present), Method::present, reason) &&
        collect(table[slot_resize_buffers], reinterpret_cast<void*>(&hook_resize_buffers), Method::resize_buffers, reason) &&
        collect(table[slot_resize_target], reinterpret_cast<void*>(&hook_resize_target), Method::resize_target, reason);
    if (ok && present1_target) ok = collect(present1_target, reinterpret_cast<void*>(&hook_present1), Method::present1, reason);
    return ok;
}
}

void set_input_observer(void (*observer)(DeviceKind) noexcept) noexcept {
    rsf_anomaly_overlay_set_input_observer(observer);
}

bool install_input_hooks(InputGate& input, HookReason* reason) noexcept {
    HookReason unused = nullptr;
    return rsf_anomaly_overlay_install_input(reinterpret_cast<HookAddress>(&input), &hook_ops, probe_input,
        reason ? reason : &unused);
}

bool install_present_hooks(const PresentSink& sink, HookReason* reason) noexcept {
    HookReason unused = nullptr;
    return rsf_anomaly_overlay_install_present(sink.before_present, sink.resize,
        reinterpret_cast<HookAddress>(sink.user), &hook_ops, probe_present, reason ? reason : &unused);
}
}
