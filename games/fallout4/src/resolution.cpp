// SPDX-License-Identifier: MIT
#include "resolution_platform.h"
#include "depth_domains.h"
#include "screen.h"
#include <windows.h>
#include <d3d11.h>
#include <intrin.h>
#include <cstdio>

namespace rsf::fallout4 {
namespace {
uintptr_t tls_slots() noexcept {
    return static_cast<uintptr_t>(__readgsqword(0x58));
}

uint32_t screen_refusals(const rsf_fallout4_cpu_identity* source) noexcept {
    const CpuIdentity cpu{source->session, source->source, source->submission,
        source->began_qpc, source->frequency};
    const rsf_game_render_pass candidate{};
    return classify_native_screen(cpu, candidate, true).refusals;
}

bool reconfigure(const rsf_game_host_services* host, void* context, const rsf_game_render_pass* planning) noexcept try {
    (void)host->process_upscale(host->user, context, planning);
    return true;
} catch (...) { return false; }

bool render_config(const rsf_game_host_services* host, rsf_game_render_config* config) noexcept try {
    return host->render_config(host->user, config) != 0;
} catch (...) { return false; }

bool reserve_output(ResolutionReserve reserve, const rsf_game_render_pass* planning) noexcept try {
    return reserve(*planning);
} catch (...) { return false; }

void build_viewport(uintptr_t routine, uintptr_t renderer, uintptr_t shadow,
    uint32_t width, uint32_t height) noexcept {
    using Viewport = void (*)(void*, void*, uint32_t, uint32_t, bool);
    reinterpret_cast<Viewport>(routine)(reinterpret_cast<void*>(renderer),
        reinterpret_cast<void*>(shadow), width, height, true);
}

void report_receipt(ResolutionReport report, const rsf_game_render_pass* pass, void* context,
    const NativeResolutionReceipt* receipt) noexcept try {
    const auto& facts = *receipt;
    UINT count{};
    D3D11_VIEWPORT actual{};
    bool rs{};
    if (facts.owner) {
        auto* device_context = static_cast<ID3D11DeviceContext*>(context);
        device_context->RSGetViewports(&count, nullptr);
        if (count == 1) {
            UINT capacity = 1;
            device_context->RSGetViewports(&capacity, &actual);
            rs = capacity == 1;
        }
    }
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
        double(facts.raster[4]), double(facts.raster[5]), facts.dirty, count, unsigned(rs),
        double(actual.TopLeftX), double(actual.TopLeftY), double(actual.Width), double(actual.Height),
        double(actual.MinDepth), double(actual.MaxDepth),
        pass->camera.render_width, pass->camera.render_height, pass->camera.output_width, pass->camera.output_height, context);
    report(text);
} catch (...) {}

const NativeResolutionLeaves leaves{tls_slots, screen_refusals, reconfigure, render_config, reserve_output,
    build_viewport, report_receipt};
}

void report_resolution_state(ResolutionStage stage, const rsf_game_render_pass& pass, void* context,
    bool published, int32_t result, ResolutionReport report) noexcept {
    struct Restore { DWORD error; ~Restore() { SetLastError(error); } } restore{GetLastError()};
    rsf_fallout4_resolution_report(&leaves, static_cast<uint32_t>(stage), &pass, context,
        published, result, report);
}

bool verify_resolution() noexcept { return rsf_fallout4_resolution_verify(); }

bool native_resolution_inputs(const rsf_game_render_pass& pass) noexcept {
    return rsf_fallout4_resolution_inputs(&pass);
}

bool configure_native_resolution(const rsf_game_host_services& host, const rsf_game_render_pass& frame,
    void* context, ResolutionReserve reserve) noexcept {
    return rsf_fallout4_resolution_configure(&leaves, &host, &frame, context, reserve);
}

bool preflight_output_resolution(const rsf_game_render_pass& pass, void* context,
    NativeOutputOwner* output) noexcept {
    return rsf_fallout4_resolution_preflight(&leaves, &pass, context, output);
}

bool begin_output_resolution(const NativeDepthSnapshot& snapshot, const rsf_game_render_pass& pass) noexcept {
    return rsf_fallout4_resolution_begin_output(&leaves, native_depth_leaves(), &snapshot, &pass);
}

bool native_resolution_write() noexcept { return rsf_fallout4_resolution_native_dr(&leaves); }
bool end_output_resolution() noexcept { return rsf_fallout4_resolution_end_output(&leaves); }
bool finish_resolution_source() noexcept { return rsf_fallout4_resolution_finish_source(&leaves); }
}
