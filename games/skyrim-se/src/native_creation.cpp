// SPDX-License-Identifier: GPL-3.0-only
#include "native_creation.h"
#include "compatibility.h"
#include "steam_present.h"
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>
#include <windows.h>
#include <d3d11.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <string>
#include <iterator>
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
std::atomic<uintptr_t> display_module{};
thread_local const char* refusal = "Native creation route not checked.";
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
bool mapped(uintptr_t address, size_t size, uintptr_t allocation, bool code) {
    MEMORY_BASIC_INFORMATION memory{};
    return VirtualQuery(reinterpret_cast<void*>(address), &memory, sizeof(memory)) &&
        memory.State == MEM_COMMIT && !(memory.Protect & (PAGE_NOACCESS | PAGE_GUARD)) &&
        (memory.Protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY)) &&
        (!code || (memory.Protect & (PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY))) &&
        (!allocation || uintptr_t(memory.AllocationBase) == allocation) &&
        address + size >= address && address + size <= uintptr_t(memory.BaseAddress) + memory.RegionSize;
}
bool creation_caller(uintptr_t& caller) {
    constexpr unsigned char native_call[]{0xe8,0xed,0x70,0x5a,0x00};
    unsigned char call[sizeof(native_call)]{};
    if (!read(call, base() + 0x1012320, sizeof(call))) {
        refusal = "Native creation caller unreadable."; return false;
    }
    if (!std::memcmp(call, native_call, sizeof(call))) { caller = base() + 0x1012325; return true; }
    refusal = "Native creation caller differs from native or verified Display Tweaks route.";
    const auto module = uintptr_t(GetModuleHandleW(L"SSEDisplayTweaks.dll"));
    if (!module || call[0] != 0xe8) return false;
    if (display_module.load(std::memory_order_acquire) != module) {
        wchar_t path[32768]{};
        const auto length = GetModuleFileNameW(reinterpret_cast<HMODULE>(module), path, DWORD(std::size(path)));
        std::string hash;
        HMODULE pinned{};
        if (!length || length >= std::size(path) || !rsf::sha256_file(path, hash) ||
            hash != "df7f352a3f13736709c5f9d265ebf311d9d77d373a9a1b8cbe2aee7fdd8ba9f4" ||
            !GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                reinterpret_cast<LPCWSTR>(module), &pinned) || uintptr_t(pinned) != module) {
            refusal = "Display Tweaks creation profile fingerprint or module pin refused."; return false;
        }
        display_module.store(module, std::memory_order_release);
    }
    // The exact build patches only the game's E8 displacement. Its SKSE relay
    // jumps to Display pre/post work, which calls the unchanged native thunk.
    int32_t displacement{}; std::memcpy(&displacement, call + 1, sizeof(displacement));
    const auto relay = uintptr_t(intptr_t(base() + 0x1012325) + displacement);
    constexpr unsigned char jump[]{0xff,0x25,0,0,0,0};
    unsigned char relay_bytes[14]{};
    uintptr_t destination{}, retained{};
    constexpr unsigned char prologue[]{0x40,0x53,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0xc8,0x01,0x00,0x00};
    constexpr unsigned char invoke[]{0x4c,0x8b,0x15,0x78,0x6e,0x03,0x00,0x4c,0x8b,0xc5,0x48,0x89,0x44,0x24,0x58,0x8b,0xd6,0x8b,0x84,0x24,0x40,0x02,0x00,0x00,0x48,0x89,0x5c,0x24,0x50,0x4c,0x89,0x64,0x24,0x48,0x4c,0x89,0x7c,0x24,0x40,0x4c,0x89,0x6c,0x24,0x38,0x89,0x44,0x24,0x30,0x8b,0x84,0x24,0x38,0x02,0x00,0x00,0x89,0x44,0x24,0x28,0x48,0x89,0x7c,0x24,0x20,0x48,0x8b,0x7c,0x24,0x70,0x48,0x8b,0xcf,0x41,0xff,0xd2,0x8b,0xd8};
    unsigned char head[sizeof(prologue)]{}, body[sizeof(invoke)]{};
    if (!mapped(relay, sizeof(relay_bytes), 0, true) || !read(relay_bytes, relay, sizeof(relay_bytes)) ||
        std::memcmp(relay_bytes, jump, sizeof(jump))) return false;
    std::memcpy(&destination, relay_bytes + sizeof(jump), sizeof(destination));
    if (destination != module + 0x27250 || !mapped(destination, sizeof(prologue), module, true) ||
        !mapped(module + 0x272e9, sizeof(invoke), module, true) ||
        !mapped(module + 0x5e168, sizeof(retained), module, false) ||
        !read(head, destination, sizeof(head)) || std::memcmp(head, prologue, sizeof(head)) ||
        !read(body, module + 0x272e9, sizeof(body)) || std::memcmp(body, invoke, sizeof(body)) ||
        !read(retained, module + 0x5e168) || retained != base() + 0x15b9412) {
        refusal = "Display Tweaks creation relay, original thunk or call-return bytes refused."; return false;
    }
    caller = module + 0x27334;
    return true;
}
bool guard() {
    constexpr unsigned char thunk[]{0xff,0x25,0xc8,0xfe,0x20,0x00};
    unsigned char thunk_bytes[sizeof(thunk)]{};
    uintptr_t caller{};
    refusal = "Native creation thunk bytes refused.";
    return read(thunk_bytes, base() + 0x15b9412, sizeof(thunk_bytes)) &&
        !std::memcmp(thunk_bytes, thunk, sizeof(thunk)) && creation_caller(caller);
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
    uintptr_t expected_caller{};
    bool route{};
    try { route = creation_caller(expected_caller); } catch (...) {}
    if (live.load(std::memory_order_acquire) && (!route || caller != expected_caller) && services.log) {
        static std::atomic<unsigned> reports{};
        if (reports.fetch_add(1, std::memory_order_relaxed) < 4)
            services.log(services.user, route ? "Native creation refused: unexpected guarded-route caller." : refusal);
    }
    const bool owned = live.load(std::memory_order_acquire) && route && caller == expected_caller &&
        read(owner, base() + 0x3330188) && owner &&
        uintptr_t(chain) == owner + 96 && uintptr_t(device) == owner + 56 && uintptr_t(context) == owner + 64 &&
        description && read(native_description, uintptr_t(description)) && read(window, owner + 72) &&
        window && native_description.OutputWindow == window && compatibility_render_allowed() && steam_present_prepare(services);
    if (owned) {
        if (expected_caller != base() + 0x1012325 && services.log)
            services.log(services.user, "Skyrim native creation matched exact Display Tweaks0.5.25 pre/native/post route; native thunk and game outputs retained.");
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
    if (calls.load(std::memory_order_acquire) || live.load(std::memory_order_acquire)) return false;
    if (!guard()) { if (host.log) host.log(host.user, refusal); return false; }
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
        "Skyrim native creation root prepared: game thunk15B9412 -> current IAT17C92E0; guarded native or exact Display Tweaks call route; scoped inner creation, outer chain retained; no graphics created yet.");
    return true;
}
bool native_creation_start() {
    if (!original || !begin || !end) return false;
    if (!guard()) { if (services.log) services.log(services.user, refusal); return false; }
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
