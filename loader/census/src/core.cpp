// SPDX-License-Identifier: MIT
// Census core: the record queue, the site and function statistics, the writer thread and the C API.
#include <intrin.h>

#include <cmath>
#include <cwchar>
#include <iterator>
#include <new>

#include "census_internal.h"
#include "wrapper_platform.h"
#include "stats_platform.h"

extern "C" void rsf_census_static_scan(void (*add)(const char*, const char*, uint64_t, uint64_t));

namespace rsf_census {

const FnDesc kFns[kFnCount] = {
#define X(id, api, name, mods) {#id, api, name, mods},
    RSF_CENSUS_FUNCTIONS(X)
#undef X
};

void register_wrappers_vk(void** table) noexcept {
    static constexpr Fn entries[] = {
#define RSF_CENSUS_VK_MAP(name) Fn::name,
        RSF_CENSUS_VK_ENTRIES(RSF_CENSUS_VK_MAP)
#undef RSF_CENSUS_VK_MAP
    };
    static_assert(std::size(entries) == RSF_CENSUS_VK_COUNT);
    static_assert(sizeof(Call) <= sizeof(rsf_census_call_storage));
    static_assert(alignof(Call) <= alignof(rsf_census_call_storage));
    static const rsf_census_wrapper_services services = {
        [](uint16_t entry) { return orig_by(entries[entry]); },
        [](rsf_census_call_storage* storage, uint16_t entry, void* caller) -> rsf_census_json* {
            auto* call = ::new (static_cast<void*>(storage)) Call(entries[entry], caller);
            return call->full() ? call->json().state() : nullptr;
        },
        [](rsf_census_call_storage* storage, uint64_t result) {
            auto* call = std::launder(reinterpret_cast<Call*>(storage));
            call->finish(result);
            call->~Call();
        },
        safe_read,
        read_arg_string,
        [](const char* name, void* target) {
            const Fn fn = lookup_name(name);
            return fn != Fn::Count && !std::strcmp(kFns[idx(fn)].api, "vulkan") ?
                bind_thunk(fn, target) : target;
        },
        [](void* queue, uint32_t family, uint32_t index) {
            queue_describe(QueueKind::Vulkan, queue, family, index, 0);
        },
        [](void* queue, uint32_t count) { queue_submit(QueueKind::Vulkan, queue, count); },
        []() { if (tl_present_depth++ == 0) note_present_begin(); },
        []() { --tl_present_depth; },
    };
    void* wrappers[RSF_CENSUS_VK_COUNT]{};
    rsf_census_vk_bind(&services, wrappers);
    for (size_t i = 0; i < std::size(entries); ++i) table[idx(entries[i])] = wrappers[i];
}

namespace {

constexpr uint32_t kModuleCapacity = 512;
constexpr uint32_t kThreadCapacity = 256;
constexpr uint32_t kFindingCapacity = 192;
constexpr size_t kSummaryCapacity = 4u << 20;
constexpr size_t kFileBuffer = 256u << 10;

struct ModuleEntry {
    std::atomic<uint64_t> base{0};
    char name[72]{};
    std::atomic<bool> named{false};
};

struct ThreadEntry {
    std::atomic<uint32_t> tid{0};
    std::atomic<uint64_t> calls{0};
    char name[40]{};
};

struct Finding {
    char vendor[24];
    char what[72];
    uint64_t rva;
    uint64_t function_rva;
};

struct State {
    std::atomic<bool> active{false};
    std::atomic<bool> ever_installed{false};
    std::atomic<bool> stop{false};
    uint32_t flags = 0;
    uint64_t budget = 0;
    uint32_t cap = 32;
    uint32_t interval = 600;
    uint32_t period_ms = 2000;
    wchar_t dir[MAX_PATH]{};
    wchar_t exe[MAX_PATH]{};

    LARGE_INTEGER qpc_freq{};
    LARGE_INTEGER qpc_start{};
    ModuleEntry modules[kModuleCapacity];
    std::atomic<uint32_t> module_count{0};
    ThreadEntry threads[kThreadCapacity];

    Finding findings[kFindingCapacity];
    std::atomic<uint32_t> finding_count{0};

