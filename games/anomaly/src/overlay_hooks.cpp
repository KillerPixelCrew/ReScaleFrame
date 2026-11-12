// SPDX-License-Identifier: GPL-3.0-only
#include "overlay_hooks.h"
#include <MinHook.h>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>

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

enum class Method : int { none, create_device, acquire, unacquire, get_state, get_data, cooperative, present,
    present1, resize_buffers, resize_target };
struct Hook { void* target{}; void* original{}; Method method{}; };
std::array<Hook, 32> hooks{};
size_t hook_count{};
std::atomic<bool> input_installed{}, present_installed{};

void* original_of(const void* target) noexcept {
    for (size_t i = 0; i < hook_count; ++i) if (hooks[i].target == target) return hooks[i].original;
    return nullptr;
}
// True when the target is hooked for this method (already, or now). A target already owned by another
// method means the linker folded two functions; refuse rather than guess which one was called.
bool add_hook(void* target, void* detour, Method method, const char** reason) noexcept {
    if (!target || !detour) { *reason = "a hook target resolved to null"; return false; }
    for (size_t i = 0; i < hook_count; ++i)
        if (hooks[i].target == target) {
            if (hooks[i].method == method) return true;
            *reason = "two different methods share one code address"; return false;
        }
    if (hook_count == hooks.size()) { *reason = "hook table is full"; return false; }
    void* original = nullptr;
    if (MH_CreateHook(target, detour, &original) != MH_OK) { *reason = "MinHook refused to create a hook"; return false; }
    hooks[hook_count++] = {target, original, method};
    return true;
}
char failure_text[96];
bool enable_all(const char** reason) noexcept {
    for (size_t i = 0; i < hook_count; ++i) {
        const auto status = MH_EnableHook(hooks[i].target);
        if (status != MH_OK && status != MH_ERROR_ENABLED) {
            // Installation is once per process, so one buffer is enough for the player-readable log line.
            std::snprintf(failure_text, sizeof(failure_text), "MinHook refused to enable hook %zu (method %d, %s)",
                i, static_cast<int>(hooks[i].method), MH_StatusToString(status));
            *reason = failure_text;
            return false;
        }
    }
    return true;
}
bool initialize_minhook(const char** reason) noexcept {
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) { *reason = "MinHook initialization failed"; return false; }
    return true;
}
void** vtable(void* object) noexcept { return object ? *reinterpret_cast<void***>(object) : nullptr; }
void release(void* object) noexcept {
    if (object) reinterpret_cast<ULONG (STDMETHODCALLTYPE*)(void*)>(vtable(object)[slot_release])(object);
}

// ---- DirectInput ---------------------------------------------------------------------------------
InputGate* gate{};

// The trampoline of the method the engine called, found from the object's own vtable so Unicode and
// ANSI device classes each reach their own original. An unhooked slot is called directly.
void* real_method(void* self, size_t slot) noexcept {
    void* target = vtable(self)[slot];
    void* original = original_of(target);
    return original ? original : target;
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
    if (SUCCEEDED(result) && gate && device && *device && kind_of(guid, kind)) {
        try { gate->add(*device, kind); } catch (...) {}
    }
    return result;
}
HRESULT STDMETHODCALLTYPE hook_acquire(void* self) {
    const auto calls = calls_for(self);
    try { if (gate) return gate->acquire(self, calls); } catch (...) {}
    return calls.acquire(self);
}
HRESULT STDMETHODCALLTYPE hook_unacquire(void* self) {
    const auto calls = calls_for(self);
    try { if (gate) return gate->unacquire(self, calls); } catch (...) {}
    return calls.unacquire(self);
}
HRESULT STDMETHODCALLTYPE hook_get_device_state(void* self, DWORD size, void* data) {
    const auto calls = calls_for(self);
    try { if (gate) return gate->get_state(self, calls, size, data); } catch (...) {}
    return calls.get_state(self, size, data);
}
HRESULT STDMETHODCALLTYPE hook_get_device_data(void* self, DWORD object_size, DIDEVICEOBJECTDATA* data, DWORD* count, DWORD flags) {
    const auto calls = calls_for(self);
    try { if (gate) return gate->get_data(self, calls, object_size, data, count, flags); } catch (...) {}
    return calls.get_data(self, object_size, data, count, flags);
}
HRESULT STDMETHODCALLTYPE hook_set_cooperative_level(void* self, HWND window, DWORD flags) {
    const auto original = reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*, HWND, DWORD)>(
        real_method(self, slot_set_cooperative_level));
    const HRESULT result = original(self, window, flags);
    if (SUCCEEDED(result) && gate) { try { gate->cooperative_level(self, flags); } catch (...) {} }
    return result;
}

