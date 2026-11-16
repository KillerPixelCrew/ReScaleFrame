// SPDX-License-Identifier: MIT
#ifndef RSF_PROVIDER_STATUS_SNAPSHOT_H
#define RSF_PROVIDER_STATUS_SNAPSHOT_H
#include <rescaleframe/d3d11_present_bridge.h>
#include <mutex>
namespace rsf::presentation {
// This cache owns copies only. No provider/COM lifetime or vendor call crosses its lock.
class ProviderStatusSnapshot {
    std::mutex guard;
    rsf_d3d11_present_status value{sizeof(value), RSF_PRESENT_STATUS_VERSION};
    bool accepting{};
public:
    void activate(uint32_t backend, uint64_t generation) {
        std::lock_guard lock(guard);
        value = {sizeof(value), RSF_PRESENT_STATUS_VERSION, 0, backend, generation};
        accepting = true;
    }
    void invalidate() {
        std::lock_guard lock(guard);
        value.valid = 0; value.state = {}; accepting = false;
    }
    void clear() {
        std::lock_guard lock(guard);
        value.valid = 0; value.state = {};
    }
    bool publish(uint32_t backend, uint64_t generation, const rsf_fg_status& state) {
        std::lock_guard lock(guard);
        if (!accepting || !generation || backend != value.backend || generation != value.generation || state.struct_size < sizeof(state)) return false;
        value.state = state; value.valid = 1; return true;
    }
    int32_t read(rsf_d3d11_present_status* output) {
        if (!output || output->struct_size < sizeof(*output)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        if (output->version != RSF_PRESENT_STATUS_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
        std::lock_guard lock(guard);
        *output = value;
        return value.valid ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_READY;
    }
};
}
#endif
