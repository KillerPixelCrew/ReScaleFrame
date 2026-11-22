// SPDX-License-Identifier: MIT
// Native MinHook and guarded Windows loader-list access for the census binding owner.
#include "MinHook.h"
#include "census_internal.h"

namespace rsf_census {
namespace {

struct LoaderData {
    uint32_t length;
    uint8_t initialized;
    uint8_t pad[3];
    void* ss_handle;
    LIST_ENTRY in_load_order;
};

struct LoaderEntry {
    LIST_ENTRY in_load_order;
    LIST_ENTRY in_memory_order;
    LIST_ENTRY in_init_order;
    void* dll_base;
    void* entry_point;
    uint32_t size_of_image;
    uint32_t pad;
    uint16_t full_length, full_max;
    wchar_t* full_buffer;
};
static_assert(offsetof(LoaderData, in_load_order) == 0x10);
static_assert(offsetof(LoaderEntry, dll_base) == 0x30);
static_assert(offsetof(LoaderEntry, full_buffer) == 0x50);

void scan_modules(rsf_census_visit_module visit) noexcept
{
    auto* teb = reinterpret_cast<uint8_t*>(NtCurrentTeb());
    uint8_t* peb = nullptr;
    LoaderData* loader = nullptr;
    if (!rd(teb + 0x60, peb) || !peb || !rd(peb + 0x18, loader) || !loader) return;
    auto* head = reinterpret_cast<LIST_ENTRY*>(
        reinterpret_cast<uint8_t*>(loader) + offsetof(LoaderData, in_load_order));
    LIST_ENTRY link{};
    if (!rd(head, link)) return;
    auto* current = link.Flink;
    // The loader lock is already held under DllMain. Outside DllMain, guard the reads
    // and bound the walk in case another thread changes the list.
    for (size_t i = 0; current && current != head && i < 16384; ++i) {
        LoaderEntry entry{};
        if (!rd(current, entry)) break;
        current = entry.in_load_order.Flink;
        if (!entry.dll_base || !entry.full_buffer || entry.full_length % sizeof(wchar_t)) continue;
        const size_t length = entry.full_length / sizeof(wchar_t);
        uint16_t path[MAX_PATH];
        if (length >= MAX_PATH || !safe_read(entry.full_buffer, path, entry.full_length)) continue;
        visit(entry.dll_base, path, length);
    }
}

} // namespace

bool hooks_install() noexcept {
    if (MH_Initialize() != MH_OK) return false;
    static_assert(kFnCount < 1024);
    static void* wrappers[kFnCount]{};
    register_wrappers_d3d(wrappers);
    register_wrappers_vk(wrappers);
    register_wrappers_ngx(wrappers);
    register_wrappers_fsr(wrappers);
    register_wrappers_misc(wrappers);
    const rsf_census_hook_platform platform = {
        kFns, wrappers, static_cast<uint16_t>(kFnCount),
        {static_cast<uint16_t>(Fn::CreateDXGIFactory), static_cast<uint16_t>(Fn::CreateDXGIFactory1),
         static_cast<uint16_t>(Fn::CreateDXGIFactory2), static_cast<uint16_t>(Fn::D3D12CreateDevice)},
        [](const uint16_t* module, const char* name, void* detour, void** original) -> int {
            return MH_CreateHookApi(reinterpret_cast<const wchar_t*>(module), name, detour, original);
        },
        [](void* target, void* detour, void** original) -> int {
            return MH_CreateHook(target, detour, original);
        },
        [](void* target) { MH_RemoveHook(target); },
        [](void* target) -> int { return MH_QueueEnableHook(target); },
        []() { MH_ApplyQueued(); },
        []() { MH_DisableHook(MH_ALL_HOOKS); },
        scan_modules, safe_read, read_arg_string, installed,
    };
    return rsf_census_hooks_install(&platform, option_flags());
}

void hooks_remove() noexcept { rsf_census_hooks_remove(); }
void hooks_apply() noexcept { rsf_census_hooks_apply(); }
void* bind_thunk(Fn fn, void* real) noexcept {
    return rsf_census_hook_bind_thunk(static_cast<uint16_t>(fn), real);
}
bool bind_inline(Fn fn, void* target, const char* module) noexcept {
    return rsf_census_hook_bind_inline(static_cast<uint16_t>(fn), target, module);
}
Fn lookup_name(const char* name) noexcept {
    const uint16_t found = rsf_census_hook_lookup(name);
    return found < kFnCount ? static_cast<Fn>(found) : Fn::Count;
}
void hook_counts(uint32_t& installed_count, uint32_t& conflicts, uint32_t& modules) noexcept {
    rsf_census_hook_counts(&installed_count, &conflicts, &modules);
}
void hooks_summary(Json& json) noexcept { rsf_census_hooks_summary(json.state()); }

} // namespace rsf_census
