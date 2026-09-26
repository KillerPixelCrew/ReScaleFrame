// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/scene_promote.h>
#include <rescaleframe/fullscreen_pass.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <new>

// The plan has to hold every promoted surface at once: the composite, the scene colour, and both
// sets. A plan that describes the frame correctly and is refused for being one entry too long is
// the worst kind of failure, because it reports itself on, and it has happened once.
static_assert(4u + RSF_PROMOTE_MAX_UI_TARGETS + RSF_PROMOTE_MAX_CHAIN_TARGETS <=
                  RSF_FRAME_TAP_MAX_SUBSTITUTIONS,
              "the frame tap's plan cannot hold everything promotion may name");

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

void release_set(Replacement* set, uint32_t& count)
{
    for (uint32_t index = 0; index < count; ++index) {
        release_replacement(set[index]);
    }
    count = 0;
}

} // namespace

struct rsf_promote {
    ID3D11Device* device = nullptr;
    uint32_t output_width = 0;
    uint32_t output_height = 0;
    rsf_promote_log_fn log = nullptr;
    void* log_user = nullptr;

    Replacement composite;
    // The recombine route: the recombined target and scene colour as a whole surface, and the pass
    // that seeds scene colour's stand-in with the reconstruction.
    Replacement composed;
    Replacement scene;
    // Where the game's own writes into scene colour go after the recombine: temporal AA's output,
    // which the reconstruction replaces. Never read.
    Replacement scratch;
    rsf_fullscreen_pass* seed_pass = nullptr;
    Replacement ui_targets[RSF_PROMOTE_MAX_UI_TARGETS];
    uint32_t ui_target_count = 0;
    Replacement chain_targets[RSF_PROMOTE_MAX_CHAIN_TARGETS];
    uint32_t chain_target_count = 0;

    // Not a replacement: the reconstruction already exists at output resolution and belongs to
    // whoever produced it. All this owns is a way to read it.
    ID3D11Texture2D* scene_color = nullptr;
    ID3D11Texture2D* reconstruction = nullptr;
    ID3D11ShaderResourceView* reconstruction_view = nullptr;
    // The recombine route's fallback: the game's own scene colour, readable, for a frame without a
    // reconstruction.
    ID3D11ShaderResourceView* scene_source_view = nullptr;

    uint32_t render_width = 0;
    uint32_t render_height = 0;
    bool ready = false;
};

