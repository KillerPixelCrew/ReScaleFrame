// SPDX-License-Identifier: GPL-3.0-only
#include "auxiliary_sr.h"
#include <rescaleframe/depth_scale.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>
#include <array>
#include <algorithm>
#include <cmath>

namespace {
using Microsoft::WRL::ComPtr;
constexpr const char* colour_shader = R"(
Texture2D<float4> Source : register(t0);
cbuffer Params : register(b0) {
    float4 SourceRect;
    float4 OutputRect;
    float4 ValidTexels;
    float2 Jitter;
    float2 Padding;
};
float4 vertex_main(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
float4 pixel_main(float4 position : SV_Position) : SV_Target {
    float2 uv = (position.xy - OutputRect.xy) / (OutputRect.zw - OutputRect.xy);
    float2 p = floor(SourceRect.xy + uv * (SourceRect.zw - SourceRect.xy) - Jitter);
    return Source.Load(int3(int2(clamp(p, ValidTexels.xy, ValidTexels.zw)), 0));
}
)";
struct Constants { float source[4], output[4], valid[4], jitter[2], padding[2]; };
static_assert(sizeof(Constants) == 64);
struct Bindings {
    ID3D11DeviceContext* context;
    rsf_d3d11_state saved{};
    bool valid;
    explicit Bindings(ID3D11DeviceContext* c) : context(c), valid(rsf_d3d11_state_save(c, &saved) != 0) {}
    ~Bindings() { if (valid) rsf_d3d11_state_restore(context, &saved); }
};
bool simple_texture(ID3D11Texture2D* texture, ID3D11Device* device, D3D11_TEXTURE2D_DESC& d) {
    if (!texture) return false;
    ComPtr<ID3D11Device> owner; texture->GetDevice(&owner); texture->GetDesc(&d);
    return owner.Get() == device && d.ArraySize == 1 && d.MipLevels == 1 && d.SampleDesc.Count == 1 && d.Usage == D3D11_USAGE_DEFAULT;
}
struct Colour {
    ComPtr<ID3D11Texture2D> original, snapshot, output;
    ComPtr<ID3D11ShaderResourceView> view;
    ComPtr<ID3D11RenderTargetView> target;
    DXGI_FORMAT format{}; UINT width{}, height{};
    bool prepared{}, committed{}, restore_contents{};
};
struct Backup {
    ComPtr<ID3D11Texture2D> original, snapshot;
    DXGI_FORMAT format{}; UINT width{}, height{};
    bool captured{};
};
bool make_backup(ID3D11Device* device, ID3D11Texture2D* texture, Backup& b) {
    D3D11_TEXTURE2D_DESC d{};
    if (!simple_texture(texture, device, d)) return false;
    if (!b.snapshot || b.width != d.Width || b.height != d.Height || b.format != d.Format) {
        d.BindFlags = d.MiscFlags = d.CPUAccessFlags = 0;
        ComPtr<ID3D11Texture2D> next;
        if (FAILED(device->CreateTexture2D(&d, nullptr, &next))) return false;
        b.snapshot = next; b.width = d.Width; b.height = d.Height; b.format = d.Format;
    }
    b.original = texture; return true;
}
bool colour_format(DXGI_FORMAT f) {
    return f == DXGI_FORMAT_R8_UNORM || f == DXGI_FORMAT_R8G8B8A8_UNORM || f == DXGI_FORMAT_R16G16_FLOAT ||
        f == DXGI_FORMAT_R16_FLOAT || f == DXGI_FORMAT_R32_FLOAT || f == DXGI_FORMAT_R16G16B16A16_FLOAT;
}
}
struct AuxiliarySurfaces::State {
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<ID3D11Device> device;
    DWORD thread{};
    ComPtr<ID3D11VertexShader> vertex;
    ComPtr<ID3D11PixelShader> pixel;
    ComPtr<ID3D11RasterizerState> raster;
    ComPtr<ID3D11Buffer> constants;
    rsf_depth_scale* depth{};
    std::array<Colour, 8> colours;
    std::array<Backup, 2> backups;
    rsf_d3d11_auxiliary_scope scope{};
    bool depth_prepared{}, depth_restore{}, ready{}, pending{}, accepted{};
    ~State() { (void)rsf_depth_scale_destroy(depth); }
};
AuxiliarySurfaces::AuxiliarySurfaces() : state_(std::make_unique<State>()) {}
AuxiliarySurfaces::~AuxiliarySurfaces() { (void)restore(); }
bool AuxiliarySurfaces::pending() const { return state_->pending || state_->ready; }
bool AuxiliarySurfaces::matches(const rsf_d3d11_auxiliary_scope& scope) const {
    const auto& s = *state_;
    return pending() && s.thread == GetCurrentThreadId() && scope.session_id == s.scope.session_id &&
        scope.native_frame == s.scope.native_frame && scope.view_key == s.scope.view_key;
}
bool AuxiliarySurfaces::prepare(ID3D11DeviceContext* context, const rsf_game_render_pass& pass,
    const rsf_d3d11_auxiliary_pass& inputs, rsf_game_log_fn log, void* user) try
{
    auto& s = *state_;
    if (pending() || !context || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE || !inputs.surface_count ||
        inputs.surface_count > s.colours.size() || !inputs.surfaces) return false;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (s.context && (s.context.Get() != context || s.thread != GetCurrentThreadId())) return false;
    if (!s.context) { s.context = context; s.device = device; s.thread = GetCurrentThreadId(); }
    // Validate every borrowed input before preparing any native snapshot.
    unsigned depth_count = 0;
    for (uint32_t i = 0; i < inputs.surface_count; ++i) {
        const auto& a = inputs.surfaces[i]; D3D11_TEXTURE2D_DESC d{};
        if (a.struct_size < sizeof(a) || a.kind > RSF_D3D11_AUXILIARY_COLOUR ||
            (a.flags & ~RSF_D3D11_AUXILIARY_RESTORE_CONTENTS) ||
            !simple_texture(static_cast<ID3D11Texture2D*>(a.texture), device.Get(), d)) return false;
        if (a.texture == pass.color_output || a.texture == pass.color_output_readable) return false;
        for (uint32_t j = 0; j < i; ++j) if (a.texture == inputs.surfaces[j].texture) return false;
        for (unsigned j = 0; j < 4; ++j) if (!std::isfinite(a.source_rect[j])) return false;
        if (a.source_rect[0] < 0 || a.source_rect[1] < 0 || a.source_rect[2] > float(d.Width) || a.source_rect[3] > float(d.Height) ||
            a.source_rect[2] - a.source_rect[0] < 1 || a.source_rect[3] - a.source_rect[1] < 1 ||
            a.output_rect[0] < 0 || a.output_rect[1] < 0 || a.output_rect[2] <= a.output_rect[0] || a.output_rect[3] <= a.output_rect[1] ||
            uint32_t(a.output_rect[2]) > d.Width || uint32_t(a.output_rect[3]) > d.Height ||
            !std::isfinite(a.jitter_pixels[0]) || !std::isfinite(a.jitter_pixels[1]) ||
            std::abs(a.jitter_pixels[0]) > 16384 || std::abs(a.jitter_pixels[1]) > 16384) return false;
        if (a.kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL) {
            if (++depth_count > 1 || d.Format != DXGI_FORMAT_R24G8_TYPELESS || !(d.BindFlags & D3D11_BIND_DEPTH_STENCIL)) return false;
            for (float bound : a.source_rect) if (std::floor(bound) != bound) return false;
        } else if (!colour_format(d.Format)) return false;
    }
    if (!s.vertex) {
        if (!rsf::compile_vertex(device.Get(), colour_shader, "auxiliary_scale", "vertex_main", &s.vertex, log, user) ||
            !rsf::compile_pixel(device.Get(), colour_shader, "auxiliary_scale", "pixel_main", &s.pixel, log, user)) {
            s.vertex.Reset(); s.pixel.Reset(); return false;
        }
        D3D11_RASTERIZER_DESC r{}; r.FillMode = D3D11_FILL_SOLID; r.CullMode = D3D11_CULL_NONE; r.DepthClipEnable = TRUE;
        D3D11_BUFFER_DESC b{}; b.ByteWidth = sizeof(Constants); b.Usage = D3D11_USAGE_DEFAULT; b.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        if (FAILED(device->CreateRasterizerState(&r, &s.raster)) || FAILED(device->CreateBuffer(&b, nullptr, &s.constants))) { s.vertex.Reset(); return false; }
    }
    s.scope = {sizeof(s.scope), RSF_D3D11_HOST_ABI_VERSION, pass.session_id, pass.native_frame, pass.view_key};
    // From this point restore() cancels partial preparation or rolls back a failed SR.
    s.ready = true;
    if (!make_backup(device.Get(), static_cast<ID3D11Texture2D*>(pass.color_output), s.backups[0]) ||
        (pass.color_output_readable && pass.color_output_readable != pass.color_output &&
        !make_backup(device.Get(), static_cast<ID3D11Texture2D*>(pass.color_output_readable), s.backups[1]))) { (void)restore(); return false; }
    Bindings bindings(context); if (!bindings.valid) { (void)restore(); return false; }
    context->ClearState();
    for (auto& b : s.backups) if (b.original) { context->CopyResource(b.snapshot.Get(), b.original.Get()); b.captured = true; }
    for (uint32_t i = 0; i < inputs.surface_count; ++i) {
        const auto& a = inputs.surfaces[i];
        if (a.kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL) {
            const rsf_depth_scale_setup setup{sizeof(setup), RSF_DEPTH_SCALE_ABI_VERSION, log, user};
            if (!s.depth && rsf_depth_scale_create(context, &setup, &s.depth)) { (void)restore(); return false; }
            rsf_depth_scale_frame f{sizeof(f), RSF_DEPTH_SCALE_ABI_VERSION, a.texture};
            for (unsigned j = 0; j < 4; ++j) { f.source_rect[j] = int32_t(a.source_rect[j]); f.output_rect[j] = a.output_rect[j]; }
            f.jitter_pixels[0] = a.jitter_pixels[0]; f.jitter_pixels[1] = a.jitter_pixels[1];
            if (rsf_depth_scale_prepare(s.depth, &f)) { (void)restore(); return false; }
            s.depth_prepared = true; s.depth_restore = (a.flags & RSF_D3D11_AUXILIARY_RESTORE_CONTENTS) != 0;
            context->ClearState(); continue;
        }
        auto& c = s.colours[i]; auto* original = static_cast<ID3D11Texture2D*>(a.texture);
        D3D11_TEXTURE2D_DESC d{}; original->GetDesc(&d);
        if (!c.snapshot || c.width != d.Width || c.height != d.Height || c.format != d.Format) {
            Colour next{}; next.width = d.Width; next.height = d.Height; next.format = d.Format;
            d.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE; d.MiscFlags = d.CPUAccessFlags = 0;
            if (FAILED(device->CreateTexture2D(&d, nullptr, &next.snapshot)) || FAILED(device->CreateTexture2D(&d, nullptr, &next.output)) ||
                FAILED(device->CreateShaderResourceView(next.snapshot.Get(), nullptr, &next.view)) ||
                FAILED(device->CreateRenderTargetView(next.output.Get(), nullptr, &next.target))) { (void)restore(); return false; }
            c = std::move(next);
        }
        context->CopyResource(c.snapshot.Get(), original); context->CopyResource(c.output.Get(), c.snapshot.Get());
        Constants p{};
        for (unsigned j = 0; j < 4; ++j) { p.source[j] = a.source_rect[j]; p.output[j] = float(a.output_rect[j]); }
        for (unsigned j = 0; j < 2; ++j) {
            p.valid[j] = std::ceil(a.source_rect[j] - .5f);
            p.valid[j + 2] = std::max(p.valid[j], std::ceil(a.source_rect[j + 2] - .5f) - 1);
            p.jitter[j] = a.jitter_pixels[j];
        }
        const auto* r = a.output_rect;
        const D3D11_VIEWPORT viewport{float(r[0]), float(r[1]), float(r[2]-r[0]), float(r[3]-r[1]), 0, 1};
        context->RSSetViewports(1, &viewport); context->RSSetState(s.raster.Get());
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        context->VSSetShader(s.vertex.Get(), nullptr, 0); context->PSSetShader(s.pixel.Get(), nullptr, 0);
        auto* target = c.target.Get(); context->OMSetRenderTargets(1, &target, nullptr);
        auto* view = c.view.Get(); context->PSSetShaderResources(0, 1, &view);
        auto* buffer = s.constants.Get(); context->PSSetConstantBuffers(0, 1, &buffer); context->UpdateSubresource(buffer, 0, nullptr, &p, 0, 0);
        context->Draw(3, 0); context->ClearState(); c.original = original; c.prepared = true;
        c.restore_contents = (a.flags & RSF_D3D11_AUXILIARY_RESTORE_CONTENTS) != 0;
    }
    if (FAILED(device->GetDeviceRemovedReason())) { (void)restore(); return false; }
    return true;
}
catch (...) { (void)restore(); return false; }
bool AuxiliarySurfaces::commit() {
    auto& s = *state_;
    if (!s.ready || s.pending || s.thread != GetCurrentThreadId()) return false;
    Bindings bindings(s.context.Get()); if (!bindings.valid) return false;
    s.context->ClearState(); s.pending = true;
    if (s.depth_prepared && rsf_depth_scale_commit(s.depth)) return false;
    for (auto& c : s.colours) if (c.prepared) { s.context->CopyResource(c.original.Get(), c.output.Get()); c.committed = true; }
    if (FAILED(s.device->GetDeviceRemovedReason())) return false;
    s.accepted = true;
    // SR and every auxiliary committed; native colour remains through HDR and is not rolled back at frame end.
    for (auto& b : s.backups) { b.original.Reset(); b.captured = false; }
    return true;
}
bool AuxiliarySurfaces::restore() {
    auto& s = *state_;
    if (!s.ready && !s.pending) return true;
    if (s.thread != GetCurrentThreadId()) return false;
    Bindings bindings(s.context.Get()); if (!bindings.valid) return false;
    s.context->ClearState();
    if (s.depth_prepared && ((s.accepted && !s.depth_restore) ? rsf_depth_scale_discard(s.depth) : rsf_depth_scale_restore(s.depth))) return false;
    s.depth_prepared = false;
    for (auto& c : s.colours) {
        if (c.committed && (!s.accepted || c.restore_contents)) s.context->CopyResource(c.original.Get(), c.snapshot.Get());
        c.original.Reset(); c.prepared = c.committed = false;
    }
    // These leases survive only until a successful commit; failures restore the native SR destinations too.
    for (auto& b : s.backups) {
        if (b.original && b.captured) s.context->CopyResource(b.original.Get(), b.snapshot.Get());
        b.original.Reset(); b.captured = false;
    }
    if (FAILED(s.device->GetDeviceRemovedReason())) return false;
    s.ready = s.pending = s.accepted = false; return true;
}
