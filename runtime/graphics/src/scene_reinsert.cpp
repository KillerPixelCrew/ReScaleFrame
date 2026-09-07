// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/scene_reinsert.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <new>

namespace {

// One texture promoted to output resolution, with the views the substitution binds in its place.
//
// Both views exist even where only one is used. A render target view and a shader resource view on
// the same texture cost nothing to hold, the composite genuinely needs both, and a replacement that
// could only be written or only be read would fail the first time the frame did the other.
struct Replacement {
    // The game's texture this stands in for, held by address only. Comparing is all this does with
    // it, and retaining a pooled engine target would change when the engine may reuse it.
    ID3D11Texture2D* original = nullptr;
    ID3D11Texture2D* texture = nullptr;
    ID3D11RenderTargetView* target_view = nullptr;
    ID3D11ShaderResourceView* shader_view = nullptr;
};

void release_replacement(Replacement& replacement)
{
    if (replacement.shader_view) {
        replacement.shader_view->Release();
    }
    if (replacement.target_view) {
        replacement.target_view->Release();
    }
    if (replacement.texture) {
        replacement.texture->Release();
    }
    replacement = Replacement{};
}

} // namespace

struct rsf_reinsert {
    ID3D11Device* device = nullptr;
    uint32_t output_width = 0;
    uint32_t output_height = 0;
    rsf_reinsert_log_fn log = nullptr;
    void* log_user = nullptr;

    Replacement composite;
    Replacement interface_targets[RSF_REINSERT_MAX_INTERFACE_TARGETS];
    uint32_t interface_target_count;

    // Not a replacement: the reconstruction already exists at output resolution and belongs to
    // whoever produced it. All this owns is a way to read it.
    ID3D11Texture2D* scene_color = nullptr;
    ID3D11Texture2D* reconstruction = nullptr;
    ID3D11ShaderResourceView* reconstruction_view = nullptr;

    uint32_t render_width = 0;
    uint32_t render_height = 0;
    bool ready = false;
};

