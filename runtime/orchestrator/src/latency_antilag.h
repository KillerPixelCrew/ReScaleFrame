// SPDX-License-Identifier: GPL-3.0-only
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
    bool available{}, enabled{};
    uint64_t updates{}, end_markers{}, refused{};
};
class AntiLag {
    AntiLagDriver& driver_;
    void* device_{};
    uint32_t api_{}, vendor_{};
    uint64_t last_update_{}, last_end_{};
    AntiLagState state_{};
public:
    explicit AntiLag(AntiLagDriver& driver) noexcept : driver_(driver) {}
    ~AntiLag() { driver_.shutdown(); }
    void bind(uint32_t api, void* device, uint32_t vendor) noexcept {
        if (api_ == api && device_ == device && vendor_ == vendor) return;
        driver_.shutdown(); state_.available = state_.enabled = false;
        api_=api; device_=device; vendor_=vendor; last_update_=last_end_=0;
        if (vendor != 0x1002 || !device || (api != 11 && api != 12)) return;
        state_.available = driver_.initialize(api, device);
        if (!state_.available) ++state_.refused;
    }
    bool pace(uint64_t id, uint32_t mode, uint32_t limit_us, uint32_t other_owner) noexcept {
        if (!state_.available || !id || id <= last_update_) return false;
        last_update_ = id;
        // A required Reflex/XeLL provider owns its one sleep. Never stack a second driver pacer on it.
        const bool enabled = mode != RSF_REFLEX_OFF && other_owner == RSF_PACING_NONE;
        const uint32_t fps = limit_us ? (1000000u/limit_us ? 1000000u/limit_us : 1u) : 0;
        if (!driver_.update(enabled, enabled ? fps : 0)) {
            ++state_.refused; state_.available = state_.enabled = false; driver_.shutdown(); return false;
        }
        state_.enabled = enabled; ++state_.updates; return enabled;
    }
    bool end_render(uint64_t id, void* chain, bool generated) noexcept {
        if (!state_.available || api_ != 12 || !chain || !id || id == last_end_) return false;
        last_end_ = id;
        if (!driver_.end_render(chain, state_.enabled, generated)) {
            ++state_.refused; state_.enabled=false; return false;
        }
        ++state_.end_markers; return true;
    }
    AntiLagState state() const noexcept { return state_; }
};
std::unique_ptr<AntiLagDriver> make_antilag_driver();
}
