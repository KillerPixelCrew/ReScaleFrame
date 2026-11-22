// SPDX-License-Identifier: MIT

#include <rescaleframe/d3d11_observer.h>

#include <windows.h>
#include <psapi.h>

#include <d3d11.h>
#include <dxgi1_2.h>

#include <MinHook.h>
#include <hde64.h>

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <memory>

#include "d3d11_observer_callbacks.h"
#include "d3d11_observer_hooks.h"
#include "d3d11_observer_module.h"
#include "d3d11_observer_present.h"

namespace {

using present_fn = HRESULT(STDMETHODCALLTYPE*)(void*, UINT, UINT);
using present1_fn = HRESULT(STDMETHODCALLTYPE*)(void*, UINT, UINT, const DXGI_PRESENT_PARAMETERS*);

template<class Callback, class... Args>
void invoke(Callback callback, Args... args) noexcept
{
    try { callback(args...); }
    catch (...) { /* Keep foreign exceptions on the native side. */ }
}

constexpr rsf_observer11_callbacks callbacks{
    invoke<rsf_dump_log_fn, void*, const char*>,
    invoke<rsf_observer_layout_fn, void*, void*, const rsf_observer_layout_element*, uint32_t, uint32_t>,
    invoke<rsf_observer_shader_fn, void*, void*, uint32_t, const void*, uint32_t>,
    [](rsf_observer_texture_fn callback, void* user, void* texture, const rsf_observer11_texture_facts* facts) noexcept {
        invoke(callback, user, texture, facts->width, facts->height, facts->format, facts->mip_levels,
            facts->array_size, facts->sample_count, facts->bind_flags, facts->misc_flags);
    },
    invoke<rsf_observer_buffer_fn, void*, void*, const void*, uint32_t, uint32_t>,
    invoke<rsf_observer_present_fn, void*, void*>,
    invoke<rsf_observer_present_event_fn, void*, const rsf_observer_present_event*>,
};

static_assert(D3D11_BIND_RENDER_TARGET == 0x20 && D3D11_BIND_CONSTANT_BUFFER == 0x4);
static_assert(RSF_OBSERVER_OK == 0 && RSF_OBSERVER_ERROR_INVALID_ARGUMENT == -1 && RSF_OBSERVER_ERROR_NOT_READY == -6);
static_assert(sizeof(rsf_observer11_texture_facts) == 32 && sizeof(rsf_observer11_callbacks) == 7 * sizeof(void*));
static_assert(RSF_OBSERVER_ERROR_ALREADY_INSTALLED == -3 && RSF_OBSERVER_ERROR_PATCH_FAILED == -5);
static_assert(sizeof(rsf_observer11_slot) == 2 * sizeof(void*) && sizeof(rsf_observer11_hook_ops) == 4 * sizeof(void*));
} // namespace

extern "C" int32_t rsf_observer11_guard_texture_extent(void* texture, uint32_t* width, uint32_t* height) noexcept try
{
    D3D11_TEXTURE2D_DESC desc{};
    static_cast<ID3D11Texture2D*>(texture)->GetDesc(&desc);
    *width = desc.Width;
    *height = desc.Height;
    return 1;
} catch (...) { return 0; }

static_assert(DXGI_PRESENT_TEST == 1 && S_OK == 0);
static_assert(sizeof(rsf_observer11_present_ops) == 3 * sizeof(void*));

extern "C" const rsf_observer11_present_ops rsf_observer11_present_platform{
    [](void* raw, uint32_t* width, uint32_t* height) noexcept -> int32_t {
        try {
            DXGI_SWAP_CHAIN_DESC desc{};
            if (FAILED(static_cast<IDXGISwapChain*>(raw)->GetDesc(&desc))) return 0;
            *width = desc.BufferDesc.Width;
            *height = desc.BufferDesc.Height;
            return 1;
        } catch (...) { return 0; }
    },
    [](void* raw, void** device) noexcept -> int32_t {
        try {
            return static_cast<IDXGISwapChain*>(raw)->GetDevice(__uuidof(ID3D11Device), device);
        } catch (...) { return E_FAIL; }
    },
    [](const rsf_observer_present_event* event, const void* parameters, void* target) noexcept -> int32_t {
        try {
            if (event->method == 1) {
                return reinterpret_cast<present1_fn>(target)(event->swapchain, event->sync_interval,
                    event->flags, static_cast<const DXGI_PRESENT_PARAMETERS*>(parameters));
            }
            return reinterpret_cast<present_fn>(target)(event->swapchain, event->sync_interval, event->flags);
        } catch (...) { return E_FAIL; }
    },
};

