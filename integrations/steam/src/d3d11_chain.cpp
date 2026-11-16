// SPDX-License-Identifier: MIT
#include <rescaleframe/steam_d3d11_chain.h>
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/sha256_file.h>
#include <MinHook.h>
#include <windows.h>
#include <dxgi1_4.h>
#include <array>
#include <atomic>
#include <cstring>
#include <iterator>
#include <mutex>
#include <string>

namespace rsf::steam {
namespace {
using Dispatch = decltype(&rsf_d3d11_host_dispatch_chain);
std::atomic<Dispatch> dispatch_service{};
std::atomic<void*> dispatch_user{};
std::atomic<uintptr_t> steam_base{};
std::atomic<bool> installed{};
std::mutex install_guard;
bool attempted{};
constexpr char steam_hash[] = "21c0c1ddb4259c1156356bdfe8432dff504d52c7b96d19718688cff2d5a6e52f";
struct Site { uintptr_t rva; uint32_t method; std::array<unsigned char, 7> load; std::array<unsigned char, 16> following; };
constexpr Site sites[]{
    {0x9c32f, RSF_D3D11_DISPATCH_RELEASE, {0x48,0x8b,0x05,0x7a,0x71,0x0e,0x00}, {0x48,0x8b,0xf9,0xff,0xd0,0x8b,0xd8,0x85,0xc0,0x74,0x42,0x48,0x8b,0xd7,0x48,0x8d}},
    {0x9c375, RSF_D3D11_DISPATCH_RELEASE, {0x48,0x8b,0x05,0x34,0x71,0x0e,0x00}, {0x48,0x8b,0xcf,0xff,0xd0,0x8b,0xd8,0x85,0xdb,0x75,0x2d,0x48,0x8b,0xd7,0x48,0x8d}},
    {0x9c154, RSF_D3D11_DISPATCH_PRESENT, {0x48,0x8b,0x05,0x5d,0x73,0x0e,0x00}, {0xff,0xd0,0xe9,0x9b,0x01,0x00,0x00,0x4c,0x8d,0x25,0x7f,0x73,0x0e,0x00,0x4c,0x89}},
    {0x9c23e, RSF_D3D11_DISPATCH_PRESENT, {0x48,0x8b,0x05,0x73,0x72,0x0e,0x00}, {0x44,0x8b,0xc6,0x8b,0xd5,0x49,0x8b,0xcf,0xff,0xd0,0x8b,0xf0,0x48,0x85,0xff,0x74}},
    {0x9c6cf, RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE, {0x48,0x8b,0x05,0xf2,0x6d,0x0e,0x00}, {0x4d,0x8b,0xc6,0x41,0x8b,0xd7,0x48,0x8b,0xce,0x48,0x83,0xc4,0x28,0x41,0x5f,0x41}},
    {0x9c5bc, RSF_D3D11_DISPATCH_RESIZE_BUFFERS, {0x4c,0x8b,0x15,0xfd,0x6e,0x0e,0x00}, {0x45,0x8b,0xc7,0x41,0x8b,0xd4,0x48,0x8b,0xcd,0x48,0x83,0xc4,0x30,0x41,0x5f,0x41}},
    {0x9bf72, RSF_D3D11_DISPATCH_PRESENT1, {0x48,0x8b,0x05,0x57,0x75,0x0e,0x00}, {0xff,0xd0,0xe9,0x92,0x01,0x00,0x00,0x4c,0x8d,0x2d,0x61,0x75,0x0e,0x00,0x4c,0x89}},
    {0x9c05d, RSF_D3D11_DISPATCH_PRESENT1, {0x48,0x8b,0x05,0x6c,0x74,0x0e,0x00}, {0x4d,0x8b,0xcc,0x45,0x8b,0xc6,0x8b,0xd5,0x49,0x8b,0xcf,0xff,0xd0,0x8b,0xe8,0x48}},
    {0x9c630, RSF_D3D11_DISPATCH_SET_COLOR_SPACE1, {0x48,0x8b,0x05,0xa1,0x6e,0x0e,0x00}, {0x8b,0xd3,0x48,0x8b,0xce,0x48,0x83,0xc4,0x28,0x5f,0x5e,0x5d,0x5b,0x48,0xff,0xe0}},
    {0x9c4df, RSF_D3D11_DISPATCH_RESIZE_BUFFERS1, {0x4c,0x8b,0x15,0xfa,0x6f,0x0e,0x00}, {0x45,0x8b,0xc4,0x48,0x89,0x9c,0x24,0xf8,0x00,0x00,0x00,0x8b,0xd5,0x48,0x89,0xbc}}
};
constexpr uintptr_t slots[]{0x1834b0,0x1834b8,0x1834c8,0x1834c0,0x1834d0,0x1834d8,0x1834e0};
ULONG WINAPI forward_release(IDXGISwapChain*);
HRESULT WINAPI forward_present(IDXGISwapChain*, UINT, UINT);
HRESULT WINAPI forward_fullscreen(IDXGISwapChain*, BOOL, IDXGIOutput*);
HRESULT WINAPI forward_resize(IDXGISwapChain*, UINT, UINT, UINT, DXGI_FORMAT, UINT);
HRESULT WINAPI forward_present1(IDXGISwapChain1*, UINT, UINT, const DXGI_PRESENT_PARAMETERS*);
decltype(&forward_present) original_present_entry{};
decltype(&forward_present1) original_present1_entry{};
constexpr uintptr_t present_entries[]{0x9c130, 0x9bf50};
constexpr std::array<unsigned char, 16> present_entry_bytes[]{
    {0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x6c,0x24,0x10,0x56,0x57,0x41,0x54,0x41,0x56},
    {0x40,0x53,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x83,0xec}
};
HRESULT WINAPI forward_color(IDXGISwapChain3*, DXGI_COLOR_SPACE_TYPE);
HRESULT WINAPI forward_resize1(IDXGISwapChain3*, UINT, UINT, UINT, DXGI_FORMAT, UINT, const UINT*, IUnknown* const*);
const void* handlers[]{reinterpret_cast<const void*>(&forward_release), reinterpret_cast<const void*>(&forward_present),
    reinterpret_cast<const void*>(&forward_fullscreen), reinterpret_cast<const void*>(&forward_resize),
    reinterpret_cast<const void*>(&forward_present1), reinterpret_cast<const void*>(&forward_color), reinterpret_cast<const void*>(&forward_resize1)};
bool read(uintptr_t address, void* out, size_t size) {
    SIZE_T copied{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<void*>(address), out, size, &copied) && copied == size;
}
bool fail(const rsf_game_host_services& host, const char* message) {
    if (host.log) try { host.log(host.user, message); } catch (...) {}
    return false;
}
rsf_d3d11_chain_dispatch request_for(uint32_t method, void* receiver) {
    rsf_d3d11_chain_dispatch request{};
    request.struct_size = sizeof(request); request.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    request.method = method; request.receiver = receiver;
    return request;
}
bool handled(rsf_d3d11_chain_dispatch& request) {
    const auto service = dispatch_service.load(std::memory_order_acquire);
    int32_t status = RSF_ERROR_NOT_READY;
    if (service) try { status = service(dispatch_user.load(std::memory_order_acquire), &request); } catch (...) {}
    if (status != RSF_OK) {
        // Release has no HRESULT channel. Fail closed without native forwarding.
        if (request.method == RSF_D3D11_DISPATCH_RELEASE) request.result.references = 0;
        else request.result.hresult = E_FAIL;
        return true;
    }
    return request.handled != 0;
}
// A facade must enter RSF before Steam acquires its global Present lock.
// XeSS's physical presenter runs on another thread and needs that same lock.
// Native receivers retain Steam's entire handler, including its overlay work.
HRESULT WINAPI present_entry(IDXGISwapChain* receiver, UINT interval, UINT flags) {
    auto request = request_for(RSF_D3D11_DISPATCH_PRESENT, receiver);
    request.args.present = {interval, flags};
    if (handled(request)) return request.result.hresult;
    return original_present_entry ? original_present_entry(receiver, interval, flags) : E_FAIL;
}
HRESULT WINAPI present1_entry(IDXGISwapChain1* receiver, UINT interval, UINT flags, const DXGI_PRESENT_PARAMETERS* parameters) {
    auto request = request_for(RSF_D3D11_DISPATCH_PRESENT1, receiver);
    request.args.present1 = {interval, flags, parameters};
    if (handled(request)) return request.result.hresult;
    return original_present1_entry ? original_present1_entry(receiver, interval, flags, parameters) : E_FAIL;
}
template<class Function> Function continuation(uint32_t method) {
    Function next{};
    if (!read(steam_base.load(std::memory_order_acquire) + slots[method - 1], &next, sizeof(next)) ||
        !next || reinterpret_cast<const void*>(next) == handlers[method - 1]) return nullptr;
    return next;
}
ULONG WINAPI forward_release(IDXGISwapChain* receiver) {
    auto request = request_for(RSF_D3D11_DISPATCH_RELEASE, receiver);
    if (handled(request)) return request.result.references;
    const auto next = continuation<decltype(&forward_release)>(request.method);
    return next ? next(receiver) : 0; // Never AddRef or touch the object after Release.
}
HRESULT WINAPI forward_present(IDXGISwapChain* receiver, UINT interval, UINT flags) {
    auto request = request_for(RSF_D3D11_DISPATCH_PRESENT, receiver);
    request.args.present = {interval, flags};
    if (handled(request)) return request.result.hresult;
    const auto next = continuation<decltype(&forward_present)>(request.method);
    return next ? next(receiver, interval, flags) : E_FAIL;
}
HRESULT WINAPI forward_fullscreen(IDXGISwapChain* receiver, BOOL fullscreen, IDXGIOutput* target) {
    auto request = request_for(RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE, receiver);
    request.args.fullscreen = {uint32_t(fullscreen), target};
    if (handled(request)) return request.result.hresult;
    const auto next = continuation<decltype(&forward_fullscreen)>(request.method);
    return next ? next(receiver, fullscreen, target) : E_FAIL;
}
HRESULT WINAPI forward_resize(IDXGISwapChain* receiver, UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags) {
    auto request = request_for(RSF_D3D11_DISPATCH_RESIZE_BUFFERS, receiver);
    request.args.resize_buffers = {count, width, height, uint32_t(format), flags};
    if (handled(request)) return request.result.hresult;
    const auto next = continuation<decltype(&forward_resize)>(request.method);
    return next ? next(receiver, count, width, height, format, flags) : E_FAIL;
}
HRESULT WINAPI forward_present1(IDXGISwapChain1* receiver, UINT interval, UINT flags, const DXGI_PRESENT_PARAMETERS* parameters) {
    auto request = request_for(RSF_D3D11_DISPATCH_PRESENT1, receiver);
    request.args.present1 = {interval, flags, parameters};
    if (handled(request)) return request.result.hresult;
    const auto next = continuation<decltype(&forward_present1)>(request.method);
    return next ? next(receiver, interval, flags, parameters) : E_FAIL;
}
HRESULT WINAPI forward_color(IDXGISwapChain3* receiver, DXGI_COLOR_SPACE_TYPE color) {
    auto request = request_for(RSF_D3D11_DISPATCH_SET_COLOR_SPACE1, receiver);
    request.args.color_space = {uint32_t(color)};
    if (handled(request)) return request.result.hresult;
    const auto next = continuation<decltype(&forward_color)>(request.method);
    return next ? next(receiver, color) : E_FAIL;
}
HRESULT WINAPI forward_resize1(IDXGISwapChain3* receiver, UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags,
    const UINT* node_masks, IUnknown* const* queues) {
    auto request = request_for(RSF_D3D11_DISPATCH_RESIZE_BUFFERS1, receiver);
    request.args.resize_buffers1 = {count, width, height, uint32_t(format), flags, node_masks, reinterpret_cast<void* const*>(queues)};
    if (handled(request)) return request.result.hresult;
    const auto next = continuation<decltype(&forward_resize1)>(request.method);
    return next ? next(receiver, count, width, height, format, flags, node_masks, queues) : E_FAIL;
}
bool pin(const void* address) {
    HMODULE module{};
    return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(address), &module) != FALSE;
}
void* near_page(uintptr_t site) {
    SYSTEM_INFO info{}; GetSystemInfo(&info);
    const auto step = uintptr_t(info.dwAllocationGranularity), centre = site & ~(step - 1);
    for (uintptr_t delta = step; delta < 0x70000000; delta += step) {
        for (auto candidate : {centre + delta, centre > delta ? centre - delta : uintptr_t(0)}) {
            if (!candidate) continue;
            MEMORY_BASIC_INFORMATION region{};
            if (VirtualQuery(reinterpret_cast<void*>(candidate), &region, sizeof(region)) && region.State == MEM_FREE)
                if (auto* page = VirtualAlloc(reinterpret_cast<void*>(candidate), 4096, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE)) return page;
        }
    }
    return nullptr;
}
}
bool prepare_d3d11_chain(const rsf_game_host_services& host, bool (*cold_owner)()) noexcept try {
    if (installed.load(std::memory_order_acquire)) return true;
    std::lock_guard<std::mutex> lock(install_guard);
    if (installed.load(std::memory_order_relaxed)) return true;
    if (attempted) return fail(host, "Steam swapchain forwarding installation previously refused; process restart required.");
    const auto module = GetModuleHandleW(L"gameoverlayrenderer64.dll");
    if (!module) return true; // Rechecked at the guarded cold creation call.
    if (!cold_owner || !cold_owner())
        return fail(host, "Steam swapchain forwarding refused late installation after native device creation.");
    wchar_t path[32768]{}; std::string hash;
    const auto length = GetModuleFileNameW(module, path, DWORD(std::size(path)));
    if (!length || length >= std::size(path) || !rsf::sha256_file(path, hash) || hash != steam_hash)
        return fail(host, "Steam swapchain forwarding refused: unrecognized overlay fingerprint.");
    const auto base = uintptr_t(module);
    auto guarded = [&] {
        for (size_t i = 0; i < std::size(present_entries); ++i) {
            unsigned char bytes[16]{};
            if (!read(base + present_entries[i], bytes, sizeof(bytes)) ||
                std::memcmp(bytes, present_entry_bytes[i].data(), sizeof(bytes))) return false;
        }
        for (const auto& site : sites) {
            unsigned char load[7]{}, following[16]{};
            if (!read(base + site.rva, load, sizeof(load)) || std::memcmp(load, site.load.data(), sizeof(load)) ||
                !read(base + site.rva + 7, following, site.following.size()) ||
                std::memcmp(following, site.following.data(), site.following.size())) return false;
        }
        return true;
    };
    if (!guarded()) return fail(host, "Steam swapchain forwarding refused: load/call continuation bytes differ.");
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    const auto service = runtime ? reinterpret_cast<Dispatch>(GetProcAddress(runtime, "rsf_d3d11_host_dispatch_chain")) : nullptr;
    if (!service || !pin(module) || !pin(reinterpret_cast<const void*>(&forward_present)) || !pin(reinterpret_cast<const void*>(service)))
        return fail(host, "Steam swapchain forwarding refused: typed chain service or process-lifetime modules unavailable.");
    auto* page = static_cast<unsigned char*>(near_page(base + sites[0].rva));
    if (!page) return fail(host, "Steam swapchain forwarding refused: no near relay page.");
    // Relays replace only their complete MOV RAX/R10,[RIP+disp32]. Steam retains
    // its original calls/tail jumps, stack arguments, register setup and flags.
    constexpr size_t cell_offset = 512;
    std::memcpy(page + cell_offset, handlers, sizeof(handlers));
    void* targets[std::size(sites) + std::size(present_entries)]{};
    for (size_t i = 0; i < std::size(sites); ++i) {
        auto* stub = page + i * 32;
        targets[i] = reinterpret_cast<void*>(base + sites[i].rva);
        const auto reach = intptr_t(stub) - intptr_t(targets[i]) - 5;
        if (reach < INT32_MIN || reach > INT32_MAX) {
            VirtualFree(page, 0, MEM_RELEASE);
            return fail(host, "Steam swapchain forwarding refused: relay outside rel32 reach.");
        }
        std::memcpy(stub, sites[i].load.data(), 3);
        const int32_t displacement = int32_t((page + cell_offset + (sites[i].method - 1) * sizeof(void*)) - (stub + 7));
        std::memcpy(stub + 3, &displacement, sizeof(displacement));
        const unsigned char jump[]{0xff,0x25,0,0,0,0};
        std::memcpy(stub + 7, jump, sizeof(jump));
        const auto continuation = base + sites[i].rva + 7;
        std::memcpy(stub + 13, &continuation, sizeof(continuation));
    }
    DWORD old{};
    if (!VirtualProtect(page, 4096, PAGE_EXECUTE_READ, &old) || !FlushInstructionCache(GetCurrentProcess(), page, 4096)) {
        VirtualFree(page, 0, MEM_RELEASE);
        return fail(host, "Steam swapchain forwarding refused: relay publication failed.");
    }
    dispatch_user.store(host.user, std::memory_order_release);
    steam_base.store(base, std::memory_order_release);
    dispatch_service.store(service, std::memory_order_release);
    attempted = true; // Relays/modules/trampolines remain process-lived after hook publication.
    size_t created = 0;
    for (; created < std::size(sites); ++created)
        if (MH_CreateHook(targets[created], page + created * 32, nullptr) != MH_OK) break;
    if (created == std::size(sites)) {
        targets[created] = reinterpret_cast<void*>(base + present_entries[0]);
        if (MH_CreateHook(targets[created], reinterpret_cast<void*>(&present_entry),
            reinterpret_cast<void**>(&original_present_entry)) == MH_OK) ++created;
    }
    if (created == std::size(sites) + 1) {
        targets[created] = reinterpret_cast<void*>(base + present_entries[1]);
        if (MH_CreateHook(targets[created], reinterpret_cast<void*>(&present1_entry),
            reinterpret_cast<void**>(&original_present1_entry)) == MH_OK) ++created;
    }
    bool queued = created == std::size(targets) && guarded();
    if (queued) for (auto* target : targets) if (MH_QueueEnableHook(target) != MH_OK) queued = false;
    if (!queued || MH_ApplyQueued() != MH_OK) {
        // Queue removal too: a failed Apply may have enabled only the first site.
        // Keep all backing memory valid even if rollback itself refuses.
        for (size_t i = 0; i < created; ++i) (void)MH_QueueDisableHook(targets[i]);
        (void)MH_ApplyQueued();
        return fail(host, "Steam swapchain forwarding refused: complete hook set enable failed; no facade admitted.");
    }
    installed.store(true, std::memory_order_release);
    if (host.log) try { host.log(host.user, "Steam swapchain forwarding installed at seven methods/ten guarded loads and two pre-lock Present entries; native receivers retain Steam handlers; original installer/global untouched; process-lived relays."); } catch (...) {}
    return true;
}
catch (...) { return false; }
}
