// SPDX-License-Identifier: MIT
// Bridge for the native swap fixture's fake engine and independent oracle.
#include "sr_swap.h"
#include "sr_swap_layout.h"

namespace rsf::anomaly::sr {
namespace {
SwapHost& host_of(void* user) noexcept { return *static_cast<SwapHost*>(user); }
bool read(void* user, const void* address, void* output, std::size_t size) noexcept {
    return host_of(user).read(address, output, size);
}
bool write(void* user, void* address, const void* input, std::size_t size) noexcept {
    return host_of(user).write(address, input, size);
}
uint32_t make_twins(void* user, const rsf_anomaly_swap_request* request, rsf_anomaly_swap_twins* twins) noexcept {
    return static_cast<uint32_t>(host_of(user).make_twins(*request, *twins));
}
void drop_twins(void* user) noexcept { host_of(user).drop_twins(); }
bool round_trip(void* user, const rsf_anomaly_swap_fields* twin, const rsf_anomaly_swap_fields* engine) noexcept {
    return host_of(user).round_trip(*twin, *engine);
}
bool upscale(void* user, const rsf_anomaly_swap_upscale* inputs) noexcept { return host_of(user).upscale(*inputs); }
void set_viewport(void* user, uint32_t width, uint32_t height) noexcept { host_of(user).set_viewport(width, height); }
bool upload_projection(void* user, const void* matrix) noexcept { return host_of(user).upload_projection(matrix); }
bool set_mip_bias(void* user, float value) noexcept { return host_of(user).set_mip_bias(value); }
uint32_t correct_position_constant(void* user, void* constant, float x, float y) noexcept {
    return host_of(user).correct_position_constant(constant, x, y);
}
// Shown only if creation failed, which a complete platform and this layout never cause.
constexpr rsf_anomaly_swap_view unavailable{.refusal = "The SR swap owner could not be created."};
}

const char* skip_name(Skip skip) noexcept { return rsf_anomaly_swap_skip_name(static_cast<uint32_t>(skip)); }
const char* restore_name(Restore restore) noexcept { return rsf_anomaly_swap_restore_name(static_cast<uint32_t>(restore)); }

SwapController::SwapController(SwapHost& host) noexcept {
    const rsf_anomaly_swap_platform platform{&host, &read, &write, &make_twins, &drop_twins, &round_trip, &upscale,
        &set_viewport, &upload_projection, &set_mip_bias, &correct_position_constant};
    state_ = rsf_anomaly_swap_create(&verified_layout, &platform);
    view_ = state_ ? rsf_anomaly_swap_view_of(state_) : &unavailable;
}
SwapController::~SwapController() { rsf_anomaly_swap_destroy(state_); }

void SwapController::set_swap_enabled(bool enabled) noexcept { rsf_anomaly_swap_set_enabled(state_, enabled); }
void SwapController::close_stale() noexcept { rsf_anomaly_swap_close_stale(state_); }
bool SwapController::prepare(const FrameFacts& facts) noexcept { return rsf_anomaly_swap_prepare(state_, &facts); }
void SwapController::prepared() noexcept { rsf_anomaly_swap_prepared(state_); }
bool SwapController::seam() noexcept { return rsf_anomaly_swap_seam(state_); }
void SwapController::combine_end() noexcept { rsf_anomaly_swap_combine_end(state_); }
void SwapController::frame_end() noexcept { rsf_anomaly_swap_frame_end(state_); }
void SwapController::rm_near() noexcept { rsf_anomaly_swap_rm_near(state_); }
bool SwapController::sun_cascades_begin() noexcept { return rsf_anomaly_swap_sun_begin(state_); }
void SwapController::sun_cascades_end(bool suspended) noexcept { rsf_anomaly_swap_sun_end(state_, suspended); }
void SwapController::position_constant_setup(void* constant) noexcept { rsf_anomaly_swap_position_constant(state_, constant); }
bool SwapController::shutdown() noexcept { return rsf_anomaly_swap_shutdown(state_); }
}
