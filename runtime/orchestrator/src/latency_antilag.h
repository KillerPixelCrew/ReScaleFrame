// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/frame_generation.h>
#include <cstdint>
#include <memory>
namespace rsf::latency {
// Driver seam is internal to the Runtime. Engine adapters never receive these SDK operations.
struct AntiLagDriver {
    virtual ~AntiLagDriver() = default;
    virtual bool initialize(uint32_t api, void* device) noexcept = 0;
    virtual bool update(bool enabled, uint32_t max_fps) noexcept = 0;
    virtual bool end_render(void* chain, bool enabled, bool generated) noexcept = 0;
    virtual void shutdown() noexcept = 0;
};
struct AntiLagState {
    uint32_t available{}, enabled{};
    uint64_t updates{}, end_markers{}, refused{};
};
struct AntiLagCallbacks {
    uint32_t (*initialize)(void*, uint32_t api, void* device);
    uint32_t (*update)(void*, uint32_t enabled, uint32_t max_fps);
    uint32_t (*end_render)(void*, void* chain, uint32_t enabled, uint32_t generated);
    void (*shutdown)(void*);
};
struct AntiLagContext;
extern "C" {
AntiLagContext* rsf_core_antilag_create(const AntiLagCallbacks*, void* driver);
void rsf_core_antilag_destroy(AntiLagContext*);
void rsf_core_antilag_bind(AntiLagContext*, uint32_t api, void* device, uint32_t vendor);
uint32_t rsf_core_antilag_pace(AntiLagContext*, uint64_t id, uint32_t mode, uint32_t limit_us, uint32_t other_owner);
uint32_t rsf_core_antilag_end_render(AntiLagContext*, uint64_t id, void* chain, uint32_t generated);
void rsf_core_antilag_state(const AntiLagContext*, AntiLagState*);
void rsf_core_antilag_reset_sources(AntiLagContext*);
}

class AntiLag {
    AntiLagContext* context_{};
public:
    explicit AntiLag(AntiLagDriver& driver) noexcept {
        static constexpr AntiLagCallbacks callbacks{
            +[](void* context, uint32_t api, void* device) -> uint32_t {
                return static_cast<AntiLagDriver*>(context)->initialize(api, device);
            },
            +[](void* context, uint32_t enabled, uint32_t fps) -> uint32_t {
                return static_cast<AntiLagDriver*>(context)->update(enabled != 0, fps);
            },
            +[](void* context, void* chain, uint32_t enabled, uint32_t generated) -> uint32_t {
                return static_cast<AntiLagDriver*>(context)->end_render(chain, enabled != 0, generated != 0);
            },
            +[](void* context) { static_cast<AntiLagDriver*>(context)->shutdown(); },
        };
        context_ = rsf_core_antilag_create(&callbacks, &driver);
    }
    AntiLag(const AntiLag&) = delete;
    AntiLag& operator=(const AntiLag&) = delete;
    ~AntiLag() { rsf_core_antilag_destroy(context_); }
    void bind(uint32_t api, void* device, uint32_t vendor) noexcept {
        rsf_core_antilag_bind(context_, api, device, vendor);
    }
    bool pace(uint64_t id, uint32_t mode, uint32_t limit_us, uint32_t other_owner) noexcept {
        return rsf_core_antilag_pace(context_, id, mode, limit_us, other_owner) != 0;
    }
    bool end_render(uint64_t id, void* chain, bool generated) noexcept {
        return rsf_core_antilag_end_render(context_, id, chain, generated) != 0;
    }
    AntiLagState state() const noexcept {
        AntiLagState out;
        rsf_core_antilag_state(context_, &out);
        return out;
    }
    void reset_sources() noexcept { rsf_core_antilag_reset_sources(context_); }
};
std::unique_ptr<AntiLagDriver> make_antilag_driver();
}
