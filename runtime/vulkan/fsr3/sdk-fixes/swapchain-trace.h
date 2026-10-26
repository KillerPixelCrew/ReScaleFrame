// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <mutex>

namespace rsf_ffx_trace {
inline uint64_t& frame() { static thread_local uint64_t value = 0; return value; }
struct Frame {
    uint64_t previous;
    explicit Frame(uint64_t value) : previous(frame()) { frame() = value; }
    ~Frame() { frame() = previous; }
};
struct Log {
    HANDLE file = INVALID_HANDLE_VALUE;
    int64_t frequency = 0;
    std::atomic<uint32_t> rows{0};
    std::mutex mutex;
    Log() noexcept {
        wchar_t path[32768]{};
        auto size = GetEnvironmentVariableW(L"RSF_FFX_VK_TRACE_FILE", path, 32768);
        if (!size || size >= 32768 || !(size > 2 && path[1] == L':' && (path[2] == L'/' || path[2] == L'\\'))) return;
        LARGE_INTEGER value{}; if (!QueryPerformanceFrequency(&value)) return; frequency = value.QuadPart;
        file = CreateFileW(path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (file != INVALID_HANDLE_VALUE && GetFileSize(file, nullptr) == 0) {
            constexpr char header[] = "pid,tid,stage,frame,object,value,before,after,start_qpc,end_qpc,frequency,duration_ms,result\n";
            DWORD written = 0; (void)WriteFile(file, header, static_cast<DWORD>(sizeof(header) - 1), &written, nullptr);
        }
    }
    ~Log() { if (file != INVALID_HANDLE_VALUE) CloseHandle(file); }
};
inline Log& log() { static Log value; return value; }
inline bool enabled() { return log().file != INVALID_HANDLE_VALUE && log().rows.load(std::memory_order_relaxed) < 8192; }
class Scope {
    const char* stage;
    uint64_t identity, object, value, before;
    int64_t start = 0;
    bool active = false;
public:
    Scope(const char* name, uint64_t handle = 0, uint64_t target = 0, uint64_t observed = 0) noexcept :
        stage(name), identity(frame()), object(handle), value(target), before(observed) {
        if (!enabled()) return;
        active = true; LARGE_INTEGER now{}; QueryPerformanceCounter(&now); start = now.QuadPart;
    }
    template<class Result> Result finish(Result result, uint64_t after = 0) noexcept {
        if (!active) return result;
        active = false; LARGE_INTEGER end{}; QueryPerformanceCounter(&end);
        auto& output = log();
        if (output.rows.fetch_add(1, std::memory_order_relaxed) >= 8192) return result;
        char text[768]{};
        const int length = std::snprintf(text, sizeof(text), "%lu,%lu,%s,%llu,%llu,%llu,%llu,%llu,%lld,%lld,%lld,%.6f,%lld\n",
            GetCurrentProcessId(), GetCurrentThreadId(), stage, static_cast<unsigned long long>(identity),
            static_cast<unsigned long long>(object), static_cast<unsigned long long>(value), static_cast<unsigned long long>(before),
            static_cast<unsigned long long>(after), static_cast<long long>(start), static_cast<long long>(end.QuadPart),
            static_cast<long long>(output.frequency), double(end.QuadPart - start) * 1000.0 / double(output.frequency), static_cast<long long>(result));
        if (length > 0 && static_cast<size_t>(length) < sizeof(text)) {
            try { std::lock_guard lock(output.mutex); DWORD written = 0; (void)WriteFile(output.file, text, static_cast<DWORD>(length), &written, nullptr); }
            catch (...) {}
        }
        return result;
    }
    ~Scope() { (void)finish(INT64_MIN); }
};
template<class Call> auto call(const char* stage, uint64_t object, uint64_t value, Call&& operation) -> decltype(operation()) {
    Scope trace(stage, object, value); return trace.finish(operation());
}
inline void event(const char* stage, uint64_t object, uint64_t value, uint64_t extra = 0) {
    Scope trace(stage, object, value, extra); (void)trace.finish(0);
}
inline uint64_t counter(VkDevice device, VkSemaphore semaphore) {
    uint64_t value = 0;
    if (enabled() && semaphore) {
        Scope trace("semaphore_counter", reinterpret_cast<uint64_t>(semaphore));
        const auto result = vkGetSemaphoreCounterValue(device, semaphore, &value);
        (void)trace.finish(result, value);
    }
    return value;
}
}
