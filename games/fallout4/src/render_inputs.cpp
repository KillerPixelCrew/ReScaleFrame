// SPDX-License-Identifier: MIT
// Receipt formatting and transfer of the Rust input lease into the native World.
#include "render_inputs.h"
#include "render_inputs_platform.h"
#include <cstdio>
#include <utility>

namespace rsf::fallout4 {
// Each nonnull field receives one owned reference from Rust. Attach transfers
// it to the result; refusal and unused slots release it here.
struct NativeInputLease {
    NativeInputReferences refs{};

    ~NativeInputLease() {
        for (auto* texture : refs.textures) if (texture) static_cast<ID3D11Texture2D*>(texture)->Release();
        if (refs.chain) static_cast<IDXGISwapChain*>(refs.chain)->Release();
        if (refs.context) static_cast<ID3D11DeviceContext*>(refs.context)->Release();
    }
};
static_assert(sizeof(NativeInputReferences) == 8 * sizeof(void*) &&
    offsetof(NativeInputReferences, textures) == 16);

extern "C" void* rsf_fallout4_input_object(const void* opaque, uint32_t slot) noexcept {
    const auto& inputs = *static_cast<const NativeInputs*>(opaque);
    switch (slot) {
    case native_input_context: return inputs.context.Get();
    case native_input_chain: return inputs.chain.Get();
    case native_input_colour: return inputs.colour.Get();
    case native_input_motion: return inputs.motion.Get();
    case native_input_depth: return inputs.depth.Get();
    case native_input_refraction_normals: return inputs.refraction_normals.Get();
    case native_input_pipboy: return inputs.pipboy.Get();
    case native_input_vats: return inputs.vats.Get();
    default: return nullptr;
    }
}

// The text the former reader receipt wrote; Rust decides the stage and its once-per-stage mask.
extern "C" void rsf_fallout4_inputs_report_reader(NativeInputReport report, const char* stage, const NativeInputReceipt* receipt,
    const rsf_game_render_pass* pass) noexcept {
    if (!stage || !receipt || !pass) return;
    try {
        const auto& r = *receipt;
        const auto& before = r.before;
        const auto& native = r.native;
        char text[1600]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 reader stage=%s native=%u partial=1 renderer=%p expected=%p device=%p context=%p chain=%p window=%p output=%ux%u camera=%p cache=%p count=%u scanned=%u camera_matches=%u matching_jitter=%u selected=%p last_key=%p last_jitter=%u taa=%u disable=%u dt=%g metadata_viewport=%g,%g,%g,%g default=%u dynamic=%u rs_count=%u rs=%g,%g,%g,%g,%g,%g confirmed_rs=%u/%g,%g ratio=%g,%g maps=%u,%u,%u swap=%ux%u/%p rtv=%u/%u/%u",
            stage,before.frame,reinterpret_cast<void*>(before.renderer),reinterpret_cast<void*>(r.expected_renderer),
            reinterpret_cast<void*>(before.device),reinterpret_cast<void*>(before.context),reinterpret_cast<void*>(before.chain),reinterpret_cast<void*>(before.window),
            before.width,before.height,reinterpret_cast<void*>(before.camera),reinterpret_cast<void*>(before.cache),before.count,r.scanned,r.camera_matches,r.matching_jitter,
            reinterpret_cast<void*>(r.selected),reinterpret_cast<void*>(r.last_key),r.last_jitter,before.taa,before.disable,double(before.delta_seconds),
            double(before.viewport[0]),double(before.viewport[1]),double(before.viewport[2]),double(before.viewport[3]),unsigned(before.default_viewport),unsigned(before.dynamic_viewport),
            r.viewport_count,double(r.viewport[0]),double(r.viewport[1]),double(r.viewport[2]),double(r.viewport[3]),double(r.viewport[4]),double(r.viewport[5]),
            r.confirmed_viewport_count,double(r.confirmed_viewport[2]),double(r.confirmed_viewport[3]),double(before.ratio[0]),double(before.ratio[1]),
            before.colour,before.motion,before.depth,r.swap.width,r.swap.height,reinterpret_cast<void*>(r.swap.window),
            r.rtv.view_format,r.rtv.view_dimension,r.rtv.view_mip);
        report(text);
        constexpr const char* names[]{"colour","motion","depth"};
        for (size_t i=0;i<3;++i) if (r.views[i].read) {
            const auto& view=r.views[i];
            std::snprintf(text,sizeof(text),
                "Fallout4 reader stage=%s %s expected=%p actual=%p resource_match=%u device_match=%u texture=%ux%u format=%u mips=%u array=%u sample=%u/%u bind=0x%x srv_format=%u srv_dimension=%u srv_mip=%u srv_mips=%u",
                stage,names[i],reinterpret_cast<void*>(r.expected[i]),reinterpret_cast<void*>(view.pointer),r.resource_match[i],r.device_match[i],
                view.width,view.height,view.format,view.mip_levels,view.array_size,view.sample_count,view.sample_quality,view.bind_flags,
                view.view_format,view.view_dimension,view.view_mip,view.view_mips);
            report(text);
        }
        if (r.camera_read) {
            std::snprintf(text,sizeof(text),
                "Fallout4 reader stage=%s projection=%g,%g,%g,%g,%g,%g origin=%g,%g,%g previous_origin=%g,%g,%g global_jitter=%g,%g confirmed_native=%u camera_valid=%u render=%ux%u near=%g far=%g jitter=%g,%g",
                stage,double(native.projection[0]),double(native.projection[5]),double(native.projection[10]),double(native.projection[11]),double(native.projection[14]),double(native.projection[15]),
                double(native.position_adjust[0]),double(native.position_adjust[1]),double(native.position_adjust[2]),double(native.previous_position_adjust[0]),double(native.previous_position_adjust[1]),double(native.previous_position_adjust[2]),
                double(native.jitter_ndc[0]),double(native.jitter_ndc[1]),r.after_frame,pass->camera_valid,pass->camera.render_width,pass->camera.render_height,
                double(pass->camera.near_plane),double(pass->camera.far_plane),double(pass->camera.jitter_pixels[0]),double(pass->camera.jitter_pixels[1]));
            report(text);
        }
    } catch (...) {}
}
// The text of the former state reread mismatch; Rust keeps the per-mask and sixteen-text budget.
extern "C" void rsf_fallout4_inputs_report_mismatch(NativeInputReport report, const NativeInputState* first, const NativeInputState* second,
    uint32_t mask) noexcept {
    if (!first || !second) return;
    try {
        const auto& before = *first;
        const auto& after = *second;
        char text[750]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 state reread mismatch native=%u/%u mask=0x%x dt=%.9g/%.9g cache_count=%u/%u taa=%u/%u disable=%u/%u default=%u/%u dynamic=%u/%u ratio=%.9g,%.9g/%.9g,%.9g viewport=%.9g,%.9g,%.9g,%.9g/%.9g,%.9g,%.9g,%.9g maps=%u,%u,%u/%u,%u,%u",
            before.frame,after.frame,mask,double(before.delta_seconds),double(after.delta_seconds),before.count,after.count,
            before.taa,after.taa,before.disable,after.disable,unsigned(before.default_viewport),unsigned(after.default_viewport),
            unsigned(before.dynamic_viewport),unsigned(after.dynamic_viewport),double(before.ratio[0]),double(before.ratio[1]),
            double(after.ratio[0]),double(after.ratio[1]),double(before.viewport[0]),double(before.viewport[1]),
            double(before.viewport[2]),double(before.viewport[3]),double(after.viewport[0]),double(after.viewport[1]),
            double(after.viewport[2]),double(after.viewport[3]),before.colour,before.motion,before.depth,after.colour,after.motion,after.depth);
        report(text);
    } catch (...) {}
}
// The text of the former camera history receipt; Rust keeps the per-reason and 32-text budget.
extern "C" void rsf_fallout4_inputs_report_history(NativeInputReport report, const NativeInputHistoryReceipt* receipt) noexcept {
    if (!receipt) return;
    try {
        const auto& h = *receipt;
        char text[350]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 camera history native=%u previous_native=%u reset=%u reasons=0x%x render=%ux%u previous_render=%ux%u raster=%g,%g,%g,%g previous_raster=%g,%g,%g,%g",
            h.frame,h.previous_frame,h.reset,h.reasons,h.width,h.height,h.previous_width,h.previous_height,
            double(h.raster.xywh[0]),double(h.raster.xywh[1]),double(h.raster.xywh[2]),double(h.raster.xywh[3]),
            double(h.previous_raster.xywh[0]),double(h.previous_raster.xywh[1]),double(h.previous_raster.xywh[2]),double(h.previous_raster.xywh[3]));
        report(text);
    } catch (...) {}
}
// The text of the former framebuffer receipt; Rust keeps its once-per-stage mask.
extern "C" void rsf_fallout4_inputs_report_framebuffer(NativeInputReport report, const char* stage, const char* owner_stage,
    const NativeFramebufferReceipt* receipt) noexcept {
    if (!stage || !owner_stage || !receipt) return;
    try {
        const auto& r = *receipt;
        const auto& before = r.before;
        const auto& after = r.after;
        char text[960]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 framebuffer stage=%s partial=1 native=%u owner_stage=%s renderer=%p context=%p chain=%p window=%p output=%ux%u logical=0 platform=%u/%u native_texture=%p rtv=%p leased_texture=%p texture=%ux%u mip=%u array=%u samples=%u format=%u rtv_kind=%u rtv_mip=%u swap=%ux%u/%p after=%u/%p/%p/%p",
            stage,before.frame,owner_stage,
            reinterpret_cast<void*>(before.renderer),reinterpret_cast<void*>(before.context),reinterpret_cast<void*>(before.chain),
            reinterpret_cast<void*>(before.window),before.width,before.height,r.platform,r.confirmed_platform,
            reinterpret_cast<void*>(r.target_texture),reinterpret_cast<void*>(r.target_rtv),reinterpret_cast<void*>(r.leased),
            r.width,r.height,r.mip_levels,r.array_size,r.sample_count,r.format,
            r.rtv_dimension,r.rtv_mip,r.swap.width,r.swap.height,
            reinterpret_cast<void*>(r.swap.window),after.frame,reinterpret_cast<void*>(after.context),
            reinterpret_cast<void*>(after.chain),reinterpret_cast<void*>(after.window));
        report(text);
    } catch (...) {}
}

