/* SPDX-License-Identifier: MIT */
/* Receipt formatting and transfer of the Rust input lease into the native World. */
#define COBJMACROS
#include "render_inputs.h"
#include "render_inputs_platform.h"
#include "native_guard.h"
#include <stdio.h>
#include <string.h>

/* Each nonnull field of a reader lease holds one owned reference from Rust. An accepted read
   transfers them to the result; a refusal releases them here. */
static void release_com(void* object)
{
    if (object) IUnknown_Release((IUnknown*)object);
}

static void release_references(NativeInputReferences* refs)
{
    for (uint32_t slot = 0; slot < native_input_slot_count; ++slot) release_com(refs->textures[slot]);
    release_com(refs->chain);
    release_com(refs->context);
    memset(refs, 0, sizeof(*refs));
}

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeInputReferences) == 8 * sizeof(void*) &&
    offsetof(NativeInputReferences, textures) == 16);

void rsf_fallout4_native_inputs_release(NativeInputs* inputs)
{
    release_com(inputs->context);
    release_com(inputs->chain);
    release_com(inputs->colour);
    release_com(inputs->depth);
    release_com(inputs->motion);
    release_com(inputs->refraction_normals);
    release_com(inputs->pipboy);
    release_com(inputs->vats);
    memset(inputs, 0, sizeof(*inputs));
}

void rsf_fallout4_native_inputs_move(NativeInputs* target, NativeInputs* source)
{
    if (target == source) return;
    rsf_fallout4_native_inputs_release(target);
    *target = *source;
    memset(source, 0, sizeof(*source));
}

void* rsf_fallout4_input_object(const void* opaque, uint32_t slot)
{
    const NativeInputs* inputs = (const NativeInputs*)opaque;
    switch (slot) {
    case native_input_context: return inputs->context;
    case native_input_chain: return inputs->chain;
    case native_input_colour: return inputs->colour;
    case native_input_motion: return inputs->motion;
    case native_input_depth: return inputs->depth;
    case native_input_refraction_normals: return inputs->refraction_normals;
    case native_input_pipboy: return inputs->pipboy;
    case native_input_vats: return inputs->vats;
    default: return NULL;
    }
}

/* The text the former reader receipt wrote; Rust decides the stage and its once-per-stage mask. */
void rsf_fallout4_inputs_report_reader(NativeInputReport report, const char* stage, const NativeInputReceipt* receipt,
    const rsf_game_render_pass* pass)
{
    if (!stage || !receipt || !pass) return;
    const NativeInputReceipt* r = receipt;
    const NativeInputState* before = &r->before;
    const NativeCamera* native = &r->native;
    char text[1600] = {0};
    snprintf(text, sizeof(text),
        "Fallout4 reader stage=%s native=%u partial=1 renderer=%p expected=%p device=%p context=%p chain=%p window=%p output=%ux%u camera=%p cache=%p count=%u scanned=%u camera_matches=%u matching_jitter=%u selected=%p last_key=%p last_jitter=%u taa=%u disable=%u dt=%g metadata_viewport=%g,%g,%g,%g default=%u dynamic=%u rs_count=%u rs=%g,%g,%g,%g,%g,%g confirmed_rs=%u/%g,%g ratio=%g,%g maps=%u,%u,%u swap=%ux%u/%p rtv=%u/%u/%u",
        stage, before->frame, (void*)before->renderer, (void*)(uintptr_t)r->expected_renderer,
        (void*)before->device, (void*)before->context, (void*)before->chain, (void*)before->window,
        before->width, before->height, (void*)before->camera, (void*)before->cache, before->count, r->scanned,
        r->camera_matches, r->matching_jitter, (void*)(uintptr_t)r->selected, (void*)(uintptr_t)r->last_key,
        r->last_jitter, before->taa, before->disable, (double)before->delta_seconds,
        (double)before->viewport[0], (double)before->viewport[1], (double)before->viewport[2],
        (double)before->viewport[3], (unsigned)before->default_viewport, (unsigned)before->dynamic_viewport,
        r->viewport_count, (double)r->viewport[0], (double)r->viewport[1], (double)r->viewport[2],
        (double)r->viewport[3], (double)r->viewport[4], (double)r->viewport[5],
        r->confirmed_viewport_count, (double)r->confirmed_viewport[2], (double)r->confirmed_viewport[3],
        (double)before->ratio[0], (double)before->ratio[1],
        before->colour, before->motion, before->depth, r->swap.width, r->swap.height,
        (void*)(uintptr_t)r->swap.window, r->rtv.view_format, r->rtv.view_dimension, r->rtv.view_mip);
    rsf_fallout4_report_guarded(report, text);
    static const char* const names[] = {"colour", "motion", "depth"};
    for (size_t i = 0; i < 3; ++i) {
        if (!r->views[i].read) continue;
        const NativeInputResourceFacts* view = &r->views[i];
        snprintf(text, sizeof(text),
            "Fallout4 reader stage=%s %s expected=%p actual=%p resource_match=%u device_match=%u texture=%ux%u format=%u mips=%u array=%u sample=%u/%u bind=0x%x srv_format=%u srv_dimension=%u srv_mip=%u srv_mips=%u",
            stage, names[i], (void*)(uintptr_t)r->expected[i], (void*)(uintptr_t)view->pointer,
            r->resource_match[i], r->device_match[i], view->width, view->height, view->format, view->mip_levels,
            view->array_size, view->sample_count, view->sample_quality, view->bind_flags, view->view_format,
            view->view_dimension, view->view_mip, view->view_mips);
        rsf_fallout4_report_guarded(report, text);
    }
    if (r->camera_read) {
        snprintf(text, sizeof(text),
            "Fallout4 reader stage=%s projection=%g,%g,%g,%g,%g,%g origin=%g,%g,%g previous_origin=%g,%g,%g global_jitter=%g,%g confirmed_native=%u camera_valid=%u render=%ux%u near=%g far=%g jitter=%g,%g",
            stage, (double)native->projection[0], (double)native->projection[5], (double)native->projection[10],
            (double)native->projection[11], (double)native->projection[14], (double)native->projection[15],
            (double)native->position_adjust[0], (double)native->position_adjust[1], (double)native->position_adjust[2],
            (double)native->previous_position_adjust[0], (double)native->previous_position_adjust[1],
            (double)native->previous_position_adjust[2], (double)native->jitter_ndc[0], (double)native->jitter_ndc[1],
            r->after_frame, pass->camera_valid, pass->camera.render_width, pass->camera.render_height,
            (double)pass->camera.near_plane, (double)pass->camera.far_plane,
            (double)pass->camera.jitter_pixels[0], (double)pass->camera.jitter_pixels[1]);
        rsf_fallout4_report_guarded(report, text);
    }
}

