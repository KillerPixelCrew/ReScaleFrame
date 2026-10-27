// SPDX-License-Identifier: GPL-3.0-only
// Social Club 2.4.0.240 selects the DXGI current backbuffer even when RAGE supplies a recording
// list and its frame's buffer index. Queued frames can differ from the DXGI current index. Keep
// that supplied index when recording into RAGE's list. Never change cache files or disable an
// overlay. Exact module, instruction and runtime evidence: docs/research/rdr2-renderer.md.
#include "social_club.h"
#include <windows.h>
#include <MinHook.h>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <mutex>

namespace {
using Dispatch = uintptr_t (*)(void*);
using Render = uintptr_t (*)(unsigned char*, void*, void*, void*, unsigned);
Dispatch original_dispatch = nullptr;
Render original_render = nullptr;
void (*write_log)(void*, const char*) = nullptr;
bool trace_enabled = false;
std::mutex install_guard;
bool attempted = false;
bool repaired = false;
std::atomic<unsigned> observed{0};
std::atomic<unsigned> mismatched{0};

uintptr_t trace_render(unsigned char* renderer, void* draws, void* unused, void* list, unsigned index)
{
    auto* swapchain = *reinterpret_cast<void**>(renderer + 0x20);
    const bool queries_current = renderer[0x40c] != 0 && swapchain;
    unsigned current = index;
    if (queries_current) {
        using CurrentIndex = unsigned (*)(void*);
        auto** table = *reinterpret_cast<void***>(swapchain);
        current = reinterpret_cast<CurrentIndex>(table[36])(swapchain);
    }
    const unsigned call = observed.fetch_add(1) + 1;
    const bool mismatch = list && queries_current && current != index;
    const unsigned differences = mismatch ? mismatched.fetch_add(1) + 1 : mismatched.load();
    if (call <= 8 || (mismatch && (differences & (differences - 1)) == 0)) {
        char message[256]{};
        std::snprintf(message, sizeof(message),
            "Social Club frame: call=%u list=%p supplied=%u current=%u queries_current=%u repaired=%u mismatches=%u",
            call, list, index, current, unsigned(queries_current), unsigned(repaired), differences);
        write_log(nullptr, message);
    }
    return original_render(renderer, draws, unused, list, index);
}

void install_renderer_fix()
{
    std::lock_guard<std::mutex> lock(install_guard);
    if (attempted) return;
    HMODULE module = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN, L"SocialClubD3D12Renderer.dll", &module)) return;
    attempted = true;
    auto* base = reinterpret_cast<unsigned char*>(module);
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    constexpr unsigned char gate[] = {0x80, 0xb9, 0x0c, 0x04, 0x00, 0x00, 0x00,
        0x4d, 0x8b, 0xf1, 0x48, 0x8b, 0xf9, 0x74, 0x17};
    constexpr unsigned char entry[] = {0x48, 0x89, 0x5c, 0x24, 0x18, 0x48, 0x89, 0x54,
        0x24, 0x10, 0x55, 0x56, 0x57, 0x41, 0x54};
    if (nt->Signature != IMAGE_NT_SIGNATURE || nt->FileHeader.TimeDateStamp != 0x6a86d560 ||
        nt->OptionalHeader.SizeOfImage != 0x2e000 ||
        std::memcmp(base + 0x4b89, gate, sizeof(gate)) ||
        std::memcmp(base + 0x4b70, entry, sizeof(entry))) {
        write_log(nullptr, "Social Club backbuffer fix refused: unsupported renderer image or modified instructions.");
        return;
    }

    // At RVA 4b89, R9 is the external command list. The displaced seven-byte cmp tests the
    // renderer's current-index flag. With an external list, set ZF without clobbering registers
    // and resume at 4b90 so its existing JZ selects the supplied fifth argument. Otherwise execute
    // the original comparison. No calls, stack changes or nonvolatile
    // register writes occur in the gate; the original prologue/epilogue and unwind data remain.
    unsigned char code[] = {
        0x4d, 0x85, 0xc9,                         // test r9,r9
        0x74, 0x11,                               // jz original_cmp
        0x4d, 0x39, 0xc9,                         // cmp r9,r9 (ZF=1)
        0xff, 0x25, 0, 0, 0, 0,                  // jmp [rip] -> after_cmp
        0, 0, 0, 0, 0, 0, 0, 0,
        0x80, 0xb9, 0x0c, 0x04, 0, 0, 0,        // original_cmp: cmp byte [rcx+40c],0
        0xff, 0x25, 0, 0, 0, 0,                  // jmp [rip] -> after_cmp
        0, 0, 0, 0, 0, 0, 0, 0};
    auto* bridge = static_cast<unsigned char*>(VirtualAlloc(nullptr, sizeof(code),
        MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE));
    if (!bridge) {
        write_log(nullptr, "Social Club backbuffer fix refused: gate allocation failed.");
        return;
    }
    // Publish a complete executable gate before registering it: census may enable its queued
    // hooks concurrently. The gate must not depend on a not-yet-published trampoline pointer.
    const void* after_cmp = base + 0x4b90;
    std::memcpy(code + 14, &after_cmp, sizeof(after_cmp));
    std::memcpy(code + 35, &after_cmp, sizeof(after_cmp));
    std::memcpy(bridge, code, sizeof(code));
    DWORD old = 0;
    auto status = MH_ERROR_MEMORY_PROTECT;
    bool created = false;
    if (VirtualProtect(bridge, sizeof(code), PAGE_EXECUTE_READ, &old)) {
        FlushInstructionCache(GetCurrentProcess(), bridge, sizeof(code));
        status = MH_CreateHook(base + 0x4b89, bridge, nullptr);
        created = status == MH_OK;
        if (created) status = MH_EnableHook(base + 0x4b89);
    }
    repaired = created && (status == MH_OK || status == MH_ERROR_ENABLED);
    char message[192]{};
    std::snprintf(message, sizeof(message), "Social Club backbuffer fix at renderer RVA 0x4b89: %s",
        MH_StatusToString(status));
    write_log(nullptr, message);
    if (!repaired) {
        // No enabled hook can enter the bridge. Remove any disabled hook before releasing it.
        if (created) MH_RemoveHook(base + 0x4b89);
        VirtualFree(bridge, 0, MEM_RELEASE);
        return;
    }
    if (trace_enabled) {
        status = MH_CreateHook(base + 0x4b70, reinterpret_cast<void*>(&trace_render),
            reinterpret_cast<void**>(&original_render));
        if (status == MH_OK) status = MH_EnableHook(base + 0x4b70);
        std::snprintf(message, sizeof(message), "Social Club frame trace: %s", MH_StatusToString(status));
        write_log(nullptr, message);
    }
    // Module, bridge and MinHook trampolines remain owned until process exit.
}

uintptr_t dispatch(void* context)
{
    install_renderer_fix();
    return original_dispatch(context);
}
}

bool rsf_rdr2_install_social_club_fix(void (*log)(void*, const char*), bool trace) noexcept
{
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    constexpr unsigned char expected[] = {0x40, 0x53, 0x48, 0x83, 0xec, 0x30, 0x48, 0x83,
        0x79, 0x08, 0x00, 0x48, 0x8b, 0xd9, 0x75, 0x29};
    if (!log || std::memcmp(base + 0x27f09e8, expected, sizeof(expected))) return false;
    write_log = log;
    trace_enabled = trace;
    auto status = MH_Initialize();
    if (status != MH_OK && status != MH_ERROR_ALREADY_INITIALIZED) return false;
    status = MH_CreateHook(base + 0x27f09e8, reinterpret_cast<void*>(&dispatch),
        reinterpret_cast<void**>(&original_dispatch));
    return status == MH_OK && MH_EnableHook(base + 0x27f09e8) == MH_OK;
}
