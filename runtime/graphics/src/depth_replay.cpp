// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/depth_replay.h>
#include <rescaleframe/d3d11_state.h>
#include <d3d11.h>
#include <new>

struct rsf_depth_replay {
    ID3D11Texture2D* texture = nullptr;
    ID3D11DepthStencilView* view = nullptr;
    ID3D11DepthStencilState* write = nullptr;
    ID3D11Texture2D* source = nullptr;
    IUnknown* layer = nullptr;
    void* context = nullptr;
    uint32_t width = 0, height = 0, draws = 0;
    bool refused = false;
};

extern "C" void rsf_depth_replay_end_frame(rsf_depth_replay* r)
{
    if (!r) {
        return;
    }
    if (r->source) {
        r->source->Release();
    }
    if (r->layer) {
        r->layer->Release();
    }
    r->source = nullptr;
    r->layer = nullptr;
    r->context = nullptr;
    r->draws = 0;
    r->refused = false;
}
extern "C" void rsf_depth_replay_destroy(rsf_depth_replay* r)
{
    if (!r) {
        return;
    }
    rsf_depth_replay_end_frame(r);
    if (r->write) {
        r->write->Release();
    }
    if (r->view) {
        r->view->Release();
    }
    if (r->texture) {
        r->texture->Release();
    }
    delete r;
}
extern "C" rsf_depth_replay* rsf_depth_replay_create(void* pointer, uint32_t width, uint32_t height)
{
    if (!pointer || !width || !height) {
        return nullptr;
    }
    auto* device = static_cast<ID3D11Device*>(pointer);
    auto* r = new (std::nothrow) rsf_depth_replay{};
    if (!r) {
        return nullptr;
    }
    r->width = width;
    r->height = height;
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
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &r->texture)) ||
        FAILED(device->CreateDepthStencilView(r->texture, &view, &r->view)) ||
        FAILED(device->CreateDepthStencilState(&state, &r->write))) {
        rsf_depth_replay_destroy(r);
        return nullptr;
    }
    return r;
}

namespace {
// Leave all programmable geometry state live. Reject stages with possible side effects or a
// different geometry contract, instead of disabling them and claiming equivalent coverage.
bool supported_pipeline(ID3D11DeviceContext* c)
{
    ID3D11HullShader* hs = nullptr;
    ID3D11DomainShader* ds = nullptr;
    ID3D11GeometryShader* gs = nullptr;
    c->HSGetShader(&hs, nullptr, nullptr);
    c->DSGetShader(&ds, nullptr, nullptr);
    c->GSGetShader(&gs, nullptr, nullptr);
    bool ok = !hs && !ds && !gs;
    if (hs) {
        hs->Release();
    }
    if (ds) {
        ds->Release();
    }
    if (gs) {
        gs->Release();
    }
    ID3D11Buffer* so[D3D11_SO_BUFFER_SLOT_COUNT]{};
    c->SOGetTargets(D3D11_SO_BUFFER_SLOT_COUNT, so);
    for (auto* b : so) {
        if (b) {
            ok = false;
            b->Release();
        }
    }
    ID3D11UnorderedAccessView* uavs[D3D11_PS_CS_UAV_REGISTER_COUNT]{};
    c->OMGetRenderTargetsAndUnorderedAccessViews(0, nullptr, nullptr, 0,
                                                 D3D11_PS_CS_UAV_REGISTER_COUNT, uavs);
    for (auto* uav : uavs) {
        if (uav) {
            ok = false;
            uav->Release();
        }
    }
    ID3D11DepthStencilState* depth = nullptr;
    UINT reference = 0;
    c->OMGetDepthStencilState(&depth, &reference);
    if (!depth) {
        return false;
    }
    D3D11_DEPTH_STENCIL_DESC desc{};
    depth->GetDesc(&desc);
    depth->Release();
    // Stencil writes used for the translucency pass are omitted. Actual stencil tests would
    // change coverage and are refused. No new state objects are built for a material's state.
    return ok && desc.DepthEnable && desc.DepthWriteMask == D3D11_DEPTH_WRITE_MASK_ZERO &&
           (desc.DepthFunc == D3D11_COMPARISON_GREATER_EQUAL ||
            desc.DepthFunc == D3D11_COMPARISON_GREATER) &&
           (!desc.StencilEnable || (desc.FrontFace.StencilFunc == D3D11_COMPARISON_ALWAYS &&
                                    desc.BackFace.StencilFunc == D3D11_COMPARISON_ALWAYS));
}
} // namespace

