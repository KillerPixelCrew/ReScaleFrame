// SPDX-License-Identifier: MIT
// Native leaves of ReScaleFrame.Game.RDR2.dll. The Rust crate rsf-rdr2 owns every RDR2 decision:
// fingerprints, RVAs and expected bytes, layouts, routing, refusals and lifecycle ordering.
#include "native_platform.h"
#include <rescaleframe/game_renderer.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <MinHook.h>
#include <intrin.h>
#include "../../../integrations/windows/xinput_facts.h"
#include <array>
#include <atomic>

extern "C" {
void* rsf_rdr2_pop_detour(void* ring, void* record, uintptr_t caller) noexcept;
uintptr_t rsf_rdr2_output_detour(void* depth, void* color, uint32_t technique, uintptr_t caller) noexcept;
uintptr_t rsf_rdr2_blit_detour(void* shader, uint32_t technique, uint32_t pass, const void* source,
    const void* destination, const void* clip, uintptr_t caller) noexcept;
int32_t rsf_rdr2_fsr_dispatch_detour(void* context, const void* description, uintptr_t caller) noexcept;
}

namespace rsf::rdr2 {
namespace {
uintptr_t return_address(void* address) noexcept { return reinterpret_cast<uintptr_t>(address); }

// MinHook enters these with the native call's own return address, which the policy matches
// against measured call sites.
void* pop(void* ring, void* record) noexcept
{
    return rsf_rdr2_pop_detour(ring, record, return_address(_ReturnAddress()));
}
uintptr_t output(void* depth, void* color, uint32_t technique) noexcept
{
    return rsf_rdr2_output_detour(depth, color, technique, return_address(_ReturnAddress()));
}
uintptr_t blit(void* shader, uint32_t technique, uint32_t pass, const void* source, const void* destination,
    const void* clip) noexcept
{
    return rsf_rdr2_blit_detour(shader, technique, pass, source, destination, clip,
        return_address(_ReturnAddress()));
}
int32_t fsr_dispatch(void* context, const void* description) noexcept
{
    return rsf_rdr2_fsr_dispatch_detour(context, description, return_address(_ReturnAddress()));
}

bool read(uintptr_t address, void* out, size_t size) noexcept
{
    SIZE_T count{};
    return address && out &&
        ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address), out, size, &count) &&
        count == size;
}

// Clone an object's own table and swap its table pointer. The original entry keeps any existing
// wrapper or overlay chain; no module-wide D3D12 or DXGI function is patched.
template<size_t N>
bool clone_table(void* object, std::array<void*, N>& copy, size_t slot, const void* detour, void** original) noexcept
{
    auto** table = *static_cast<void***>(object);
    if (!read(reinterpret_cast<uintptr_t>(table), copy.data(), sizeof(copy))) return false;
    *original = copy[slot];
    copy[slot] = const_cast<void*>(detour);
    DWORD protection{};
    if (!VirtualProtect(object, sizeof(void*), PAGE_READWRITE, &protection)) return false;
    const auto replaced = InterlockedCompareExchangePointer(static_cast<void* volatile*>(object), copy.data(), table);
    DWORD ignored{};
    VirtualProtect(object, sizeof(void*), protection, &ignored);
    return replaced == table;
}
std::array<void*, 19> queue_table{};
std::array<void*, 41> swapchain_table{};
constexpr size_t execute_slot = 10;
constexpr size_t present_slot = 8;

bool attach_queue(void* candidate, const void* detour, void** original) noexcept
{
    auto* queue = static_cast<ID3D12CommandQueue*>(candidate);
    if (!queue || queue->GetDesc().Type != D3D12_COMMAND_LIST_TYPE_DIRECT ||
        !clone_table(candidate, queue_table, execute_slot, detour, original)) return false;
    // The cloned table calls into this DLL, so the queue stays referenced until process teardown.
    queue->AddRef();
    return true;
}
bool attach_present(void* candidate, const void* detour, void** original) noexcept
{
    IDXGISwapChain4* latest{};
    if (!candidate || FAILED(static_cast<IUnknown*>(candidate)->QueryInterface(IID_PPV_ARGS(&latest)))) return false;
    if (static_cast<void*>(latest) != candidate ||
        !clone_table(candidate, swapchain_table, present_slot, detour, original)) {
        latest->Release();
        return false;
    }
    // The QueryInterface reference is retained with the cloned table.
    return true;
}

// Window messages the game's input thread removes, and successful public XInput reads.
using MessageCallback = void (*)(uint32_t, uint32_t);
std::atomic<MessageCallback> message_callback{};
std::atomic<uint32_t> message_readers{};
HHOOK message_hook{};
windows::XInputFacts controller_facts;
LRESULT CALLBACK received_message(int code, WPARAM removed, LPARAM argument)
{
    ++message_readers;
    if (code >= 0 && removed == PM_REMOVE) {
        const auto& message = *reinterpret_cast<const MSG*>(argument);
        uint32_t kind = 0;
        if (message.message >= WM_KEYFIRST && message.message <= WM_KEYLAST) kind = RSF_GAME_INPUT_KEYBOARD;
        if (message.message >= WM_MOUSEFIRST && message.message <= WM_MOUSELAST) kind = RSF_GAME_INPUT_MOUSE;
        if (message.message == WM_INPUT) {
            RAWINPUTHEADER header{};
            UINT bytes = sizeof(header);
            if (GetRawInputData(reinterpret_cast<HRAWINPUT>(message.lParam), RID_HEADER, &header, &bytes,
                    sizeof(header)) == sizeof(header))
                kind = header.dwType == RIM_TYPEKEYBOARD ? RSF_GAME_INPUT_KEYBOARD :
                    header.dwType == RIM_TYPEMOUSE ? RSF_GAME_INPUT_MOUSE : 0u;
        }
        if (const auto callback = message_callback.load()) callback(kind, message.message);
    }
    const auto result = CallNextHookEx(nullptr, code, removed, argument);
    --message_readers;
    return result;
}

