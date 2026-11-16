// SPDX-License-Identifier: MIT
#include "auxiliary_sr.h"
#include "../../graphics/src/depth_scale_private.h"
#include <rescaleframe/depth_scale.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>
#include <array>
#include <algorithm>
#include <cmath>
#include <cstring>

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
    ComPtr<ID3D11Texture2D> original, source, snapshot, output;
    ComPtr<ID3D11ShaderResourceView> view;
    ComPtr<ID3D11RenderTargetView> target;
    DXGI_FORMAT format{}, source_format{}; UINT width{}, height{}, source_width{}, source_height{};
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
bool colour_format(ID3D11Device* device, DXGI_FORMAT f) {
    if (f == DXGI_FORMAT_R11G11B10_FLOAT) {
        // The nearest numeric path loads a snapshot and draws into a same-format
        // RTV. Typed UAV access is not used and is not an admission requirement.
        UINT support{};
        constexpr UINT required = D3D11_FORMAT_SUPPORT_TEXTURE2D | D3D11_FORMAT_SUPPORT_SHADER_LOAD |
            D3D11_FORMAT_SUPPORT_RENDER_TARGET;
        return SUCCEEDED(device->CheckFormatSupport(f, &support)) && (support & required) == required;
    }
    return f == DXGI_FORMAT_R32G32B32A32_FLOAT || f == DXGI_FORMAT_R8_UNORM || f == DXGI_FORMAT_R8G8B8A8_UNORM || f == DXGI_FORMAT_R16G16_FLOAT ||
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
    std::array<Colour, 10> colours;
    std::array<rsf_d3d11_auxiliary_surface, 10> inputs{};
    uint32_t count{};
    bool reserved{};
    uint32_t main_begin{10};
    std::array<Backup, 2> backups;
    rsf_d3d11_auxiliary_scope scope{};
    bool depth_prepared{}, depth_restore{}, ready{}, pending{}, accepted{};
    ~State() { (void)rsf_depth_scale_destroy(depth); }
};
AuxiliarySurfaces::AuxiliarySurfaces() : state_(std::make_unique<State>()) {}
AuxiliarySurfaces::~AuxiliarySurfaces() { (void)restore(); }
bool AuxiliarySurfaces::pending() const { return state_->reserved || state_->pending || state_->ready; }
bool AuxiliarySurfaces::matches(const rsf_d3d11_auxiliary_scope& scope) const {
    const auto& s = *state_;
    return pending() && s.thread == GetCurrentThreadId() && scope.session_id == s.scope.session_id &&
        scope.native_frame == s.scope.native_frame && scope.view_key == s.scope.view_key;
}
bool AuxiliarySurfaces::prepare(ID3D11DeviceContext* context, const rsf_game_render_pass& pass,
    const rsf_d3d11_auxiliary_pass& inputs, rsf_game_log_fn log, void* user) {
    return reserve(context, pass, inputs, log, user) && seal();
}
bool AuxiliarySurfaces::reserve(ID3D11DeviceContext* context, const rsf_game_render_pass& pass,
    const rsf_d3d11_auxiliary_pass& inputs, rsf_game_log_fn log, void* user, bool recovery) try
{
    auto& s = *state_;
    if (pending() || !context || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE || !inputs.surface_count ||
        inputs.surface_count > (recovery ? 10u : 8u) || !inputs.surfaces) return false;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (s.context && (s.context.Get() != context || s.thread != GetCurrentThreadId())) return false;
    if (!s.context) { s.context = context; s.device = device; s.thread = GetCurrentThreadId(); }
    for (auto& c : s.colours) { c.original.Reset(); c.source.Reset(); c.prepared = c.committed = false; }
    for (auto& b : s.backups) { b.original.Reset(); b.captured = false; }
    // Validate every borrowed input before preparing any native snapshot.
    unsigned depth_count = 0;
    for (uint32_t i = 0; i < inputs.surface_count; ++i) {
        const auto& a = inputs.surfaces[i]; D3D11_TEXTURE2D_DESC d{};
        if (a.struct_size < sizeof(a) || a.kind > RSF_D3D11_AUXILIARY_COLOUR ||
            (a.flags & ~RSF_D3D11_AUXILIARY_RESTORE_CONTENTS) ||
            !simple_texture(static_cast<ID3D11Texture2D*>(a.texture), device.Get(), d)) return false;
        if (!recovery && (a.texture == pass.color_output || a.texture == pass.color_output_readable)) return false;
        for (uint32_t j = 0; j < i; ++j) if (a.texture == inputs.surfaces[j].texture) return false;
        for (unsigned j = 0; j < 4; ++j) if (!std::isfinite(a.source_rect[j])) return false;
        if (a.source_rect[0] < 0 || a.source_rect[1] < 0 || a.source_rect[2] > float(d.Width) || a.source_rect[3] > float(d.Height) ||
            a.source_rect[2] - a.source_rect[0] < 1 || a.source_rect[3] - a.source_rect[1] < 1 ||
            a.output_rect[0] < 0 || a.output_rect[1] < 0 || a.output_rect[2] <= a.output_rect[0] || a.output_rect[3] <= a.output_rect[1] ||
            uint32_t(a.output_rect[2]) > d.Width || uint32_t(a.output_rect[3]) > d.Height ||
            !std::isfinite(a.jitter_pixels[0]) || !std::isfinite(a.jitter_pixels[1]) ||
            std::abs(a.jitter_pixels[0]) > 16384 || std::abs(a.jitter_pixels[1]) > 16384) return false;
        if (recovery) {
            for (float bound : a.source_rect) if (std::floor(bound) != bound) return false;
            if (a.jitter_pixels[0] || a.jitter_pixels[1]) return false;
        }
        if (a.kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL) {
            if (++depth_count > 1 || d.Format != DXGI_FORMAT_R24G8_TYPELESS || !(d.BindFlags & D3D11_BIND_DEPTH_STENCIL)) return false;
            for (float bound : a.source_rect) if (std::floor(bound) != bound) return false;
        } else if (!colour_format(device.Get(), d.Format)) return false;
    }
    if (FAILED(device->GetDeviceRemovedReason())) return false;
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
    if (!make_backup(device.Get(), static_cast<ID3D11Texture2D*>(pass.color_output), s.backups[0]) ||
        (pass.color_output_readable && pass.color_output_readable != pass.color_output &&
        !make_backup(device.Get(), static_cast<ID3D11Texture2D*>(pass.color_output_readable), s.backups[1]))) return false;
    for (uint32_t i = 0; i < inputs.surface_count; ++i) {
        const auto& a = inputs.surfaces[i];
        if (a.kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL) {
            const rsf_depth_scale_setup setup{sizeof(setup), RSF_DEPTH_SCALE_ABI_VERSION, log, user};
            if ((!s.depth && rsf_depth_scale_create(context, &setup, &s.depth)) ||
                !rsf_depth_scale_reserve(s.depth, static_cast<ID3D11Texture2D*>(a.texture))) return false;
            continue;
        }
        auto& c = s.colours[i]; auto* original = static_cast<ID3D11Texture2D*>(a.texture);
        auto* source = recovery && (a.texture == pass.color_output || a.texture == pass.color_output_readable) ?
            static_cast<ID3D11Texture2D*>(pass.color_input) : original;
        D3D11_TEXTURE2D_DESC d{}, src{};
        if (!simple_texture(source, device.Get(), src) || !colour_format(device.Get(), src.Format) ||
            a.source_rect[2] > float(src.Width) || a.source_rect[3] > float(src.Height)) return false;
        original->GetDesc(&d);
        if (!c.snapshot || c.width != d.Width || c.height != d.Height || c.format != d.Format ||
            c.source_width != src.Width || c.source_height != src.Height || c.source_format != src.Format) {
            Colour next{}; next.width = d.Width; next.height = d.Height; next.format = d.Format;
            next.source_width = src.Width; next.source_height = src.Height; next.source_format = src.Format;
            d.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE; d.MiscFlags = d.CPUAccessFlags = 0;
            src.BindFlags = D3D11_BIND_SHADER_RESOURCE; src.MiscFlags = src.CPUAccessFlags = 0;
            if (FAILED(device->CreateTexture2D(&src, nullptr, &next.snapshot)) ||
                FAILED(device->CreateTexture2D(&d, nullptr, &next.output)) ||
                FAILED(device->CreateShaderResourceView(next.snapshot.Get(), nullptr, &next.view)) ||
                FAILED(device->CreateRenderTargetView(next.output.Get(), nullptr, &next.target))) return false;
            c = std::move(next);
        }
        c.original = original; c.source = source;
        c.restore_contents = (a.flags & RSF_D3D11_AUXILIARY_RESTORE_CONTENTS) != 0;
    }
    s.main_begin = inputs.surface_count;
    if (recovery) for (uint32_t i = 0; i < inputs.surface_count; ++i)
        if (inputs.surfaces[i].texture == pass.color_output) { s.main_begin = i; break; }
    s.count = inputs.surface_count;
    for (uint32_t i = 0; i < s.count; ++i) {
        s.inputs[i] = inputs.surfaces[i]; s.inputs[i].name = s.inputs[i].source_name = nullptr;
    }
    s.reserved = true;
    return true;
}
catch (...) { return false; }
bool AuxiliarySurfaces::refine(const rsf_d3d11_auxiliary_pass& request) {
    auto& s = *state_;
    if (!s.reserved || s.ready || s.thread != GetCurrentThreadId() || request.surface_count != s.count || !request.surfaces) return false;
    for (uint32_t i = 0; i < s.count; ++i) {
        const auto& a = request.surfaces[i]; const auto& old = s.inputs[i];
        if (a.struct_size < sizeof(a) || a.texture != old.texture || a.kind != old.kind || a.flags != old.flags ||
            std::memcmp(a.output_rect, old.output_rect, sizeof(a.output_rect)) || a.jitter_pixels[0] || a.jitter_pixels[1]) return false;
        D3D11_TEXTURE2D_DESC d{}; static_cast<ID3D11Texture2D*>(a.texture)->GetDesc(&d);
        for (float bound : a.source_rect) if (!std::isfinite(bound) || std::floor(bound) != bound) return false;
        if (a.source_rect[0] < 0 || a.source_rect[1] < 0 || a.source_rect[2] > float(d.Width) || a.source_rect[3] > float(d.Height) ||
            a.source_rect[2] <= a.source_rect[0] || a.source_rect[3] <= a.source_rect[1]) return false;
        if (s.colours[i].source) {
            s.colours[i].source->GetDesc(&d);
            if (a.source_rect[2] > float(d.Width) || a.source_rect[3] > float(d.Height)) return false;
        }
    }
    for (uint32_t i = 0; i < s.count; ++i) {
        s.inputs[i] = request.surfaces[i]; s.inputs[i].name = s.inputs[i].source_name = nullptr;
    }
    return true;
}
ID3D11Texture2D* AuxiliarySurfaces::original_colour() const {
    const auto& s = *state_;
    return s.ready && s.main_begin < s.count ? s.colours[s.main_begin].snapshot.Get() : nullptr;
}
bool AuxiliarySurfaces::sealed() const { return state_->ready; }
bool AuxiliarySurfaces::seal() { return render(nullptr, true); }
bool AuxiliarySurfaces::spatial_prepare() { return render(nullptr, false); }
bool AuxiliarySurfaces::temporal_prepare(const rsf_d3d11_auxiliary_pass& request) {
    auto& s = *state_;
    if (!s.ready || s.pending || request.surface_count != s.main_begin || !request.surfaces) return false;
    for (uint32_t i = 0; i < request.surface_count; ++i) {
        const auto& a = request.surfaces[i]; const auto& original = s.inputs[i];
        if (a.struct_size < sizeof(a) || a.texture != original.texture || a.kind != original.kind || a.flags != original.flags ||
            std::memcmp(a.output_rect, original.output_rect, sizeof(a.output_rect))) return false;
        D3D11_TEXTURE2D_DESC d{}; static_cast<ID3D11Texture2D*>(a.texture)->GetDesc(&d);
        for (float bound : a.source_rect) if (!std::isfinite(bound)) return false;
        if (a.source_rect[0] < 0 || a.source_rect[1] < 0 || a.source_rect[2] > float(d.Width) || a.source_rect[3] > float(d.Height) ||
            a.source_rect[2] - a.source_rect[0] < 1 || a.source_rect[3] - a.source_rect[1] < 1 ||
            !std::isfinite(a.jitter_pixels[0]) || !std::isfinite(a.jitter_pixels[1]) ||
            std::abs(a.jitter_pixels[0]) > 16384 || std::abs(a.jitter_pixels[1]) > 16384) return false;
        if (a.kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL)
            for (float bound : a.source_rect) if (std::floor(bound) != bound) return false;
    }
    return render(&request, false);
}
bool AuxiliarySurfaces::render(const rsf_d3d11_auxiliary_pass* mapping, bool capture) try {
    auto& s = *state_;
    if (!s.reserved || (capture ? s.ready : !s.ready) || s.pending || s.thread != GetCurrentThreadId()) return false;
    auto* context = s.context.Get(); auto* device = s.device.Get();
    if (FAILED(device->GetDeviceRemovedReason())) return false;
    s.ready = true;
    Bindings bindings(context); if (!bindings.valid) { (void)restore(); return false; }
    context->ClearState();
    for (auto& b : s.backups) if (capture && b.original) { context->CopyResource(b.snapshot.Get(), b.original.Get()); b.captured = true; }
    for (uint32_t i = 0; i < (mapping ? mapping->surface_count : s.count); ++i) {
        const auto& a = mapping ? mapping->surfaces[i] : s.inputs[i];
        if (a.kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL) {
            rsf_depth_scale_frame f{sizeof(f), RSF_DEPTH_SCALE_ABI_VERSION, a.texture};
            for (unsigned j = 0; j < 4; ++j) { f.source_rect[j] = int32_t(a.source_rect[j]); f.output_rect[j] = a.output_rect[j]; }
            f.jitter_pixels[0] = a.jitter_pixels[0]; f.jitter_pixels[1] = a.jitter_pixels[1];
            if (capture ? rsf_depth_scale_prepare(s.depth, &f) : rsf_depth_scale_redraw(s.depth, &f)) return false;
            s.depth_prepared = true; s.depth_restore = (a.flags & RSF_D3D11_AUXILIARY_RESTORE_CONTENTS) != 0;
            context->ClearState(); continue;
        }
        auto& c = s.colours[i];
        if (capture) context->CopyResource(c.snapshot.Get(), c.source.Get());
        ID3D11Texture2D* baseline = c.snapshot.Get();
        if (i >= s.main_begin) for (auto& b : s.backups)
            if (b.original.Get() == c.original.Get()) { baseline = b.snapshot.Get(); break; }
        context->CopyResource(c.output.Get(), baseline);
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
        context->Draw(3, 0); context->ClearState(); c.prepared = true;
        c.restore_contents = (a.flags & RSF_D3D11_AUXILIARY_RESTORE_CONTENTS) != 0;
    }
    return SUCCEEDED(device->GetDeviceRemovedReason());
} catch (...) { return false; }
bool AuxiliarySurfaces::cancel() {
    auto& s = *state_;
    if (!s.reserved || s.ready || s.thread != GetCurrentThreadId()) return false;
    for (auto& c : s.colours) { c.original.Reset(); c.source.Reset(); }
    for (auto& b : s.backups) b.original.Reset();
    s.reserved = false; return true;
}
bool AuxiliarySurfaces::commit(bool temporal) {
    auto& s = *state_;
    if (!s.ready || s.pending || s.thread != GetCurrentThreadId()) return false;
    Bindings bindings(s.context.Get()); if (!bindings.valid) return false;
    s.context->ClearState(); s.pending = true;
    if (s.depth_prepared && rsf_depth_scale_commit(s.depth)) return false;
    for (uint32_t i = 0; i < s.count; ++i) {
        auto& c = s.colours[i];
        if (c.prepared && (!temporal || i < s.main_begin)) { s.context->CopyResource(c.original.Get(), c.output.Get()); c.committed = true; }
    }
    if (FAILED(s.device->GetDeviceRemovedReason())) return false;
    s.accepted = true;
    // SR and every auxiliary committed; native colour remains through HDR and is not rolled back at frame end.
    for (auto& b : s.backups) { b.original.Reset(); b.captured = false; }
    return true;
}
bool AuxiliarySurfaces::restore() {
    auto& s = *state_;
    if (!s.ready && !s.pending) return !s.reserved || cancel();
    if (s.thread != GetCurrentThreadId()) return false;
    Bindings bindings(s.context.Get()); if (!bindings.valid) return false;
    s.context->ClearState();
    if (s.depth_prepared && ((s.accepted && !s.depth_restore) ? rsf_depth_scale_discard(s.depth) : rsf_depth_scale_restore(s.depth))) return false;
    s.depth_prepared = false;
    for (uint32_t i = 0; i < s.count; ++i) {
        auto& c = s.colours[i];
        if (i < s.main_begin && c.committed && (!s.accepted || c.restore_contents))
            s.context->CopyResource(c.original.Get(), c.snapshot.Get());
    }
    // Only unsuccessful publication rolls Main back. Successful output survives retirement failure.
    for (auto& b : s.backups)
        if (b.original && b.captured) s.context->CopyResource(b.original.Get(), b.snapshot.Get());
    if (FAILED(s.device->GetDeviceRemovedReason())) return false;
    for (auto& c : s.colours) { c.original.Reset(); c.source.Reset(); c.prepared = c.committed = false; }
    for (auto& b : s.backups) { b.original.Reset(); b.captured = false; }
    s.reserved = s.ready = s.pending = s.accepted = false; return true;
}
