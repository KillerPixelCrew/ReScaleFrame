// SPDX-License-Identifier: MIT
// Native leaves of the Rust render-input owner (render_inputs.rs). Rust owns the code guards, the
// guarded engine reads of the renderer owner, temporal state, camera cache and target maps, every
// admission decision, the camera history, the passes and the receipt budgets. This file keeps
// what needs C++ or COM: the lease that holds the context, chain and textures as typed COM
// references, canonical object identity, the D3D11 and DXGI facts, and the receipts with their
// original text. The leaves only report; none of them decides admission.
#include "render_inputs.h"
#include "render_inputs_platform.h"
#include <windows.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <utility>

namespace rsf::fallout4 {
namespace {
using Microsoft::WRL::ComPtr;
// render_inputs.rs compares these enumerators by value.
static_assert(DXGI_FORMAT_R11G11B10_FLOAT == 26 && DXGI_FORMAT_R16G16_FLOAT == 34 &&
    DXGI_FORMAT_R32_TYPELESS == 39 && DXGI_FORMAT_R32_FLOAT == 41 && DXGI_FORMAT_R24G8_TYPELESS == 44 &&
    DXGI_FORMAT_R24_UNORM_X8_TYPELESS == 46);
static_assert(D3D11_SRV_DIMENSION_TEXTURE2D == 4 && D3D11_RTV_DIMENSION_TEXTURE2D == 4);
static_assert(RSF_SCREEN_UNKNOWN == 0);

// Typed references a reader holds until it returns. The wrappers move them into their output on
// admission; a refusal releases them with the lease.
struct NativeInputLease {
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<IDXGISwapChain> chain;
    ComPtr<ID3D11Texture2D> textures[native_input_slot_count];
};

uint64_t identity_of(IUnknown* object) noexcept {
    ComPtr<IUnknown> canonical;
    return object && SUCCEEDED(object->QueryInterface(IID_PPV_ARGS(&canonical)))
        ? reinterpret_cast<uint64_t>(canonical.Get()) : 0;
}

uint64_t identity(uint64_t object) noexcept { return identity_of(reinterpret_cast<IUnknown*>(object)); }
void owner(void* lease, uint64_t context, uint64_t chain, NativeInputDeviceFacts* output) noexcept {
    *output = {};
    auto& held = *static_cast<NativeInputLease*>(lease);
    held.context = reinterpret_cast<ID3D11DeviceContext*>(context);
    held.chain = reinterpret_cast<IDXGISwapChain*>(chain);
    if (!held.context || !held.chain) return;
    output->immediate = held.context->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE;
    ComPtr<ID3D11Device> device, chain_device;
    held.context->GetDevice(&device);
    output->device = identity_of(device.Get());
    output->chain_device_read = SUCCEEDED(held.chain->GetDevice(IID_PPV_ARGS(&chain_device)));
    output->chain_device = identity_of(chain_device.Get());
}
void viewport(uint64_t key, NativeInputViewportFacts* output) noexcept {
    *output = {};
    auto* context = reinterpret_cast<ID3D11DeviceContext*>(key);
    if (!context) return;
    UINT count{};
    context->RSGetViewports(&count, nullptr);
    output->count = count;
    if (count != 1) return;
    UINT capacity = 1;
    D3D11_VIEWPORT actual{};
    context->RSGetViewports(&capacity, &actual);
    output->copied = capacity;
    const float values[]{actual.TopLeftX, actual.TopLeftY, actual.Width, actual.Height, actual.MinDepth, actual.MaxDepth};
    std::memcpy(output->viewport, values, sizeof(values));
}
void swap(uint64_t key, NativeInputSwapFacts* output) noexcept {
    *output = {};
    auto* chain = reinterpret_cast<IDXGISwapChain*>(key);
    if (!chain) return;
    DXGI_SWAP_CHAIN_DESC description{};
    output->read = SUCCEEDED(chain->GetDesc(&description));
    output->window = reinterpret_cast<uint64_t>(description.OutputWindow);
    output->width = description.BufferDesc.Width; output->height = description.BufferDesc.Height;
}
void resource(void* lease, uint32_t slot, uint64_t key, uint32_t render_target,
    NativeInputResourceFacts* output) noexcept {
    *output = {};
    ComPtr<ID3D11Resource> viewed;
    if (render_target) {
        auto* view = reinterpret_cast<ID3D11RenderTargetView*>(key);
        if (!view) return;
        D3D11_RENDER_TARGET_VIEW_DESC description{};
        view->GetDesc(&description);
        output->read = 1;
        output->view_format = description.Format; output->view_dimension = description.ViewDimension;
        output->view_mip = description.Texture2D.MipSlice;
        view->GetResource(&viewed);
    } else {
        auto* view = reinterpret_cast<ID3D11ShaderResourceView*>(key);
        if (!view) return;
        D3D11_SHADER_RESOURCE_VIEW_DESC description{};
        view->GetDesc(&description);
        output->read = 1;
        output->view_format = description.Format; output->view_dimension = description.ViewDimension;
        output->view_mip = description.Texture2D.MostDetailedMip; output->view_mips = description.Texture2D.MipLevels;
        view->GetResource(&viewed);
    }
    output->identity = identity_of(viewed.Get());
    ComPtr<ID3D11Texture2D> texture;
    if (!viewed || FAILED(viewed.As(&texture))) return;
    output->pointer = reinterpret_cast<uint64_t>(texture.Get());
    D3D11_TEXTURE2D_DESC allocation{};
    texture->GetDesc(&allocation);
    output->width = allocation.Width; output->height = allocation.Height;
    output->format = allocation.Format; output->mip_levels = allocation.MipLevels;
    output->array_size = allocation.ArraySize;
    output->sample_count = allocation.SampleDesc.Count; output->sample_quality = allocation.SampleDesc.Quality;
    output->bind_flags = allocation.BindFlags;
    ComPtr<ID3D11Device> device;
    texture->GetDevice(&device);
    output->device = identity_of(device.Get());
    if (slot < native_input_slot_count) static_cast<NativeInputLease*>(lease)->textures[slot] = std::move(texture);
}
void release(void* lease, uint32_t slot) noexcept {
    if (slot < native_input_slot_count) static_cast<NativeInputLease*>(lease)->textures[slot].Reset();
}

// The text the former reader receipt wrote; Rust decides the stage and its once-per-stage mask.
void reader(NativeInputReport report, const char* stage, const NativeInputReceipt* receipt,
    const rsf_game_render_pass* pass) noexcept {
    if (!report || !stage || !receipt || !pass) return;
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
void mismatch(NativeInputReport report, const detail::NativeState* first, const detail::NativeState* second,
    uint32_t mask) noexcept {
    if (!report || !first || !second) return;
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
void history(NativeInputReport report, const NativeInputHistoryReceipt* receipt) noexcept {
    if (!report || !receipt) return;
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
void framebuffer(NativeInputReport report, const char* stage, const char* owner_stage,
    const NativeFramebufferReceipt* receipt) noexcept {
    if (!report || !stage || !owner_stage || !receipt) return;
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

constexpr NativeInputLeaves leaves{identity, owner, viewport, swap, resource, release, reader, mismatch,
    history, framebuffer};

void take(NativeInputs& result, NativeInputLease& lease) noexcept {
    result.context = std::move(lease.context);
    result.chain = std::move(lease.chain);
    result.colour = std::move(lease.textures[native_input_colour]);
    result.motion = std::move(lease.textures[native_input_motion]);
    result.depth = std::move(lease.textures[native_input_depth]);
    result.refraction_normals = std::move(lease.textures[native_input_refraction_normals]);
    result.pipboy = std::move(lease.textures[native_input_pipboy]);
    result.vats = std::move(lease.textures[native_input_vats]);
}
}

bool verify_render_inputs() noexcept { return rsf_fallout4_inputs_verify(); }

bool read_native_inputs(NativeInputs& output, void (*report)(const char*), const NativeDepthSnapshot* producer) noexcept try {
    output={};
    NativeInputLease lease;
    NativeInputs result;
    if (!rsf_fallout4_inputs_read(&leaves,native_depth_leaves(),&lease,report,producer,&result.pass)) return false;
    take(result,lease);
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
    if (!rsf_fallout4_inputs_read_raster(&leaves,native_depth_leaves(),&lease,&frame,producer,&result.pass)) return false;
    take(result,lease);
    output=std::move(result); return true;
} catch (...) { return false; }

bool read_native_framebuffer(NativeFramebuffer& output, void (*report)(const char*)) noexcept try {
    output={};
    NativeInputLease lease;
    NativeFramebufferFacts facts{};
    if (!rsf_fallout4_inputs_read_framebuffer(&leaves,&lease,report,&facts)) return false;
    NativeFramebuffer result;
    result.context=std::move(lease.context);
    result.chain=std::move(lease.chain);
    result.texture=std::move(lease.textures[native_input_colour]);
    result.native_frame=facts.native_frame; result.window_key=facts.window_key;
    output=std::move(result); return true;
} catch (...) {
    static std::atomic_flag reported=ATOMIC_FLAG_INIT;
    if (report && !reported.test_and_set(std::memory_order_relaxed)) try { report("Fallout4 framebuffer stage=exception"); } catch (...) {}
    return false;
}
}
