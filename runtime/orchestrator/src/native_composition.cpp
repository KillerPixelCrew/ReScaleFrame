// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_composition.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <cstring>

namespace {
using Microsoft::WRL::ComPtr;
struct Image {
    ComPtr<ID3D11Texture2D> texture;
    D3D11_TEXTURE2D_DESC descriptor{};
    // Own a same-format immutable snapshot for this scope. Generation changes only when storage
    // changes; CopyResource completion is ordered by the caller's immediate context.
    bool capture(ID3D11DeviceContext* context, void* source, uint32_t& generation)
    {
        if (!source) return false;
        ComPtr<ID3D11Texture2D> input;
        if (FAILED(static_cast<ID3D11Resource*>(source)->QueryInterface(IID_PPV_ARGS(&input)))) return false;
        D3D11_TEXTURE2D_DESC desc{}; input->GetDesc(&desc);
        if (desc.SampleDesc.Count != 1 || desc.ArraySize != 1 || desc.MipLevels != 1) return false;
        ComPtr<ID3D11Device> device, input_device, previous_device;
        context->GetDevice(&device); input->GetDevice(&input_device);
        if (input_device.Get() != device.Get()) return false;
        if (texture) texture->GetDevice(&previous_device);
        if (!texture || previous_device.Get() != device.Get() || descriptor.Width != desc.Width ||
            descriptor.Height != desc.Height || descriptor.Format != desc.Format) {
            desc.Usage = D3D11_USAGE_DEFAULT; desc.CPUAccessFlags = 0; desc.MiscFlags = 0;
            desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
            ComPtr<ID3D11Texture2D> replacement;
            if (FAILED(device->CreateTexture2D(&desc, nullptr, &replacement))) return false;
            texture = replacement; descriptor = desc; ++generation;
        }
        context->CopyResource(texture.Get(), input.Get());
        return true;
    }
};
// One graphics owner, one pending UI-composite pair. A new requested begin invalidates the
// previously complete metadata before copying any inputs, so failures cannot expose stale data.
struct Composition {
    Image scene, ui, composed;
    rsf_native_composition_frame frame{};
    uint32_t generation = 0;
    bool pending = false;
    uint32_t requested = 0;
};
Composition state;
}
extern "C" RSF_RUNTIME_API int rsf_native_composition_capture(void* native_context,
    const rsf_game_render_pass* pass, uint32_t begin)
{
    if (!native_context || !pass || pass->struct_size < sizeof(*pass) ||
        pass->role != RSF_GAME_RENDER_UI_COMPOSITE || !(pass->flags & RSF_GAME_RENDER_PRIMARY) ||
        !pass->session_id || !pass->view_key || !pass->scope_id) return 0;
    auto* context = static_cast<ID3D11DeviceContext*>(native_context);
    auto& frame = state.frame;
    if (begin && !state.requested) return 0;
    if (begin) {
        if (state.requested != UINT32_MAX) --state.requested;
        frame = {}; frame.struct_size = sizeof(frame); state.pending = false;
        frame.session_id = pass->session_id; frame.family_key = pass->family_key;
        frame.view_key = pass->view_key; frame.native_frame = pass->native_frame;
        frame.scope_id = pass->scope_id;
        frame.source_frame_id = pass->source_frame_id; frame.submission_id = pass->submission_id;
        frame.flags = pass->flags; frame.viewport_key = pass->viewport_key;
        std::memcpy(frame.output_rect, pass->output_rect, sizeof(frame.output_rect));
        if (!state.scene.capture(context, pass->color_input, state.generation)) return 0;
        frame.scene = state.scene.texture.Get();
        if (pass->ui_input) {
            if (!state.ui.capture(context, pass->ui_input, state.generation)) return 0;
            frame.ui_raster = state.ui.texture.Get();
        }
        state.pending = true;
        return 1;
    }
    if (!state.pending || frame.session_id != pass->session_id || frame.scope_id != pass->scope_id ||
        frame.family_key != pass->family_key || frame.view_key != pass->view_key ||
        frame.native_frame != pass->native_frame || frame.source_frame_id != pass->source_frame_id ||
        frame.submission_id != pass->submission_id || frame.viewport_key != pass->viewport_key ||
        std::memcmp(frame.output_rect, pass->output_rect, sizeof(frame.output_rect))) return 0;
    state.pending = false;
    if (!state.composed.capture(context,
        pass->color_output_readable ? pass->color_output_readable : pass->color_output,
        state.generation)) return 0;
    frame.composed = state.composed.texture.Get();
    frame.resource_generation = state.generation; frame.complete = 1;
    return 1;
}
extern "C" RSF_RUNTIME_API void rsf_native_composition_request(uint32_t scopes)
{
    state.requested = scopes;
    if (!scopes) { state.pending = false; state.frame = {}; }
}
extern "C" RSF_RUNTIME_API int rsf_native_composition_read(rsf_native_composition_frame* frame)
{
    if (!frame || frame->struct_size < sizeof(*frame) || !state.frame.complete) return 0;
    *frame = state.frame; return 1;
}
extern "C" RSF_RUNTIME_API void rsf_native_composition_release(void)
{ state = {}; }
