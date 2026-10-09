// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/resource_roles.h>

#include <windows.h>

#include <d3d11.h>

namespace {

// What each format and size means is the caller's table, not this file's. Only the shape tests that
// make a table safe to write live here: plain surfaces only, and sizes judged against the frame.

// The depth formats a D3D11 depth buffer is created with, typeless and fully typed alike. A depth
// target created any of these ways is still the depth target, and refusing one would silently drop
// the role.
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

// No role is a multisampled, mip chained or array texture. A texture that is any of those is refused
// before a rule is even looked at, which is what keeps a cube face or a mip chained scene texture of
// the right format and size out of the scene colour candidates.
//
// Zero is accepted for the counts because a caller filling the struct by hand leaves them zero more
// often than it means an empty resource, while a D3D11 descriptor never carries zero for a real
// single surface. Mip levels are the exception: zero there means "allocate the full chain", so it
// is a chain and is refused.
bool single_plain_surface(const rsf_texture_facts& facts)
{
    return facts.sample_count <= 1 && facts.mip_levels == 1 && facts.array_size <= 1;
}

// Render resolution, judged against the frame rather than against a fixed number, since the whole
// point of this project is to make render and output resolution differ.
//
// With no render size yet, the accepted band runs from half the output size up to it and both axes
// have to be scaled by about the same factor. Half is the bottom because this project drives
// r.ScreenPercentage down to 50 and the capture at that setting shows scene colour, depth and
// velocity all following the render size.
//
// The aspect test is not decoration. Chunks 2278 to 2413 render 1024x1024 shadow cascades, and a
// square 1024 sits inside the size band of a 2048x1152 frame on both axes, so size alone reports
// one as scene depth with full confidence. Only the aspect separates it from the 1024x576 the
// scene pipeline actually uses at half scale.
//
// Both the half floor and the aspect slack are chosen, not measured. The band still accepts a post
// process stage that happens to land in it at the frame's aspect, which is why the scene colour
// verdict below is a candidate.
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

    // Aspect compared by cross multiplication, so no float rounding enters a verdict. The 1/64
    // slack absorbs a render size rounded to a whole pixel or to a multiple of eight. It is a
    // chosen figure: wide enough for the roundings UE4 applies to a screen percentage, and far
    // narrower than the gap between 16:9 and the square cascade it exists to reject.
    const uint64_t width_cross = uint64_t(facts.width) * shape.output_height;
    const uint64_t height_cross = uint64_t(facts.height) * shape.output_width;
    const uint64_t difference =
        width_cross > height_cross ? width_cross - height_cross : height_cross - width_cross;
    return difference * 64u <= width_cross + height_cross;
}

bool rule_matches(const rsf_role_rule& rule, const rsf_texture_facts& facts,
                  const rsf_frame_shape& shape)
{
    if (!has_flags(facts, rule.bind_flags)) {
        return false;
    }
    if ((rule.flags & RSF_ROLE_RULE_ANY_DEPTH_FORMAT) != 0 ? !is_depth_format(facts.format)
                                                           : facts.format != rule.format) {
        return false;
    }
    if ((rule.flags & RSF_ROLE_RULE_FIXED_SIZE) != 0 &&
        (facts.width != rule.width || facts.height != rule.height)) {
        return false;
    }
    // Render resolution, judged last because it is the only test that can need the frame's shape.
    return (rule.flags & RSF_ROLE_RULE_RENDER_SIZED) == 0 || at_render_resolution(facts, shape);
}

} // namespace

extern "C" uint32_t rsf_classify_texture(const rsf_role_table* table, const rsf_texture_facts* facts,
                                         const rsf_frame_shape* shape, rsf_role_verdict* verdict)
{
    if (!table || !facts || !shape || !verdict || table->struct_size < sizeof(rsf_role_table) ||
        (table->count != 0 && !table->rules) || facts->struct_size < sizeof(rsf_texture_facts) ||
        shape->struct_size < sizeof(rsf_frame_shape) ||
        verdict->struct_size < sizeof(rsf_role_verdict)) {
        return 0;
    }

    // Unknown is a settled verdict about a descriptor rather than a hedge, so it carries the
    // confident confidence. Only a rule that says candidate ever reports otherwise.
    verdict->role = RSF_ROLE_UNKNOWN;
    verdict->confidence = RSF_ROLE_CONFIDENT;

    if (!single_plain_surface(*facts)) {
        return 1;
    }
    for (uint32_t index = 0; index < table->count; ++index) {
        if (rule_matches(table->rules[index], *facts, *shape)) {
            verdict->role = table->rules[index].role;
            verdict->confidence = table->rules[index].confidence;
            return 1;
        }
    }
    return 1;
}
