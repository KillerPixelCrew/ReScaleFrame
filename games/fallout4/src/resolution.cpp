// SPDX-License-Identifier: MIT
#include "resolution_platform.h"
#include <cstdio>

namespace rsf::fallout4 {
extern "C" bool rsf_fallout4_resolution_reconfigure(const rsf_game_host_services* host, void* context, const rsf_game_render_pass* planning) noexcept try {
    (void)host->process_upscale(host->user, context, planning);
    return true;
} catch (...) { return false; }

extern "C" bool rsf_fallout4_resolution_config(const rsf_game_host_services* host, rsf_game_render_config* config) noexcept try {
    return host->render_config(host->user, config) != 0;
} catch (...) { return false; }

extern "C" bool rsf_fallout4_resolution_reserve(ResolutionReserve reserve, const rsf_game_render_pass* planning) noexcept try {
    return reserve(planning);
} catch (...) { return false; }

extern "C" void rsf_fallout4_resolution_report_receipt(ResolutionReport report, const rsf_game_render_pass* pass, void* context,
    const NativeResolutionReceipt* receipt) noexcept try {
    const auto& facts = *receipt;
    char text[1700]{};
    std::snprintf(text, sizeof(text),
        "Fallout4 reduced-stage=%s session=%llu source=%llu native=%llu/%u result=%d result_is_evaluation=%u published=%u scope=%u/%u changed=%u scalar_read=%u owner=%u input_ratio=%g,%g current_ratio=%g,%g viewport=%u active=%u partial=%u geometry=%p,%p,%p shadow_read=%u shadow=%g,%g,%g,%g,%g,%g dirty=0x%x rs_count=%u rs_read=%u rs=%g,%g,%g,%g,%g,%g camera=%ux%u->%ux%u context=%p",
        facts.stage, static_cast<unsigned long long>(pass->session_id),
        static_cast<unsigned long long>(pass->source_frame_id), static_cast<unsigned long long>(pass->native_frame), facts.frame,
        facts.result, unsigned(facts.evaluation), unsigned(facts.published), unsigned(facts.input), unsigned(facts.output),
        unsigned(facts.changed), unsigned(facts.scalars), unsigned(facts.owner),
        double(facts.input_ratio[0]), double(facts.input_ratio[1]), double(facts.ratio[0]), double(facts.ratio[1]),
        unsigned(facts.viewport), unsigned(facts.active), unsigned(facts.partial),
        reinterpret_cast<void*>(facts.geometry[0]), reinterpret_cast<void*>(facts.geometry[1]),
        reinterpret_cast<void*>(facts.geometry[2]), unsigned(facts.shadow),
        double(facts.raster[0]), double(facts.raster[1]), double(facts.raster[2]), double(facts.raster[3]),
        double(facts.raster[4]), double(facts.raster[5]), facts.dirty, facts.actual_count, facts.actual_read,
        double(facts.actual_raster[0]), double(facts.actual_raster[1]),
        double(facts.actual_raster[2]), double(facts.actual_raster[3]),
        double(facts.actual_raster[4]), double(facts.actual_raster[5]),
        pass->camera.render_width, pass->camera.render_height, pass->camera.output_width, pass->camera.output_height, context);
    report(text);
} catch (...) {}
}
