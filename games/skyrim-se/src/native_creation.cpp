// SPDX-License-Identifier: GPL-3.0-only
#include "native_creation.h"
#include "compatibility.h"
#include "steam_present.h"
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>
#include <windows.h>
#include <d3d11.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::skyrim {
namespace {
using Create = decltype(&D3D11CreateDeviceAndSwapChain);
using Begin = decltype(&rsf_d3d11_host_creation_begin);
using End = decltype(&rsf_d3d11_host_creation_end);
Create original{};
Begin begin{};
End end{};
rsf_game_host_services services{};
std::atomic<bool> live{};
std::atomic<uint32_t> calls{};
uintptr_t base() { return uintptr_t(GetModuleHandleW(nullptr)); }
void* target() { return reinterpret_cast<void*>(base() + 0x15b9412); }
bool read(void* destination, uintptr_t address, size_t size) noexcept {
#if defined(_MSC_VER)
    __try { std::memcpy(destination, reinterpret_cast<const void*>(address), size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION memory{};
    if (!VirtualQuery(reinterpret_cast<void*>(address), &memory, sizeof(memory)) || memory.State != MEM_COMMIT ||
        (memory.Protect & (PAGE_NOACCESS | PAGE_GUARD)) || address + size < address ||
        address + size > uintptr_t(memory.BaseAddress) + memory.RegionSize) return false;
    std::memcpy(destination, reinterpret_cast<const void*>(address), size); return true;
#endif
}
template<class T> bool read(T& value, uintptr_t address) { return read(&value, address, sizeof(value)); }
bool guard() {
    constexpr unsigned char thunk[]{0xff,0x25,0xc8,0xfe,0x20,0x00};
    constexpr unsigned char call[]{0xe8,0xed,0x70,0x5a,0x00};
    unsigned char thunk_bytes[sizeof(thunk)]{}, call_bytes[sizeof(call)]{};
    return read(thunk_bytes, base() + 0x15b9412, sizeof(thunk_bytes)) &&
        read(call_bytes, base() + 0x1012320, sizeof(call_bytes)) &&
        !std::memcmp(thunk_bytes, thunk, sizeof(thunk)) && !std::memcmp(call_bytes, call, sizeof(call));
}
void report(HRESULT native_result, int32_t admission, void* outer) noexcept {
    if (!services.log) return;
    char message[320]{};
    std::snprintf(message, sizeof(message),
        "Skyrim native creation scoped-end native_HRESULT=0x%08lx admission=%d outer=%p; outer outputs and wrapper Present preserved",
        static_cast<unsigned long>(native_result), admission, outer);
    try { services.log(services.user, message); } catch (...) {}
}
HRESULT WINAPI create(IDXGIAdapter* adapter, D3D_DRIVER_TYPE driver, HMODULE software, UINT flags,
    const D3D_FEATURE_LEVEL* levels, UINT level_count, UINT sdk, const DXGI_SWAP_CHAIN_DESC* description,
    IDXGISwapChain** chain, ID3D11Device** device, D3D_FEATURE_LEVEL* selected, ID3D11DeviceContext** context) {
    struct Lease { Lease() { calls.fetch_add(1); } ~Lease() { calls.fetch_sub(1); } } lease;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    struct Scope {
        rsf_d3d11_creation_scope scope{sizeof(scope), RSF_D3D11_HOST_ABI_VERSION};
        rsf_d3d11_native_chain outputs{sizeof(outputs), RSF_D3D11_HOST_ABI_VERSION};
        bool begun{}, matched{};
        HRESULT native_result{E_FAIL};
        void* outer{};
        ~Scope() {
            if (!begun) return;
            const auto error = GetLastError();
            int32_t admitted = RSF_ERROR_NOT_READY;
            try { admitted = end(services.user, &scope, matched ? &outputs : nullptr); } catch (...) {}
            report(native_result, admitted, outer);
            SetLastError(error);
        }
    } transaction;
    const auto entry_error = GetLastError();
    uintptr_t owner{}; HWND window{}; DXGI_SWAP_CHAIN_DESC native_description{};
    // Only this verified game owner may admit inner creation. The original call
    // still uses the current IAT and all original arguments, including outputs.
    const bool owned = live.load(std::memory_order_acquire) && caller == base() + 0x1012325 &&
        read(owner, base() + 0x3330188) && owner &&
        uintptr_t(chain) == owner + 96 && uintptr_t(device) == owner + 56 && uintptr_t(context) == owner + 64 &&
        description && read(native_description, uintptr_t(description)) && read(window, owner + 72) &&
        window && native_description.OutputWindow == window && compatibility_render_allowed() && steam_present_prepare(services);
    if (owned) {
        transaction.scope.window = window;
        try { transaction.begun = begin(services.user, &transaction.scope) == RSF_OK; } catch (...) {}
    }
    SetLastError(entry_error);
    const auto result = original(adapter, driver, software, flags, levels, level_count, sdk,
        description, chain, device, selected, context);
    const auto native_error = GetLastError();
    transaction.native_result = result;
    if (transaction.begun && SUCCEEDED(result) && live.load(std::memory_order_acquire) && compatibility_render_allowed()) {
        uintptr_t current_owner{}; HWND current_window{};
        ID3D11Device* native_device{}; ID3D11DeviceContext* native_context{}; IDXGISwapChain* outer{};
        if (read(current_owner, base() + 0x3330188) && current_owner == owner &&
            read(current_window, owner + 72) && current_window == window &&
            read(native_device, uintptr_t(device)) && native_device &&
            read(native_context, uintptr_t(context)) && native_context &&
            read(outer, uintptr_t(chain)) && outer) {
            transaction.outputs.device = native_device;
            transaction.outputs.immediate_context = native_context;
            transaction.outputs.swapchain = reinterpret_cast<void**>(chain);
            transaction.outer = outer;
            transaction.matched = true;
        }
    }
    SetLastError(native_error);
    return result; // End borrows outputs; the outer native HRESULT is unchanged.
}
}
bool native_creation_prepare(const rsf_game_host_services& host) {
    if (calls.load(std::memory_order_acquire) || live.load(std::memory_order_acquire) || !guard()) return false;
    uintptr_t owner{}; ID3D11Device* existing{};
    if (!read(owner, base() + 0x3330188) || (owner && (!read(existing, owner + 56) || existing))) return false;
    if (!steam_present_prepare(host)) return false;
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    const auto begin_service = runtime ? reinterpret_cast<Begin>(GetProcAddress(runtime, "rsf_d3d11_host_creation_begin")) : nullptr;
    const auto end_service = runtime ? reinterpret_cast<End>(GetProcAddress(runtime, "rsf_d3d11_host_creation_end")) : nullptr;
    if (!begin_service || !end_service) return false;
    if (!original && MH_CreateHook(target(), reinterpret_cast<void*>(&create), reinterpret_cast<void**>(&original)) != MH_OK) return false;
    services = host; begin = begin_service; end = end_service;
    if (services.log) services.log(services.user,
        "Skyrim native creation root prepared: game thunk15B9412 -> current IAT17C92E0; expected caller1012320, return1012325; scoped inner creation, outer chain retained; no graphics created yet.");
    return true;
}
bool native_creation_start() {
    if (!original || !begin || !end || !guard()) return false;
    const auto result = MH_EnableHook(target());
    if (result != MH_OK && result != MH_ERROR_ENABLED) return false;
    live.store(true, std::memory_order_release); return true;
}
void native_creation_quiesce() { live.store(false, std::memory_order_release); }
bool native_creation_busy() { return calls.load(std::memory_order_acquire) != 0; }
bool native_creation_stop() {
    native_creation_quiesce();
    if (!original) return true;
    const auto result = MH_DisableHook(target());
    return (result == MH_OK || result == MH_ERROR_DISABLED) && !native_creation_busy();
}
}