    std::atomic<uint64_t> queued{0};
    std::atomic<uint64_t> written{0};
    std::atomic<uint64_t> dropped_queue{0};
    std::atomic<uint64_t> dropped_budget{0};
    std::atomic<uint64_t> dropped_size{0};
    std::atomic<uint64_t> bytes_written{0};
    std::atomic<bool> budget_exhausted{false};

    SRWLOCK io = SRWLOCK_INIT;
    HANDLE file = INVALID_HANDLE_VALUE;
    char* file_buf = nullptr;
    size_t file_len = 0;
    char* summary_buf = nullptr;
    uint64_t last_summary_us = 0;
    uint64_t last_write_us = 0;

    HANDLE thread = nullptr;
    HANDLE scan_thread = nullptr;
    HANDLE wake = nullptr;
    PVOID (NTAPI* pc_to_header)(PVOID, PVOID*) = nullptr;
};

State g;

uint64_t now_us() noexcept {
    LARGE_INTEGER t;
    QueryPerformanceCounter(&t);
    return static_cast<uint64_t>((t.QuadPart - g.qpc_start.QuadPart) * 1000000ll / g.qpc_freq.QuadPart);
}

// ---- Module registry --------------------------------------------------------------------------
uint32_t module_index(uint64_t base) noexcept {
    uint32_t n = g.module_count.load(std::memory_order_acquire);
    for (uint32_t i = 0; i < n; ++i) {
        if (g.modules[i].base.load(std::memory_order_acquire) == base) return i;
    }
    for (;;) {
        n = g.module_count.load(std::memory_order_acquire);
        if (n >= kModuleCapacity) return 0;
        if (g.module_count.compare_exchange_weak(n, n + 1)) {
            g.modules[n].base.store(base, std::memory_order_release);
            return n;
        }
    }
}

void resolve_module_name(uint32_t i) noexcept {
    ModuleEntry& m = g.modules[i];
    if (m.named.load(std::memory_order_acquire)) return;
    wchar_t path[MAX_PATH];
    const uint64_t base = m.base.load();
    DWORD n = base ? GetModuleFileNameW(reinterpret_cast<HMODULE>(base), path, MAX_PATH) : 0;
    if (n == 0 || n >= MAX_PATH) {
        std::snprintf(m.name, sizeof(m.name), base ? "module@0x%llx" : "unknown",
                      static_cast<unsigned long long>(base));
    } else {
        const wchar_t* slash = std::wcsrchr(path, L'\\');
        WideCharToMultiByte(CP_UTF8, 0, slash ? slash + 1 : path, -1, m.name,
                            static_cast<int>(sizeof(m.name)), nullptr, nullptr);
        m.name[sizeof(m.name) - 1] = 0;
    }
    m.named.store(true, std::memory_order_release);
}

// ---- Sites and thread table -------------------------------------------------------------------
void note_thread(uint32_t tid) noexcept {
    uint32_t i = (tid * 2654435761u >> 8) & (kThreadCapacity - 1);
    for (uint32_t probe = 0; probe < kThreadCapacity; ++probe, i = (i + 1) & (kThreadCapacity - 1)) {
        ThreadEntry& t = g.threads[i];
        uint32_t cur = t.tid.load(std::memory_order_acquire);
        if (cur == tid) {
            t.calls.fetch_add(1, std::memory_order_relaxed);
            return;
        }
        if (cur == 0) {
            uint32_t expected = 0;
            if (t.tid.compare_exchange_strong(expected, tid) || expected == tid) {
                t.calls.fetch_add(1, std::memory_order_relaxed);
                return;
            }
        }
    }
}

struct ThreadName {
    uint32_t tid = 0;
    uint64_t stamp = 0;
    char name[40]{};
};
thread_local ThreadName tl_thread;

const char* thread_name(uint64_t now) noexcept {
    ThreadName& t = tl_thread;
    if (t.tid == 0 || now - t.stamp > 1000000) {
        t.tid = GetCurrentThreadId();
        t.stamp = now ? now : 1;
        PWSTR desc = nullptr;
        t.name[0] = 0;
        if (SUCCEEDED(GetThreadDescription(GetCurrentThread(), &desc)) && desc) {
            WideCharToMultiByte(CP_UTF8, 0, desc, -1, t.name, static_cast<int>(sizeof(t.name)), nullptr,
                                nullptr);
            t.name[sizeof(t.name) - 1] = 0;
            LocalFree(desc);
        }
        // Remember the latest description for the summary.
        uint32_t i = (t.tid * 2654435761u >> 8) & (kThreadCapacity - 1);
        for (uint32_t probe = 0; probe < kThreadCapacity; ++probe, i = (i + 1) & (kThreadCapacity - 1)) {
            if (g.threads[i].tid.load(std::memory_order_acquire) == t.tid) {
                std::memcpy(g.threads[i].name, t.name, sizeof(t.name));
                break;
            }
        }
    }
    return t.name;
}

// ---- Writer -----------------------------------------------------------------------------------
void file_flush_locked() noexcept {
    if (g.file == INVALID_HANDLE_VALUE || g.file_len == 0) return;
    DWORD done = 0;
    WriteFile(g.file, g.file_buf, static_cast<DWORD>(g.file_len), &done, nullptr);
    g.file_len = 0;
    g.last_write_us = now_us();
}

void file_append_locked(const char* data, size_t n) noexcept {
    if (g.bytes_written.load() + n > g.budget) {
        g.dropped_budget.fetch_add(1, std::memory_order_relaxed);
        g.budget_exhausted.store(true, std::memory_order_relaxed);
        return;
    }
    if (g.file_len + n > kFileBuffer) file_flush_locked();
    std::memcpy(g.file_buf + g.file_len, data, n);
    g.file_len += n;
    g.bytes_written.fetch_add(n, std::memory_order_relaxed);
    g.written.fetch_add(1, std::memory_order_relaxed);
}

void drain_locked() noexcept {
    static const rsf_census_record_writer writer = {
        kFns, kFnCount,
        [](uint64_t base) -> const char* {
            const uint32_t module = module_index(base);
            resolve_module_name(module);
            return g.modules[module].name;
        },
        [](void* address) -> uint64_t {
            PVOID base = nullptr;
            if (g.pc_to_header) g.pc_to_header(address, &base);
            return reinterpret_cast<uint64_t>(base);
        },
        file_append_locked,
        []() { g.dropped_size.fetch_add(1, std::memory_order_relaxed); },
    };
    rsf_census_record_drain(&writer);
}

void write_summary_locked() noexcept {
    if (!g.summary_buf) return;
    Json j(g.summary_buf, kSummaryCapacity - 16);
    j.obj();
    j.u64("census_abi", RSF_CENSUS_ABI_VERSION);
    char exe[MAX_PATH * 3];
    WideCharToMultiByte(CP_UTF8, 0, g.exe, -1, exe, sizeof(exe), nullptr, nullptr);
    const char* slash = std::strrchr(exe, '\\');
    j.str("process", slash ? slash + 1 : exe);
    j.u64("pid", GetCurrentProcessId());
    j.u64("elapsed_ms", now_us() / 1000);
    uint64_t calls_seen = 0, present = 0;
    uint32_t sites = 0, site_overflow = 0;
    rsf_census_stats_counts(&calls_seen, &present, &sites, &site_overflow);
    j.u64("present_index", present);
    j.obj("budget");
    j.u64("byte_budget", g.budget);
    j.u64("bytes_written", g.bytes_written.load());
    j.boolean("exhausted", g.budget_exhausted.load());
    j.u64("full_record_cap", g.cap);
    j.u64("sample_interval", g.interval);
    j.end_obj();
    j.obj("counters");
    j.u64("calls_seen", calls_seen);
    j.u64("records_queued", g.queued.load());
    j.u64("records_written", g.written.load());
    j.u64("dropped_queue", g.dropped_queue.load());
    j.u64("dropped_budget", g.dropped_budget.load());
    j.u64("dropped_size", g.dropped_size.load());
    j.u64("sites", sites);
    j.u64("site_overflow", site_overflow);
    j.end_obj();

    static const rsf_census_stats_names names = {
        kFns,
        [](uint32_t index) -> const char* {
            resolve_module_name(index);
            return g.modules[index].name;
        },
    };
    rsf_census_stats_sites(j.state(), &names);
    rsf_census_stats_histograms(j.state(), kFns);

    j.arr("threads");
    for (uint32_t i = 0; i < kThreadCapacity; ++i) {
        uint32_t tid = g.threads[i].tid.load(std::memory_order_acquire);
        if (!tid) continue;
        j.obj();
        j.u64("tid", tid);
        j.str("name", g.threads[i].name);
        j.u64("calls", g.threads[i].calls.load());
        j.end_obj();
    }
    j.end_arr();

    rsf_census_stats_queues(j.state());

    hooks_summary(j);

    j.arr("static_findings");
    uint32_t nf = g.finding_count.load();
    for (uint32_t i = 0; i < nf && i < kFindingCapacity; ++i) {
        j.obj();
        j.str("vendor", g.findings[i].vendor);
        j.str("what", g.findings[i].what);
        j.hex("rva", g.findings[i].rva);
        j.hex("function_rva", g.findings[i].function_rva);
        j.end_obj();
    }
    j.end_arr();
    j.end_obj();
    if (!j.ok()) return;

    wchar_t tmp[MAX_PATH + 32], fin[MAX_PATH + 32];
    std::swprintf(tmp, std::size(tmp), L"%ls\\census-summary.json.tmp", g.dir);
    std::swprintf(fin, std::size(fin), L"%ls\\census-summary.json", g.dir);
    HANDLE h = CreateFileW(tmp, GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_ALWAYS,
                           FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) return;
    DWORD done = 0;
    bool ok = WriteFile(h, g.summary_buf, static_cast<DWORD>(j.size()), &done, nullptr) != 0;
    CloseHandle(h);
    if (ok) MoveFileExW(tmp, fin, MOVEFILE_REPLACE_EXISTING);
    g.last_summary_us = now_us();
}

void flush_all_locked(bool summary) noexcept {
    drain_locked();
    file_flush_locked();
    if (summary) write_summary_locked();
}

// The static scan reads the whole main image, which takes seconds for a large game, so it runs on
// its own thread and never delays the writer.
DWORD WINAPI scan_main(void*) noexcept {
    const uint64_t start = now_us();
    rsf_census_static_scan([](const char* vendor, const char* what, uint64_t rva, uint64_t function) {
        add_finding(vendor, what, rva, function);
    });
    Call c(Fn::CensusEvent, reinterpret_cast<void*>(&scan_main));
    if (c.full()) {
        c.json().str("event", "static_scan_done");
        c.json().u64("elapsed_ms", (now_us() - start) / 1000);
        c.json().u64("findings", g.finding_count.load());
    }
    c.finish(0);
    return 0;
}

// Exceptions that end the process: RAGE's fatal-error int3, crashpad's dump request, fail-fast and
// stack overflow. First-chance access violations are left alone; games and protectors take them
// routinely. The handler records the exception and drains the queue so the last calls before a
// crash reach the file. It never waits indefinitely for the writer's lock.
LONG CALLBACK fatal_exception(EXCEPTION_POINTERS* info) noexcept {
    const DWORD code = info && info->ExceptionRecord ? info->ExceptionRecord->ExceptionCode : 0;
    if (code != 0x80000003u && code != 0x0517a7edu && code != 0xC0000409u && code != 0xC00000FDu) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    static std::atomic<uint32_t> reported{0};
    if (reported.fetch_add(1) >= 8 || !g.active.load(std::memory_order_acquire)) return EXCEPTION_CONTINUE_SEARCH;
    if (g.thread && GetThreadId(g.thread) == GetCurrentThreadId()) return EXCEPTION_CONTINUE_SEARCH;
    {
        Call c(Fn::CensusEvent, info->ExceptionRecord->ExceptionAddress);
        if (c.full()) {
            c.json().str("event", "fatal_exception");
            c.json().hex("code", code);
            c.json().hex("address", reinterpret_cast<uint64_t>(info->ExceptionRecord->ExceptionAddress));
            c.json().u64("present", rsf_census_stats_present_index());
        }
        c.finish(code);
    }
    for (int attempt = 0; attempt < 200; ++attempt) {
        if (TryAcquireSRWLockExclusive(&g.io)) {
            flush_all_locked(true);
            ReleaseSRWLockExclusive(&g.io);
            break;
        }
        Sleep(10);
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

DWORD WINAPI writer_main(void*) noexcept {
    while (!g.stop.load(std::memory_order_acquire)) {
        WaitForSingleObject(g.wake, 200);
        AcquireSRWLockExclusive(&g.io);
        drain_locked();
        uint64_t now = now_us();
        if (g.file_len && now - g.last_write_us > 250000) file_flush_locked();
        if (now - g.last_summary_us >= static_cast<uint64_t>(g.period_ms) * 1000) {
            flush_all_locked(true);
        }
        ReleaseSRWLockExclusive(&g.io);
    }
    return 0;
}

void make_dirs(wchar_t* path) noexcept {
    for (wchar_t* p = path + 1; *p; ++p) {
        if (*p == L'\\' || *p == L'/') {
            wchar_t c = *p;
            *p = 0;
            CreateDirectoryW(path, nullptr);
            *p = c;
        }
    }
    CreateDirectoryW(path, nullptr);
}

} // namespace

// ---- Json -------------------------------------------------------------------------------------
void Json::f32(const char* key, double v) noexcept {
    if (!std::isfinite(v)) {
        null(key);
        return;
    }
    char text[40];
    std::snprintf(text, sizeof(text), "%.9g", v);
    raw(key, text);
}

// ---- Call -------------------------------------------------------------------------------------
Call::Call(Fn fn, void* return_address) noexcept : fn_(fn), full_(false), json_(buf_, kBodyMax) {
    if (!g.active.load(std::memory_order_acquire)) return;
    PVOID base = nullptr;
    if (g.pc_to_header) g.pc_to_header(return_address, &base);
    base_ = reinterpret_cast<uint64_t>(base);
    rva_ = base ? static_cast<uint32_t>(reinterpret_cast<uintptr_t>(return_address) - base_)
                : static_cast<uint32_t>(reinterpret_cast<uintptr_t>(return_address));
    const uint32_t mi = module_index(base_);
    tid_ = GetCurrentThreadId();
    rsf_census_sample sample{};
    full_ = rsf_census_stats_sample(static_cast<uint16_t>(fn), mi, rva_, tid_, &sample);
    ord_ = sample.ordinal;
    site_ord_ = sample.site_ordinal;
    if (site_ord_) note_thread(tid_);
    if (full_) {
        t_us_ = now_us();
        // Walk from here and keep the frames from the intercepted call's caller outward. The walk
        // uses the exception directory, so frames in code without unwind data end it early.
        void* frames[kStackFrames + 6] = {};
        const USHORT captured = RtlCaptureStackBackTrace(0, static_cast<DWORD>(std::size(frames)), frames, nullptr);
        USHORT first = 0;
        while (first < captured && frames[first] != return_address) ++first;
        if (first == captured) first = 0;
        for (USHORT i = first; i < captured && stack_n_ < kStackFrames; ++i) stack_[stack_n_++] = frames[i];
        json_.obj();
    }
}

void Call::finish(uint64_t ret) noexcept {
    if (!full_) return;
    const DWORD last_error = GetLastError();
    json_.end_obj();
    if (!json_.ok()) {
        g.dropped_size.fetch_add(1, std::memory_order_relaxed);
        SetLastError(last_error);
        return;
    }
    size_t pos;
    rsf_census_queued_record* c = rsf_census_record_acquire(&pos);
    if (!c) {
        g.dropped_queue.fetch_add(1, std::memory_order_relaxed);
        SetLastError(last_error);
        return;
    }
    c->len = static_cast<uint32_t>(json_.size());
    c->fn = static_cast<uint16_t>(idx(fn_));
    c->t_us = t_us_;
    c->present = rsf_census_stats_present_index();
    c->ord = ord_;
    c->site_ord = site_ord_;
    c->base = base_;
    c->rva = rva_;
    c->tid = tid_;
    c->ret = ret;
    c->stack_n = stack_n_;
    std::memcpy(c->stack, stack_, sizeof(stack_));
    std::strncpy(c->thread, thread_name(t_us_), sizeof(c->thread) - 1);
    c->thread[sizeof(c->thread) - 1] = 0;
    std::memcpy(c->body, buf_, c->len);
    rsf_census_record_publish(pos);
    g.queued.fetch_add(1, std::memory_order_relaxed);
    SetLastError(last_error);
}

// ---- Present and queue statistics -------------------------------------------------------------
void note_present_begin() noexcept {
    if (installed()) rsf_census_stats_present();
}

uint64_t present_index() noexcept { return rsf_census_stats_present_index(); }

void queue_describe(QueueKind kind, const void* queue, uint32_t a, uint32_t b, uint32_t c) noexcept {
    if (installed()) rsf_census_stats_queue_describe(static_cast<uint8_t>(kind), queue, a, b, c);
}

bool queue_submit(QueueKind kind, const void* queue, uint32_t lists) noexcept {
    return installed() && rsf_census_stats_queue_submit(static_cast<uint8_t>(kind), queue, lists);
}

void add_finding(const char* vendor, const char* what, uint64_t rva, uint64_t function_rva) noexcept {
    uint32_t i = g.finding_count.fetch_add(1);
    if (i >= kFindingCapacity) return;
    Finding& f = g.findings[i];
    std::snprintf(f.vendor, sizeof(f.vendor), "%s", vendor);
    std::snprintf(f.what, sizeof(f.what), "%s", what);
    f.rva = rva;
    f.function_rva = function_rva;
}

bool installed() noexcept { return g.active.load(std::memory_order_acquire); }
uint32_t option_flags() noexcept { return g.flags; }

rsf_census_result core_install(const rsf_census_options* opt) noexcept {
    bool expected = false;
    if (!g.ever_installed.compare_exchange_strong(expected, true)) return RSF_CENSUS_ERROR_ALREADY_INSTALLED;

    g.flags = opt->flags;
    g.budget = opt->byte_budget ? opt->byte_budget : (64ull << 20);
    g.cap = opt->full_record_cap ? opt->full_record_cap : 32;
    g.interval = opt->sample_interval ? opt->sample_interval : 600;
    g.period_ms = opt->summary_period_ms ? opt->summary_period_ms : 2000;

    if (opt->output_dir_utf8 && opt->output_dir_utf8[0]) {
        if (!MultiByteToWideChar(CP_UTF8, 0, opt->output_dir_utf8, -1, g.dir, MAX_PATH)) {
            g.ever_installed.store(false);
            return RSF_CENSUS_ERROR_INVALID_ARGUMENT;
        }
    } else {
        std::wcscpy(g.dir, L".");
    }
    size_t dl = std::wcslen(g.dir);
    while (dl > 1 && (g.dir[dl - 1] == L'\\' || g.dir[dl - 1] == L'/')) g.dir[--dl] = 0;
    make_dirs(g.dir);
    GetModuleFileNameW(nullptr, g.exe, MAX_PATH);

    QueryPerformanceFrequency(&g.qpc_freq);
    QueryPerformanceCounter(&g.qpc_start);

    HMODULE nt = GetModuleHandleW(L"ntdll.dll");
    if (nt) {
        g.pc_to_header = reinterpret_cast<PVOID(NTAPI*)(PVOID, PVOID*)>(
            GetProcAddress(nt, "RtlPcToFileHeader"));
    }

    if (!rsf_census_record_init()) return RSF_CENSUS_ERROR_IO;
    if (!rsf_census_stats_init(static_cast<uint16_t>(kFnCount), g.cap, g.interval)) {
        return RSF_CENSUS_ERROR_IO;
    }
    g.file_buf = static_cast<char*>(VirtualAlloc(nullptr, kFileBuffer, MEM_COMMIT | MEM_RESERVE,
                                                 PAGE_READWRITE));
    g.summary_buf = static_cast<char*>(VirtualAlloc(nullptr, kSummaryCapacity, MEM_COMMIT | MEM_RESERVE,
                                                    PAGE_READWRITE));
    if (!g.file_buf || !g.summary_buf) return RSF_CENSUS_ERROR_IO;

    wchar_t path[MAX_PATH + 32];
    std::swprintf(path, std::size(path), L"%ls\\census.jsonl", g.dir);
    g.file = CreateFileW(path, GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_ALWAYS,
                         FILE_ATTRIBUTE_NORMAL, nullptr);
    if (g.file == INVALID_HANDLE_VALUE) return RSF_CENSUS_ERROR_IO;

    {
        char head[1024];
        char exe[MAX_PATH * 3];
        WideCharToMultiByte(CP_UTF8, 0, g.exe, -1, exe, sizeof(exe), nullptr, nullptr);
        Json j(head, sizeof(head) - 2);
        j.obj();
        j.str("type", "census_start");
        j.u64("census_abi", RSF_CENSUS_ABI_VERSION);
        j.u64("pid", GetCurrentProcessId());
        j.str("exe", exe);
        j.u64("qpc_frequency", static_cast<uint64_t>(g.qpc_freq.QuadPart));
        j.u64("byte_budget", g.budget);
        j.u64("full_record_cap", g.cap);
        j.u64("sample_interval", g.interval);
        j.end_obj();
        head[j.size()] = '\n';
        file_append_locked(head, j.size() + 1);
        g.written.store(0);
    }

    g.wake = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!g.wake) return RSF_CENSUS_ERROR_IO;

    g.active.store(true, std::memory_order_release);
    return RSF_CENSUS_OK;
}

} // namespace rsf_census

using namespace rsf_census;

extern "C" {

rsf_census_result rsf_census_install(const rsf_census_options* options) {
    if (!options || options->struct_size < sizeof(rsf_census_options)) return RSF_CENSUS_ERROR_INVALID_ARGUMENT;
    if (options->abi_version != RSF_CENSUS_ABI_VERSION) return RSF_CENSUS_ERROR_ABI_MISMATCH;
    rsf_census_result r = core_install(options);
    if (r != RSF_CENSUS_OK) return r;
    // CreateThread under the loader lock is fine: a new thread waits for the lock to be released
    // before it runs, and nothing here waits for it.
    g.thread = CreateThread(nullptr, 0, writer_main, nullptr, 0, nullptr);
    if (!g.thread) return RSF_CENSUS_ERROR_IO;
    AddVectoredExceptionHandler(1, fatal_exception);
    if (!hooks_install()) return RSF_CENSUS_ERROR_HOOK; // the summary still shows what happened
    if (!(g.flags & RSF_CENSUS_FLAG_NO_STATIC_SCAN)) g.scan_thread = CreateThread(nullptr, 0, scan_main, nullptr, 0, nullptr);
    return RSF_CENSUS_OK;
}

rsf_census_result rsf_census_flush(void) {
    if (!g.ever_installed.load()) return RSF_CENSUS_ERROR_NOT_INSTALLED;
    AcquireSRWLockExclusive(&g.io);
    flush_all_locked(true);
    ReleaseSRWLockExclusive(&g.io);
    return RSF_CENSUS_OK;
}

rsf_census_result rsf_census_get_counters(rsf_census_counters* out) {
    if (!out || out->struct_size < sizeof(rsf_census_counters)) return RSF_CENSUS_ERROR_INVALID_ARGUMENT;
    if (out->abi_version != RSF_CENSUS_ABI_VERSION) return RSF_CENSUS_ERROR_ABI_MISMATCH;
    rsf_census_stats_counts(&out->calls_seen, &out->present_index,
                            &out->sites, &out->site_overflow);
    out->records_queued = g.queued.load();
    out->records_written = g.written.load();
    out->records_dropped_queue = g.dropped_queue.load();
    out->records_dropped_budget = g.dropped_budget.load();
    out->records_dropped_size = g.dropped_size.load();
    out->bytes_written = g.bytes_written.load();
    hook_counts(out->hooks_installed, out->hook_conflicts, out->modules_hooked);
    out->static_findings = g.finding_count.load() > kFindingCapacity ? kFindingCapacity : g.finding_count.load();
    return RSF_CENSUS_OK;
}

rsf_census_result rsf_census_shutdown(void) {
    if (!g.active.load()) return RSF_CENSUS_ERROR_NOT_INSTALLED;
    g.stop.store(true, std::memory_order_release);
    if (g.wake) SetEvent(g.wake);
    if (g.thread) {
        WaitForSingleObject(g.thread, 10000);
        CloseHandle(g.thread);
        g.thread = nullptr;
    }
    if (g.scan_thread) {
        // The scan cannot be interrupted; give it time to finish its pass over the image.
        WaitForSingleObject(g.scan_thread, 60000);
        CloseHandle(g.scan_thread);
        g.scan_thread = nullptr;
    }
    AcquireSRWLockExclusive(&g.io);
    flush_all_locked(true);
    ReleaseSRWLockExclusive(&g.io);
    // Stop recording first so calls already inside a wrapper finish without touching the queue,
    // then disable the detours. Trampolines stay mapped for threads still inside them.
    g.active.store(false, std::memory_order_release);
    hooks_remove();
    AcquireSRWLockExclusive(&g.io);
    if (g.file != INVALID_HANDLE_VALUE) {
        CloseHandle(g.file);
        g.file = INVALID_HANDLE_VALUE;
    }
    ReleaseSRWLockExclusive(&g.io);
    return RSF_CENSUS_OK;
}

} // extern "C"