namespace {

// The function a module's own image has in a vtable slot, for a slot another hook has patched.
//
// Never forward to another hook. Measured on 26 September 2026, first Windows run: Steam's overlay
// (`gameoverlayrenderer64.dll`) hooks `IDXGISwapChain::Present` in the same table, notices when its
// entry is displaced, and re-hooks, taking whatever it displaced as its original. With this
// module's hook in the slot that made the two forward to each other, and every Present recursed
// until the stack ran out, sixty-four frames deep in the crash report. The genuine function is not
// in the table any more by then, but it is in the module on disk: the table is read-only data in
// `dxgi.dll`, so the slot's unrelocated value can be read from the file at the same RVA and
// relocated to the loaded base. `current` is what the slot held before this module patched it;
// it is returned as-is when it already lies inside the owning module.
// The loaded module is queried by Windows. The on-disk bytes and section table belong to Rust.
using DiskImage = std::unique_ptr<rsf_observer11_module_file, decltype(&rsf_core_observer11_module_close)>;
struct ModuleImage {
    HMODULE module = nullptr;
    const unsigned char* base = nullptr;
    size_t image_size = 0;
    wchar_t path[MAX_PATH] = L"?";
    DiskImage file{nullptr, rsf_core_observer11_module_close};
};

bool locate_module(const void* inside, ModuleImage& out, char* message, size_t size)
{
    HMODULE module = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            static_cast<LPCWSTR>(inside), &module)) {
        std::snprintf(message, size, "%p is not inside any module", inside);
        return false;
    }
    if (out.module != module) {
        MODULEINFO info{};
        if (!GetModuleInformation(GetCurrentProcess(), module, &info, sizeof(info)) ||
            !info.lpBaseOfDll || !info.SizeOfImage) {
            std::snprintf(message, size, "%p has no usable module extent", inside);
            return false;
        }
        out = ModuleImage{};
        out.module = module;
        out.base = static_cast<const unsigned char*>(info.lpBaseOfDll);
        out.image_size = info.SizeOfImage;
    }
    return true;
}

// Only a parsed file is cached. A later lookup retries an earlier refusal.
bool read_module_image(const void* inside, ModuleImage& out, char* message, size_t size)
{
    if (!locate_module(inside, out, message, size)) return false;
    if (out.file) return true;
    const auto length = GetModuleFileNameW(out.module, out.path, MAX_PATH);
    if (!length || length >= MAX_PATH) {
        std::snprintf(message, size, "the module's path could not be read");
        return false;
    }
    rsf_observer11_module_file* file = nullptr;
    switch (rsf_core_observer11_module_open(out.path, length, &file)) {
    case RSF_OBSERVER11_MODULE_OK:
        out.file.reset(file);
        return true;
    case RSF_OBSERVER11_MODULE_OPEN_FAILED:
        std::snprintf(message, size, "%ls could not be opened", out.path);
        return false;
    case RSF_OBSERVER11_MODULE_INVALID_HEADERS:
        std::snprintf(message, size, "%ls has no usable headers", out.path);
        return false;
    default:
        std::snprintf(message, size, "%ls could not be read", out.path);
        return false;
    }
}

const unsigned char* disk_bytes(const ModuleImage& image, const void* address, size_t count)
{
    const auto loaded = reinterpret_cast<uintptr_t>(address);
    const auto base = reinterpret_cast<uintptr_t>(image.base);
    if (!image.file || loaded < base) return nullptr;
    const auto rva = loaded - base;
    if (rva >= image.image_size || count > image.image_size - rva) return nullptr;
    return rsf_core_observer11_module_span(image.file.get(), rva, count);
}

