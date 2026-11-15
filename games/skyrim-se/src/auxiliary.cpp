// SPDX-License-Identifier: GPL-3.0-only
#include "auxiliary.h"
#include <rescaleframe/d3d11_host.h>
#include <windows.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <array>
#include <cstdio>

namespace rsf::skyrim {
void observe_auxiliaries(void* context, const rsf_game_render_pass& pass, const rsf_game_host_services& host) {
    using Microsoft::WRL::ComPtr;
    // Guarded exact-build main callback. Native DOF setup 1551820 selects 49+selector;
    // refraction setup 15655B0 selects 13, and DOF conditionally uses underwater12.
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    const auto selector = *reinterpret_cast<const uint32_t*>(base + 0x3436358);
    if (selector > 11) return;
    using Capture = decltype(&rsf_d3d11_host_capture_texture);
    static const auto capture = reinterpret_cast<Capture>(reinterpret_cast<void*>(
        GetProcAddress(GetModuleHandleW(L"ReScaleFrame.Runtime.dll"), "rsf_d3d11_host_capture_texture")));
    static uint32_t logged_width{}, logged_height{}, logged_selector = UINT32_MAX;
    const bool describe = logged_width != pass.camera.render_width || logged_height != pass.camera.render_height || logged_selector != selector;
    const std::array<uint32_t, 4> indices{12, 13, 49, 49 + selector};
    const std::array<const char*, 4> names{"native_underwater_mask", "native_refraction_normals", "native_sao_camera_z", "native_sao_dof_view"};
    for (size_t i = 0; i < indices.size(); ++i) {
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
        if (capture && i < 3) {
            const rsf_d3d11_host_capture request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION,
                pass.session_id, pass.native_frame, pass.view_key, names[i], "Native pre-HDR auxiliary allocation", texture.Get(), 0, 1};
            (void)capture(host.user, context, &request);
        }
    }
    logged_width = pass.camera.render_width; logged_height = pass.camera.render_height; logged_selector = selector;
}
}
