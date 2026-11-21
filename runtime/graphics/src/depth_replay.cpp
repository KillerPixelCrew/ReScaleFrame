// SPDX-License-Identifier: MIT
#include <rescaleframe/depth_replay.h>
#include "depth_platform.h"
#include <rescaleframe/d3d11_state.h>
#include <d3d11.h>
#include <wrl/client.h>

namespace {
using Microsoft::WRL::ComPtr;

int32_t make(void* raw, uint32_t width, uint32_t height, rsf_depth_replay_resources* out)
{
    auto* device = static_cast<ID3D11Device*>(raw);
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R32G8X24_TYPELESS;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE;
    D3D11_DEPTH_STENCIL_VIEW_DESC view{};
    view.Format = DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
    view.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    D3D11_DEPTH_STENCIL_DESC state{};
    state.DepthEnable = TRUE;
    state.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
    state.DepthFunc = D3D11_COMPARISON_GREATER_EQUAL;
    return SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &out->texture)) &&
        SUCCEEDED(device->CreateDepthStencilView(out->texture, &view, &out->view)) &&
        SUCCEEDED(device->CreateDepthStencilState(&state, &out->write));
}

void view_info(void* raw, rsf_depth_view_info* out)
{
    D3D11_DEPTH_STENCIL_VIEW_DESC desc{};
    static_cast<ID3D11DepthStencilView*>(raw)->GetDesc(&desc);
    *out = {uint32_t(desc.Format), uint32_t(desc.ViewDimension), uint32_t(desc.Flags), desc.Texture2D.MipSlice};
}

void* view_texture(void* raw, rsf_depth_texture_info* out)
{
    ComPtr<ID3D11Resource> resource;
    static_cast<ID3D11DepthStencilView*>(raw)->GetResource(&resource);
    ComPtr<ID3D11Texture2D> texture;
    if (!resource || FAILED(resource.As(&texture)) || !texture) return nullptr;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    *out = {desc.Width, desc.Height, uint32_t(desc.Format), desc.SampleDesc.Count, desc.SampleDesc.Quality,
        desc.MipLevels, desc.ArraySize};
    return texture.Detach();
}

// Reports the live programmable geometry and output state without changing it.
void pipeline(void* raw, rsf_depth_pipeline_info* out)
{
    auto* c = static_cast<ID3D11DeviceContext*>(raw);
    *out = {};
    ComPtr<ID3D11HullShader> hs;
    ComPtr<ID3D11DomainShader> ds;
    ComPtr<ID3D11GeometryShader> gs;
    c->HSGetShader(&hs, nullptr, nullptr);
    c->DSGetShader(&ds, nullptr, nullptr);
    c->GSGetShader(&gs, nullptr, nullptr);
    out->stages = hs || ds || gs;
    ID3D11Buffer* so[D3D11_SO_BUFFER_SLOT_COUNT]{};
    c->SOGetTargets(D3D11_SO_BUFFER_SLOT_COUNT, so);
    for (auto* b : so) {
        if (b) {
            out->stream_out = 1;
            b->Release();
        }
    }
    ID3D11UnorderedAccessView* uavs[D3D11_PS_CS_UAV_REGISTER_COUNT]{};
    c->OMGetRenderTargetsAndUnorderedAccessViews(0, nullptr, nullptr, 0, D3D11_PS_CS_UAV_REGISTER_COUNT, uavs);
    for (auto* uav : uavs) {
        if (uav) {
            out->uavs = 1;
            uav->Release();
        }
    }
    ComPtr<ID3D11DepthStencilState> depth;
    UINT reference = 0;
    c->OMGetDepthStencilState(&depth, &reference);
    if (depth) {
        D3D11_DEPTH_STENCIL_DESC desc{};
        depth->GetDesc(&desc);
        out->depth_state = 1;
        out->depth_enable = desc.DepthEnable != FALSE;
        out->depth_write_mask = uint32_t(desc.DepthWriteMask);
        out->depth_func = uint32_t(desc.DepthFunc);
        out->stencil_enable = desc.StencilEnable != FALSE;
        out->front_func = uint32_t(desc.FrontFace.StencilFunc);
        out->back_func = uint32_t(desc.BackFace.StencilFunc);
    }
    D3D11_VIEWPORT viewport{};
    UINT count = 1;
    c->RSGetViewports(&count, &viewport);
    out->viewport_count = count;
    const float values[]{viewport.TopLeftX, viewport.TopLeftY, viewport.Width, viewport.Height,
        viewport.MinDepth, viewport.MaxDepth};
    for (int i = 0; i < 6; ++i) out->viewport[i] = values[i];
}