namespace {

void say(const rsf_reinsert* reinsert, const char* format, ...)
{
    if (!reinsert || !reinsert->log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    reinsert->log(reinsert->log_user, message);
}

// Build an output resolution stand-in for one of the game's render resolution targets.
//
// The format is copied from the original rather than chosen. What goes into the composite is the
// game's own tonemapped output and its own interface, drawn by the game's own shaders, so the
// target they write has to be the one they expect in everything except its size.
bool build_replacement(rsf_reinsert* reinsert, ID3D11Texture2D* original, Replacement& out,
                       const char* what)
{
    release_replacement(out);
    if (!original) {
        return true;  // not identified, which is a stated outcome rather than a failure
    }

    D3D11_TEXTURE2D_DESC description{};
    original->GetDesc(&description);
    say(reinsert, "reinsert: promoting the %s from %ux%u to %ux%u, format %u", what,
        description.Width, description.Height, reinsert->output_width, reinsert->output_height,
        unsigned(description.Format));

    description.Width = reinsert->output_width;
    description.Height = reinsert->output_height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.SampleDesc.Count = 1;
    description.SampleDesc.Quality = 0;
    description.Usage = D3D11_USAGE_DEFAULT;
    // Both, whatever the original declared. The composite is written by the tonemap and read by the
    // last draw, and a replacement that inherited a write-only original could not be read back.
    description.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    description.CPUAccessFlags = 0;
    description.MiscFlags = 0;

    if (FAILED(reinsert->device->CreateTexture2D(&description, nullptr, &out.texture)) ||
        !out.texture) {
        say(reinsert, "reinsert: the %s replacement could not be created", what);
        release_replacement(out);
        return false;
    }
    if (FAILED(reinsert->device->CreateRenderTargetView(out.texture, nullptr, &out.target_view)) ||
        FAILED(reinsert->device->CreateShaderResourceView(out.texture, nullptr, &out.shader_view))) {
        say(reinsert, "reinsert: the %s replacement views could not be created", what);
        release_replacement(out);
        return false;
    }
    out.original = original;
    return true;
}

} // namespace

extern "C" rsf_reinsert_result rsf_reinsert_create(const rsf_reinsert_setup* setup,
                                                   rsf_reinsert** out)
{
    if (!setup || !out || setup->struct_size < sizeof(rsf_reinsert_setup) || !setup->device ||
        setup->output_width == 0 || setup->output_height == 0) {
        return RSF_REINSERT_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_REINSERT_ABI_VERSION) {
        return RSF_REINSERT_ERROR_ABI_MISMATCH;
    }
    auto* reinsert = new (std::nothrow) rsf_reinsert{};
    if (!reinsert) {
        return RSF_REINSERT_ERROR_RESOURCE_FAILED;
    }
    reinsert->device = static_cast<ID3D11Device*>(setup->device);
    reinsert->device->AddRef();
    reinsert->output_width = setup->output_width;
    reinsert->output_height = setup->output_height;
    reinsert->log = setup->log;
    reinsert->log_user = setup->log_user;
    *out = reinsert;
    return RSF_REINSERT_OK;
}

extern "C" rsf_reinsert_result rsf_reinsert_prepare(rsf_reinsert* reinsert,
                                                    const rsf_reinsert_frame_tail* tail)
{
    if (!reinsert || !tail || tail->struct_size < sizeof(rsf_reinsert_frame_tail) ||
        !tail->composite || !tail->scene_color || !tail->reconstruction || tail->render_width == 0 ||
        tail->render_height == 0) {
        return RSF_REINSERT_ERROR_INVALID_ARGUMENT;
    }
    // Substituting a texture for one of its own size changes nothing except how many textures the
    // frame has. Saying so is the difference between a run that shows no improvement and a run that
    // was never upscaling: the game puts its own screen percentage back when a mission loads, and
    // that is exactly what this looks like from inside the frame.
    if (tail->render_width >= reinsert->output_width ||
        tail->render_height >= reinsert->output_height) {
        say(reinsert,
            "reinsert: the game is rendering at %ux%u against an output of %ux%u, so there is "
            "nothing to reinsert into",
            tail->render_width, tail->render_height, reinsert->output_width,
            reinsert->output_height);
        return RSF_REINSERT_ERROR_NOT_SCALED;
    }

    reinsert->ready = false;
    if (reinsert->reconstruction_view) {
        reinsert->reconstruction_view->Release();
        reinsert->reconstruction_view = nullptr;
    }

    if (!build_replacement(reinsert, static_cast<ID3D11Texture2D*>(tail->composite),
                           reinsert->composite, "composite")) {
        return RSF_REINSERT_ERROR_RESOURCE_FAILED;
    }
    for (uint32_t index = 0; index < reinsert->interface_target_count; ++index) {
        release_replacement(reinsert->interface_targets[index]);
    }
    reinsert->interface_target_count = 0;
    for (uint32_t index = 0;
         index < tail->interface_target_count && index < RSF_REINSERT_MAX_INTERFACE_TARGETS;
         ++index) {
        if (!tail->interface_targets[index]) {
            continue;
        }
        if (!build_replacement(reinsert,
                               static_cast<ID3D11Texture2D*>(tail->interface_targets[index]),
                               reinsert->interface_targets[reinsert->interface_target_count],
                               "interface target")) {
            for (uint32_t undo = 0; undo < reinsert->interface_target_count; ++undo) {
                release_replacement(reinsert->interface_targets[undo]);
            }
            reinsert->interface_target_count = 0;
            release_replacement(reinsert->composite);
            return RSF_REINSERT_ERROR_RESOURCE_FAILED;
        }
        ++reinsert->interface_target_count;
    }
    if (reinsert->interface_target_count == 0) {
        say(reinsert,
            "reinsert: the interface's own target was not identified, so the interface is "
            "magnified with the scene rather than drawn at output resolution");
    }

    if (FAILED(reinsert->device->CreateShaderResourceView(
            static_cast<ID3D11Resource*>(tail->reconstruction), nullptr,
            &reinsert->reconstruction_view))) {
        say(reinsert, "reinsert: the reconstruction could not be made readable");
        release_replacement(reinsert->composite);
        for (uint32_t index = 0; index < reinsert->interface_target_count; ++index) {
            release_replacement(reinsert->interface_targets[index]);
        }
        reinsert->interface_target_count = 0;
        return RSF_REINSERT_ERROR_RESOURCE_FAILED;
    }

    reinsert->scene_color = static_cast<ID3D11Texture2D*>(tail->scene_color);
    reinsert->reconstruction = static_cast<ID3D11Texture2D*>(tail->reconstruction);
    reinsert->render_width = tail->render_width;
    reinsert->render_height = tail->render_height;
    reinsert->ready = true;
    say(reinsert, "reinsert: ready, scaling %ux%u up to %ux%u", tail->render_width,
        tail->render_height, reinsert->output_width, reinsert->output_height);
    return RSF_REINSERT_OK;
}

extern "C" rsf_reinsert_result rsf_reinsert_fill_plan(rsf_reinsert* reinsert,
                                                      rsf_frame_tap_plan* plan)
{
    if (!reinsert || !plan || plan->struct_size < sizeof(rsf_frame_tap_plan)) {
        return RSF_REINSERT_ERROR_INVALID_ARGUMENT;
    }
    if (!reinsert->ready) {
        return RSF_REINSERT_ERROR_INVALID_ARGUMENT;
    }

    const uint32_t size = plan->struct_size;
    *plan = rsf_frame_tap_plan{};
    plan->struct_size = size;
    plan->viewport_scale_x = float(reinsert->output_width) / float(reinsert->render_width);
    plan->viewport_scale_y = float(reinsert->output_height) / float(reinsert->render_height);

    // The composite first, and ungated: everything drawn into it has to land at output resolution,
    // including the interface passes that run before the tonemap.
    rsf_frame_tap_substitution& composite = plan->items[plan->count++];
    composite.texture = reinsert->composite.original;
    composite.shader_view = reinsert->composite.shader_view;
    composite.render_view = reinsert->composite.target_view;

    for (uint32_t index = 0;
         index < reinsert->interface_target_count && plan->count < RSF_FRAME_TAP_MAX_SUBSTITUTIONS;
         ++index) {
        if (!reinsert->interface_targets[index].original) {
            continue;
        }
        rsf_frame_tap_substitution& interface_item = plan->items[plan->count++];
        interface_item.texture = reinsert->interface_targets[index].original;
        interface_item.shader_view = reinsert->interface_targets[index].shader_view;
        interface_item.render_view = reinsert->interface_targets[index].target_view;
    }

    // Scene colour last and gated on the composite, because the scene passes read scene colour
    // while they are still writing it. An ungated substitution here hands a lighting pass a
    // reconstruction of the frame it has not finished, which is a feedback loop rather than an
    // upscale, and it would look like a smear that gets worse the longer the camera holds still.
    rsf_frame_tap_substitution& scene = plan->items[plan->count++];
    scene.texture = reinsert->scene_color;
    scene.shader_view = reinsert->reconstruction_view;
    scene.after_target = reinsert->composite.original;
    return RSF_REINSERT_OK;
}

extern "C" rsf_reinsert_result rsf_reinsert_get_status(rsf_reinsert* reinsert,
                                                       rsf_reinsert_status* status)
{
    if (!reinsert || !status || status->struct_size < sizeof(rsf_reinsert_status)) {
        return RSF_REINSERT_ERROR_INVALID_ARGUMENT;
    }
    status->ready = reinsert->ready ? 1u : 0u;
    status->interface_promoted = reinsert->interface_target_count;
    status->render_width = reinsert->render_width;
    status->render_height = reinsert->render_height;
    status->output_width = reinsert->output_width;
    status->output_height = reinsert->output_height;
    return RSF_REINSERT_OK;
}

extern "C" void rsf_reinsert_destroy(rsf_reinsert* reinsert)
{
    if (!reinsert) {
        return;
    }
    // The caller clears the tap's plan before this, which the header says. Nothing here can check
    // it: the views are the tap's to stop using, and releasing them while a plan still names them
    // would take the game down inside a binding call rather than here.
    if (reinsert->reconstruction_view) {
        reinsert->reconstruction_view->Release();
    }
    for (uint32_t index = 0; index < reinsert->interface_target_count; ++index) {
        release_replacement(reinsert->interface_targets[index]);
    }
    release_replacement(reinsert->composite);
    if (reinsert->device) {
        reinsert->device->Release();
    }
    delete reinsert;
}