bool dummy_input_objects(const char** reason) noexcept {
    wchar_t path[MAX_PATH]{};
    const UINT length = GetSystemDirectoryW(path, MAX_PATH);
    constexpr wchar_t suffix[] = L"\\dinput8.dll";
    if (!length || length + _countof(suffix) > MAX_PATH) { *reason = "System32 directory unavailable"; return false; }
    std::memcpy(path + length, suffix, sizeof(suffix));
    // The same absolute path the carrier forwards to, so these are the engine's DirectInput classes.
    HMODULE module = LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) { *reason = "system dinput8.dll could not be loaded"; return false; }
    using Create = HRESULT (WINAPI*)(HINSTANCE, DWORD, REFIID, LPVOID*, LPUNKNOWN);
    const auto create = reinterpret_cast<Create>(reinterpret_cast<void*>(GetProcAddress(module, "DirectInput8Create")));
    if (!create) { *reason = "system DirectInput8Create missing"; return false; }
    size_t devices = 0;
    for (const GUID* iid : {&iid_direct_input8_a, &iid_direct_input8_w}) {
        void* input = nullptr;
        if (FAILED(create(GetModuleHandleW(nullptr), 0x0800, *iid, &input, nullptr)) || !input) continue;
        const bool created = add_hook(vtable(input)[slot_create_device], reinterpret_cast<void*>(&hook_create_device),
            Method::create_device, reason);
        for (const GUID* guid : {&guid_sys_mouse, &guid_sys_keyboard}) {
            if (!created) break;
            void* device = nullptr;
            const auto create_device = reinterpret_cast<HRESULT (STDMETHODCALLTYPE*)(void*, REFGUID, void**, void*)>(
                vtable(input)[slot_create_device]);
            if (FAILED(create_device(input, *guid, &device, nullptr)) || !device) continue;
            void** table = vtable(device);
            const bool ok =
                add_hook(table[slot_acquire], reinterpret_cast<void*>(&hook_acquire), Method::acquire, reason) &&
                add_hook(table[slot_unacquire], reinterpret_cast<void*>(&hook_unacquire), Method::unacquire, reason) &&
                add_hook(table[slot_get_device_state], reinterpret_cast<void*>(&hook_get_device_state), Method::get_state, reason) &&
                add_hook(table[slot_get_device_data], reinterpret_cast<void*>(&hook_get_device_data), Method::get_data, reason) &&
                add_hook(table[slot_set_cooperative_level], reinterpret_cast<void*>(&hook_set_cooperative_level), Method::cooperative, reason);
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
PresentSink sink{};
using PresentFn = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
using Present1Fn = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain1*, UINT, UINT, const DXGI_PRESENT_PARAMETERS*);
using ResizeBuffersFn = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT, UINT, DXGI_FORMAT, UINT);
using ResizeTargetFn = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, const DXGI_MODE_DESC*);
PresentFn present_original{};
Present1Fn present1_original{};
ResizeBuffersFn resize_buffers_original{};
ResizeTargetFn resize_target_original{};
thread_local bool in_present = false;

void before_present(IDXGISwapChain* chain, UINT flags) noexcept {
    if ((flags & DXGI_PRESENT_TEST) || in_present || !sink.before_present) return;
    in_present = true;
    sink.before_present(sink.user, chain);
    in_present = false;
}
HRESULT STDMETHODCALLTYPE hook_present(IDXGISwapChain* self, UINT sync, UINT flags) {
    before_present(self, flags);
    return present_original(self, sync, flags);
}
HRESULT STDMETHODCALLTYPE hook_present1(IDXGISwapChain1* self, UINT sync, UINT flags, const DXGI_PRESENT_PARAMETERS* parameters) {
    before_present(self, flags);
    return present1_original(self, sync, flags, parameters);
}
HRESULT STDMETHODCALLTYPE hook_resize_buffers(IDXGISwapChain* self, UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags) {
    if (sink.resize) sink.resize(sink.user, self);
    return resize_buffers_original(self, count, width, height, format, flags);
}
HRESULT STDMETHODCALLTYPE hook_resize_target(IDXGISwapChain* self, const DXGI_MODE_DESC* mode) {
    if (sink.resize) sink.resize(sink.user, self);
    return resize_target_original(self, mode);
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
}

bool install_input_hooks(InputGate& input, const char** reason) noexcept {
    const char* unused = nullptr;
    if (!reason) reason = &unused;
    if (input_installed.exchange(true)) { *reason = "input hooks were already installed"; return false; }
    if (!initialize_minhook(reason)) return false;
    // Hooks are created for all probed device classes first and enabled together, so a refusal
    // leaves no half-routed device class.
    const size_t first = hook_count;
    gate = &input;
    if (!dummy_input_objects(reason) || !enable_all(reason)) {
        for (size_t i = first; i < hook_count; ++i) (void)MH_DisableHook(hooks[i].target);
        gate = nullptr;
        return false;
    }
    return true;
}

bool install_present_hooks(const PresentSink& target_sink, const char** reason) noexcept {
    const char* unused = nullptr;
    if (!reason) reason = &unused;
    if (!target_sink.before_present) { *reason = "no Present sink"; return false; }
    if (present_installed.exchange(true)) { *reason = "Present hooks were already installed"; return false; }
    if (!initialize_minhook(reason)) return false;
    HMODULE d3d11 = LoadLibraryExW(L"d3d11.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
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
    sink = target_sink;
    const size_t first = hook_count;
    bool ok = add_hook(table[slot_present], reinterpret_cast<void*>(&hook_present), Method::present, reason) &&
        add_hook(table[slot_resize_buffers], reinterpret_cast<void*>(&hook_resize_buffers), Method::resize_buffers, reason) &&
        add_hook(table[slot_resize_target], reinterpret_cast<void*>(&hook_resize_target), Method::resize_target, reason);
    if (ok && present1_target) ok = add_hook(present1_target, reinterpret_cast<void*>(&hook_present1), Method::present1, reason);
    if (ok) {
        present_original = reinterpret_cast<PresentFn>(original_of(table[slot_present]));
        resize_buffers_original = reinterpret_cast<ResizeBuffersFn>(original_of(table[slot_resize_buffers]));
        resize_target_original = reinterpret_cast<ResizeTargetFn>(original_of(table[slot_resize_target]));
        present1_original = reinterpret_cast<Present1Fn>(original_of(present1_target));
        ok = enable_all(reason);
    }
    if (!ok) {
        for (size_t i = first; i < hook_count; ++i) (void)MH_DisableHook(hooks[i].target);
        sink = {};
        return false;
    }
    return true;
}
}
