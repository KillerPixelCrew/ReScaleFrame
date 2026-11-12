// SPDX-License-Identifier: GPL-3.0-only
#include "sr_swap.h"
#include "sr_profile.h"
#include <cstring>
#include <iterator>
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
static_assert(world_targets_match_profile());
static_assert(texture_desc_offset + texture2d_desc_size <= texture_srv_offset);
static_assert(crt_format_offset + sizeof(uint32_t) <= crt_order_offset);
static_assert(ssao_viewport_height_offset == ssao_viewport_width_offset + sizeof(float));
}

const char* skip_name(Skip skip) noexcept {
    static constexpr const char* names[] = {"off", "scope", "msaa", "layout", "twins", "depth"};
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

bool SwapController::read_targets(const FrameFacts& facts, std::array<EngineTarget, slot_count>& engine,
    TwinRequest& request) noexcept {
    engine = {};
    request = {};
    request.stage = stage_;
    request.output_width = facts.device_width;
    request.output_height = facts.device_height;
    request.render_width = stage_ == 1 ? facts.device_width : facts.render_width;
    request.render_height = stage_ == 1 ? facts.device_height : facts.render_height;
    for (std::size_t i = 0; i < slots(); ++i) {
        const auto& world = sr_world_targets[i];
        auto& out = engine[i];
        if (!host_.read(facts.render_target + world.offset, &out.crt, sizeof(void*))) return false;
        if (!out.crt) {
            if (stage_ == 2 && world.optional) continue;   // option off: the engine never renders into it
            return false;
        }
        if (!host_.read(out.crt + crt_texture_offset, &out.texture, sizeof(void*)) || !out.texture) return false;
        if (!load(out, out.fields)) return false;
        const auto& f = out.fields;
        // Verified layout: a colour CRT of the expected format whose CTexture binds that same surface.
        if (!f.surface || !f.rtv || f.dsv || !f.srv || f.texture_surface != f.surface ||
            (f.desc_cache && f.desc_cache != f.surface))
            return false;
        if (world.d3d_format ? f.format != world.d3d_format : f.format != d3dfmt_r16f && f.format != d3dfmt_r32f) return false;
        auto& spec = request.spec[i];
        if (stage_ == 1) {
            if (f.width != facts.device_width || f.height != facts.device_height) return false;
            spec = {true, f.width, f.height, true};
        } else {
            Extent horizontal{}, vertical{};
            uint32_t width{}, height{};
            if (!twin_extent(f.width, facts.device_width, facts.render_width, world.half_allowed, width, horizontal) ||
                !twin_extent(f.height, facts.device_height, facts.render_height, world.half_allowed, height, vertical) ||
                horizontal != vertical)
                return false;
            spec = {true, width, height, slot_promoted(i)};
        }
        out.present = true;
    }
    // Distinct records and surfaces (two slots sharing a CRT would be exchanged twice).
    for (std::size_t i = 0; i < slots(); ++i)
        for (std::size_t j = i + 1; j < slots(); ++j)
            if (engine[i].present && engine[j].present &&
                (engine[i].crt == engine[j].crt || engine[i].fields.surface == engine[j].fields.surface))
                return false;
    if (stage_ == 2)
        for (const auto offset : sr_world_absent_offsets) {
            std::byte* crt{};
            if (!host_.read(facts.render_target + offset, &crt, sizeof(crt)) || crt) return false;
        }
    request.engine = engine;
    return true;
}

bool SwapController::twins_match(const TwinRequest& request) const noexcept {
    for (std::size_t i = 0; i < slots(); ++i) {
        const auto& spec = request.spec[i];
        const auto& twin = twins_.targets[i];
        const auto& original = request.engine[i].fields;
        if (!spec.present) {
            if (!(twin == TargetFields{})) return false;
            continue;
        }
        if (!twin.surface || !twin.rtv || twin.dsv || !twin.srv || twin.texture_surface != twin.surface ||
            twin.desc_cache != twin.surface || twin.format != (spec.promote ? d3dfmt_a16b16g16r16f : original.format) ||
            twin.width != spec.width || twin.height != spec.height || bool(twin.uav) != bool(original.uav) ||
            twin.surface == original.surface)
            return false;
    }
    if (request.stage == 2 && (!twins_.depth_dsv || !twins_.depth_srv || twins_.depth_dsv == request.engine_dsv ||
        twins_.depth_width != request.render_width || twins_.depth_height != request.render_height || !twins_.base_rt ||
        twins_.base_rt == request.engine_rtv))
        return false;
    return true;
}

void SwapController::invalidate_caches() noexcept {
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
    // Stage 2 changes Device.dwWidth/dwHeight, which the screen_res and pos_decompression_params binders read when
    // CBackend::set_Constants processes a constant table. set_Constants returns early on ctable == C, so clearing
    // ctable makes the next set_Element re-run every binder with the current size.
    if (stage_ == 2) ok = host_.write(backend_ + backend_ctable_offset, &null, sizeof(null)) && ok;
    if (!ok) { ++counters_.write_faults; refuse("RCache caches could not be invalidated."); }
}
void SwapController::drop() noexcept {
    if (!have_twins_) return;
    host_.drop_twins();
    have_twins_ = false;
    twins_ = {};
    key_ = {};
    ++counters_.twin_drops;
}

bool SwapController::viewport_initialised() noexcept {
    int32_t guard{};
    // MSVC magic static: 0 never initialised, -1 initialisation in progress, anything else initialised.
    return globals_.viewport_guard && host_.read(globals_.viewport_guard, &guard, sizeof(guard)) && guard != 0 && guard != -1;
}
bool SwapController::open_globals() noexcept {
    auto& g = globals_;
    bool ok = host_.write(g.device_size, &render_width_, sizeof(uint32_t)) &&
        host_.write(g.device_size + sizeof(uint32_t), &render_height_, sizeof(uint32_t)) &&
        host_.write(g.base_zb, &twins_.depth_dsv, sizeof(void*)) &&
        host_.write(g.base_rt, &twins_.base_rt, sizeof(void*));
    if (ok && g.viewport && viewport_initialised()) {
        float saved[2]{};
        const float window[2]{float(render_width_) * 0.5f, float(render_height_) * 0.5f};
        if (host_.read(g.viewport + ssao_viewport_width_offset, saved, sizeof(saved))) {
            g.viewport_width = saved[0];
            g.viewport_height = saved[1];
            g.viewport_swapped = true;   // before the write: a partial write is restored too
            ok = host_.write(g.viewport + ssao_viewport_width_offset, window, sizeof(window));
            if (ok) ++counters_.ssao_viewport_swaps;
        } else ok = false;
    }
    return ok;
}
void SwapController::restore_globals(bool rollback) noexcept {
    auto& g = globals_;
    uint32_t size[2]{};
    void* zb{};
    void* rt{};
    if (!rollback && host_.read(g.device_size, size, sizeof(size)) && (size[0] != render_width_ || size[1] != render_height_)) {
        ++counters_.foreign_changes;
        refuse("Device.dwWidth/dwHeight changed inside the world window.");
    }
    if (!rollback && host_.read(g.base_zb, &zb, sizeof(zb)) && zb != twins_.depth_dsv) {
        ++counters_.foreign_changes;
        refuse("HW.pBaseZB changed inside the world window.");
    }
    if (!rollback && host_.read(g.base_rt, &rt, sizeof(rt)) && rt != twins_.base_rt) {
        ++counters_.foreign_changes;
        refuse("HW.pBaseRT changed inside the world window.");
    }
    const uint32_t output[2]{g.width, g.height};
    bool ok = host_.write(g.device_size, output, sizeof(output)) && host_.write(g.base_zb, &g.zb, sizeof(void*)) &&
        host_.write(g.base_rt, &g.rt, sizeof(void*));
    if (g.viewport_swapped) {
        const float saved[2]{g.viewport_width, g.viewport_height};
        ok = host_.write(g.viewport + ssao_viewport_width_offset, saved, sizeof(saved)) && ok;
        g.viewport_swapped = false;
    } else if (!rollback && g.viewport && !g.viewport_guard_was_set && viewport_initialised()) {
        // First SSAO call of the process happened inside the window and stored render/2: store what stock would have.
        const float stock[2]{float(g.width) * 0.5f, float(g.height) * 0.5f};
        ok = host_.write(g.viewport + ssao_viewport_width_offset, stock, sizeof(stock)) && ok;
    }
    // CRenderTarget::dwWidth/dwHeight follow the last u_setrt (a render-size twin): make rmNear/rmNormal and the
    // next viewport describe the output again.
    if (!rollback && render_target_) {
        ok = host_.write(render_target_ + target_width_offset, &g.width, sizeof(uint32_t)) &&
            host_.write(render_target_ + target_height_offset, &g.height, sizeof(uint32_t)) && ok;
    }
    if (!ok) { ++counters_.write_faults; refuse("An engine global could not be restored."); }
}

void SwapController::restore(Restore reason) noexcept {
    if (!open_) return;
    for (std::size_t i = 0; i < slots(); ++i) {
        if (!engine_[i].present) continue;
        TargetFields current{};
        // Inside the window nothing but the engine's own binds should touch these fields. A different value means
        // something replaced them (surface_set on a twinned texture): restore anyway and refuse further swaps.
        if (load(engine_[i], current) && !(current == twins_.targets[i]) && reason != Restore::Rollback) {
            ++counters_.foreign_changes;
            refuse("A twinned engine field changed inside the world window.");
        }
        if (!store(engine_[i], engine_[i].fields)) {
            ++counters_.write_faults;
            refuse("An engine field write failed while restoring.");
        }
    }
    if (stage_ == 2) restore_globals(reason == Restore::Rollback);
    open_ = false;
    invalidate_caches();
    ++counters_.restores[static_cast<std::size_t>(reason)];
}

void SwapController::close_stale() noexcept {
    if (!open_) return;
    restore(Restore::StalePrepare);
    if (stage_ == 2) { host_.set_viewport(output_width_, output_height_); ++counters_.viewports; }
}

bool SwapController::prepare(const FrameFacts& facts) noexcept {
    ++counters_.prepares;
    close_stale();
    const auto skip = [this](Skip reason) { ++counters_.skipped[static_cast<std::size_t>(reason)]; return false; };
    if (!swap_enabled_ || refusal_) return skip(Skip::Off);
    if (facts.scope) return skip(Skip::Scope);
    if (facts.msaa) return skip(Skip::Msaa);
    stage_ = facts.stage == 2 ? 2u : 1u;
    if (stage_ == 2 && (!facts.device_size || !facts.base_zb || !facts.base_rt || !facts.render_width || !facts.render_height ||
        facts.render_width > facts.device_width || facts.render_height > facts.device_height)) {
        refuse("The Stage 2 frame facts are incomplete.");
        return skip(Skip::Layout);
    }
    std::array<EngineTarget, slot_count> engine{};
    TwinRequest request{};
    if (!facts.render_target || !read_targets(facts, engine, request)) {
        refuse(stage_ == 1 ? "generic0/generic_temp do not match the verified CRT/CTexture layout."
                           : "A world target does not match the verified CRT/CTexture layout.");
        return skip(Skip::Layout);
    }
    if (stage_ == 2) {
        uint32_t size[2]{};
        if (!host_.read(facts.device_size, size, sizeof(size)) || size[0] != facts.device_width || size[1] != facts.device_height) {
            refuse("Device.dwWidth/dwHeight do not match the frame facts.");
            return skip(Skip::Layout);
        }
        if (!host_.read(facts.base_zb, &request.engine_dsv, sizeof(void*)) || !request.engine_dsv) {
            refuse("HW.pBaseZB is unreadable or null.");
            return skip(Skip::Depth);
        }
        if (!host_.read(facts.base_rt, &request.engine_rtv, sizeof(void*)) || !request.engine_rtv) {
            refuse("HW.pBaseRT is unreadable or null.");
            return skip(Skip::Layout);
        }
    }
    TwinKey key{stage_, facts.render_target, {}, {}, request.engine_dsv, request.engine_rtv, facts.device, facts.generation,
        facts.device_width, facts.device_height, request.render_width, request.render_height};
    for (std::size_t i = 0; i < slot_count; ++i) { key.crt[i] = engine[i].crt; key.surface[i] = engine[i].fields.surface; }
    if (failed_key_valid_ && key == failed_key_) return skip(Skip::Twins);   // allocation failed for this exact shape
    if (!have_twins_ || !(key == key_)) {
        // Reset, resolution or scale change, or a recreated CRenderTarget: drop the old twins and build new ones.
        drop();
        TwinSet made{};
        switch (host_.make_twins(request, made)) {
        case MakeResult::Created: break;
        case MakeResult::Failed:
            ++counters_.allocation_failures;
            failed_key_ = key;
            failed_key_valid_ = true;
            return skip(Skip::Twins);
        case MakeResult::Depth:
            refuse("HW.pBaseZB is not a single-sample D24S8 depth view of output size.");
            return skip(Skip::Depth);
        default:
            refuse("An engine target cannot be twinned.");
            return skip(Skip::Twins);
        }
        twins_ = made;
        have_twins_ = true;
        if (!twins_match(request)) {
            drop();
            refuse("Created twins do not match the engine targets.");
            return skip(Skip::Twins);
        }
        key_ = key;
        failed_key_valid_ = false;
        ++counters_.twin_generations;
    }
    ++counters_.eligible;
    backend_ = facts.backend;
    render_target_ = facts.render_target;
    engine_ = engine;
    output_width_ = facts.device_width;
    output_height_ = facts.device_height;
    render_width_ = request.render_width;
    render_height_ = request.render_height;
    globals_ = {};
    if (stage_ == 2) {
        globals_ = {facts.device_size, facts.base_zb, facts.base_rt, facts.ssao_viewport, facts.ssao_viewport_guard,
            facts.device_width, facts.device_height, request.engine_dsv, request.engine_rtv};
        globals_.viewport_guard_was_set = viewport_initialised();
    }
    open_ = true;
    bool written = true;
    for (std::size_t i = 0; i < slots() && written; ++i)
        if (engine_[i].present) written = store(engine_[i], twins_.targets[i]);
    if (written && stage_ == 2) written = open_globals();
    if (!written) {
        ++counters_.write_faults;
        refuse("An engine field write failed during the twin exchange.");
        restore(Restore::Rollback);
        return false;
    }
    invalidate_caches();
    ++counters_.swapped;
    return true;
}

void SwapController::prepared() noexcept {
    // phase_scene_prepare bound rt_Position (render size) with u_setrt(Device.dwWidth, ...), but the viewport is still
    // the one CRender::Render's rmNormal set from the previous frame's output-size target.
    if (!open_ || stage_ != 2) return;
    host_.set_viewport(render_width_, render_height_);
    ++counters_.viewports;
}

bool SwapController::seam() noexcept {
    if (!open_) { ++counters_.seams_without_window; return false; }
    ++counters_.seams;
    restore(Restore::Seam);
    ++counters_.round_trips;
    bool ok;
    if (stage_ == 1) ok = host_.round_trip(twins_.targets[0], engine_[0].fields);
    else {
        UpscaleInputs inputs{&twins_.targets[slot_index(Slot::Generic0)], &twins_.targets[slot_index(Slot::Position)],
            &twins_.targets[slot_index(Slot::Generic1)], twins_.depth_srv, &engine_[slot_index(Slot::Generic0)].fields,
            &engine_[slot_index(Slot::Position)].fields, &engine_[slot_index(Slot::Generic1)].fields, globals_.zb,
            render_width_, render_height_, output_width_, output_height_};
        ok = host_.upscale(inputs);
        host_.set_viewport(output_width_, output_height_);
        ++counters_.viewports;
    }
    if (!ok) {
        ++counters_.round_trip_failures;
        refuse("The seam pass failed; engine generic0 holds no world image for this frame.");
    }
    return true;
}

void SwapController::combine_end() noexcept {
    ++counters_.combine_ends;
    if (!open_) return;
    restore(Restore::CombineEnd);
    if (stage_ == 2) { host_.set_viewport(output_width_, output_height_); ++counters_.viewports; }
    refuse("phase_combine returned without the S-B seam; the frame was rendered without its world image.");
}

void SwapController::frame_end() noexcept {
    if (!open_) return;
    restore(Restore::FrameEnd);
    if (stage_ == 2) { host_.set_viewport(output_width_, output_height_); ++counters_.viewports; }
    refuse("A world window was still open at the end of the frame.");
}

void SwapController::rm_near() noexcept {
    if (open_) ++counters_.rm_near_in_window;
}

void SwapController::shutdown() noexcept {
    restore(Restore::Shutdown);
    drop();
    failed_key_valid_ = false;
}
}
