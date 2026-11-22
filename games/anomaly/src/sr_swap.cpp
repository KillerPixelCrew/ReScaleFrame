// SPDX-License-Identifier: MIT
// The native side of the SR world-window swap: the verified engine layout from sr_profile.h, and the SwapHost calls
// lent to the Rust owner (sr_swap.rs) as its platform.
#include "sr_swap.h"
#include "sr_profile.h"
#include <string_view>

namespace rsf::anomaly::sr {
namespace {
// Every world target must be the profile's verified CRenderTarget slot with the same offset and format.
consteval bool world_targets_match_profile() {
    for (const auto& world : sr_world_targets) {
        bool found = false;
        for (const auto& item : target_slots)
            if (std::string_view{item.name} == std::string_view{world.name})
                found = item.offset == world.offset && item.d3d_format == world.d3d_format;
        if (!found) return false;
    }
    return std::string_view{sr_world_targets[slot_index(Slot::Generic0)].name} == "$user$generic0" &&
        std::string_view{sr_world_targets[slot_index(Slot::GenericTemp)].name} == "$user$generic_temp" &&
        std::string_view{sr_world_targets[slot_index(Slot::Position)].name} == "$user$position" &&
        std::string_view{sr_world_targets[slot_index(Slot::Generic1)].name} == "$user$generic1" &&
        sr_world_targets[slot_index(Slot::Generic0)].d3d_format == d3dfmt_a8r8g8b8 &&
        sr_world_targets[slot_index(Slot::GenericTemp)].d3d_format == d3dfmt_a8r8g8b8;
}
static_assert(sr_world_targets.size() == slot_count);
static_assert(sr_world_absent_offsets.size() == RSF_ANOMALY_SWAP_ABSENT_COUNT);
static_assert(world_targets_match_profile());
static_assert(texture_desc_offset + texture2d_desc_size <= texture_srv_offset);
static_assert(crt_format_offset + sizeof(uint32_t) <= crt_order_offset);
static_assert(ssao_viewport_height_offset == ssao_viewport_width_offset + sizeof(float));
static_assert(sizeof(rsf_anomaly_swap_fields) == 120 && offsetof(rsf_anomaly_swap_fields, desc) == 64);

// The layout shared by both executable profiles, in the Rust owner's form.
constexpr rsf_anomaly_swap_layout engine_layout() noexcept {
    rsf_anomaly_swap_layout layout{};
    layout.struct_size = sizeof(layout);
    layout.crt_surface = crt_surface_offset;
    layout.crt_rtv = crt_rtv_offset;
    layout.crt_dsv = crt_dsv_offset;
    layout.crt_uav = crt_uav_offset;
    layout.crt_texture = crt_texture_offset;
    layout.crt_width = crt_width_offset;
    layout.crt_height = crt_height_offset;
    layout.crt_format = crt_format_offset;
    layout.texture_surface = texture_surface_offset;
    layout.texture_desc_cache = texture_desc_cache_offset;
    layout.texture_desc = texture_desc_offset;
    layout.texture_srv = texture_srv_offset;
    layout.target_width = target_width_offset;
    layout.target_height = target_height_offset;
    layout.backend_texture_list = backend_texture_list_offset;
    layout.backend_ctable = backend_ctable_offset;
    layout.backend_textures[0] = {backend_textures_ps_offset, backend_textures_ps_count};
    layout.backend_textures[1] = {backend_textures_vs_offset, backend_textures_vs_count};
    layout.backend_textures[2] = {backend_textures_gs_offset, backend_textures_stage_count};
    layout.backend_textures[3] = {backend_textures_hs_offset, backend_textures_stage_count};
    layout.backend_textures[4] = {backend_textures_ds_offset, backend_textures_stage_count};
    layout.backend_textures[5] = {backend_textures_cs_offset, backend_textures_stage_count};
    layout.ssao_viewport_width = ssao_viewport_width_offset;
    layout.format_a16b16g16r16f = d3dfmt_a16b16g16r16f;
    layout.format_r16f = d3dfmt_r16f;
    layout.format_r32f = d3dfmt_r32f;
    for (std::size_t i = 0; i < slot_count; ++i)
        layout.world[i] = {sr_world_targets[i].offset, sr_world_targets[i].d3d_format, sr_world_targets[i].optional,
            sr_world_targets[i].half_allowed};
    for (std::size_t i = 0; i < sr_world_absent_offsets.size(); ++i) layout.absent[i] = sr_world_absent_offsets[i];
    return layout;
}
constexpr rsf_anomaly_swap_layout verified_layout = engine_layout();

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
