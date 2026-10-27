// SPDX-License-Identifier: GPL-3.0-only
// Hook layer: GetProcAddress and LoadLibrary interception, vendor module export detours, and the
// bookkeeping that binds a wrapper to the function it forwards to.
#include <cwchar>

#include "MinHook.h"
#include "census_internal.h"

namespace rsf_census {

namespace {

constexpr uint32_t kNameSlots = 1024;
constexpr uint32_t kSeenSlots = 1024;
constexpr uint32_t kHookRecords = 384;

struct HookRec {
    Fn fn;
    uint8_t kind;  // 0 thunk, 1 detour
    int8_t status; // MH_STATUS for a detour, 0 for a thunk
    bool captured; // `patched` holds the entry bytes as they were right after the detour went in
    uint8_t patched[5];
    void* target;
    char module[40];
};

struct State {
    FARPROC(WINAPI* gpa)(HMODULE, LPCSTR) = nullptr;
    FARPROC(WINAPI* gpa_caller)(HMODULE, LPCSTR, void*) = nullptr;
    HMODULE(WINAPI* load_w)(LPCWSTR) = nullptr;
    HMODULE(WINAPI* load_a)(LPCSTR) = nullptr;
    HMODULE(WINAPI* load_ex_w)(LPCWSTR, HANDLE, DWORD) = nullptr;
    HMODULE(WINAPI* load_ex_a)(LPCSTR, HANDLE, DWORD) = nullptr;

    Fn names[kNameSlots];
    uint16_t name_used[kNameSlots]{};

