// SPDX-License-Identifier: GPL-3.0-only
// Census core: the record queue, the site and function statistics, the writer thread and the C API.
#include <intrin.h>

#include <cmath>
#include <cwchar>
#include <iterator>
#include <new>

#include "census_internal.h"

namespace rsf_census {

const FnDesc kFns[kFnCount] = {
#define X(id, api, name, mods) {#id, api, name, mods},
    RSF_CENSUS_FUNCTIONS(X)
#undef X
};

std::atomic<void*> g_orig[kFnCount];
void* g_wrapper[kFnCount];
std::atomic<void*> g_target[kFnCount];

namespace {

constexpr uint32_t kSiteCapacity = 8192;
constexpr uint32_t kSiteThreads = 6;
constexpr uint32_t kRingCells = 1024;
constexpr uint32_t kModuleCapacity = 512;
constexpr uint32_t kThreadCapacity = 256;
constexpr uint32_t kQueueCapacity = 128;
constexpr uint32_t kFindingCapacity = 192;
constexpr uint32_t kHistBuckets = 66; // 0 unused, 1..64 exact, 65 means 65 or more
constexpr size_t kSummaryCapacity = 4u << 20;
constexpr size_t kFileBuffer = 256u << 10;

struct Site {
    std::atomic<uint64_t> key{0};
    std::atomic<uint64_t> count{0};
    std::atomic<uint64_t> first_present{0}; // present index + 1, 0 when unset
    std::atomic<uint64_t> last_present{0};
    std::atomic<uint32_t> tids[kSiteThreads]{};
    std::atomic<uint32_t> tid_count{0};
};

struct FnStat {
    std::atomic<uint64_t> calls{0};
    std::atomic<uint32_t> cur{0};
    std::atomic<uint64_t> hist[kHistBuckets]{};
    std::atomic<uint64_t> presents_with_calls{0};
};

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

struct QueueStat {
    std::atomic<uint64_t> ptr{0};
    std::atomic<uint8_t> kind{0};
    std::atomic<uint32_t> a{0}, b{0}, c{0};
    std::atomic<uint64_t> calls{0};
    std::atomic<uint64_t> lists{0};
    std::atomic<uint32_t> cur{0};
    std::atomic<uint64_t> first_present{0};
    std::atomic<uint64_t> last_present{0};
    std::atomic<uint64_t> hist[kHistBuckets]{};
};

struct Finding {
    char vendor[24];
    char what[72];
    uint64_t rva;
    uint64_t function_rva;
};

struct Cell {
    std::atomic<size_t> seq;
    uint32_t len;
    uint16_t fn;
    uint16_t pad;
    uint64_t t_us;
    uint64_t present;
    uint64_t ord;
    uint64_t site_ord;
    uint64_t base;
    uint64_t ret;
    uint32_t rva;
    uint32_t tid;
    uint32_t stack_n;
    void* stack[kStackFrames];
    char thread[40];
    char body[kBodyMax];
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
    std::atomic<uint64_t> present{0};

    Cell* cells = nullptr;
    char pad0[64]{};
    std::atomic<size_t> enq{0};
    char pad1[64]{};
    std::atomic<size_t> deq{0};
    char pad2[64]{};

    Site* sites = nullptr;
    std::atomic<uint32_t> site_count{0};
    std::atomic<uint32_t> site_overflow{0};
    FnStat fn[kFnCount];

    ModuleEntry modules[kModuleCapacity];
    std::atomic<uint32_t> module_count{0};
    ThreadEntry threads[kThreadCapacity];
    QueueStat queues[kQueueCapacity];

    Finding findings[kFindingCapacity];
    std::atomic<uint32_t> finding_count{0};