void* genuine_slot(ModuleImage& image, void** vtable, size_t slot, void* current, char* message, size_t size)
{
    // The cheap answer first: a slot nobody has patched already points into the module.
    if (!locate_module(vtable, image, message, size)) {
        return nullptr;
    }
    const auto candidate = reinterpret_cast<uintptr_t>(current);
    const auto base = reinterpret_cast<uintptr_t>(image.base);
    if (candidate >= base && candidate - base < image.image_size) {
        return current;
    }
    if (!read_module_image(vtable, image, message, size)) {
        return nullptr;
    }
    const unsigned char* stored = disk_bytes(image, vtable + slot, sizeof(uint64_t));
    if (!stored) {
        std::snprintf(message, size, "no section of %ls covers the table", image.path);
        return nullptr;
    }
    uint64_t unrelocated = 0;
    std::memcpy(&unrelocated, stored, sizeof(unrelocated));
    const auto preferred_base = rsf_core_observer11_module_preferred_base(image.file.get());
    if (unrelocated < preferred_base || unrelocated - preferred_base >= image.image_size) {
        std::snprintf(message, size, "%ls holds 0x%llx in that slot, which is not inside it",
                      image.path, static_cast<unsigned long long>(unrelocated));
        return nullptr;
    }
    return const_cast<unsigned char*>(image.base + (unrelocated - preferred_base));
}

// The target and length of a jump another hook wrote at a function's entry. `bytes` are the
// entry's bytes, `entry` is where they live, which relative jumps are measured from. Null with a
// length of zero when the bytes are not a jump this recognises: the three forms hooking libraries
// write are a near jump, an indirect jump through a pointer after it, and a move into rax followed
// by a jump through it.
void* decode_jump(const unsigned char* bytes, const unsigned char* entry, size_t& length)
{
    if (bytes[0] == 0xE9) {
        int32_t displacement = 0;
        std::memcpy(&displacement, bytes + 1, sizeof(displacement));
        length = 5;
        return const_cast<unsigned char*>(entry + 5 + displacement);
    }
    if (bytes[0] == 0xFF && bytes[1] == 0x25) {
        int32_t displacement = 0;
        std::memcpy(&displacement, bytes + 2, sizeof(displacement));
        void* target = nullptr;
        std::memcpy(&target, entry + 6 + displacement, sizeof(target));
        length = 14;
        return target;
    }
    if (bytes[0] == 0x48 && bytes[1] == 0xB8 && bytes[10] == 0xFF && bytes[11] == 0xE0) {
        void* target = nullptr;
        std::memcpy(&target, bytes + 2, sizeof(target));
        length = 12;
        return target;
    }
    length = 0;
    return nullptr;
}

// Executable memory within a near jump of `near`, or null. Walks down from the address in
// allocation-granularity steps until the system gives a block, which is what a trampoline needs
// when the instructions it carries address memory relative to where they used to be.
void* allocate_near(const void* anchor, size_t size)
{
    SYSTEM_INFO info{};
    GetSystemInfo(&info);
    const uintptr_t granularity = info.dwAllocationGranularity ? info.dwAllocationGranularity
                                                                : 0x10000;
    const uintptr_t origin = reinterpret_cast<uintptr_t>(anchor) & ~(granularity - 1);
    const uintptr_t lowest = origin > 0x70000000 ? origin - 0x70000000 : granularity;
    for (uintptr_t candidate = origin - granularity; candidate > lowest;
         candidate -= granularity) {
        void* block = VirtualAlloc(reinterpret_cast<void*>(candidate), size,
                                   MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE);
        if (block) {
            return block;
        }
    }
    return nullptr;
}

