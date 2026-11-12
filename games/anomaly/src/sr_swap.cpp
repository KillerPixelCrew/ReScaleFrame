// SPDX-License-Identifier: GPL-3.0-only
#include "sr_swap.h"
#include "sr_profile.h"
#include <cstring>
#include <iterator>
#include <string_view>

namespace rsf::anomaly::sr {
namespace {
constexpr uint32_t slot_offset(Slot slot) noexcept {
    return slot == Slot::Generic0 ? 0x3a8u : 0x3b8u;
}
// The offsets above must stay the profile's verified generic0 / generic_temp slots.
consteval bool slots_match_profile() {
    bool generic0 = false, temp = false;
    for (const auto& item : target_slots) {
        const std::string_view name{item.name};
        if (name == "$user$generic0") generic0 = item.offset == slot_offset(Slot::Generic0) && item.d3d_format == d3dfmt_a8r8g8b8;
        if (name == "$user$generic_temp") temp = item.offset == slot_offset(Slot::GenericTemp) && item.d3d_format == d3dfmt_a8r8g8b8;
    }
    return generic0 && temp;
}
static_assert(slots_match_profile());
static_assert(texture_desc_offset + texture2d_desc_size <= texture_srv_offset);
static_assert(crt_format_offset + sizeof(uint32_t) <= crt_order_offset);
}

const char* skip_name(Skip skip) noexcept {
    static constexpr const char* names[] = {"off", "scope", "msaa", "layout", "twins"};
    const auto index = static_cast<std::size_t>(skip);
    return index < std::size(names) ? names[index] : "unknown";
}
const char* restore_name(Restore restore) noexcept {
    static constexpr const char* names[] = {"seam", "combine_end", "stale_prepare", "shutdown", "rollback", "frame_end"};
    const auto index = static_cast<std::size_t>(restore);
    return index < std::size(names) ? names[index] : "unknown";
}

bool SwapController::load(const EngineTarget& target, TargetFields& out) noexcept {
    auto* crt = target.crt;
    auto* texture = target.texture;
    return host_.read(crt + crt_surface_offset, &out.surface, sizeof(void*)) &&
        host_.read(crt + crt_rtv_offset, &out.rtv, sizeof(void*)) &&
        host_.read(crt + crt_dsv_offset, &out.dsv, sizeof(void*)) &&
        host_.read(crt + crt_uav_offset, &out.uav, sizeof(void*)) &&
        host_.read(crt + crt_width_offset, &out.width, sizeof(uint32_t)) &&
        host_.read(crt + crt_height_offset, &out.height, sizeof(uint32_t)) &&
        host_.read(crt + crt_format_offset, &out.format, sizeof(uint32_t)) &&
        host_.read(texture + texture_surface_offset, &out.texture_surface, sizeof(void*)) &&
        host_.read(texture + texture_desc_cache_offset, &out.desc_cache, sizeof(void*)) &&
        host_.read(texture + texture_desc_offset, out.desc.data(), out.desc.size()) &&
        host_.read(texture + texture_srv_offset, &out.srv, sizeof(void*));
}
bool SwapController::store(const EngineTarget& target, const TargetFields& in) noexcept {
    auto* crt = target.crt;
    auto* texture = target.texture;
    return host_.write(crt + crt_surface_offset, &in.surface, sizeof(void*)) &&
        host_.write(crt + crt_rtv_offset, &in.rtv, sizeof(void*)) &&
        host_.write(crt + crt_dsv_offset, &in.dsv, sizeof(void*)) &&
        host_.write(crt + crt_uav_offset, &in.uav, sizeof(void*)) &&
        host_.write(crt + crt_width_offset, &in.width, sizeof(uint32_t)) &&
        host_.write(crt + crt_height_offset, &in.height, sizeof(uint32_t)) &&
        host_.write(crt + crt_format_offset, &in.format, sizeof(uint32_t)) &&
        host_.write(texture + texture_surface_offset, &in.texture_surface, sizeof(void*)) &&
        host_.write(texture + texture_desc_cache_offset, &in.desc_cache, sizeof(void*)) &&
        host_.write(texture + texture_desc_offset, in.desc.data(), in.desc.size()) &&
        host_.write(texture + texture_srv_offset, &in.srv, sizeof(void*));
}
bool SwapController::read_target(std::byte* render_target, Slot slot, const FrameFacts& facts, EngineTarget& out) noexcept {
    out = {};
    if (!host_.read(render_target + slot_offset(slot), &out.crt, sizeof(void*)) || !out.crt) return false;
    if (!host_.read(out.crt + crt_texture_offset, &out.texture, sizeof(void*)) || !out.texture) return false;
    if (!load(out, out.fields)) return false;
    const auto& f = out.fields;
    // Verified layout: a colour CRT of device size and A8R8G8B8, whose CTexture binds that same surface.
    return f.surface && f.rtv && !f.dsv && f.srv && f.texture_surface == f.surface &&
        (!f.desc_cache || f.desc_cache == f.surface) && f.format == d3dfmt_a8r8g8b8 &&
        f.width == facts.device_width && f.height == facts.device_height;
}
bool SwapController::twins_match(const std::array<EngineTarget, slot_count>& engine) const noexcept {
    for (std::size_t i = 0; i < slot_count; ++i) {
        const auto& twin = twins_[i];
        const auto& original = engine[i].fields;
        if (!twin.surface || !twin.rtv || twin.dsv || !twin.srv || twin.texture_surface != twin.surface ||
            twin.desc_cache != twin.surface || twin.format != d3dfmt_a16b16g16r16f || twin.width != original.width ||
            twin.height != original.height || bool(twin.uav) != bool(original.uav) || twin.surface == original.surface)
            return false;
    }
    return true;
}
void SwapController::invalidate_texture_cache() noexcept {
    if (!backend_) return;
    // CBackend::set_Textures returns early on T == _T and then compares CTexture*, which a surface swap does not
    // change. Clearing T and every per-stage CTexture* slot forces the next set_Textures to bind the new SRVs.
    void* null{};
    bool ok = host_.write(backend_ + backend_texture_list_offset, &null, sizeof(null));
    const struct { uint32_t offset, count; } stages[] = {{backend_textures_ps_offset, backend_textures_ps_count},
        {backend_textures_vs_offset, backend_textures_vs_count}, {backend_textures_gs_offset, backend_textures_stage_count},
        {backend_textures_hs_offset, backend_textures_stage_count}, {backend_textures_ds_offset, backend_textures_stage_count},
        {backend_textures_cs_offset, backend_textures_stage_count}};
    for (const auto& stage : stages)
        for (uint32_t i = 0; i < stage.count; ++i)
            ok = host_.write(backend_ + stage.offset + i * sizeof(void*), &null, sizeof(null)) && ok;
    if (!ok) { ++counters_.write_faults; refuse("RCache texture cache could not be invalidated."); }
}
void SwapController::drop() noexcept {
    if (!have_twins_) return;
    host_.drop_twins();
    have_twins_ = false;
    twins_ = {};
    key_ = {};
    ++counters_.twin_drops;
}

bool SwapController::prepare(const FrameFacts& facts) noexcept {
    ++counters_.prepares;
    if (open_) restore(Restore::StalePrepare);
    const auto skip = [this](Skip reason) { ++counters_.skipped[static_cast<std::size_t>(reason)]; return false; };
    if (!swap_enabled_ || refusal_) return skip(Skip::Off);
    if (facts.scope) return skip(Skip::Scope);
    if (facts.msaa) return skip(Skip::Msaa);
    std::array<EngineTarget, slot_count> engine{};
    if (!facts.render_target || !read_target(facts.render_target, Slot::Generic0, facts, engine[0]) ||
        !read_target(facts.render_target, Slot::GenericTemp, facts, engine[1]) || engine[0].crt == engine[1].crt ||
        engine[0].fields.surface == engine[1].fields.surface) {
        refuse("generic0/generic_temp do not match the verified CRT/CTexture layout.");
        return skip(Skip::Layout);
    }
    const TwinKey key{facts.render_target, {engine[0].crt, engine[1].crt},
        {engine[0].fields.surface, engine[1].fields.surface}, facts.device, facts.generation,
        facts.device_width, facts.device_height};
    if (!have_twins_ || !(key == key_)) {
        // Reset, resolution change or a recreated CRenderTarget: drop the old twins and build new ones.
        drop();
        std::array<TargetFields, slot_count> made{};
        if (!host_.make_twins(engine, made)) {
            refuse("FP16 twins could not be created.");
            return skip(Skip::Twins);
        }
        twins_ = made;
        have_twins_ = true;
        if (!twins_match(engine)) {
            drop();
            refuse("Created twins do not match the engine targets.");
            return skip(Skip::Twins);
        }
        key_ = key;
        ++counters_.twin_generations;
    }
    ++counters_.eligible;
    backend_ = facts.backend;
    engine_ = engine;
    std::size_t written = 0;
    for (; written < slot_count; ++written)
        if (!store(engine_[written], twins_[written])) break;
    open_ = true;
    if (written != slot_count) {
        ++counters_.write_faults;
        refuse("An engine field write failed during the twin exchange.");
        restore(Restore::Rollback);
        return false;
    }
    invalidate_texture_cache();
    ++counters_.swapped;
    return true;
}

void SwapController::restore(Restore reason) noexcept {
    if (!open_) return;
    for (std::size_t i = 0; i < slot_count; ++i) {
        TargetFields current{};
        // Inside the window nothing but the engine's own binds should touch these fields. A different value means
        // something replaced them (surface_set on a twinned texture): restore anyway and refuse further swaps.
        if (load(engine_[i], current) && !(current == twins_[i]) && reason != Restore::Rollback) {
            ++counters_.foreign_changes;
            refuse("A twinned engine field changed inside the world window.");
        }
        if (!store(engine_[i], engine_[i].fields)) {
            ++counters_.write_faults;
            refuse("An engine field write failed while restoring.");
        }
    }
    open_ = false;
    invalidate_texture_cache();
    ++counters_.restores[static_cast<std::size_t>(reason)];
}

bool SwapController::seam() noexcept {
    if (!open_) { ++counters_.seams_without_window; return false; }
    ++counters_.seams;
    restore(Restore::Seam);
    ++counters_.round_trips;
    if (!host_.round_trip(twins_[0], engine_[0].fields)) {
        ++counters_.round_trip_failures;
        refuse("The round-trip pass failed; engine generic0 holds no world image for this frame.");
    }
    return true;
}

void SwapController::combine_end() noexcept {
    ++counters_.combine_ends;
    if (!open_) return;
    restore(Restore::CombineEnd);
    refuse("phase_combine returned without the S-B seam; the frame was rendered without its world image.");
}

void SwapController::frame_end() noexcept {
    if (!open_) return;
    restore(Restore::FrameEnd);
    refuse("A world window was still open at the end of the frame.");
}

void SwapController::rm_near() noexcept {
    if (open_) ++counters_.rm_near_in_window;
}

void SwapController::shutdown() noexcept {
    restore(Restore::Shutdown);
    drop();
}
}