extern "C" uint32_t rsf_depth_replay_draw(rsf_depth_replay* r, const rsf_frame_tap_geometry* g)
{
    if (!r || !g || !g->context || !g->depth_view || !g->target || g->width != r->width ||
        g->height != r->height || g->samples != 1) {
        return 0;
    }
    if (r->refused) {
        return 0;
    }
    auto refuse = [r]() {
        r->refused = true;
        return 0u;
    };
    if (g->kind > 3 || !g->vertex_shader || !g->count || !g->instances ||
        (g->topology != D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST &&
         g->topology != D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP)) {
        return refuse();
    }
    auto* c = static_cast<ID3D11DeviceContext*>(g->context);
    auto* dsv = static_cast<ID3D11DepthStencilView*>(g->depth_view);
    D3D11_DEPTH_STENCIL_VIEW_DESC vd{};
    dsv->GetDesc(&vd);
    if (vd.Format != DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
        vd.ViewDimension != D3D11_DSV_DIMENSION_TEXTURE2D || vd.Texture2D.MipSlice != 0 ||
        !(vd.Flags & D3D11_DSV_READ_ONLY_DEPTH)) {
        return refuse();
    }
    ID3D11Resource* resource = nullptr;
    dsv->GetResource(&resource);
    ID3D11Texture2D* source = nullptr;
    resource->QueryInterface(__uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&source));
    resource->Release();
    if (!source) {
        return refuse();
    }
    D3D11_TEXTURE2D_DESC desc{};
    source->GetDesc(&desc);
    const bool valid = desc.Width == r->width && desc.Height == r->height &&
                       desc.Format == DXGI_FORMAT_R32G8X24_TYPELESS && desc.SampleDesc.Count == 1 &&
                       desc.SampleDesc.Quality == 0 && desc.MipLevels == 1 && desc.ArraySize == 1;
    if (!valid) {
        source->Release();
        return refuse();
    }
    if (!r->source) {
        r->source = source;
        r->layer = static_cast<IUnknown*>(g->target);
        r->layer->AddRef();
        r->context = c;
    } else {
        const bool same = r->source == source && r->layer == g->target && r->context == c;
        source->Release();
        if (!same) {
            return refuse();
        }
    }
    if (!supported_pipeline(c)) {
        return refuse();
    }
    D3D11_VIEWPORT viewport{};
    UINT count = 1;
    c->RSGetViewports(&count, &viewport);
    if (count != 1 || viewport.TopLeftX != 0 || viewport.TopLeftY != 0 ||
        viewport.Width != float(r->width) || viewport.Height != float(r->height) ||
        viewport.MinDepth != 0 || viewport.MaxDepth != 1) {
        return refuse();
    }

    rsf_d3d11_state saved;
    rsf_d3d11_depth_state_save(c, &saved);
    c->OMSetRenderTargets(0, nullptr, nullptr);
    if (!r->draws) {
        c->CopyResource(r->texture, r->source);
    }
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
        c->DrawIndexedInstanced(g->count, g->instances, g->start, g->base_vertex,
                                g->start_instance);
        break;
    }
    rsf_d3d11_depth_state_restore(c, &saved);
    ++r->draws;
    return 1;
}

extern "C" void* rsf_depth_replay_selected(rsf_depth_replay* r, void* context, void* depth,
                                           void* layer)
{
    return r && !r->refused && r->draws && layer && r->layer == layer && r->source == depth &&
                   r->context == context
               ? r->texture
               : depth;
}
