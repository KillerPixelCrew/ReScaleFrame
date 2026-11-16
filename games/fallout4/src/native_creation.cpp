// SPDX-License-Identifier: GPL-3.0-only
#include "native_creation.h"
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/steam_d3d11_chain.h>
#include <MinHook.h>
#include <d3d11.h>
#include <windows.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <iterator>
#include <string>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::fallout4 {
namespace {
// AE1.11.240 disassembly: creation1824930+410 -> thunk22C4CD9;
// renderer owner38E13A0, native device48/context50/window58/chain70.
constexpr uintptr_t owner_rva = 0x38e13a0, call_rva = 0x1824d40, thunk_rva = 0x22c4cd9;
using NativeCreate = decltype(&D3D11CreateDeviceAndSwapChain);
using Begin = decltype(&rsf_d3d11_host_creation_begin);
using End = decltype(&rsf_d3d11_host_creation_end);
using Bind = decltype(&rsf_d3d11_host_bind_presentation);
NativeCreate native_create{};
Begin begin_creation{};
End end_creation{};
Bind bind_presentation{};
rsf_game_host_services host{};
std::atomic<bool> enabled{};
std::atomic<uint32_t> entered{};
uintptr_t image() { return uintptr_t(GetModuleHandleW(nullptr)); }
void* hook_address() { return reinterpret_cast<void*>(image() + thunk_rva); }
bool copy(uintptr_t address, void* out, size_t size) {
    SIZE_T copied{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address), out, size, &copied) && copied == size;
}
template<class T> bool read(uintptr_t address, T& value) { return copy(address, &value, sizeof(value)); }
bool cold_owner() {
    uintptr_t renderer{}, device{};
    return read(image() + owner_rva, renderer) &&
        (!renderer || (read(renderer + 0x48, device) && !device));
}
bool expected_creation() {
    constexpr unsigned char call[]{0xe8,0x94,0xff,0xa9,0x00};
    constexpr unsigned char thunk[]{0xff,0x25,0x79,0xca,0x17,0x00};
    unsigned char actual_call[sizeof(call)]{}, actual_thunk[sizeof(thunk)]{};
    return copy(image() + call_rva, actual_call, sizeof(actual_call)) &&
        !std::memcmp(call, actual_call, sizeof(call)) &&
        copy(image() + thunk_rva, actual_thunk, sizeof(actual_thunk)) &&
        !std::memcmp(thunk, actual_thunk, sizeof(thunk));
}
void log(const char* message) noexcept {
    if (host.log) try { host.log(host.user, message); } catch (...) {}
}
struct Entry {
    Entry() { entered.fetch_add(1, std::memory_order_acq_rel); }
    ~Entry() { entered.fetch_sub(1, std::memory_order_acq_rel); }
};
struct Creation {
    rsf_d3d11_creation_scope scope{sizeof(scope), RSF_D3D11_HOST_ABI_VERSION};
    rsf_d3d11_native_chain outputs{sizeof(outputs), RSF_D3D11_HOST_ABI_VERSION};
    bool open{}, matched{};
    HRESULT native_result{E_FAIL};
    ~Creation() {
        if (!open) return;
        const auto last_error = GetLastError();
        int32_t admitted = RSF_ERROR_NOT_READY, bound = RSF_ERROR_NOT_READY;
        try {
            admitted = end_creation(host.user, &scope, matched ? &outputs : nullptr);
            if (matched && admitted == RSF_OK) {
                const rsf_d3d11_presentation_binding binding{sizeof(binding), RSF_D3D11_HOST_ABI_VERSION, *outputs.swapchain};
                bound = bind_presentation(host.user, &binding);
            }
        } catch (...) {}
        char message[224]{};
        std::snprintf(message, sizeof(message),
            "Fallout4 cold creation native_HRESULT=0x%08lx scope_end=%d binding=%d; outer outputs retained, no scene/input producers",
            static_cast<unsigned long>(native_result), admitted, bound);
        log(message);
        SetLastError(last_error);
    }
};
HRESULT WINAPI observe_create(IDXGIAdapter* adapter, D3D_DRIVER_TYPE type, HMODULE software, UINT flags,
    const D3D_FEATURE_LEVEL* levels, UINT level_count, UINT sdk, const DXGI_SWAP_CHAIN_DESC* desc,
    IDXGISwapChain** chain, ID3D11Device** device, D3D_FEATURE_LEVEL* selected, ID3D11DeviceContext** context) {
    Entry entry;
    const auto entry_error = GetLastError();
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    Creation creation;
    uintptr_t renderer{};
    HWND window{};
    DXGI_SWAP_CHAIN_DESC description{};
    const bool owner = enabled.load(std::memory_order_acquire) && caller == image() + call_rva + 5 &&
        read(image() + owner_rva, renderer) && renderer &&
        uintptr_t(chain) == renderer + 0x70 && uintptr_t(device) == renderer + 0x48 &&
        uintptr_t(context) == renderer + 0x50 && read(renderer + 0x58, window) && window &&
        desc && read(uintptr_t(desc), description) && description.OutputWindow == window &&
        cold_owner() && rsf::steam::prepare_d3d11_chain(host, cold_owner);
    if (owner) {
        creation.scope.window = window;
        try { creation.open = begin_creation(host.user, &creation.scope) == RSF_OK; } catch (...) {}
    }
    SetLastError(entry_error);
    const auto result = native_create(adapter, type, software, flags, levels, level_count, sdk, desc, chain, device, selected, context);
    const auto native_error = GetLastError();
    creation.native_result = result;
    if (creation.open && SUCCEEDED(result) && enabled.load(std::memory_order_acquire)) {
        ID3D11Device* returned_device{};
        ID3D11DeviceContext* returned_context{};
        IDXGISwapChain* returned_chain{};
        if (read(uintptr_t(device), returned_device) && returned_device &&
            read(uintptr_t(context), returned_context) && returned_context &&
            read(uintptr_t(chain), returned_chain) && returned_chain) {
            creation.outputs.device = returned_device;
            creation.outputs.immediate_context = returned_context;
            creation.outputs.swapchain = reinterpret_cast<void**>(chain);
            creation.matched = true;
        }
    }
    SetLastError(native_error);
    return result;
}
}
bool verify_executable() {
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(nullptr, path, DWORD(std::size(path)));
    if (!length || length >= std::size(path)) return false;
    const auto* leaf = std::wcsrchr(path, L'\\');
    std::string digest;
    return !_wcsicmp(leaf ? leaf + 1 : path, L"Fallout4.exe") &&
        rsf::sha256_file(path, digest) && digest == executable_sha256;
}
int accept_window(void*, void* candidate) {
    uintptr_t renderer{};
    HWND window{};
    DWORD process{};
    return candidate && read(image() + owner_rva, renderer) && renderer &&
        read(renderer + 0x58, window) && window == candidate &&
        GetWindowThreadProcessId(window, &process) && process == GetCurrentProcessId();
}
bool preflight_creation() {
    return !enabled.load(std::memory_order_acquire) && !entered.load(std::memory_order_acquire) &&
        expected_creation() && cold_owner();
}
bool prepare_creation(const rsf_game_host_services& services) {
    if (enabled.load(std::memory_order_acquire) || entered.load(std::memory_order_acquire) ||
        !expected_creation() || !cold_owner()) return false;
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (!runtime) return false;
    const auto begin = reinterpret_cast<Begin>(reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_d3d11_host_creation_begin")));
    const auto end = reinterpret_cast<End>(reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_d3d11_host_creation_end")));
    const auto bind = reinterpret_cast<Bind>(reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_d3d11_host_bind_presentation")));
    if (!begin || !end || !bind || !rsf::steam::prepare_d3d11_chain(services, cold_owner)) return false;
    if (!native_create && MH_CreateHook(hook_address(), reinterpret_cast<void*>(&observe_create),
        reinterpret_cast<void**>(&native_create)) != MH_OK) return false;
    host = services; begin_creation = begin; end_creation = end; bind_presentation = bind;
    log("Fallout4 AE cold creation prepared at thunk22C4CD9, caller1824D40; native arguments and outer wrapper retained; scene admission remains separate.");
    return true;
}
bool start_creation() {
    if (!native_create || !expected_creation() || !cold_owner()) return false;
    const auto result = MH_EnableHook(hook_address());
    if (result != MH_OK) return false;
    enabled.store(true, std::memory_order_release);
    return true;
}
void quiesce_creation() { enabled.store(false, std::memory_order_release); }
bool stop_creation() {
    quiesce_creation();
    const auto result = native_create ? MH_DisableHook(hook_address()) : MH_ERROR_DISABLED;
    return (result == MH_OK || result == MH_ERROR_DISABLED) && !creation_busy();
}
bool creation_busy() { return entered.load(std::memory_order_acquire) != 0; }
}