/* The text of the former state reread mismatch; Rust keeps the per-mask and sixteen-text budget. */
void rsf_fallout4_inputs_report_mismatch(NativeInputReport report, const NativeInputState* first,
    const NativeInputState* second, uint32_t mask)
{
    if (!first || !second) return;
    const NativeInputState* before = first;
    const NativeInputState* after = second;
    char text[750] = {0};
    snprintf(text, sizeof(text),
        "Fallout4 state reread mismatch native=%u/%u mask=0x%x dt=%.9g/%.9g cache_count=%u/%u taa=%u/%u disable=%u/%u default=%u/%u dynamic=%u/%u ratio=%.9g,%.9g/%.9g,%.9g viewport=%.9g,%.9g,%.9g,%.9g/%.9g,%.9g,%.9g,%.9g maps=%u,%u,%u/%u,%u,%u",
        before->frame, after->frame, mask, (double)before->delta_seconds, (double)after->delta_seconds,
        before->count, after->count, before->taa, after->taa, before->disable, after->disable,
        (unsigned)before->default_viewport, (unsigned)after->default_viewport,
        (unsigned)before->dynamic_viewport, (unsigned)after->dynamic_viewport, (double)before->ratio[0],
        (double)before->ratio[1], (double)after->ratio[0], (double)after->ratio[1], (double)before->viewport[0],
        (double)before->viewport[1], (double)before->viewport[2], (double)before->viewport[3],
        (double)after->viewport[0], (double)after->viewport[1], (double)after->viewport[2],
        (double)after->viewport[3], before->colour, before->motion, before->depth, after->colour, after->motion,
        after->depth);
    rsf_fallout4_report_guarded(report, text);
}

/* The text of the former camera history receipt; Rust keeps the per-reason and 32-text budget. */
void rsf_fallout4_inputs_report_history(NativeInputReport report, const NativeInputHistoryReceipt* receipt)
{
    if (!receipt) return;
    const NativeInputHistoryReceipt* h = receipt;
    char text[350] = {0};
    snprintf(text, sizeof(text),
        "Fallout4 camera history native=%u previous_native=%u reset=%u reasons=0x%x render=%ux%u previous_render=%ux%u raster=%g,%g,%g,%g previous_raster=%g,%g,%g,%g",
        h->frame, h->previous_frame, h->reset, h->reasons, h->width, h->height, h->previous_width,
        h->previous_height, (double)h->raster.xywh[0], (double)h->raster.xywh[1], (double)h->raster.xywh[2],
        (double)h->raster.xywh[3], (double)h->previous_raster.xywh[0], (double)h->previous_raster.xywh[1],
        (double)h->previous_raster.xywh[2], (double)h->previous_raster.xywh[3]);
    rsf_fallout4_report_guarded(report, text);
}