namespace {
template<class T>
void attach(Microsoft::WRL::ComPtr<T>& target, void*& source) noexcept {
    target.Attach(static_cast<T*>(std::exchange(source, nullptr)));
}

void take(NativeInputs& result, NativeInputReferences& refs) noexcept {
    attach(result.context, refs.context);
    attach(result.chain, refs.chain);
    attach(result.colour, refs.textures[native_input_colour]);
    attach(result.motion, refs.textures[native_input_motion]);
    attach(result.depth, refs.textures[native_input_depth]);
    attach(result.refraction_normals, refs.textures[native_input_refraction_normals]);
    attach(result.pipboy, refs.textures[native_input_pipboy]);
    attach(result.vats, refs.textures[native_input_vats]);
}
}

bool read_native_inputs(NativeInputs& output, void (*report)(const char*), const NativeDepthSnapshot* producer) noexcept try {
    output={};
    NativeInputLease lease;
    NativeInputs result;
    if (!rsf_fallout4_inputs_read(&lease.refs,report,producer,&result.pass)) return false;
    take(result,lease.refs);
    output=std::move(result); return true;
} catch (...) {
    if (report && rsf_fallout4_inputs_claim_exception()) try { report("Fallout4 reader stage=exception"); } catch (...) {}
    return false;
}

bool read_native_raster_inputs(NativeInputs& output, const rsf_game_render_pass& frame,
    const NativeDepthSnapshot* producer) noexcept try {
    output={};
    NativeInputLease lease;
    NativeInputs result;
    if (!rsf_fallout4_inputs_read_raster(&lease.refs,&frame,producer,&result.pass)) return false;
    take(result,lease.refs);
    output=std::move(result); return true;
} catch (...) { return false; }

}