void unbind(void* context)
{
    static_cast<ID3D11DeviceContext*>(context)->OMSetRenderTargets(0, nullptr, nullptr);
}

// Leave all programmable geometry state live; only depth output and the pixel shader change.
void draw(void* raw, const rsf_depth_replay_resources* r, const rsf_frame_tap_geometry* g)
{
    auto* c = static_cast<ID3D11DeviceContext*>(raw);
    c->OMSetRenderTargets(0, nullptr, r->view);
    c->OMSetDepthStencilState(r->write, 0);
    c->PSSetShader(nullptr, nullptr, 0);
    switch (g->kind) {
    case 0:
        c->Draw(g->count, g->start);
        break;
    case 1:
        c->DrawIndexed(g->count, g->start, g->base_vertex);
        break;
    case 2:
        c->DrawInstanced(g->count, g->instances, g->start, g->start_instance);
        break;
    case 3:
        c->DrawIndexedInstanced(g->count, g->instances, g->start, g->base_vertex, g->start_instance);
        break;
    }
}

constexpr rsf_depth_replay_platform platform{
    make, view_info, view_texture, pipeline, rsf_d3d11_depth_state_save, rsf_d3d11_depth_state_restore, unbind,
    rsf_d3d11_copy_resource, draw,
};
static_assert(sizeof(rsf_depth_replay_platform) == 9 * sizeof(void*));
static_assert(sizeof(rsf_depth_replay_resources) == 3 * sizeof(void*));
static_assert(sizeof(rsf_depth_replay_candidate) == 64 && sizeof(rsf_depth_replay_detail) == 72);
static_assert(sizeof(rsf_depth_view_info) == 16 && sizeof(rsf_depth_texture_info) == 28 &&
    sizeof(rsf_depth_pipeline_info) == 68);
// Values the Rust acceptance compares against.
static_assert(DXGI_FORMAT_R32G8X24_TYPELESS == 19 && DXGI_FORMAT_D32_FLOAT_S8X24_UINT == 20);
static_assert(D3D11_DSV_DIMENSION_TEXTURE2D == 3 && D3D11_DSV_READ_ONLY_DEPTH == 1);
static_assert(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST == 4 && D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP == 5);
static_assert(D3D11_DEPTH_WRITE_MASK_ZERO == 0 && D3D11_COMPARISON_GREATER == 5 &&
    D3D11_COMPARISON_GREATER_EQUAL == 7 && D3D11_COMPARISON_ALWAYS == 8);
} // namespace

extern "C" rsf_depth_replay* rsf_depth_replay_create(void* device, uint32_t width, uint32_t height)
{
    return rsf_core_depth_replay_create(device, width, height, &platform);
}

extern "C" uint32_t rsf_depth_replay_last_reject(const rsf_depth_replay* r)
{
    return rsf_core_depth_replay_last_reject(r);
}

extern "C" void rsf_depth_replay_get_detail(const rsf_depth_replay* r, rsf_depth_replay_detail* out)
{
    rsf_core_depth_replay_get_detail(r, out);
}

extern "C" void rsf_depth_replay_end_frame(rsf_depth_replay* r)
{
    rsf_core_depth_replay_end_frame(r);
}

extern "C" void rsf_depth_replay_destroy(rsf_depth_replay* r)
{
    rsf_core_depth_replay_destroy(r);
}

extern "C" uint32_t rsf_depth_replay_draw(rsf_depth_replay* r, const rsf_frame_tap_geometry* g)
{
    if (!g) return rsf_core_depth_replay_draw(r, nullptr);
    const rsf_depth_replay_candidate candidate{g->context, g->target, g->depth_view, g->width, g->height,
        g->samples, g->kind, g->count, g->instances, g->topology, g->vertex_shader != nullptr, g};
    return rsf_core_depth_replay_draw(r, &candidate);
}

extern "C" void* rsf_depth_replay_selected(rsf_depth_replay* r, void* context, void* depth, void* layer)
{
    return rsf_core_depth_replay_selected(r, context, depth, layer);
}