// A trampoline over a function's bytes as the file has them, so calling it reaches the function's
// body without passing anything patched at its entry.
//
// Copies whole instructions from the file's bytes until at least `covered` bytes are past, which
// has to be the longest patch anyone has at the entry, then jumps to the entry past that length.
// Instructions that address memory relative to the instruction pointer are re-pointed, which is
// why the block is allocated near; a relative jump or call this early in a function is refused,
// because relocating one is more than a prologue should need.
void* build_genuine_trampoline(const unsigned char* entry, const unsigned char* file_bytes,
                               size_t available, size_t covered, char* message, size_t size)
{
    constexpr size_t block_size = 64;
    auto* block = static_cast<unsigned char*>(allocate_near(entry, block_size));
    if (!block) {
        std::snprintf(message, size, "no executable memory near %p for a trampoline",
                      static_cast<const void*>(entry));
        return nullptr;
    }
    size_t copied = 0;
    while (copied < covered) {
        hde64s instruction{};
        const unsigned int length = hde64_disasm(file_bytes + copied, &instruction);
        if ((instruction.flags & F_ERROR) || length == 0 || copied + length > available ||
            copied + length > block_size - 14) {
            std::snprintf(message, size, "Present's prologue could not be decoded at byte %zu",
                          copied);
            VirtualFree(block, 0, MEM_RELEASE);
            return nullptr;
        }
        const unsigned char opcode = instruction.opcode;
        if (opcode == 0xE8 || opcode == 0xE9 || opcode == 0xEB || (opcode >= 0x70 && opcode <= 0x7F) ||
            (instruction.opcode2 >= 0x80 && instruction.opcode2 <= 0x8F && opcode == 0x0F) ||
            opcode == 0xC3 || opcode == 0xC2) {
            std::snprintf(message, size,
                          "Present's prologue has a relative jump or return at byte %zu, which "
                          "this does not relocate",
                          copied);
            VirtualFree(block, 0, MEM_RELEASE);
            return nullptr;
        }
        std::memcpy(block + copied, file_bytes + copied, length);
        if ((instruction.flags & F_MODRM) && (instruction.modrm & 0xC7) == 0x05) {
            // The displacement sits before any immediate, whose size the flags carry.
            const size_t immediate = (instruction.flags & 0x3C) >> 2;
            const size_t at = copied + length - immediate - 4;
            int32_t displacement = 0;
            std::memcpy(&displacement, block + at, sizeof(displacement));
            const intptr_t target =
                reinterpret_cast<intptr_t>(entry + copied + length) + displacement;
            const intptr_t moved = target - reinterpret_cast<intptr_t>(block + copied + length);
            if (moved > INT32_MAX || moved < INT32_MIN) {
                std::snprintf(message, size, "Present's prologue addresses memory too far from "
                                             "the trampoline");
                VirtualFree(block, 0, MEM_RELEASE);
                return nullptr;
            }
            displacement = static_cast<int32_t>(moved);
            std::memcpy(block + at, &displacement, sizeof(displacement));
        }
        copied += length;
    }
    // jmp [rip+0]; dq entry + copied
    const unsigned char jump[6] = {0xFF, 0x25, 0x00, 0x00, 0x00, 0x00};
    std::memcpy(block + copied, jump, sizeof(jump));
    const unsigned char* resume = entry + copied;
    std::memcpy(block + copied + sizeof(jump), &resume, sizeof(resume));
    FlushInstructionCache(GetCurrentProcess(), block, block_size);
    return block;
}

// The module a code address lives in, or null, with its file name for the log.
HMODULE module_of(const void* address, wchar_t* name, size_t count)
{
    HMODULE module = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            static_cast<LPCWSTR>(address), &module)) {
        return nullptr;
    }
    wchar_t path[MAX_PATH] = L"?";
    GetModuleFileNameW(module, path, MAX_PATH);
    const wchar_t* base = std::wcsrchr(path, L'\\');
    std::wcsncpy(name, base ? base + 1 : path, count - 1);
    name[count - 1] = L'\0';
    return module;
}

