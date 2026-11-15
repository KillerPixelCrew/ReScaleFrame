// SPDX-License-Identifier: GPL-3.0-only
#include "native_creation.h"
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
using Wrap = decltype(&rsf_d3d11_host_wrap_native_swapchain);
Create original{};
Wrap wrap{};
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
void report(const char* phase, HRESULT native_result, int32_t result, void* before, void* after) noexcept {
    if (!services.log) return;
    char message[360]{};
    std::snprintf(message, sizeof(message),
        "Skyrim native creation %s native_HRESULT=0x%08lx facade_result=%d before=%p after=%p; outer wrappers completed before facade admission",
        phase, static_cast<unsigned long>(native_result), result, before, after);
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
    // This game-owned thunk still jumps through the current IAT. Do not bypass
    // D3D11/RTSS/Steam wrappers by resolving an unhooked implementation instead.
    const auto result = original(adapter, driver, software, flags, levels, level_count, sdk,
        description, chain, device, selected, context);
    struct LastError { DWORD value; ~LastError() { SetLastError(value); } } native_error{GetLastError()};
    if (!live.load(std::memory_order_acquire) || caller != base() + 0x1012325 || FAILED(result)) return result;
    uintptr_t owner{}; HWND window{};
    if (!read(owner, base() + 0x3330188) || !owner ||
        uintptr_t(chain) != owner + 96 || uintptr_t(device) != owner + 56 || uintptr_t(context) != owner + 64 ||
        !description || !read(window, owner + 72) || description->OutputWindow != window ||
        !chain || !*chain || !device || !*device || !context || !*context) {
        report("refused-native-owner", result, RSF_ERROR_NATIVE_REFUSED, nullptr, nullptr);
        return result;
    }
    auto* before = *chain;
    // ABI4 consumes only the caller's native-chain reference after a successful
    // replacement. Device/context and every original API argument stay native.
    const rsf_d3d11_native_chain request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION,
        *device, *context, reinterpret_cast<void**>(chain)};
    int32_t wrapped = RSF_ERROR_NOT_READY;
    try { wrapped = wrap(services.user, &request); } catch (...) {}
    report("outer-return", result, wrapped, before, *chain);
    return result; // A refused facade must never turn a native success into failure.
}
}
bool native_creation_prepare(const rsf_game_host_services& host) {
    if (calls.load(std::memory_order_acquire) || live.load(std::memory_order_acquire) || !guard()) return false;
    uintptr_t owner{}; ID3D11Device* existing{};
    if (!read(owner, base() + 0x3330188) || (owner && (!read(existing, owner + 56) || existing))) return false;
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    const auto service = runtime ? reinterpret_cast<Wrap>(GetProcAddress(runtime, "rsf_d3d11_host_wrap_native_swapchain")) : nullptr;
    if (!service) return false;
    if (!original && MH_CreateHook(target(), reinterpret_cast<void*>(&create), reinterpret_cast<void**>(&original)) != MH_OK) return false;
    services = host; wrap = service;
    if (services.log) services.log(services.user,
        "Skyrim native creation root prepared: game thunk15B9412 -> current IAT17C92E0; expected caller1012320, return1012325; no graphics created yet.");
    return true;
}
bool native_creation_start() {
    if (!original || !wrap || !guard()) return false;
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