namespace {

void say(const rsf_promote* promote, const char* format, ...)
{
    if (!promote || !promote->log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    promote->log(promote->log_user, message);
}

// The format a view on a texture takes: the one the game binds with, where the tail says, and
// otherwise the texture's own format with a typeless family resolved to its plain UNORM member,
// because a view on a typeless texture has to name one.
DXGI_FORMAT typed_view_format(DXGI_FORMAT texture_format, uint32_t hint)
{
    if (hint != 0) {
        return static_cast<DXGI_FORMAT>(hint);
    }
    switch (texture_format) {
    case DXGI_FORMAT_R8G8B8A8_TYPELESS:
        return DXGI_FORMAT_R8G8B8A8_UNORM;
    case DXGI_FORMAT_B8G8R8A8_TYPELESS:
        return DXGI_FORMAT_B8G8R8A8_UNORM;
    case DXGI_FORMAT_B8G8R8X8_TYPELESS:
        return DXGI_FORMAT_B8G8R8X8_UNORM;
    case DXGI_FORMAT_R10G10B10A2_TYPELESS:
        return DXGI_FORMAT_R10G10B10A2_UNORM;
    case DXGI_FORMAT_R16G16B16A16_TYPELESS:
        return DXGI_FORMAT_R16G16B16A16_FLOAT;
    case DXGI_FORMAT_R11G11B10_FLOAT:
    default:
        return texture_format;
    }
}

// Build an output resolution stand-in for one of the game's render resolution targets.
//
// The format is copied from the original rather than chosen. What goes into these targets is the
// game's own tonemapped output and its own interface, drawn by the game's own shaders, so the
// target they write has to be the one they expect in everything except its size. The views are
// typed, see `typed_view_format`: the first Windows run refused every promotion because a view on
// a typeless texture cannot be created without a format.
bool build_replacement(rsf_promote* promote, ID3D11Texture2D* original, Replacement& out,
                       const char* what, uint32_t view_format_hint)
{
    release_replacement(out);
    if (!original) {
        return true;  // not identified, which is a stated outcome rather than a failure
    }

    D3D11_TEXTURE2D_DESC description{};
    original->GetDesc(&description);
    say(promote, "promote: the %s %p, %ux%u format %u, becomes %ux%u", what,
        static_cast<void*>(original), description.Width, description.Height,
        unsigned(description.Format), promote->output_width, promote->output_height);

    description.Width = promote->output_width;
    description.Height = promote->output_height;
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

    if (FAILED(promote->device->CreateTexture2D(&description, nullptr, &out.texture)) ||
        !out.texture) {
        say(promote, "promote: the %s replacement could not be created", what);
        release_replacement(out);
        return false;
    }
    const DXGI_FORMAT view_format = typed_view_format(description.Format, view_format_hint);
    D3D11_RENDER_TARGET_VIEW_DESC target_description{};
    target_description.Format = view_format;
    target_description.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
    D3D11_SHADER_RESOURCE_VIEW_DESC shader_description{};
    shader_description.Format = view_format;
    shader_description.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    shader_description.Texture2D.MipLevels = 1;
    if (FAILED(promote->device->CreateRenderTargetView(out.texture, &target_description,
                                                       &out.target_view)) ||
        FAILED(promote->device->CreateShaderResourceView(out.texture, &shader_description,
                                                         &out.shader_view))) {
        say(promote, "promote: the %s replacement views could not be created with view format %u",
            what, unsigned(view_format));
        release_replacement(out);
        return false;
    }
    out.original = original;
    return true;
}

// Build replacements for a whole set, skipping null entries and refusing duplicates, which a caller
// collecting targets from several frames can easily hand over twice.
bool build_set(rsf_promote* promote, void* const* originals, uint32_t offered, Replacement* set,
               uint32_t capacity, uint32_t& count, const char* what, uint32_t view_format_hint)
{
    release_set(set, count);
    for (uint32_t index = 0; index < offered && index < capacity; ++index) {
        auto* original = static_cast<ID3D11Texture2D*>(originals[index]);
        if (!original) {
            continue;
        }
        bool seen = false;
        for (uint32_t previous = 0; previous < count; ++previous) {
            if (set[previous].original == original) {
                seen = true;
                break;
            }
        }
        if (seen) {
            continue;
        }
        if (!build_replacement(promote, original, set[count], what, view_format_hint)) {
            release_set(set, count);
            return false;
        }
        ++count;
    }
    return true;
}

void release_everything(rsf_promote* promote)
{
    if (promote->reconstruction_view) {
        promote->reconstruction_view->Release();
        promote->reconstruction_view = nullptr;
    }
    if (promote->scene_source_view) {
        promote->scene_source_view->Release();
        promote->scene_source_view = nullptr;
    }
    release_set(promote->ui_targets, promote->ui_target_count);
    release_set(promote->chain_targets, promote->chain_target_count);
    release_replacement(promote->composite);
    release_replacement(promote->composed);
    release_replacement(promote->scene);
    release_replacement(promote->scratch);
    promote->ready = false;
}

void add_promoted(rsf_frame_tap_plan* plan, const Replacement* set, uint32_t count)
{
    for (uint32_t index = 0; index < count && plan->count < RSF_FRAME_TAP_MAX_SUBSTITUTIONS;
         ++index) {
        if (!set[index].original) {
            continue;
        }
        rsf_frame_tap_substitution& item = plan->items[plan->count++];
        item.texture = set[index].original;
        item.shader_view = set[index].shader_view;
        item.render_view = set[index].target_view;
    }
}

} // namespace

extern "C" rsf_promote_result rsf_promote_create(const rsf_promote_setup* setup,
                                                 rsf_promote** out)
{
    if (!setup || !out || setup->struct_size < sizeof(rsf_promote_setup) || !setup->device ||
        setup->output_width == 0 || setup->output_height == 0) {
        return RSF_PROMOTE_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_PROMOTE_ABI_VERSION) {
        return RSF_PROMOTE_ERROR_ABI_MISMATCH;
    }
    auto* promote = new (std::nothrow) rsf_promote{};
    if (!promote) {
        return RSF_PROMOTE_ERROR_RESOURCE_FAILED;
    }
    promote->device = static_cast<ID3D11Device*>(setup->device);
    promote->device->AddRef();
    promote->output_width = setup->output_width;
    promote->output_height = setup->output_height;
    promote->log = setup->log;
    promote->log_user = setup->log_user;
    *out = promote;
    return RSF_PROMOTE_OK;
}

extern "C" rsf_promote_result rsf_promote_prepare(rsf_promote* promote,
                                                  const rsf_promote_frame_tail* tail)
{
    if (!promote || !tail || tail->struct_size < sizeof(rsf_promote_frame_tail) ||
        !tail->composite || !tail->scene_color || !tail->reconstruction ||
        tail->render_width == 0 || tail->render_height == 0) {
        return RSF_PROMOTE_ERROR_INVALID_ARGUMENT;
    }
    // Substituting a texture for one of its own size changes nothing except how many textures the
    // frame has. Saying so is the difference between a run that shows no improvement and a run that
    // was never upscaling: the game puts its own screen percentage back when a mission loads, and
    // that is exactly what this looks like from inside the frame.
    if (tail->render_width >= promote->output_width ||
        tail->render_height >= promote->output_height) {
        say(promote,
            "promote: the game is rendering at %ux%u against an output of %ux%u, so there is "
            "nothing to promote",
            tail->render_width, tail->render_height, promote->output_width,
            promote->output_height);
        return RSF_PROMOTE_ERROR_NOT_SCALED;
    }

    release_everything(promote);

    if (!build_replacement(promote, static_cast<ID3D11Texture2D*>(tail->composite),
                           promote->composite, "composite", tail->composite_view_format)) {
        return RSF_PROMOTE_ERROR_RESOURCE_FAILED;
    }
    if (!build_set(promote, tail->ui_targets, tail->ui_target_count, promote->ui_targets,
                   RSF_PROMOTE_MAX_UI_TARGETS, promote->ui_target_count, "interface layer",
                   tail->ui_target_view_format) ||
        !build_set(promote, tail->chain_targets, tail->chain_target_count, promote->chain_targets,
                   RSF_PROMOTE_MAX_CHAIN_TARGETS, promote->chain_target_count, "chain target",
                   tail->chain_view_format)) {
        release_everything(promote);
        return RSF_PROMOTE_ERROR_RESOURCE_FAILED;
    }
    if (tail->composed) {
        if (!build_replacement(promote, static_cast<ID3D11Texture2D*>(tail->composed),
                               promote->composed, "recombined colour", 0) ||
            !build_replacement(promote, static_cast<ID3D11Texture2D*>(tail->scene_color),
                               promote->scene, "scene colour", 0) ||
            !build_replacement(promote, static_cast<ID3D11Texture2D*>(tail->scene_color),
                               promote->scratch, "scene colour's discarded writes", 0)) {
            release_everything(promote);
            return RSF_PROMOTE_ERROR_RESOURCE_FAILED;
        }
        D3D11_TEXTURE2D_DESC scene_description{};
        static_cast<ID3D11Texture2D*>(tail->scene_color)->GetDesc(&scene_description);
        D3D11_SHADER_RESOURCE_VIEW_DESC source_description{};
        source_description.Format = typed_view_format(scene_description.Format, 0);
        source_description.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        source_description.Texture2D.MipLevels = 1;
        if (FAILED(promote->device->CreateShaderResourceView(
                static_cast<ID3D11Resource*>(tail->scene_color), &source_description,
                &promote->scene_source_view))) {
            say(promote, "promote: scene colour could not be made readable for the fallback seed");
            release_everything(promote);
            return RSF_PROMOTE_ERROR_RESOURCE_FAILED;
        }
        if (!promote->seed_pass) {
            rsf_fullscreen_setup pass{};
            pass.struct_size = sizeof(pass);
            pass.abi_version = RSF_FULLSCREEN_PASS_ABI_VERSION;
            if (rsf_fullscreen_pass_create(promote->device, &pass, &promote->seed_pass) !=
                RSF_FULLSCREEN_OK) {
                say(promote, "promote: the seed pass could not be created");
                release_everything(promote);
                return RSF_PROMOTE_ERROR_RESOURCE_FAILED;
            }
        }
    }
    if (promote->ui_target_count == 0) {
        say(promote,
            "promote: no interface layer has been named, so the interface is magnified with the "
            "scene rather than drawn at output resolution");
    }
    if (promote->chain_target_count == 0) {
        say(promote,
            "promote: no chain target has been named, so the scene is downsampled between the "
            "tonemap and the interface composite and the composite's promotion buys nothing "
            "visible");
    }

    if (FAILED(promote->device->CreateShaderResourceView(
            static_cast<ID3D11Resource*>(tail->reconstruction), nullptr,
            &promote->reconstruction_view))) {
        say(promote, "promote: the reconstruction could not be made readable");
        release_everything(promote);
        return RSF_PROMOTE_ERROR_RESOURCE_FAILED;
    }

    promote->scene_color = static_cast<ID3D11Texture2D*>(tail->scene_color);
    promote->reconstruction = static_cast<ID3D11Texture2D*>(tail->reconstruction);
    promote->render_width = tail->render_width;
    promote->render_height = tail->render_height;
    promote->ready = true;
    say(promote, "promote: ready, %ux%u up to %ux%u, %u interface layer%s, %u chain target%s",
        tail->render_width, tail->render_height, promote->output_width, promote->output_height,
        promote->ui_target_count, promote->ui_target_count == 1 ? "" : "s",
        promote->chain_target_count, promote->chain_target_count == 1 ? "" : "s");
    return RSF_PROMOTE_OK;
}

extern "C" rsf_promote_result rsf_promote_fill_plan(rsf_promote* promote, rsf_frame_tap_plan* plan)
{
    if (!promote || !plan || plan->struct_size < sizeof(rsf_frame_tap_plan)) {
        return RSF_PROMOTE_ERROR_INVALID_ARGUMENT;
    }
    if (!promote->ready) {
        return RSF_PROMOTE_ERROR_INVALID_ARGUMENT;
    }

    const uint32_t size = plan->struct_size;
    *plan = rsf_frame_tap_plan{};
    plan->struct_size = size;
    plan->viewport_scale_x = float(promote->output_width) / float(promote->render_width);
    plan->viewport_scale_y = float(promote->output_height) / float(promote->render_height);

    // The composite first, and ungated: everything drawn into it has to land at output resolution,
    // including the interface composite that runs after the tonemap.
    rsf_frame_tap_substitution& composite = plan->items[plan->count++];
    composite.texture = promote->composite.original;
    composite.shader_view = promote->composite.shader_view;
    composite.render_view = promote->composite.target_view;

    // The interface layers and the chain, ungated for the same reason: the quads draw before the
    // tonemap and the chain is filled after it, and both have to be at output resolution whenever
    // they are touched.
    add_promoted(plan, promote->ui_targets, promote->ui_target_count);
    add_promoted(plan, promote->chain_targets, promote->chain_target_count);

    // Scene colour last and gated on the composite, because the scene passes read scene colour
    // while they are still writing it. An ungated substitution here hands a lighting pass a
    // reconstruction of the frame it has not finished, which is a feedback loop rather than an
    // upscale, and it would look like a smear that gets worse the longer the camera holds still.
    if (promote->composed.original && promote->scene.original) {
        // The recombine route. The recombined target is promoted from the start of the frame, like
        // the composite. Scene colour becomes its stand-in once the recombined target is bound: the
        // gate is where the reconstruction runs and is drawn into that stand-in, so the recombine
        // reads it, the game's copy of the recombined result back into scene colour lands in it,
        // and the tonemap reads output resolution colour with full-size translucency on top.
        rsf_frame_tap_substitution& composed = plan->items[plan->count++];
        composed.texture = promote->composed.original;
        composed.shader_view = promote->composed.shader_view;
        composed.render_view = promote->composed.target_view;
        rsf_frame_tap_substitution& scene = plan->items[plan->count++];
        scene.texture = promote->scene.original;
        scene.shader_view = promote->scene.shader_view;
        scene.render_view = promote->scratch.target_view;
        scene.after_target = promote->composed.original;
        // A gate and nothing else: keyed on the scratch texture, which the game never binds, and
        // opened when the composite is bound for the tonemap. That is where `rsf_promote_finish`
        // puts the recombined result into scene colour's stand-in.
        rsf_frame_tap_substitution& finish = plan->items[plan->count++];
        finish.texture = promote->scratch.texture;
        finish.shader_view = promote->scratch.shader_view;
        finish.after_target = promote->composite.original;
        return RSF_PROMOTE_OK;
    }
    rsf_frame_tap_substitution& scene = plan->items[plan->count++];
    scene.texture = promote->scene_color;
    scene.shader_view = promote->reconstruction_view;
    scene.after_target = promote->composite.original;
    return RSF_PROMOTE_OK;
}

extern "C" rsf_promote_result rsf_promote_seed(rsf_promote* promote, void* context,
                                               uint32_t reconstructed)
{
    if (!promote || !context || !promote->ready) {
        return RSF_PROMOTE_ERROR_INVALID_ARGUMENT;
    }
    ID3D11ShaderResourceView* source =
        reconstructed ? promote->reconstruction_view : promote->scene_source_view;
    if (!promote->scene.target_view || !promote->seed_pass || !source) {
        return RSF_PROMOTE_OK;
    }
    rsf_fullscreen_draw draw{};
    draw.struct_size = sizeof(draw);
    draw.mode = RSF_FULLSCREEN_COPY;
    return rsf_fullscreen_pass_draw(promote->seed_pass, context, promote->scene.target_view,
                                    source, &draw) == RSF_FULLSCREEN_OK
               ? RSF_PROMOTE_OK
               : RSF_PROMOTE_ERROR_RESOURCE_FAILED;
}

extern "C" rsf_promote_result rsf_promote_finish(rsf_promote* promote, void* context)
{
    if (!promote || !context || !promote->ready) {
        return RSF_PROMOTE_ERROR_INVALID_ARGUMENT;
    }
    if (!promote->scene.texture || !promote->composed.texture) {
        return RSF_PROMOTE_OK;
    }
    D3D11_TEXTURE2D_DESC scene{};
    D3D11_TEXTURE2D_DESC composed{};
    promote->scene.texture->GetDesc(&scene);
    promote->composed.texture->GetDesc(&composed);
    if (scene.Format == composed.Format) {
        static_cast<ID3D11DeviceContext*>(context)->CopyResource(promote->scene.texture,
                                                                 promote->composed.texture);
        return RSF_PROMOTE_OK;
    }
    if (!promote->seed_pass || !promote->scene.target_view || !promote->composed.shader_view) {
        return RSF_PROMOTE_OK;
    }
    rsf_fullscreen_draw draw{};
    draw.struct_size = sizeof(draw);
    draw.mode = RSF_FULLSCREEN_COPY;
    return rsf_fullscreen_pass_draw(promote->seed_pass, context, promote->scene.target_view,
                                    promote->composed.shader_view, &draw) == RSF_FULLSCREEN_OK
               ? RSF_PROMOTE_OK
               : RSF_PROMOTE_ERROR_RESOURCE_FAILED;
}

extern "C" rsf_promote_result rsf_promote_get_stand_ins(rsf_promote* promote, void** scene,
                                                       void** composed)
{
    if (!promote || !scene || !composed) {
        return RSF_PROMOTE_ERROR_INVALID_ARGUMENT;
    }
    *scene = promote->ready ? promote->scene.texture : nullptr;
    *composed = promote->ready ? promote->composed.texture : nullptr;
    return RSF_PROMOTE_OK;
}

extern "C" rsf_promote_result rsf_promote_get_status(rsf_promote* promote,
                                                     rsf_promote_status* status)
{
    if (!promote || !status || status->struct_size < sizeof(rsf_promote_status)) {
        return RSF_PROMOTE_ERROR_INVALID_ARGUMENT;
    }
    status->ready = promote->ready ? 1u : 0u;
    status->ui_targets_promoted = promote->ui_target_count;
    status->chain_targets_promoted = promote->chain_target_count;
    status->at_recombine = (promote->composed.original && promote->scene.original) ? 1u : 0u;
    status->render_width = promote->render_width;
    status->render_height = promote->render_height;
    status->output_width = promote->output_width;
    status->output_height = promote->output_height;
    return RSF_PROMOTE_OK;
}

extern "C" void rsf_promote_destroy(rsf_promote* promote)
{
    if (!promote) {
        return;
    }
    // The caller clears the tap's plan before this, which the header says. Nothing here can check
    // it: the views are the tap's to stop using, and releasing them while a plan still names them
    // would take the game down inside a binding call rather than here.
    release_everything(promote);
    rsf_fullscreen_pass_destroy(promote->seed_pass);
    if (promote->device) {
        promote->device->Release();
    }
    delete promote;
}