    std::atomic<uint64_t> calls_seen{0};
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
    uint64_t seq = 0;
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
Site* find_site(uint64_t key) noexcept {
    uint64_t h = key * 0x9E3779B97F4A7C15ull;
    uint32_t i = static_cast<uint32_t>(h >> 40) & (kSiteCapacity - 1);
    for (uint32_t probe = 0; probe < 64; ++probe, i = (i + 1) & (kSiteCapacity - 1)) {
        Site& s = g.sites[i];
        uint64_t k = s.key.load(std::memory_order_acquire);
        if (k == key) return &s;
        if (k == 0) {
            uint64_t expected = 0;
            if (s.key.compare_exchange_strong(expected, key)) {
                g.site_count.fetch_add(1, std::memory_order_relaxed);
                return &s;
            }
            if (expected == key) return &s;
        }
    }
    return nullptr;
}

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

// ---- Record queue (bounded multi-producer ring) ------------------------------------------------
Cell* queue_acquire(size_t& pos) noexcept {
    pos = g.enq.load(std::memory_order_relaxed);
    for (;;) {
        Cell& c = g.cells[pos & (kRingCells - 1)];
        size_t seq = c.seq.load(std::memory_order_acquire);
        intptr_t diff = static_cast<intptr_t>(seq) - static_cast<intptr_t>(pos);
        if (diff == 0) {
            if (g.enq.compare_exchange_weak(pos, pos + 1, std::memory_order_relaxed)) return &c;
        } else if (diff < 0) {
            return nullptr;
        } else {
            pos = g.enq.load(std::memory_order_relaxed);
        }
    }
}

Cell* queue_peek(size_t& pos) noexcept {
    pos = g.deq.load(std::memory_order_relaxed);
    Cell& c = g.cells[pos & (kRingCells - 1)];
    size_t seq = c.seq.load(std::memory_order_acquire);
    return static_cast<intptr_t>(seq) - static_cast<intptr_t>(pos + 1) == 0 ? &c : nullptr;
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

void compose_line_locked(const Cell& c) noexcept {
    // Only the thread holding g.io gets here, so one static buffer is enough.
    // Reserve the decoded body independently of the escaped module names in the stack.
    static char line[kBodyMax + 768 + kStackFrames * 192];
    const uint32_t fi = c.fn;
    const uint32_t mi = module_index(c.base);
    resolve_module_name(mi);
    Json h(line, sizeof(line) - 16);
    h.obj();
    h.u64("seq", g.seq++);
    h.u64("t_us", c.t_us);
    h.u64("present", c.present);
    h.str("api", kFns[fi].api);
    h.str("fn", kFns[fi].name ? kFns[fi].name : kFns[fi].id);
    h.u64("ord", c.ord);
    h.u64("site_ord", c.site_ord);
    h.str("module", g.modules[mi].name);
    h.hex("rva", c.rva);
    h.u64("tid", c.tid);
    h.str("thread", c.thread);
    h.hex("ret", c.ret);
    if (c.stack_n) {
        h.arr("stack");
        for (uint32_t i = 0; i < c.stack_n && i < kStackFrames; ++i) {
            PVOID frame_base = nullptr;
            if (g.pc_to_header) g.pc_to_header(c.stack[i], &frame_base);
            char frame[96];
            if (frame_base) {
                const uint32_t fm = module_index(reinterpret_cast<uint64_t>(frame_base));
                resolve_module_name(fm);
                std::snprintf(frame, sizeof(frame), "%s+0x%llx", g.modules[fm].name,
                              static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(c.stack[i]) -
                                                              reinterpret_cast<uintptr_t>(frame_base)));
            } else {
                std::snprintf(frame, sizeof(frame), "0x%llx",
                              static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(c.stack[i])));
            }
            h.str(nullptr, frame);
        }
        h.end_arr();
    }
    if (!h.ok() || h.size() + c.len + 12 > sizeof(line)) {
        g.dropped_size.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    size_t n = h.size();
    std::memcpy(line + n, ",\"args\":", 8);
    n += 8;
    std::memcpy(line + n, c.body, c.len);
    n += c.len;
    line[n++] = '}';
    line[n++] = '\n';
    file_append_locked(line, n);
}

void drain_locked() noexcept {
    for (;;) {
        size_t pos;
        Cell* c = queue_peek(pos);
        if (!c) break;
        g.deq.store(pos + 1, std::memory_order_relaxed);
        compose_line_locked(*c);
        c->seq.store(pos + kRingCells, std::memory_order_release);
    }
}

void roll_hist(std::atomic<uint32_t>& cur, std::atomic<uint64_t>* hist) noexcept {
    uint32_t c = cur.exchange(0, std::memory_order_relaxed);
    if (c) hist[c >= 65 ? 65 : c].fetch_add(1, std::memory_order_relaxed);
}

void summary_hist(Json& j, const std::atomic<uint64_t>* hist) noexcept {
    j.obj("calls_per_present");
    for (uint32_t b = 1; b < kHistBuckets; ++b) {
        uint64_t n = hist[b].load(std::memory_order_relaxed);
        if (!n) continue;
        char key[8];
        if (b == kHistBuckets - 1) {
            std::snprintf(key, sizeof(key), "65+");
        } else {
            std::snprintf(key, sizeof(key), "%u", b);
        }
        j.u64(key, n);
    }
    j.end_obj();
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
    j.u64("present_index", g.present.load());
    j.obj("budget");
    j.u64("byte_budget", g.budget);
    j.u64("bytes_written", g.bytes_written.load());
    j.boolean("exhausted", g.budget_exhausted.load());
    j.u64("full_record_cap", g.cap);
    j.u64("sample_interval", g.interval);
    j.end_obj();
    j.obj("counters");
    j.u64("calls_seen", g.calls_seen.load());
    j.u64("records_queued", g.queued.load());
    j.u64("records_written", g.written.load());
    j.u64("dropped_queue", g.dropped_queue.load());
    j.u64("dropped_budget", g.dropped_budget.load());
    j.u64("dropped_size", g.dropped_size.load());
    j.u64("sites", g.site_count.load());
    j.u64("site_overflow", g.site_overflow.load());
    j.end_obj();

    j.arr("sites");
    for (uint32_t i = 0; i < kSiteCapacity; ++i) {
        const Site& s = g.sites[i];
        uint64_t key = s.key.load(std::memory_order_acquire);
        if (!key) continue;
        const uint32_t fi = static_cast<uint32_t>(key >> 48);
        const uint32_t mi = static_cast<uint32_t>(key >> 32) & 0xFFFF;
        resolve_module_name(mi);
        j.obj();
        j.str("api", kFns[fi].api);
        j.str("function", kFns[fi].name ? kFns[fi].name : kFns[fi].id);
        j.str("module", g.modules[mi].name);
        j.hex("rva", key & 0xFFFFFFFFull);
        j.arr("threads");
        uint32_t nt = s.tid_count.load();
        for (uint32_t t = 0; t < nt && t < kSiteThreads; ++t) j.u64(nullptr, s.tids[t].load());
        j.end_arr();
        if (nt > kSiteThreads) j.u64("threads_beyond_listed", nt - kSiteThreads);
        j.u64("count", s.count.load());
        uint64_t fp = s.first_present.load();
        j.u64("first_present", fp ? fp - 1 : 0);
        uint64_t lp = s.last_present.load();
        j.u64("last_present", lp ? lp - 1 : 0);
        j.end_obj();
    }
    j.end_arr();

    j.arr("histograms");
    for (uint32_t f = 0; f < kFnCount; ++f) {
        const FnStat& fs = g.fn[f];
        uint64_t calls = fs.calls.load();
        if (!calls) continue;
        j.obj();
        j.str("function", kFns[f].name ? kFns[f].name : kFns[f].id);
        j.u64("calls", calls);
        summary_hist(j, fs.hist);
        j.end_obj();
    }
    j.end_arr();

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

    j.arr("queues");
    for (uint32_t i = 0; i < kQueueCapacity; ++i) {
        const QueueStat& q = g.queues[i];
        uint64_t p = q.ptr.load(std::memory_order_acquire);
        if (!p) continue;
        j.obj();
        j.str("kind", q.kind.load() == static_cast<uint8_t>(QueueKind::D3d12) ? "d3d12" : "vulkan");
        j.hex("queue", p);
        if (q.kind.load() == static_cast<uint8_t>(QueueKind::D3d12)) {
            j.u64("type", q.a.load());
            j.i64("priority", static_cast<int32_t>(q.b.load()));
            j.hex("flags", q.c.load());
        } else {
            j.u64("family", q.a.load());
            j.u64("index", q.b.load());
        }
        j.u64("submit_calls", q.calls.load());
        j.u64("lists_or_batches", q.lists.load());
        uint64_t fp = q.first_present.load();
        j.u64("first_present", fp ? fp - 1 : 0);
        uint64_t lp = q.last_present.load();
        j.u64("last_present", lp ? lp - 1 : 0);
        summary_hist(j, q.hist);
        j.end_obj();
    }
    j.end_arr();

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
    static_scan_run();
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
            c.json().u64("present", g.present.load());
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

// ---- Safe memory ------------------------------------------------------------------------------
bool safe_read(const void* src, void* dst, size_t n) noexcept {
    __try {
        std::memcpy(dst, src, n);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

bool safe_read_cstr(const char* src, char* dst, size_t cap) noexcept {
    if (!cap) return false;
    __try {
        size_t i = 0;
        for (; i + 1 < cap; ++i) {
            char c = src[i];
            dst[i] = c;
            if (!c) return true;
        }
        dst[i] = 0;
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        dst[0] = 0;
        return false;
    }
}

bool safe_read_wstr(const wchar_t* src, wchar_t* dst, size_t cap) noexcept {
    if (!cap) return false;
    __try {
        size_t i = 0;
        for (; i + 1 < cap; ++i) {
            wchar_t c = src[i];
            dst[i] = c;
            if (!c) return true;
        }
        dst[i] = 0;
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        dst[0] = 0;
        return false;
    }
}

bool plausible_ptr(const void* p) noexcept {
    const uintptr_t v = reinterpret_cast<uintptr_t>(p);
    return v >= 0x10000 && v < 0x00007FFFFFFF0000ull;
}

bool is_code_ptr(const void* p) noexcept {
    if (!plausible_ptr(p)) return false;
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery(p, &mbi, sizeof(mbi)) != sizeof(mbi)) return false;
    constexpr DWORD kExec = PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    return mbi.State == MEM_COMMIT && (mbi.Protect & kExec) != 0 && (mbi.Protect & PAGE_GUARD) == 0;
}

void* vtable_slot(const void* obj, size_t slot) noexcept {
    void* const* vt = nullptr;
    if (!plausible_ptr(obj) || !rd(obj, vt) || !plausible_ptr(vt)) return nullptr;
    void* fn = nullptr;
    return rd(vt + slot, fn) && is_code_ptr(fn) ? fn : nullptr;
}

bool guarded(void (*fn)(void*), void* ctx) noexcept {
    __try {
        fn(ctx);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// ---- Json -------------------------------------------------------------------------------------
void Json::put(const char* s, size_t n) noexcept {
    if (overflow_) return;
    if (len_ + n >= cap_) {
        overflow_ = true;
        return;
    }
    std::memcpy(buf_ + len_, s, n);
    len_ += n;
}

void Json::put_escaped(const char* s) noexcept {
    put("\"", 1);
    for (; s && *s; ++s) {
        const unsigned char c = static_cast<unsigned char>(*s);
        if (c == '"' || c == '\\') {
            char e[2] = {'\\', static_cast<char>(c)};
            put(e, 2);
        } else if (c < 0x20) {
            char e[8];
            int n = std::snprintf(e, sizeof(e), "\\u%04x", c);
            put(e, static_cast<size_t>(n));
        } else {
            put(reinterpret_cast<const char*>(&c), 1);
        }
    }
    put("\"", 1);
}

void Json::lead(const char* key) noexcept {
    if (depth_ > 0) {
        const uint32_t bit = 1u << (depth_ - 1);
        if (comma_ & bit) put(",", 1);
        comma_ |= bit;
    }
    if (key) {
        put_escaped(key);
        put(":", 1);
    }
}

void Json::open(const char* key, char c) noexcept {
    lead(key);
    put(&c, 1);
    if (depth_ < 31) ++depth_;
    comma_ &= ~(1u << (depth_ - 1));
}

void Json::close(char c) noexcept {
    if (depth_ > 0) --depth_;
    put(&c, 1);
}

void Json::str(const char* key, const char* v) noexcept {
    lead(key);
    put_escaped(v ? v : "");
}

void Json::wstr(const char* key, const wchar_t* v) noexcept {
    char tmp[512];
    tmp[0] = 0;
    if (v) WideCharToMultiByte(CP_UTF8, 0, v, -1, tmp, sizeof(tmp), nullptr, nullptr);
    tmp[sizeof(tmp) - 1] = 0;
    str(key, tmp);
}

void Json::u64(const char* key, uint64_t v) noexcept {
    lead(key);
    char t[24];
    int n = std::snprintf(t, sizeof(t), "%llu", static_cast<unsigned long long>(v));
    put(t, static_cast<size_t>(n));
}

void Json::i64(const char* key, int64_t v) noexcept {
    lead(key);
    char t[24];
    int n = std::snprintf(t, sizeof(t), "%lld", static_cast<long long>(v));
    put(t, static_cast<size_t>(n));
}

void Json::hex(const char* key, uint64_t v) noexcept {
    lead(key);
    char t[24];
    int n = std::snprintf(t, sizeof(t), "\"0x%llx\"", static_cast<unsigned long long>(v));
    put(t, static_cast<size_t>(n));
}

void Json::f32(const char* key, double v) noexcept {
    lead(key);
    if (!std::isfinite(v)) {
        put("null");
        return;
    }
    char t[40];
    int n = std::snprintf(t, sizeof(t), "%.9g", v);
    put(t, static_cast<size_t>(n));
}

void Json::boolean(const char* key, bool v) noexcept {
    lead(key);
    put(v ? "true" : "false");
}

void Json::null(const char* key) noexcept {
    lead(key);
    put("null");
}

void Json::bytes_hex(const char* key, const void* data, size_t n) noexcept {
    lead(key);
    put("\"", 1);
    static const char digits[] = "0123456789abcdef";
    const unsigned char* p = static_cast<const unsigned char*>(data);
    for (size_t i = 0; i < n; ++i) {
        char two[2] = {digits[p[i] >> 4], digits[p[i] & 15]};
        put(two, 2);
    }
    put("\"", 1);
}

void Json::raw(const char* key, const char* json_text) noexcept {
    lead(key);
    put(json_text ? json_text : "null");
}

// ---- Call -------------------------------------------------------------------------------------
Call::Call(Fn fn, void* return_address) noexcept : fn_(fn), full_(false), json_(buf_, kBodyMax) {
    if (!g.active.load(std::memory_order_acquire)) return;
    g.calls_seen.fetch_add(1, std::memory_order_relaxed);
    FnStat& fs = g.fn[idx(fn)];
    ord_ = fs.calls.fetch_add(1, std::memory_order_relaxed) + 1;
    fs.cur.fetch_add(1, std::memory_order_relaxed);

    PVOID base = nullptr;
    if (g.pc_to_header) g.pc_to_header(return_address, &base);
    base_ = reinterpret_cast<uint64_t>(base);
    rva_ = base ? static_cast<uint32_t>(reinterpret_cast<uintptr_t>(return_address) - base_)
                : static_cast<uint32_t>(reinterpret_cast<uintptr_t>(return_address));
    const uint32_t mi = module_index(base_);
    const uint64_t key = (static_cast<uint64_t>(idx(fn)) << 48) | (static_cast<uint64_t>(mi) << 32) | rva_;
    Site* s = find_site(key ? key : 1);
    if (!s) {
        g.site_overflow.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    tid_ = GetCurrentThreadId();
    const uint64_t present = g.present.load(std::memory_order_relaxed);
    site_ord_ = s->count.fetch_add(1, std::memory_order_relaxed) + 1;
    uint64_t zero = 0;
    s->first_present.compare_exchange_strong(zero, present + 1);
    s->last_present.store(present + 1, std::memory_order_relaxed);
    note_thread(tid_);
    uint32_t nt = s->tid_count.load(std::memory_order_relaxed);
    bool seen = false;
    for (uint32_t i = 0; i < nt && i < kSiteThreads; ++i) {
        if (s->tids[i].load(std::memory_order_relaxed) == tid_) {
            seen = true;
            break;
        }
    }
    if (!seen) {
        uint32_t slot = s->tid_count.fetch_add(1, std::memory_order_relaxed);
        if (slot < kSiteThreads) s->tids[slot].store(tid_, std::memory_order_relaxed);
    }
    const uint64_t n = site_ord_ - 1;
    full_ = n < g.cap || (g.interval && (n - g.cap) % g.interval == 0);
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
    Cell* c = queue_acquire(pos);
    if (!c) {
        g.dropped_queue.fetch_add(1, std::memory_order_relaxed);
        SetLastError(last_error);
        return;
    }
    c->len = static_cast<uint32_t>(json_.size());
    c->fn = static_cast<uint16_t>(idx(fn_));
    c->t_us = t_us_;
    c->present = g.present.load(std::memory_order_relaxed);
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
    c->seq.store(pos + 1, std::memory_order_release);
    g.queued.fetch_add(1, std::memory_order_relaxed);
    SetLastError(last_error);
}

// ---- Present and queue statistics -------------------------------------------------------------
void note_present_begin() noexcept {
    if (!g.active.load(std::memory_order_acquire)) return;
    for (size_t f = 0; f < kFnCount; ++f) roll_hist(g.fn[f].cur, g.fn[f].hist);
    for (uint32_t i = 0; i < kQueueCapacity; ++i) {
        if (g.queues[i].ptr.load(std::memory_order_acquire)) roll_hist(g.queues[i].cur, g.queues[i].hist);
    }
    g.present.fetch_add(1, std::memory_order_relaxed);
}

uint64_t present_index() noexcept { return g.present.load(std::memory_order_relaxed); }

namespace {
QueueStat* find_queue(QueueKind kind, const void* queue, bool create) noexcept {
    const uint64_t p = reinterpret_cast<uint64_t>(queue);
    if (!p) return nullptr;
    uint32_t i = static_cast<uint32_t>((p >> 4) * 2654435761u >> 8) & (kQueueCapacity - 1);
    for (uint32_t probe = 0; probe < kQueueCapacity; ++probe, i = (i + 1) & (kQueueCapacity - 1)) {
        QueueStat& q = g.queues[i];
        uint64_t cur = q.ptr.load(std::memory_order_acquire);
        if (cur == p && q.kind.load() == static_cast<uint8_t>(kind)) return &q;
        if (cur == 0) {
            if (!create) return nullptr;
            uint64_t expected = 0;
            if (q.ptr.compare_exchange_strong(expected, p)) {
                q.kind.store(static_cast<uint8_t>(kind));
                return &q;
            }
        }
    }
    return nullptr;
}
} // namespace

void queue_describe(QueueKind kind, const void* queue, uint32_t a, uint32_t b, uint32_t c) noexcept {
    if (!g.active.load(std::memory_order_acquire)) return;
    QueueStat* q = find_queue(kind, queue, true);
    if (!q) return;
    q->a.store(a);
    q->b.store(b);
    q->c.store(c);
}

bool queue_submit(QueueKind kind, const void* queue, uint32_t lists) noexcept {
    if (!g.active.load(std::memory_order_acquire)) return false;
    QueueStat* q = find_queue(kind, queue, true);
    if (!q) return false;
    const uint64_t present = g.present.load(std::memory_order_relaxed);
    const bool first = q->calls.fetch_add(1, std::memory_order_relaxed) == 0;
    q->lists.fetch_add(lists, std::memory_order_relaxed);
    q->cur.fetch_add(1, std::memory_order_relaxed);
    uint64_t zero = 0;
    q->first_present.compare_exchange_strong(zero, present + 1);
    q->last_present.store(present + 1, std::memory_order_relaxed);
    return first;
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

    g.cells = static_cast<Cell*>(VirtualAlloc(nullptr, sizeof(Cell) * kRingCells,
                                              MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE));
    g.sites = static_cast<Site*>(VirtualAlloc(nullptr, sizeof(Site) * kSiteCapacity,
                                              MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE));
    g.file_buf = static_cast<char*>(VirtualAlloc(nullptr, kFileBuffer, MEM_COMMIT | MEM_RESERVE,
                                                 PAGE_READWRITE));
    g.summary_buf = static_cast<char*>(VirtualAlloc(nullptr, kSummaryCapacity, MEM_COMMIT | MEM_RESERVE,
                                                    PAGE_READWRITE));
    if (!g.cells || !g.sites || !g.file_buf || !g.summary_buf) return RSF_CENSUS_ERROR_IO;
    for (uint32_t i = 0; i < kRingCells; ++i) new (&g.cells[i].seq) std::atomic<size_t>(i);
    for (uint32_t i = 0; i < kSiteCapacity; ++i) new (&g.sites[i]) Site();

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
    out->calls_seen = g.calls_seen.load();
    out->records_queued = g.queued.load();
    out->records_written = g.written.load();
    out->records_dropped_queue = g.dropped_queue.load();
    out->records_dropped_budget = g.dropped_budget.load();
    out->records_dropped_size = g.dropped_size.load();
    out->bytes_written = g.bytes_written.load();
    out->present_index = g.present.load();
    out->sites = g.site_count.load();
    out->site_overflow = g.site_overflow.load();
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