/* The text of the former framebuffer receipt; Rust keeps its once-per-stage mask. */
void rsf_fallout4_inputs_report_framebuffer(NativeInputReport report, const char* stage, const char* owner_stage,
    const NativeFramebufferReceipt* receipt)
{
    if (!stage || !owner_stage || !receipt) return;
    const NativeFramebufferReceipt* r = receipt;
    const NativeInputState* before = &r->before;
    const NativeInputState* after = &r->after;
    char text[960] = {0};
    snprintf(text, sizeof(text),
        "Fallout4 framebuffer stage=%s partial=1 native=%u owner_stage=%s renderer=%p context=%p chain=%p window=%p output=%ux%u logical=0 platform=%u/%u native_texture=%p rtv=%p leased_texture=%p texture=%ux%u mip=%u array=%u samples=%u format=%u rtv_kind=%u rtv_mip=%u swap=%ux%u/%p after=%u/%p/%p/%p",
        stage, before->frame, owner_stage, (void*)before->renderer, (void*)before->context, (void*)before->chain,
        (void*)before->window, before->width, before->height, r->platform, r->confirmed_platform,
        (void*)(uintptr_t)r->target_texture, (void*)(uintptr_t)r->target_rtv, (void*)(uintptr_t)r->leased,
        r->width, r->height, r->mip_levels, r->array_size, r->sample_count, r->format,
        r->rtv_dimension, r->rtv_mip, r->swap.width, r->swap.height,
        (void*)(uintptr_t)r->swap.window, after->frame, (void*)after->context,
        (void*)after->chain, (void*)after->window);
    rsf_fallout4_report_guarded(report, text);
}

/* Move the Rust lease into the result: each field's one owned reference becomes the result's. */
static void take(NativeInputs* result, NativeInputReferences* refs)
{
    result->context = (ID3D11DeviceContext*)refs->context;
    result->chain = (IDXGISwapChain*)refs->chain;
    result->colour = (ID3D11Texture2D*)refs->textures[native_input_colour];
    result->motion = (ID3D11Texture2D*)refs->textures[native_input_motion];
    result->depth = (ID3D11Texture2D*)refs->textures[native_input_depth];
    result->refraction_normals = (ID3D11Texture2D*)refs->textures[native_input_refraction_normals];
    result->pipboy = (ID3D11Texture2D*)refs->textures[native_input_pipboy];
    result->vats = (ID3D11Texture2D*)refs->textures[native_input_vats];
    memset(refs, 0, sizeof(*refs));
}

bool rsf_fallout4_read_native_inputs(NativeInputs* output, void (*report)(const char*),
    const NativeDepthSnapshot* producer)
{
    rsf_fallout4_native_inputs_release(output);
    NativeInputReferences refs;
    memset(&refs, 0, sizeof(refs));
    NativeInputs result;
    memset(&result, 0, sizeof(result));
    bool accepted = false;
    bool threw = false;
    RSF_FOREIGN_TRY { accepted = rsf_fallout4_inputs_read(&refs, report, producer, &result.pass); }
    RSF_FOREIGN_CATCH { threw = true; }
    if (!accepted) {
        release_references(&refs);
        if (threw && report && rsf_fallout4_inputs_claim_exception())
            rsf_fallout4_report_guarded(report, "Fallout4 reader stage=exception");
        return false;
    }
    take(&result, &refs);
    *output = result;
    return true;
}

bool rsf_fallout4_read_native_raster_inputs(NativeInputs* output, const rsf_game_render_pass* frame,
    const NativeDepthSnapshot* producer)
{
    rsf_fallout4_native_inputs_release(output);
    NativeInputReferences refs;
    memset(&refs, 0, sizeof(refs));
    NativeInputs result;
    memset(&result, 0, sizeof(result));
    bool accepted = false;
    RSF_FOREIGN_TRY { accepted = rsf_fallout4_inputs_read_raster(&refs, frame, producer, &result.pass); }
    RSF_FOREIGN_CATCH { accepted = false; }
    if (!accepted) {
        release_references(&refs);
        return false;
    }
    take(&result, &refs);
    *output = result;
    return true;
}