    std::atomic<uint64_t> seen[kSeenSlots]{};
    HookRec recs[kHookRecords];
    std::atomic<uint32_t> rec_count{0};
    std::atomic<uint32_t> installed{0};
    std::atomic<uint32_t> conflicts{0};
    std::atomic<uint32_t> modules{0};
    char hooked_modules[24][40]{};
    bool ready = false;
};

State s;
thread_local int tl_busy = 0;

struct Busy {
    Busy() noexcept { ++tl_busy; }
    ~Busy() noexcept { --tl_busy; }
};

uint32_t hash_name(const char* n) noexcept {
    uint32_t h = 2166136261u;
    for (; *n; ++n) h = (h ^ static_cast<unsigned char>(*n)) * 16777619u;
    return h;
}

void name_table_build() noexcept {
    for (size_t f = 0; f < kFnCount; ++f) {
        const char* n = kFns[f].name;
        if (!n) continue;
        uint32_t i = hash_name(n) & (kNameSlots - 1);
        while (s.name_used[i]) i = (i + 1) & (kNameSlots - 1);
        s.names[i] = static_cast<Fn>(f);
        s.name_used[i] = 1;
    }
}

bool fn_enabled(Fn f) noexcept {
    const char* api = kFns[idx(f)].api;
    const uint32_t flags = option_flags();
    if ((flags & RSF_CENSUS_FLAG_D3D12_DEBUG_ONLY) && f != Fn::D3D12CreateDevice) return false;
    if ((flags & RSF_CENSUS_FLAG_NO_DXGI) && (!std::strcmp(api, "dxgi") || !std::strcmp(api, "d3d12"))) return false;
    if ((flags & RSF_CENSUS_FLAG_NO_VULKAN) && !std::strcmp(api, "vulkan")) return false;
    return true;
}

void add_rec(Fn fn, uint8_t kind, int status, const char* module, void* target = nullptr) noexcept {
    uint32_t i = s.rec_count.fetch_add(1);
    if (i >= kHookRecords) return;
    HookRec& r = s.recs[i];
    r.fn = fn;
    r.kind = kind;
    r.status = static_cast<int8_t>(status);
    r.captured = false;
    r.target = target;
    std::snprintf(r.module, sizeof(r.module), "%s", module ? module : "");
}

bool module_in_list(const char* base, const char* list) noexcept {
    const size_t bl = std::strlen(base);
    while (*list) {
        const char* e = std::strchr(list, '|');
        const size_t n = e ? static_cast<size_t>(e - list) : std::strlen(list);
        if (n == bl && !_strnicmp(base, list, n)) return true;
        if (!e) break;
        list = e + 1;
    }
    return false;
}

bool seen_before(uint64_t base) noexcept {
    uint32_t i = static_cast<uint32_t>((base >> 12) * 2654435761u >> 8) & (kSeenSlots - 1);
    for (uint32_t p = 0; p < kSeenSlots; ++p, i = (i + 1) & (kSeenSlots - 1)) {
        uint64_t cur = s.seen[i].load(std::memory_order_acquire);
        if (cur == base) return true;
        if (cur == 0) {
            uint64_t expected = 0;
            if (s.seen[i].compare_exchange_strong(expected, base)) return false;
            if (expected == base) return true;
        }
    }
    return true;
}

// Detour every known export of `h` that belongs to a vendor module with this basename.
void hook_module(HMODULE h, const char* basename) noexcept {
    bool any = false;
    for (size_t f = 0; f < kFnCount; ++f) {
        const FnDesc& d = kFns[f];
        if (!d.name || !d.modules[0] || !module_in_list(basename, d.modules)) continue;
        if (!fn_enabled(static_cast<Fn>(f))) continue;
        void* real = reinterpret_cast<void*>(s.gpa(h, d.name));
        if (!real) continue;
        if (bind_inline(static_cast<Fn>(f), real, basename)) any = true;
    }
    if (any) {
        hooks_apply();
        uint32_t n = s.modules.fetch_add(1);
        if (n < 24) std::snprintf(s.hooked_modules[n], sizeof(s.hooked_modules[n]), "%s", basename);
    }
}

void lower_basename(const wchar_t* path, char* out, size_t cap) noexcept {
    const wchar_t* slash = std::wcsrchr(path, L'\\');
    const wchar_t* b = slash ? slash + 1 : path;
    size_t i = 0;
    for (; b[i] && i + 1 < cap; ++i) {
        wchar_t c = b[i];
        out[i] = c < 128 ? static_cast<char>(c >= L'A' && c <= L'Z' ? c + 32 : c) : '?';
    }
    out[i] = 0;
}

void post_load(HMODULE h, DWORD flags) noexcept {
    if (!h || (reinterpret_cast<uintptr_t>(h) & 3) || (flags & (LOAD_LIBRARY_AS_DATAFILE | 0x20 | 0x40))) return;
    if (tl_busy || !installed()) return;
    if (seen_before(reinterpret_cast<uint64_t>(h))) return;
    Busy busy;
    DWORD le = GetLastError();
    wchar_t path[MAX_PATH];
    DWORD n = GetModuleFileNameW(h, path, MAX_PATH);
    if (n && n < MAX_PATH) {
        char base[48];
        lower_basename(path, base, sizeof(base));
        hook_module(h, base);
    }
    SetLastError(le);
}

// Returns the pointer to hand back for `real`, the address `name` resolved to.
FARPROC intercept(FARPROC real, LPCSTR name) noexcept {
    if (!real || reinterpret_cast<uintptr_t>(name) <= 0xFFFF || tl_busy) return real;
    const Fn f = lookup_name(name);
    if (f == Fn::Count || !installed() || !fn_enabled(f)) return real;
    Busy busy;
    DWORD le = GetLastError();
    void* r = bind_thunk(f, reinterpret_cast<void*>(real));
    SetLastError(le);
    return reinterpret_cast<FARPROC>(r);
}

FARPROC WINAPI hk_GetProcAddress(HMODULE h, LPCSTR name) noexcept { return intercept(s.gpa(h, name), name); }

// kernel32!GetProcAddress is a stub that passes its caller to kernelbase!GetProcAddressForCaller,
// so a game importing it from kernel32 never reaches kernelbase!GetProcAddress.
FARPROC WINAPI hk_GetProcAddressForCaller(HMODULE h, LPCSTR name, void* caller) noexcept {
    return intercept(s.gpa_caller(h, name, caller), name);
}

HMODULE WINAPI hk_LoadLibraryW(LPCWSTR n) noexcept {
    HMODULE h = s.load_w(n);
    post_load(h, 0);
    return h;
}
HMODULE WINAPI hk_LoadLibraryA(LPCSTR n) noexcept {
    HMODULE h = s.load_a(n);
    post_load(h, 0);
    return h;
}
HMODULE WINAPI hk_LoadLibraryExW(LPCWSTR n, HANDLE f, DWORD fl) noexcept {
    HMODULE h = s.load_ex_w(n, f, fl);
    post_load(h, fl);
    return h;
}
HMODULE WINAPI hk_LoadLibraryExA(LPCSTR n, HANDLE f, DWORD fl) noexcept {
    HMODULE h = s.load_ex_a(n, f, fl);
    post_load(h, fl);
    return h;
}

template <class P>
bool hook_api(const wchar_t* module, const char* proc, void* detour, P& orig) noexcept {
    void* o = nullptr;
    MH_STATUS st = MH_CreateHookApi(module, proc, detour, &o);
    if (st != MH_OK) return false;
    orig = reinterpret_cast<P>(o);
    MH_QueueEnableHook(MH_ALL_HOOKS);
    return true;
}

// The loader's module list, declared here so the walk does not depend on winternl.h. Offsets are
// those of the x64 PEB_LDR_DATA and LDR_DATA_TABLE_ENTRY.
struct LdrData {
    uint32_t length;
    uint8_t initialized;
    uint8_t pad[3];
    void* ss_handle;
    LIST_ENTRY in_load_order;
};
struct LdrEntry {
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
static_assert(offsetof(LdrData, in_load_order) == 0x10 && offsetof(LdrEntry, dll_base) == 0x30);
static_assert(offsetof(LdrEntry, full_buffer) == 0x48 + 8);

// Modules already mapped when the census installs: walk the loader list without taking its lock.
// Install runs under the loader lock when called from DllMain, so the list cannot change.
void scan_loaded_modules() noexcept {
    auto* teb = reinterpret_cast<uint8_t*>(NtCurrentTeb());
    auto* peb = *reinterpret_cast<uint8_t**>(teb + 0x60);
    auto* ldr = peb ? *reinterpret_cast<LdrData**>(peb + 0x18) : nullptr;
    if (!ldr) return;
    LIST_ENTRY* head = &ldr->in_load_order;
    for (LIST_ENTRY* e = head->Flink; e && e != head; e = e->Flink) {
        auto* ent = CONTAINING_RECORD(e, LdrEntry, in_load_order);
        if (!ent->dll_base || !ent->full_buffer) continue;
        wchar_t path[MAX_PATH];
        size_t n = ent->full_length / sizeof(wchar_t);
        if (n >= MAX_PATH) continue;
        std::wmemcpy(path, ent->full_buffer, n);
        path[n] = 0;
        char base[48];
        lower_basename(path, base, sizeof(base));
        seen_before(reinterpret_cast<uint64_t>(ent->dll_base));
        hook_module(static_cast<HMODULE>(ent->dll_base), base);
    }
}

} // namespace

Fn lookup_name(const char* name) noexcept {
    uint32_t i = hash_name(name) & (kNameSlots - 1);
    for (uint32_t p = 0; p < kNameSlots && s.name_used[i]; ++p, i = (i + 1) & (kNameSlots - 1)) {
        const char* n = kFns[idx(s.names[i])].name;
        if (!std::strcmp(n, name)) return s.names[i];
    }
    return Fn::Count;
}

void* bind_thunk(Fn fn, void* real) noexcept {
    void* wrapper = g_wrapper[idx(fn)];
    if (!wrapper) return real;
    // A function detoured at its entry is already recorded; its pointer must not be wrapped again.
    if (g_target[idx(fn)].load(std::memory_order_acquire) == real) return real;
    void* expected = nullptr;
    if (g_orig[idx(fn)].compare_exchange_strong(expected, real)) {
        s.installed.fetch_add(1);
        add_rec(fn, 0, 0, "");
        return wrapper;
    }
    if (expected == real) return wrapper;
    s.conflicts.fetch_add(1);
    return real;
}

bool bind_inline(Fn fn, void* target, const char* why) noexcept {
    void* wrapper = g_wrapper[idx(fn)];
    if (!wrapper || !target || !fn_enabled(fn)) return false;
    if (g_orig[idx(fn)].load(std::memory_order_acquire) != nullptr) {
        if (g_target[idx(fn)].load() != target) s.conflicts.fetch_add(1);
        return false;
    }
    void* tramp = nullptr;
    MH_STATUS st = MH_CreateHook(target, wrapper, &tramp);
    if (st != MH_OK) {
        add_rec(fn, 1, st, why);
        return false;
    }
    void* expected = nullptr;
    if (!g_orig[idx(fn)].compare_exchange_strong(expected, tramp)) {
        MH_RemoveHook(target);
        return false;
    }
    g_target[idx(fn)].store(target, std::memory_order_release);
    st = MH_QueueEnableHook(target);
    add_rec(fn, 1, st, why, target);
    if (st == MH_OK) s.installed.fetch_add(1);
    return st == MH_OK;
}

// Remember how each enabled detour's entry looks so a later overwrite by an overlay can be reported.
void capture_patched() noexcept {
    const uint32_t n = s.rec_count.load();
    for (uint32_t i = 0; i < n && i < kHookRecords; ++i) {
        HookRec& r = s.recs[i];
        if (r.kind == 1 && r.status == 0 && !r.captured && r.target && safe_read(r.target, r.patched, sizeof(r.patched))) {
            r.captured = true;
        }
    }
}

void hooks_apply() noexcept {
    MH_ApplyQueued();
    capture_patched();
}

bool hooks_install() noexcept {
    if (MH_Initialize() != MH_OK) return false;
    register_wrappers_d3d(g_wrapper);
    register_wrappers_vk(g_wrapper);
    register_wrappers_ngx(g_wrapper);
    register_wrappers_fsr(g_wrapper);
    register_wrappers_misc(g_wrapper);
    name_table_build();

    const wchar_t* mod = GetModuleHandleW(L"kernelbase.dll") ? L"kernelbase.dll" : L"kernel32.dll";
    const uint32_t flags = option_flags();
    // The module scan resolves exports through s.gpa, so it needs the real function even when the
    // detour is left out.
    s.gpa = &GetProcAddress;
    if (!(flags & RSF_CENSUS_FLAG_NO_GETPROCADDRESS_HOOK)) {
        if (!hook_api(mod, "GetProcAddress", reinterpret_cast<void*>(hk_GetProcAddress), s.gpa)) return false;
        hook_api(mod, "GetProcAddressForCaller", reinterpret_cast<void*>(hk_GetProcAddressForCaller), s.gpa_caller);
    }
    if (!(flags & RSF_CENSUS_FLAG_NO_LOADLIBRARY_HOOK)) {
        hook_api(mod, "LoadLibraryW", reinterpret_cast<void*>(hk_LoadLibraryW), s.load_w);
        hook_api(mod, "LoadLibraryA", reinterpret_cast<void*>(hk_LoadLibraryA), s.load_a);
        hook_api(mod, "LoadLibraryExW", reinterpret_cast<void*>(hk_LoadLibraryExW), s.load_ex_w);
        hook_api(mod, "LoadLibraryExA", reinterpret_cast<void*>(hk_LoadLibraryExA), s.load_ex_a);
    }
    // The queue holds exactly the detours above, so enabling all of it applies only them.
    MH_ApplyQueued();
    s.ready = true;
    scan_loaded_modules();
    return true;
}

void hooks_remove() noexcept {
    // The trampolines stay mapped: a thread may still be running inside one.
    MH_DisableHook(MH_ALL_HOOKS);
}

void hook_counts(uint32_t& installed, uint32_t& conflicts, uint32_t& modules) noexcept {
    installed = s.installed.load();
    conflicts = s.conflicts.load();
    modules = s.modules.load();
}

void hooks_summary(Json& j) noexcept {
    j.arr("hooks");
    uint32_t n = s.rec_count.load();
    for (uint32_t i = 0; i < n && i < kHookRecords; ++i) {
        const HookRec& r = s.recs[i];
        j.obj();
        j.str("function", kFns[idx(r.fn)].name ? kFns[idx(r.fn)].name : kFns[idx(r.fn)].id);
        j.str("api", kFns[idx(r.fn)].api);
        j.str("binding", r.kind ? "detour" : "getprocaddress_thunk");
        if (r.module[0]) j.str("module", r.module);
        if (r.kind) j.i64("minhook_status", r.status);
        if (r.kind && r.captured) {
            uint8_t now[5];
            const bool readable = safe_read(r.target, now, sizeof(now));
            j.boolean("entry_modified", !readable || std::memcmp(now, r.patched, sizeof(now)) != 0);
        }
        j.end_obj();
    }
    j.end_arr();
    j.u64("hook_conflicts", s.conflicts.load());
    j.arr("modules_hooked");
    uint32_t m = s.modules.load();
    for (uint32_t i = 0; i < m && i < 24; ++i) j.str(nullptr, s.hooked_modules[i]);
    j.end_arr();
}

} // namespace rsf_census
