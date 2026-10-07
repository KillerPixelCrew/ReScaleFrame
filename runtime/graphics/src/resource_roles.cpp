// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/resource_roles.h>

#include <windows.h>

#include <d3d11.h>

namespace {

// Capture-derived heuristics: docs/research/ac7-frame-capture.md records observed descriptors.
// Match confidence describes these rules, not verified resource identity in an arbitrary game.

// The depth formats a D3D11 depth buffer is created with, typeless and fully typed alike. AC7 uses
// R32G8X24_TYPELESS for scene depth; the rest are here because a depth target created any of these
// ways is still the depth target, and refusing one would silently drop the role.
bool is_depth_format(uint32_t format)
{
    switch (format) {
    case static_cast<uint32_t>(DXGI_FORMAT_R32G8X24_TYPELESS):
    case static_cast<uint32_t>(DXGI_FORMAT_D32_FLOAT_S8X24_UINT):
    case static_cast<uint32_t>(DXGI_FORMAT_R32_TYPELESS):
    case static_cast<uint32_t>(DXGI_FORMAT_D32_FLOAT):
    case static_cast<uint32_t>(DXGI_FORMAT_R24G8_TYPELESS):
    case static_cast<uint32_t>(DXGI_FORMAT_D24_UNORM_S8_UINT):
    case static_cast<uint32_t>(DXGI_FORMAT_R24_UNORM_X8_TYPELESS):
        return true;
    default:
        return false;
    }
}

bool has_flags(const rsf_texture_facts& facts, uint32_t wanted)
{
    return (facts.bind_flags & wanted) == wanted;
}

// Reject MSAA, arrays and mip chains. sample_count/array_size zero are accepted for manually
// filled facts; mip_levels zero means a full chain in D3D11 and is therefore rejected.
bool single_plain_surface(const rsf_texture_facts& facts)
{
    return facts.sample_count <= 1 && facts.mip_levels == 1 && facts.array_size <= 1;
}

// With a known render extent require an exact match. Otherwise use the chosen half-to-full
// output band and aspect tolerance to exclude square shadow maps of otherwise matching size.
bool at_render_resolution(const rsf_texture_facts& facts, const rsf_frame_shape& shape)
{
    if (shape.render_width != 0 && shape.render_height != 0) {
        return facts.width == shape.render_width && facts.height == shape.render_height;
    }
    if (shape.output_width == 0 || shape.output_height == 0) {
        // Nothing to judge against. Refusing here is the honest answer: a shadow cascade and the
        // depth buffer differ only by size, so without a frame reference there is no verdict.
        return false;
    }
    if (facts.width > shape.output_width || facts.height > shape.output_height ||
        facts.width < shape.output_width / 2 || facts.height < shape.output_height / 2) {
        return false;
    }

    // Cross multiplication avoids floating rounding; 1/64 slack permits rounded scaled extents.
    // The tolerance is a policy choice, not a measured guarantee of resource identity.
    const uint64_t width_cross = uint64_t(facts.width) * shape.output_height;
    const uint64_t height_cross = uint64_t(facts.height) * shape.output_width;
    const uint64_t difference =
        width_cross > height_cross ? width_cross - height_cross : height_cross - width_cross;
    return difference * 64u <= width_cross + height_cross;
}

} // namespace

extern "C" uint32_t rsf_classify_texture(const rsf_texture_facts* facts,
                                         const rsf_frame_shape* shape, rsf_role_verdict* verdict)
{
    if (!facts || !shape || !verdict || facts->struct_size < sizeof(rsf_texture_facts) ||
        shape->struct_size < sizeof(rsf_frame_shape) ||
        verdict->struct_size < sizeof(rsf_role_verdict)) {
        return 0;
    }

    // Valid unrecognized descriptors return UNKNOWN. Only a scene-colour match is CANDIDATE.
    verdict->role = RSF_ROLE_UNKNOWN;
    verdict->confidence = RSF_ROLE_CONFIDENT;

    if (!single_plain_surface(*facts)) {
        return 1;
    }

    // Eye adaptation writes one draw into a 1x1 R32G32_FLOAT target, chunk 4448 of the timeline.
    // A 1x1 target of that format is not something the rest of the frame allocates, and its size
    // does not follow render resolution, so it is tested on its own terms.
    if (facts->width == 1 && facts->height == 1 &&
        facts->format == static_cast<uint32_t>(DXGI_FORMAT_R32G32_FLOAT)) {
        verdict->role = RSF_ROLE_EXPOSURE;
        return 1;
    }

    const bool render_sized = at_render_resolution(*facts, *shape);

    // Velocity: resource 2163, R16G16_UNORM, rendered into as a render target, present in every
    // capture. The render target flag is what separates it from resource 63083, which shares the
    // format and sits at half resolution but is written through an unordered access view and turned
    // out to be a mask. Size alone cannot tell those apart once render scale is halved.
    if (render_sized && facts->format == static_cast<uint32_t>(DXGI_FORMAT_R16G16_UNORM) &&
        has_flags(*facts, D3D11_BIND_RENDER_TARGET)) {
        verdict->role = RSF_ROLE_MOTION;
        return 1;
    }

    // Depth: a depth format bound as a depth stencil, at render resolution. The size test is doing
    // real work here, since the shadow cascades at 5120x1024, 2048x2048 and 1024x1024 are the same
    // formats bound the same way. The first two are out on size, the square one only on aspect.
    if (render_sized && is_depth_format(facts->format) &&
        has_flags(*facts, D3D11_BIND_DEPTH_STENCIL)) {
        verdict->role = RSF_ROLE_DEPTH;
        return 1;
    }

    // RGBA16F scene candidates share descriptors with other engine targets. Binding/frame/view
    // evidence must settle which candidate is the reconstruction input.
    if (render_sized && facts->format == static_cast<uint32_t>(DXGI_FORMAT_R16G16B16A16_FLOAT) &&
        has_flags(*facts, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE)) {
        verdict->role = RSF_ROLE_SCENE_COLOR;
        verdict->confidence = RSF_ROLE_CANDIDATE;
        return 1;
    }

    return 1;
}
