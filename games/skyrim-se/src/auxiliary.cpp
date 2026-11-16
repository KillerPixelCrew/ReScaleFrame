// SPDX-License-Identifier: MIT
#include "auxiliary.h"
#include "resolution.h"
#include <rescaleframe/d3d11_host.h>
#include <windows.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <array>
#include <cstdio>
#include <atomic>

namespace rsf::skyrim {
namespace {
std::atomic<bool> pending{};
rsf_d3d11_auxiliary_scope pending_scope{};
using Restore = decltype(&rsf_d3d11_host_restore_auxiliaries);
Restore restore{};
}
bool auxiliaries_pending() { return pending.load(std::memory_order_acquire); }
bool reconstruct_auxiliaries(void* context, const rsf_game_render_pass& pass, const rsf_game_host_services& host) {
    using Microsoft::WRL::ComPtr;
    if (auxiliaries_pending() || !output_resolution_ready()) return false;
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    using Upscale = decltype(&rsf_d3d11_host_upscale_auxiliaries);
    const auto upscale = reinterpret_cast<Upscale>(reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_d3d11_host_upscale_auxiliaries")));
    restore = reinterpret_cast<Restore>(reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_d3d11_host_restore_auxiliaries")));
    if (!upscale || !restore) return false;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    std::array<ComPtr<ID3D11Texture2D>, 4> textures;
    textures[0] = static_cast<ID3D11Texture2D*>(pass.depth);
    textures[1] = *reinterpret_cast<ID3D11Texture2D**>(base + 0x3331f40 + 0xa58 + 13 * 0x30);
    textures[2] = static_cast<ID3D11Texture2D*>(pass.motion);
    textures[3] = *reinterpret_cast<ID3D11Texture2D**>(base + 0x3331f40 + 0xa58 + 12 * 0x30);
    const std::array<DXGI_FORMAT, 4> formats{DXGI_FORMAT_R24G8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R8_UNORM};
    const std::array<const char*, 4> names{"post_sr_depth", "post_sr_refraction_normals", "post_sr_motion", "post_sr_underwater_mask"};
    std::array<rsf_d3d11_auxiliary_surface, 4> surfaces{};
    for (size_t i = 0; i < textures.size(); ++i) {
        if (!textures[i]) return false;
        D3D11_TEXTURE2D_DESC d{}; textures[i]->GetDesc(&d);
        const uint32_t width = i == 3 ? pass.camera.output_width / 2 : pass.camera.output_width;
        const uint32_t height = i == 3 ? pass.camera.output_height / 2 : pass.camera.output_height;
        if (d.Format != formats[i] || d.Width != width || d.Height != height) return false;
        auto& a = surfaces[i]; a.struct_size = sizeof(a); a.kind = i ? RSF_D3D11_AUXILIARY_COLOUR : RSF_D3D11_AUXILIARY_DEPTH_STENCIL;
        a.texture = textures[i].Get(); a.name = names[i];
        // DOF consumes and then clears target12. Retiring that current-frame mask
        // must preserve the native clear, not resurrect the earlier mask snapshot.
        a.flags = i == 3 ? 0 : RSF_D3D11_AUXILIARY_RESTORE_CONTENTS;
        a.source_rect[2] = i == 3 ? float(d.Width) * (float(pass.camera.render_width) / float(pass.camera.output_width)) : float(pass.camera.render_width);
        a.source_rect[3] = i == 3 ? float(d.Height) * (float(pass.camera.render_height) / float(pass.camera.output_height)) : float(pass.camera.render_height);
        a.source_name = i == 1 ? "native_refraction_normals" : i == 3 ? "native_underwater_mask" : nullptr;
        a.output_rect[2] = int32_t(d.Width); a.output_rect[3] = int32_t(d.Height);
        // The splitter producer constructs clip-space water-plane geometry from the
        // native frustum, rather than a jittered main-scene vertex projection.
        if (i != 3) {
            // The resampler subtracts its offset; sampling jittered geometry at
            // an unjittered output position requires adding geometry displacement.
            a.jitter_pixels[0] = -pass.camera.jitter_pixels[0] * float(d.Width) / float(pass.camera.output_width);
            a.jitter_pixels[1] = -pass.camera.jitter_pixels[1] * float(d.Height) / float(pass.camera.output_height);
        }
    }
    const rsf_d3d11_auxiliary_pass request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION, &pass, uint32_t(surfaces.size()), surfaces.data()};
    if (upscale(host.user, context, &request) != RSF_OK) return false;
    pending_scope = {sizeof(pending_scope), RSF_D3D11_HOST_ABI_VERSION, pass.session_id, pass.native_frame, pass.view_key};
    pending.store(true, std::memory_order_release);
    (void)enter_output_resolution();
    return true;
}
void finish_auxiliaries(const rsf_game_host_services& host) {
    if (!auxiliaries_pending() || !restore) return;
    if (restore(host.user, &pending_scope) != RSF_OK) return;
    static unsigned reports{};
    if (host.log && reports++ < 4) {
        const auto base = uintptr_t(GetModuleHandleW(nullptr));
        auto* context = *reinterpret_cast<ID3D11DeviceContext**>(base + 0x3331f30);
        D3D11_VIEWPORT viewport{}; UINT count = 1; context->RSGetViewports(&count, &viewport);
        const auto* ratios = reinterpret_cast<const float*>(base + 0x33364b4);
        char message[240]{}; std::snprintf(message, sizeof(message),
            "Skyrim auxiliary scope restored after native frame %llu; final viewport=%gx%g ratios=%g,%g,%g,%g before producer-state restoration",
            static_cast<unsigned long long>(pending_scope.native_frame), double(viewport.Width), double(viewport.Height),
            double(ratios[0]), double(ratios[1]), double(ratios[2]), double(ratios[3]));
        host.log(host.user, message);
    }
    leave_output_resolution(); pending.store(false, std::memory_order_release);
}
void observe_auxiliaries(void* context, const rsf_game_render_pass& pass, const rsf_game_host_services& host) {
    using Microsoft::WRL::ComPtr;
    // Guarded exact-build main callback. Native DOF setup 1551820 selects 49+selector;
    // refraction setup 15655B0 selects 13, and DOF conditionally uses underwater12.
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    const auto selector = *reinterpret_cast<const uint32_t*>(base + 0x3436358);
    using Capture = decltype(&rsf_d3d11_host_capture_texture);
    static const auto capture = reinterpret_cast<Capture>(reinterpret_cast<void*>(
        GetProcAddress(GetModuleHandleW(L"ReScaleFrame.Runtime.dll"), "rsf_d3d11_host_capture_texture")));
    static uint32_t logged_width{}, logged_height{}, logged_selector = UINT32_MAX;
    const bool describe = logged_width != pass.camera.render_width || logged_height != pass.camera.render_height || logged_selector != selector;
    const std::array<uint32_t, 4> indices{12, 13, 49, 49 + selector};
    const std::array<const char*, 4> names{"native_underwater_mask", "native_refraction_normals", "native_sao_camera_z", "native_sao_dof_view"};
    for (size_t i = 0; i < indices.size(); ++i) {
        if (i == 3 && selector > 12) continue; // Known native SAO mip aliases end at level 11.
        const auto data = base + 0x3331f40 + 0xa58 + indices[i] * 0x30;
        auto* raw = *reinterpret_cast<ID3D11Texture2D* const*>(data);
        ComPtr<ID3D11Texture2D> texture = raw;
        ComPtr<ID3D11RenderTargetView> target = *reinterpret_cast<ID3D11RenderTargetView* const*>(data + 0x10);
        ComPtr<ID3D11ShaderResourceView> view = *reinterpret_cast<ID3D11ShaderResourceView* const*>(data + 0x18);
        // Native mip aliases can have views without a texture pointer in this slot.
        // Resolve the allocation through the view; this does not identify its active pixels.
        if (!texture) {
            ComPtr<ID3D11Resource> resource;
            if (view) view->GetResource(&resource);
            else if (target) target->GetResource(&resource);
            if (resource) (void)resource.As(&texture);
        }
        if (!texture) continue;
        if (describe && host.log) {
            D3D11_TEXTURE2D_DESC d{}; texture->GetDesc(&d);
            D3D11_RENDER_TARGET_VIEW_DESC r{}; if (target) target->GetDesc(&r);
            D3D11_SHADER_RESOURCE_VIEW_DESC s{}; if (view) view->GetDesc(&s);
            char message[480]{};
            std::snprintf(message, sizeof(message),
                "Skyrim auxiliary %s target=%u selector=%u native_texture=%p allocation=%p size=%ux%u format=%u mips=%u slices=%u samples=%u bind=0x%x RTV=%u:%u:mip%u SRV=%u:%u:mip%u+%u native_frame=%llu; descriptor only, active coverage unverified",
                names[i], indices[i], selector, static_cast<void*>(raw), static_cast<void*>(texture.Get()), d.Width, d.Height, unsigned(d.Format),
                d.MipLevels, d.ArraySize, d.SampleDesc.Count, d.BindFlags, unsigned(r.Format), unsigned(r.ViewDimension),
                r.Texture2D.MipSlice, unsigned(s.Format), unsigned(s.ViewDimension), s.Texture2D.MostDetailedMip, s.Texture2D.MipLevels,
                static_cast<unsigned long long>(pass.native_frame));
            host.log(host.user, message);
        }
        // The readback service snapshots subresource zero. Do not mislabel the
        // allocation's base mip as the selected DOF view's mip contents.
        if (capture && i < 3 && (!auxiliaries_pending() || i == 2)) {
            const rsf_d3d11_host_capture request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION,
                pass.session_id, pass.native_frame, pass.view_key, names[i], "Native pre-HDR auxiliary allocation", texture.Get(), 0, 1};
            (void)capture(host.user, context, &request);
        }
    }
    logged_width = pass.camera.render_width; logged_height = pass.camera.render_height; logged_selector = selector;
}
}
