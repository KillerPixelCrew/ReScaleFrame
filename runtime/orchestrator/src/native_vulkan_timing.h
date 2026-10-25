// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <mutex>

namespace rsf {
namespace vk_timing_detail {
// Opt-in bounded diagnostics. This file does not create or submit GPU work.
struct Config {
    HANDLE file = INVALID_HANDLE_VALUE;
    int64_t frequency = 0;
    std::atomic<uint32_t> records{0};
    std::mutex guard;
    Config() noexcept {
        wchar_t path[32768]{};
        const auto length = GetEnvironmentVariableW(L"RSF_VULKAN_TIMING_FILE", path, 32768);
        if (!length || length >= 32768) return;
        const bool drive = length > 2 && path[1] == L':' && (path[2] == L'\\' || path[2] == L'/');
        const bool unc = length > 1 && path[0] == L'\\' && path[1] == L'\\';
        if (!drive && !unc) return;
        LARGE_INTEGER value{};
        if (!QueryPerformanceFrequency(&value) || value.QuadPart <= 0) return;
        frequency = value.QuadPart;
        file = CreateFileW(path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (file != INVALID_HANDLE_VALUE && GetFileSize(file, nullptr) == 0) {
            constexpr char header[] = "pid,tid,stage,frame,start_qpc,end_qpc,frequency,duration_ms\n";
            DWORD written = 0; (void)WriteFile(file, header, static_cast<DWORD>(sizeof(header) - 1), &written, nullptr);
        }
    }
    ~Config() { if (file != INVALID_HANDLE_VALUE) CloseHandle(file); }
    void write(const char* stage, uint64_t frame, DWORD thread, int64_t start, int64_t end) noexcept try {
        if (file == INVALID_HANDLE_VALUE || records.fetch_add(1, std::memory_order_relaxed) >= 2048) return;
        char line[512]{};
        const auto length = std::snprintf(line, sizeof(line), "%lu,%lu,%s,%llu,%lld,%lld,%lld,%.6f\n",
            GetCurrentProcessId(), thread, stage, static_cast<unsigned long long>(frame), static_cast<long long>(start),
            static_cast<long long>(end), static_cast<long long>(frequency), double(end - start) * 1000.0 / double(frequency));
        if (length <= 0 || static_cast<size_t>(length) >= sizeof(line)) return;
        std::lock_guard lock(guard);
        DWORD written = 0; (void)WriteFile(file, line, static_cast<DWORD>(length), &written, nullptr);
    } catch (...) {}
};
inline Config& config() { static Config value; return value; }
}

class VulkanTimingScope {
    vk_timing_detail::Config* configuration = nullptr;
    const char* stage = nullptr;
    uint64_t frame = 0;
    DWORD thread = 0;
    int64_t start = 0;
public:
    VulkanTimingScope(const char* value, uint64_t identity) noexcept : stage(value), frame(identity) {
        try {
            if (!value) return;
            auto& current = vk_timing_detail::config();
            if (current.file == INVALID_HANDLE_VALUE || current.records.load(std::memory_order_relaxed) >= 2048) return;
            configuration = &current; thread = GetCurrentThreadId();
            LARGE_INTEGER now{}; QueryPerformanceCounter(&now); start = now.QuadPart;
        } catch (...) {}
    }
    ~VulkanTimingScope() {
        if (!configuration) return;
        LARGE_INTEGER end{}; QueryPerformanceCounter(&end);
        configuration->write(stage, frame, thread, start, end.QuadPart);
    }
    VulkanTimingScope(const VulkanTimingScope&) = delete;
    VulkanTimingScope& operator=(const VulkanTimingScope&) = delete;
};

template<class Call> decltype(auto) vk_timed_call(const char* stage, uint64_t frame, Call&& call) {
    const VulkanTimingScope scope(stage, frame);
    return call();
}
}