constexpr Platform native{
    sizeof(Platform),
    platform_version,
    []() noexcept { return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)); },
    [](const char* name) noexcept { return reinterpret_cast<uintptr_t>(GetModuleHandleA(name)); },
    [](uintptr_t module, const char* name) noexcept {
        return module ? reinterpret_cast<uintptr_t>(GetProcAddress(reinterpret_cast<HMODULE>(module), name)) : 0;
    },
    read,
    []() noexcept { return static_cast<uintptr_t>(__readgsqword(0x58)); },
    [](uint64_t* now, uint64_t* frequency) noexcept {
        LARGE_INTEGER counter{}, rate{};
        QueryPerformanceCounter(&counter);
        QueryPerformanceFrequency(&rate);
        *now = static_cast<uint64_t>(counter.QuadPart);
        *frequency = static_cast<uint64_t>(rate.QuadPart);
    },
    [](uintptr_t address, uintptr_t* base, char* path, size_t capacity) noexcept {
        *base = 0;
        if (!capacity) return false;
        path[0] = '\0';
        HMODULE owner{};
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                reinterpret_cast<LPCWSTR>(address), &owner) || !owner) return false;
        *base = reinterpret_cast<uintptr_t>(owner);
        wchar_t wide[MAX_PATH]{};
        const auto length = GetModuleFileNameW(owner, wide, MAX_PATH);
        if (!length) return false;
        const auto written = WideCharToMultiByte(CP_UTF8, 0, wide, static_cast<int>(length), path,
            static_cast<int>(capacity - 1), nullptr, nullptr);
        path[written > 0 ? written : 0] = '\0';
        return written > 0;
    },
    []() noexcept {
        const auto status = MH_Initialize();
        return status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](uintptr_t target, const void* detour, void** original) noexcept {
        return MH_CreateHook(reinterpret_cast<void*>(target), const_cast<void*>(detour), original) == MH_OK;
    },
    [](uintptr_t target) noexcept { return MH_EnableHook(reinterpret_cast<void*>(target)) == MH_OK; },
    [](uintptr_t target) noexcept {
        const auto status = MH_DisableHook(reinterpret_cast<void*>(target));
        return status == MH_OK || status == MH_ERROR_DISABLED || status == MH_ERROR_NOT_CREATED;
    },
    [](uintptr_t target) noexcept {
        const auto status = MH_RemoveHook(reinterpret_cast<void*>(target));
        return status == MH_OK || status == MH_ERROR_NOT_CREATED;
    },
    pop,
    output,
    blit,
    fsr_dispatch,
    attach_queue,
    attach_present,
    [](void* resource, ResourceFacts* out) noexcept {
        if (!resource || !out) return false;
        const auto description = static_cast<ID3D12Resource*>(resource)->GetDesc();
        *out = {description.Width, description.Height, static_cast<uint32_t>(description.Format),
            description.SampleDesc.Count, 0};
        return true;
    },
    [](void* list, void* resource, uint32_t before, uint32_t after) noexcept {
        D3D12_RESOURCE_BARRIER barrier{};
        barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition = {static_cast<ID3D12Resource*>(resource), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            static_cast<D3D12_RESOURCE_STATES>(before), static_cast<D3D12_RESOURCE_STATES>(after)};
        static_cast<ID3D12GraphicsCommandList*>(list)->ResourceBarrier(1, &barrier);
    },
    [](void* swapchain, uint32_t* buffers, uint64_t* window) noexcept {
        DXGI_SWAP_CHAIN_DESC description{};
        if (!swapchain || FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetDesc(&description))) return false;
        *buffers = description.BufferCount;
        *window = reinterpret_cast<uint64_t>(description.OutputWindow);
        return true;
    },
    [](void* swapchain, uint32_t index) noexcept -> void* {
        ID3D12Resource* buffer{};
        return swapchain && SUCCEEDED(static_cast<IDXGISwapChain*>(swapchain)->GetBuffer(index, IID_PPV_ARGS(&buffer))) ?
            buffer : nullptr;
    },
    // Attach retained this object through its IDXGISwapChain4 identity.
    [](void* swapchain) noexcept -> uint32_t {
        return static_cast<IDXGISwapChain3*>(swapchain)->GetCurrentBackBufferIndex();
    },
    [](void* object) noexcept {
        if (object) static_cast<IUnknown*>(object)->Release();
    },
    [](MessageCallback message, void (*controller)()) noexcept {
        message_callback.store(message);
        if (!message_hook) message_hook = SetWindowsHookExW(WH_GETMESSAGE, received_message, nullptr, GetCurrentThreadId());
        controller_facts.install_loaded(controller);
    },
    []() noexcept { controller_facts.disable(); },
    []() noexcept { return message_readers.load() == 0 && controller_facts.idle(); },
    []() noexcept {
        if (!message_hook) return true;
        if (!UnhookWindowsHookEx(message_hook) && GetLastError() != ERROR_INVALID_HOOK_HANDLE) return false;
        message_hook = nullptr;
        return true;
    },
    []() noexcept { return controller_facts.remove(); },
};
}

const Platform& platform() noexcept { return native; }
}
