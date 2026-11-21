// SPDX-License-Identifier: MIT
#include "overlay_hooks.h"
#include "native_hook_ops.h"
#include <d3d11.h>
#include <dxgi1_2.h>

using HookAddress = uintptr_t;
using HookReason = const char*;
using InputObserver = void (*)(rsf::anomaly::overlay::DeviceKind) noexcept;
using PresentCallback = void (*)(void*, IDXGISwapChain*) noexcept;

struct ComProbeOps {
    HookAddress (*slot)(void*, size_t) noexcept;
    void (*release)(void*) noexcept;
};

struct InputProbeOps {
    ComProbeOps com;
    void* (*create_input)(FARPROC, const GUID*, uint32_t) noexcept;
    void* (*create_device)(void*, const GUID*) noexcept;
    HookAddress create_device_detour, acquire, unacquire, get_state, get_data, cooperative;
};

struct ChainObjects {
    IDXGISwapChain* chain;
    ID3D11DeviceContext* context;
    ID3D11Device* device;
};

struct PresentProbeOps {
    ComProbeOps com;
    HRESULT (*create_warp_chain)(FARPROC, HWND, uint32_t, ChainObjects*) noexcept;
    void* (*query_chain1)(IDXGISwapChain*) noexcept;
    HookAddress present, present1, resize_buffers, resize_target;
};

using rsf::anomaly::HookOps;

extern "C" {
bool rsf_anomaly_overlay_install_input(HookAddress, const HookOps*, const InputProbeOps*, HookReason*) noexcept;
bool rsf_anomaly_overlay_install_present(PresentCallback, PresentCallback, HookAddress,
    const HookOps*, const PresentProbeOps*, HookReason*) noexcept;
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
static_assert(sizeof(GUID) == 16 && offsetof(GUID, Data4) == 8);
static_assert(sizeof(ComProbeOps) == 16 && sizeof(ChainObjects) == 24);
static_assert(sizeof(InputProbeOps) == 80 && offsetof(InputProbeOps, create_device_detour) == 32);
static_assert(sizeof(PresentProbeOps) == 64 && offsetof(PresentProbeOps, present) == 32);

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

// GUIDs accepted by the live CreateDevice detour. The probe only requests the system pair.
constexpr GUID guid_sys_mouse{0x6F1D2B60, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_keyboard{0x6F1D2B61, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_mouse_em{0x6F1D2B80, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_mouse_em2{0x6F1D2B81, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_keyboard_em{0x6F1D2B82, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_sys_keyboard_em2{0x6F1D2B83, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};

// IDs in overlay_hooks.rs used to retrieve the DXGI trampolines.
enum class Method : uint32_t { present = 6, present1, resize_buffers, resize_target };

InputGate* active_gate() noexcept {
    return reinterpret_cast<InputGate*>(rsf_anomaly_overlay_input_gate());
}

void** vtable(void* object) noexcept { return object ? *reinterpret_cast<void***>(object) : nullptr; }
HookAddress probe_slot(void* object, size_t slot) noexcept {
    return reinterpret_cast<HookAddress>(vtable(object)[slot]);
}
void release(void* object) noexcept {
    if (object) reinterpret_cast<ULONG (STDMETHODCALLTYPE*)(void*)>(vtable(object)[slot_release])(object);
}

constexpr ComProbeOps com_probe_ops{probe_slot, release};

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

void* create_input(FARPROC address, const GUID* iid, uint32_t version) noexcept {
    using Create = HRESULT (WINAPI*)(HINSTANCE, DWORD, REFIID, LPVOID*, LPUNKNOWN);
    const auto create = reinterpret_cast<Create>(reinterpret_cast<void*>(address));
    void* input = nullptr;
    return SUCCEEDED(create(GetModuleHandleW(nullptr), version, *iid, &input, nullptr)) ? input : nullptr;
}

void* create_probe_device(void* input, const GUID* guid) noexcept {
    using Create = HRESULT (STDMETHODCALLTYPE*)(void*, REFGUID, void**, void*);
    const auto create = reinterpret_cast<Create>(vtable(input)[slot_create_device]);
    void* device = nullptr;
    return SUCCEEDED(create(input, *guid, &device, nullptr)) ? device : nullptr;
}

const InputProbeOps input_probe_ops{
    .com = com_probe_ops,
    .create_input = create_input,
    .create_device = create_probe_device,
    .create_device_detour = reinterpret_cast<HookAddress>(&hook_create_device),
    .acquire = reinterpret_cast<HookAddress>(&hook_acquire),
    .unacquire = reinterpret_cast<HookAddress>(&hook_unacquire),
    .get_state = reinterpret_cast<HookAddress>(&hook_get_device_state),
    .get_data = reinterpret_cast<HookAddress>(&hook_get_device_data),
    .cooperative = reinterpret_cast<HookAddress>(&hook_set_cooperative_level),
};

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

HRESULT create_warp_chain(FARPROC address, HWND window, uint32_t extent, ChainObjects* objects) noexcept {
    const auto create = reinterpret_cast<PFN_D3D11_CREATE_DEVICE_AND_SWAP_CHAIN>(reinterpret_cast<void*>(address));
    DXGI_SWAP_CHAIN_DESC desc{};
    desc.BufferCount = 1;
    desc.BufferDesc.Width = extent;
    desc.BufferDesc.Height = extent;
    desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.OutputWindow = window;
    desc.SampleDesc.Count = 1;
    desc.Windowed = TRUE;
    desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    // WARP only touches the shared DXGI swap-chain class; the engine's adapter is not initialized here.
    return create(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0, D3D11_SDK_VERSION, &desc,
        &objects->chain, &objects->device, nullptr, &objects->context);
}

void* query_chain1(IDXGISwapChain* chain) noexcept {
    IDXGISwapChain1* chain1 = nullptr;
    return SUCCEEDED(chain->QueryInterface(__uuidof(IDXGISwapChain1), reinterpret_cast<void**>(&chain1))) ? chain1 : nullptr;
}

const PresentProbeOps present_probe_ops{
    .com = com_probe_ops,
    .create_warp_chain = create_warp_chain,
    .query_chain1 = query_chain1,
    .present = reinterpret_cast<HookAddress>(&hook_present),
    .present1 = reinterpret_cast<HookAddress>(&hook_present1),
    .resize_buffers = reinterpret_cast<HookAddress>(&hook_resize_buffers),
    .resize_target = reinterpret_cast<HookAddress>(&hook_resize_target),
};
}

void set_input_observer(void (*observer)(DeviceKind) noexcept) noexcept {
    rsf_anomaly_overlay_set_input_observer(observer);
}

bool install_input_hooks(InputGate& input, HookReason* reason) noexcept {
    HookReason unused = nullptr;
    return rsf_anomaly_overlay_install_input(reinterpret_cast<HookAddress>(&input), &hook_ops, &input_probe_ops,
        reason ? reason : &unused);
}

bool install_present_hooks(const PresentSink& sink, HookReason* reason) noexcept {
    HookReason unused = nullptr;
    return rsf_anomaly_overlay_install_present(sink.before_present, sink.resize,
        reinterpret_cast<HookAddress>(sink.user), &hook_ops, &present_probe_ops, reason ? reason : &unused);
}
}
