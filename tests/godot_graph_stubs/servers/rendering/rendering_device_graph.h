// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstdint>
#include <cstdlib>
#include <vector>
#define CRASH_COND_MSG(condition, message) do { if (condition) std::abort(); } while (false)

template <class T> class LocalVector : public std::vector<T> {};
class RenderingDeviceDriver {
public:
    template <class Tag> struct ID {
        uint64_t id = 0;
        explicit operator bool() const { return id != 0; }
        bool operator==(const ID &other) const { return id == other.id; }
        bool operator!=(const ID &other) const { return id != other.id; }
    };
    using CommandBufferID = ID<struct Buffer>;
    using CommandPoolID = ID<struct Pool>;
    using SemaphoreID = ID<struct Semaphore>;
};
class RenderingDeviceGraph {
public:
    struct CommandBufferPool {
        RenderingDeviceDriver::CommandPoolID pool;
        LocalVector<RenderingDeviceDriver::CommandBufferID> buffers;
        LocalVector<RenderingDeviceDriver::SemaphoreID> semaphores;
        uint32_t buffers_used = 0;
    };
};