// Hook Present by detour, in a way that coexists with whatever else has hooked it.
//
// Measured on 26 September 2026, on a machine that turned out to have three hooks on dxgi's
// Present in a game: Steam's overlay, RivaTuner Statistics Server, and this. Each of the two others
// patches the function's entry with a jump and, on its first present, checks the entry and writes
// its own bytes back if something displaced them. Steam takes whatever it displaced as its
// original, which with a table patch of ours made the two forward to each other until the stack
// ran out; RivaTuner restores what it saved at its own install, which drops anything patched over
// it out of the chain entirely. Nothing patched at that entry can be trusted to stay.
//
// So this never owns the entry. It follows the jump chain from the entry to the first function
// that lives inside a module, which is the outermost hook's own function, or dxgi's Present itself
// when nothing is patched, and detours that. A hook re-asserts its target's entry; none re-asserts
// its own function's prologue. Steam and RivaTuner then run first, exactly as they would without
// this, and reach this hook through their own chains, which end in the genuine body.
//
// Two more pieces cover the rest: the chain is never entered twice by one present, because a
// re-entered call goes to a trampoline of this module's own over the bytes the file has for the
// entry, which reaches the body without passing any hook; and everything is put back on uninstall.
//
// Returns false with a reason when no target could be established; the caller then falls back to
// the table patch, with the re-entry guard as the only protection.
bool install_present_route(ModuleImage& image, void* genuine, void* hook,
    uint32_t method, const char* label, char* message, size_t size)
{
    if (!read_module_image(genuine, image, message, size)) {
        return false;
    }
    constexpr size_t window = 32;
    const unsigned char* stored = disk_bytes(image, genuine, window);
    if (!stored) {
        std::snprintf(message, size, "no section of %ls covers Present", image.path);
        return false;
    }
    auto* entry = static_cast<unsigned char*>(genuine);
    const bool patched = std::memcmp(entry, stored, window) != 0;

    // The loop breaker, over the file's bytes. Fourteen bytes past the entry is beyond the longest
    // jump anyone writes there.
    void* block = build_genuine_trampoline(entry, stored, window, 14, message, size);
    if (!block) {
        return false;
    }

    // Follow the chain to the first function inside a module. Relays, the near pages hooking
    // libraries allocate to reach a far target with a short jump, are outside every module and are
    // followed through. This module's own hook is refused as a target, because meeting it would
    // mean something already chains to it.
    void* target = genuine;
    wchar_t owner[MAX_PATH] = L"dxgi.dll";
    size_t hops = 0;
    if (patched) {
        const unsigned char* at = entry;
        target = nullptr;
        for (hops = 0; hops < 8; ++hops) {
            size_t length = 0;
            void* next = decode_jump(at, at, length);
            if (!next) {
                break;
            }
            wchar_t name[MAX_PATH];
            HMODULE module = module_of(next, name, MAX_PATH);
            if (module) {
                if (next == hook) {
                    break;
                }
                target = next;
                std::wcsncpy(owner, name, MAX_PATH - 1);
                ++hops;
                break;
            }
            at = static_cast<const unsigned char*>(next);
        }
        if (!target) {
            VirtualFree(block, 0, MEM_RELEASE);
            std::snprintf(message, size,
                          "Present's entry is patched (%02x %02x %02x %02x %02x) and the chain "
                          "leads to no function inside a module",
                          entry[0], entry[1], entry[2], entry[3], entry[4]);
            return false;
        }
    }

    const MH_STATUS initialised = MH_Initialize();
    if (initialised != MH_OK && initialised != MH_ERROR_ALREADY_INITIALIZED) {
        VirtualFree(block, 0, MEM_RELEASE);
        std::snprintf(message, size, "MinHook did not initialise, status %d", int(initialised));
        return false;
    }
    void* chain = nullptr;
    MH_STATUS status = MH_CreateHook(target, hook, &chain);
    if (status != MH_OK) {
        VirtualFree(block, 0, MEM_RELEASE);
        std::snprintf(message, size, "the detour on %ls could not be created, status %d", owner,
                      int(status));
        return false;
    }
    rsf_core_observer11_publish_route(method, target, chain, block);
    status = MH_EnableHook(target);
    if (status != MH_OK && status != MH_ERROR_ENABLED) {
        const auto removed = MH_RemoveHook(target);
        if (removed == MH_OK || removed == MH_ERROR_NOT_CREATED) {
            rsf_core_observer11_publish_route(method, nullptr, nullptr, nullptr);
            VirtualFree(block, 0, MEM_RELEASE);
        }
        std::snprintf(message, size, "the detour on %ls could not be enabled, status %d", owner, int(status));
        return false;
    }

    if (rsf_core_observer11_logging()) {
        char note[512];
        if (patched) {
            std::snprintf(note, sizeof(note),
                          "observer: %s entry at %p is hooked by others; detoured the "
                          "outermost hook's function at %p in %ls, %zu hop%s down the chain, so it "
                          "runs first and this runs after it. Genuine body at hand through a "
                          "trampoline at %p",
                          label, genuine, target, owner, hops, hops == 1 ? "" : "s", block);
        } else {
            std::snprintf(note, sizeof(note),
                          "observer: %s detoured at %p in dxgi, entry unpatched", label, genuine);
        }
        rsf_core_observer11_log(note);
    }
    return true;
}

struct HookSetup {
    rsf_observer11_probe_route routes[2];
    ModuleImage image;
};

int32_t try_present_detour(void* user, uint32_t method) noexcept try
{
    auto& setup = *static_cast<HookSetup*>(user);
    const auto& route = setup.routes[method];
    auto** table = route.table;
    const auto slot = route.index;
    auto* hook = route.hook;
    const auto* label = method == 0 ? "Present" : "Present1";
    char why[512]{};
    auto* genuine = genuine_slot(setup.image, table, slot, table[slot], why, sizeof(why));
    if (genuine && install_present_route(setup.image, genuine, hook, method, label, why, sizeof(why))) {
        return 1;
    }
    if (rsf_core_observer11_logging()) {
        char note[768]{};
        if (method == 0) {
            std::snprintf(note, sizeof(note), "observer: Present detour unavailable because %s. "
                "The table fallback refuses re-entry when the genuine route is unavailable", why);
        } else {
            std::snprintf(note, sizeof(note), "observer: Present1 hook unavailable: %s", why);
        }
        rsf_core_observer11_log(note);
    }
    return 0;
} catch (...) { return 0; }

int32_t remove_present_detour(void* target) noexcept
{
    const auto disabled = MH_DisableHook(target);
    if (disabled == MH_ERROR_NOT_CREATED) return 1;
    if (disabled != MH_OK && disabled != MH_ERROR_DISABLED) return 0;
    const auto removed = MH_RemoveHook(target);
    return removed == MH_OK || removed == MH_ERROR_NOT_CREATED;
}

constexpr rsf_observer11_hook_ops hook_ops{
    [](void** address) { return *reinterpret_cast<void* volatile*>(address); },
    rsf_graphics_replace_vtable_slot,
    try_present_detour,
    remove_present_detour,
};

// The Rust probe owns the temporary window and COM references.
static_assert(D3D_DRIVER_TYPE_HARDWARE == 1 && D3D_DRIVER_TYPE_WARP == 5 &&
    D3D_FEATURE_LEVEL_11_0 == 0xb000 && DXGI_FORMAT_R8G8B8A8_UNORM == 28 &&
    DXGI_USAGE_RENDER_TARGET_OUTPUT == 0x20);
static_assert(sizeof(rsf_observer11_probe_attempt) == 32 &&
    offsetof(rsf_observer11_probe_attempt, result) == 24 && sizeof(rsf_observer11_probe_route) == 24);

} // namespace

extern "C" void rsf_observer11_guard_probe_create(uint32_t driver, uint32_t feature_level,
    const DXGI_SWAP_CHAIN_DESC* desc, rsf_observer11_probe_attempt* out) noexcept
{
    IDXGISwapChain* chain{};
    ID3D11Device* device{};
    ID3D11DeviceContext* context{};
    const auto wanted = static_cast<D3D_FEATURE_LEVEL>(feature_level);
    D3D_FEATURE_LEVEL obtained{};
    HRESULT result = E_FAIL;
    uint32_t returned = 0;
    try {
        result = D3D11CreateDeviceAndSwapChain(nullptr, static_cast<D3D_DRIVER_TYPE>(driver), nullptr, 0,
            &wanted, 1, D3D11_SDK_VERSION, desc, &chain, &device, &obtained, &context);
        returned = 1;
    } catch (...) {}
    *out = {context, chain, device, result, returned};
}

extern "C" uint32_t rsf_observer11_probe_chain1(void* chain, void** extended, int32_t* result) noexcept try
{
    *result = static_cast<IDXGISwapChain*>(chain)->QueryInterface(__uuidof(IDXGISwapChain1), extended);
    return 1;
} catch (...) { return 0; }

namespace {

int32_t probe_hooks(void* user, rsf_observer11_slot* slots, uint32_t* has_present1) noexcept try
{
    auto& setup = *static_cast<HookSetup*>(user);
    if (!rsf_core_observer11_probe(slots, setup.routes)) {
        return RSF_OBSERVER_ERROR_NO_DEVICE;
    }
    *has_present1 = setup.routes[1].table != nullptr;
    return RSF_OBSERVER_OK;
} catch (...) { return RSF_OBSERVER_ERROR_NO_DEVICE; }

} // namespace

extern "C" rsf_observer_result rsf_observer_install(const rsf_observer_options* options)
{
    // Both native routes can reuse the module file borrowed for this probe.
    HookSetup setup{};
    return rsf_core_observer11_install(options, &callbacks, &setup, probe_hooks, &hook_ops);
}

extern "C" rsf_observer_result rsf_observer_uninstall(void)
{
    return rsf_core_observer11_uninstall_hooks(&hook_ops);
}
