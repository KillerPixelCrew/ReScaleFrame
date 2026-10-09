// SPDX-License-Identifier: GPL-3.0-only
//
// Classify descriptors that were actually seen in an AC7 frame, and check the verdicts.
//
// No device and no GPU: classification reads a descriptor and nothing else, so the facts are built
// by hand here. The numbers come from docs/research/ac7-frame-capture.md, which read them off a
// replayed capture of the running game, and each case says which resource it stands for. The point
// of writing them out rather than referring to the capture generally is that a rule changed on a
// hunch then disagrees with a recorded observation instead of quietly moving with the code.
//
// The classifier knows no game, so the AC7 rules are a table built here. It mirrors the table the
// AC7 loader hands the frame tap; the cases below are what that table has to keep answering.

#include <rescaleframe/resource_roles.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdint>
#include <cstdio>

namespace {

bool passed = true;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "%s\n", message);
        passed = false;
    }
}

// The frame as captured: 2048x1152 presented, and the same size rendered until a render scale is
// applied.
constexpr uint32_t output_width = 2048;
constexpr uint32_t output_height = 1152;

rsf_texture_facts make_facts(uint32_t width, uint32_t height, uint32_t format, uint32_t bind_flags)
{
    rsf_texture_facts facts{};
    facts.struct_size = sizeof(facts);
    facts.width = width;
    facts.height = height;
    facts.format = format;
    facts.bind_flags = bind_flags;
    facts.mip_levels = 1;
    facts.array_size = 1;
    facts.sample_count = 1;
    return facts;
}

// The AC7 role table, in priority order.
const rsf_role_rule ac7_rules[] = {
    // Chunk 4448: the eye adaptation value, a 1x1 target whatever the frame's size.
    {RSF_ROLE_EXPOSURE, RSF_ROLE_CONFIDENT, RSF_ROLE_RULE_FIXED_SIZE, DXGI_FORMAT_R32G32_FLOAT, 0, 1,
     1},
    // Resource 2163. The render target flag separates it from the mask at 63083.
    {RSF_ROLE_MOTION, RSF_ROLE_CONFIDENT, RSF_ROLE_RULE_RENDER_SIZED, DXGI_FORMAT_R16G16_UNORM,
     D3D11_BIND_RENDER_TARGET, 0, 0},
    {RSF_ROLE_DEPTH, RSF_ROLE_CONFIDENT, RSF_ROLE_RULE_RENDER_SIZED | RSF_ROLE_RULE_ANY_DEPTH_FORMAT,
     0, D3D11_BIND_DEPTH_STENCIL, 0, 0},
    // Many render targets share these shapes, so both are candidates and the bound set decides.
    {RSF_ROLE_HISTORY, RSF_ROLE_CANDIDATE, RSF_ROLE_RULE_RENDER_SIZED,
     DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE, 0, 0},
    {RSF_ROLE_SCENE_COLOR, RSF_ROLE_CANDIDATE, RSF_ROLE_RULE_RENDER_SIZED,
     DXGI_FORMAT_R11G11B10_FLOAT, D3D11_BIND_RENDER_TARGET, 0, 0},
};
const rsf_role_table ac7_table = {sizeof(rsf_role_table), ac7_rules,
                                  uint32_t(sizeof(ac7_rules) / sizeof(ac7_rules[0]))};

rsf_frame_shape make_shape(uint32_t render_width, uint32_t render_height)
{
    rsf_frame_shape shape{};
    shape.struct_size = sizeof(shape);
    shape.output_width = output_width;
    shape.output_height = output_height;
    shape.render_width = render_width;
    shape.render_height = render_height;
    return shape;
}

void check_verdict(const rsf_texture_facts& facts, const rsf_frame_shape& shape,
                   rsf_resource_role role, rsf_role_confidence confidence, const char* message)
{
    rsf_role_verdict verdict{};
    verdict.struct_size = sizeof(verdict);
    const uint32_t classified = rsf_classify_texture(&ac7_table, &facts, &shape, &verdict);
    if (classified == 0) {
        std::fprintf(stderr, "%s (the call refused its arguments)\n", message);
        passed = false;
        return;
    }
    if (verdict.role != role || verdict.confidence != confidence) {
        std::fprintf(stderr, "%s (got role %u confidence %u, expected role %u confidence %u)\n",
                     message, unsigned(verdict.role), unsigned(verdict.confidence), unsigned(role),
                     unsigned(confidence));
        passed = false;
    }
}

void check_unknown(const rsf_texture_facts& facts, const rsf_frame_shape& shape,
                   const char* message)
{
    check_verdict(facts, shape, RSF_ROLE_UNKNOWN, RSF_ROLE_CONFIDENT, message);
}

} // namespace

int main()
{
    const rsf_frame_shape full = make_shape(output_width, output_height);
    const rsf_frame_shape halved = make_shape(output_width / 2, output_height / 2);
    const rsf_frame_shape unknown_render = make_shape(0, 0);

    // Resource 2163: R16G16_UNORM, full resolution, SRV + RTV, present in all 22 captures. Nothing
    // else in the frame shares that shape, so the verdict is confident.
    const rsf_texture_facts velocity =
        make_facts(output_width, output_height, DXGI_FORMAT_R16G16_UNORM,
                   D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    check_verdict(velocity, full, RSF_ROLE_MOTION, RSF_ROLE_CONFIDENT,
                  "The velocity target must be recognised.");

    // Resource 63083: same format, half resolution, SRV + UAV. The capture replay showed it is a
    // mask rather than motion, and the render target flag is the only thing in the descriptor that
    // separates the two once render scale is halved. This is the case format alone gets wrong.
    const rsf_texture_facts mask =
        make_facts(output_width / 2, output_height / 2, DXGI_FORMAT_R16G16_UNORM,
                   D3D11_BIND_UNORDERED_ACCESS | D3D11_BIND_SHADER_RESOURCE);
    check_unknown(mask, halved, "A compute written R16G16_UNORM is not the velocity target.");
    check_unknown(mask, unknown_render,
                  "A compute written R16G16_UNORM stays refused when the render size is unknown.");

    // Depth: resource 2052 is R32G8X24_TYPELESS at full resolution. The other depth formats are
    // accepted because a game creating its depth buffer any of these ways still has a depth buffer.
    const uint32_t depth_formats[] = {
        DXGI_FORMAT_R32G8X24_TYPELESS, DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R32_TYPELESS,
        DXGI_FORMAT_D32_FLOAT,         DXGI_FORMAT_R24G8_TYPELESS,       DXGI_FORMAT_D24_UNORM_S8_UINT,
        DXGI_FORMAT_R24_UNORM_X8_TYPELESS};
    for (const uint32_t format : depth_formats) {
        const rsf_texture_facts depth =
            make_facts(output_width, output_height, format,
                       D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE);
        check_verdict(depth, full, RSF_ROLE_DEPTH, RSF_ROLE_CONFIDENT,
                      "A depth stencil at render resolution must be recognised.");
    }

    // A depth format without the depth stencil binding is something else wearing the format.
    const rsf_texture_facts depth_srv_only = make_facts(
        output_width, output_height, DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_SHADER_RESOURCE);
    check_unknown(depth_srv_only, full, "A depth format alone is not the depth buffer.");

    // Shadow cascades in flight: chunks 2278 to 2413 render 1024x1024 and 2048x2048, and shadow
    // depth also appears at 5120x1024. All carry the same format and the same binding as scene
    // depth, so only the size and the aspect tell them apart.
    const rsf_texture_facts cascade =
        make_facts(2048, 2048, DXGI_FORMAT_R32G8X24_TYPELESS, D3D11_BIND_DEPTH_STENCIL);
    check_unknown(cascade, full, "A shadow cascade must not be taken for scene depth.");
    check_unknown(cascade, unknown_render,
                  "A shadow cascade stays refused when the render size is unknown.");
    const rsf_texture_facts wide_cascade =
        make_facts(5120, 1024, DXGI_FORMAT_R32G8X24_TYPELESS, D3D11_BIND_DEPTH_STENCIL);
    check_unknown(wide_cascade, unknown_render, "The 5120x1024 shadow cascade is not scene depth.");

    // The square 1024 cascade is the one that size alone gets wrong. Against a 2048x1152 frame it
    // is inside the accepted band on both axes, so without the aspect test it comes back as scene
    // depth with full confidence, and a backend would be handed a shadow map.
    const rsf_texture_facts square_cascade =
        make_facts(1024, 1024, DXGI_FORMAT_R32G8X24_TYPELESS, D3D11_BIND_DEPTH_STENCIL);
    check_unknown(square_cascade, unknown_render,
                  "A 1024x1024 shadow cascade sits inside the size band and must be refused on "
                  "aspect.");
    check_unknown(square_cascade, halved,
                  "A 1024x1024 shadow cascade is not the depth buffer at half render scale.");

    // A render size rounded to a multiple of eight still has to read as render sized, or the aspect
    // test would refuse the very thing it exists to admit. 70 percent of 2048x1152 rounded up that
    // way is 1440x808, whose aspect is a little off 16:9.
    const rsf_texture_facts rounded_depth =
        make_facts(1440, 808, DXGI_FORMAT_R32G8X24_TYPELESS, D3D11_BIND_DEPTH_STENCIL);
    check_verdict(rounded_depth, unknown_render, RSF_ROLE_DEPTH, RSF_ROLE_CONFIDENT,
                  "A render size rounded to a multiple of eight must still be render sized.");

    // Chunk 4448: one draw into a 1x1 R32G32_FLOAT, the eye adaptation value temporal AA reads.
    const rsf_texture_facts exposure = make_facts(
        1, 1, DXGI_FORMAT_R32G32_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    check_verdict(exposure, full, RSF_ROLE_EXPOSURE, RSF_ROLE_CONFIDENT,
                  "The 1x1 exposure target must be recognised.");

    // The honest case. #1732, #2168 and #2172 are all full resolution R16G16B16A16_FLOAT render
    // targets and only their bindings tell them apart, so two textures with identical descriptors
    // both have to come back as candidates rather than one being picked.
    const rsf_texture_facts scene_colour =
        make_facts(output_width, output_height, DXGI_FORMAT_R16G16B16A16_FLOAT,
                   D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    check_verdict(scene_colour, full, RSF_ROLE_HISTORY, RSF_ROLE_CANDIDATE,
                  "A shape many targets share must be reported as a candidate, never as confident.");
    const rsf_texture_facts also_scene_colour = scene_colour;
    check_verdict(also_scene_colour, full, RSF_ROLE_HISTORY, RSF_ROLE_CANDIDATE,
                  "A second texture of the same shape must be a candidate too.");

    // Without the shader resource binding it cannot be the input a reconstruction pass samples.
    const rsf_texture_facts write_only =
        make_facts(output_width, output_height, DXGI_FORMAT_R16G16B16A16_FLOAT,
                   D3D11_BIND_RENDER_TARGET);
    check_unknown(write_only, full, "A scene colour candidate has to be sampleable.");

    // The frame is otherwise full of seven B8G8R8A8_TYPELESS targets at full resolution. They are
    // not a role, and reporting one would be worse than reporting none.
    const rsf_texture_facts colour_buffer =
        make_facts(output_width, output_height, DXGI_FORMAT_B8G8R8A8_TYPELESS,
                   D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    check_unknown(colour_buffer, full, "An ordinary colour target has no role.");

    // R11G11B10_FLOAT is the colour the post chain actually reads in this frame. It used to be
    // chosen by the tap's own list, which had drifted from this classifier; it is a rule now, and
    // like the other colour shapes it stays a candidate.
    const rsf_texture_facts frame_colour =
        make_facts(output_width, output_height, DXGI_FORMAT_R11G11B10_FLOAT,
                   D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    check_verdict(frame_colour, full, RSF_ROLE_SCENE_COLOR, RSF_ROLE_CANDIDATE,
                  "The R11G11B10 frame colour must be a scene colour candidate.");
    const rsf_texture_facts frame_colour_unbindable =
        make_facts(output_width, output_height, DXGI_FORMAT_R11G11B10_FLOAT,
                   D3D11_BIND_SHADER_RESOURCE);
    check_unknown(frame_colour_unbindable, full, "Scene colour has to be a render target.");
    rsf_texture_facts frame_colour_mipped = frame_colour;
    frame_colour_mipped.mip_levels = 4;
    check_unknown(frame_colour_mipped, full, "A mip chained colour target is not scene colour.");

    // At 50 screen percentage the scene pipeline genuinely moves to 1024x576, so the same
    // descriptors have to be recognised against a halved render size and refused against a full
    // one. This is the case that would break if sizes were compared against the presented size.
    const rsf_texture_facts half_velocity =
        make_facts(output_width / 2, output_height / 2, DXGI_FORMAT_R16G16_UNORM,
                   D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    check_verdict(half_velocity, halved, RSF_ROLE_MOTION, RSF_ROLE_CONFIDENT,
                  "Velocity at half resolution must be recognised at half render scale.");
    check_unknown(half_velocity, full,
                  "A half resolution target is not the velocity target at full render scale.");

    // With no render size established yet, half the output up to the output is the accepted band,
    // because halving the render scale is what this project does on purpose.
    check_verdict(half_velocity, unknown_render, RSF_ROLE_MOTION, RSF_ROLE_CONFIDENT,
                  "A half resolution render target must be accepted when render size is unknown.");
    check_verdict(velocity, unknown_render, RSF_ROLE_MOTION, RSF_ROLE_CONFIDENT,
                  "A full resolution render target must be accepted when render size is unknown.");
    const rsf_texture_facts quarter_velocity =
        make_facts(output_width / 4, output_height / 4, DXGI_FORMAT_R16G16_UNORM,
                   D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    check_unknown(quarter_velocity, unknown_render,
                  "Below half the output size is outside the accepted band.");

    // With no frame reference at all there is nothing to judge a size against, so the size
    // dependent roles have to refuse rather than guess. Exposure does not depend on the frame.
    rsf_frame_shape nothing_known = make_shape(0, 0);
    nothing_known.output_width = 0;
    nothing_known.output_height = 0;
    check_unknown(velocity, nothing_known, "With no frame sizes at all there is no size verdict.");
    check_verdict(exposure, nothing_known, RSF_ROLE_EXPOSURE, RSF_ROLE_CONFIDENT,
                  "The 1x1 exposure target does not need a frame reference.");

    // None of the four targets is multisampled or mip chained. A mip level count of zero is a
    // request for the full chain, so it is a chain too.
    rsf_texture_facts multisampled = velocity;
    multisampled.sample_count = 4;
    check_unknown(multisampled, full, "A multisampled texture is none of these targets.");
    rsf_texture_facts mipped = scene_colour;
    mipped.mip_levels = 9;
    check_unknown(mipped, full, "A mip chained texture is none of these targets.");
    rsf_texture_facts full_chain = scene_colour;
    full_chain.mip_levels = 0;
    check_unknown(full_chain, full, "A zero mip count means a full chain and is refused too.");
    rsf_texture_facts array_slice = velocity;
    array_slice.array_size = 6;
    check_unknown(array_slice, full, "An array or cube texture is none of these targets.");

    // Short structs are rejected, and the verdict is left alone rather than half written.
    {
        rsf_texture_facts short_facts = velocity;
        short_facts.struct_size = sizeof(rsf_texture_facts) - 4;
        rsf_role_verdict verdict{};
        verdict.struct_size = sizeof(verdict);
        check(rsf_classify_texture(&ac7_table, &short_facts, &full, &verdict) == 0,
              "Short facts must be rejected.");
        check(verdict.role == RSF_ROLE_UNKNOWN, "A rejected call must leave the role unknown.");
    }
    {
        rsf_frame_shape short_shape = full;
        short_shape.struct_size = sizeof(rsf_frame_shape) - 4;
        rsf_role_verdict verdict{};
        verdict.struct_size = sizeof(verdict);
        check(rsf_classify_texture(&ac7_table, &velocity, &short_shape, &verdict) == 0,
              "A short frame shape must be rejected.");
        check(verdict.role == RSF_ROLE_UNKNOWN, "A rejected call must leave the role unknown.");
    }
    {
        rsf_role_verdict verdict{};
        verdict.struct_size = sizeof(rsf_role_verdict) - 4;
        verdict.role = RSF_ROLE_DEPTH;
        check(rsf_classify_texture(&ac7_table, &velocity, &full, &verdict) == 0,
              "A short verdict must be rejected.");
        check(verdict.role == RSF_ROLE_DEPTH, "A short verdict must not be written to.");
    }
    {
        rsf_role_verdict verdict{};
        verdict.struct_size = sizeof(verdict);
        check(rsf_classify_texture(&ac7_table, nullptr, &full, &verdict) == 0,
              "Null facts must be rejected.");
        check(rsf_classify_texture(&ac7_table, &velocity, nullptr, &verdict) == 0,
              "A null frame shape must be rejected.");
        check(rsf_classify_texture(&ac7_table, &velocity, &full, nullptr) == 0,
              "A null verdict must be rejected.");
        check(rsf_classify_texture(nullptr, &velocity, &full, &verdict) == 0,
              "A null table must be rejected.");
        rsf_role_table short_table = ac7_table;
        short_table.struct_size = sizeof(rsf_role_table) - 4;
        check(rsf_classify_texture(&short_table, &velocity, &full, &verdict) == 0,
              "A short table must be rejected.");
        rsf_role_table no_rules = ac7_table;
        no_rules.rules = nullptr;
        check(rsf_classify_texture(&no_rules, &velocity, &full, &verdict) == 0,
              "A table that counts rules and has none must be rejected.");
    }

    // A struct longer than this build knows about is a newer caller, not an error. Only the fields
    // named here are read, which is what appending to these structs is supposed to allow.
    {
        rsf_texture_facts extended = velocity;
        extended.struct_size = sizeof(rsf_texture_facts) + 8;
        check_verdict(extended, full, RSF_ROLE_MOTION, RSF_ROLE_CONFIDENT,
                      "A longer facts struct must still classify.");
    }

    // The table is data. An empty one classifies everything as unknown, the first matching rule
    // wins, and a rule with a fixed size ignores the frame.
    {
        const rsf_role_table empty = {sizeof(rsf_role_table), nullptr, 0};
        rsf_role_verdict verdict{};
        verdict.struct_size = sizeof(verdict);
        verdict.role = RSF_ROLE_DEPTH;
        check(rsf_classify_texture(&empty, &velocity, &full, &verdict) == 1 &&
                  verdict.role == RSF_ROLE_UNKNOWN && verdict.confidence == RSF_ROLE_CONFIDENT,
              "An empty table must classify everything as unknown.");

        const rsf_role_rule ordered[] = {
            {RSF_ROLE_MOTION, RSF_ROLE_CANDIDATE, RSF_ROLE_RULE_FIXED_SIZE,
             DXGI_FORMAT_R16G16_UNORM, 0, 640, 360},
            {RSF_ROLE_DEPTH, RSF_ROLE_CONFIDENT, 0, DXGI_FORMAT_R16G16_UNORM, 0, 0, 0},
        };
        const rsf_role_table table = {sizeof(rsf_role_table), ordered, 2};
        const rsf_texture_facts fixed = make_facts(640, 360, DXGI_FORMAT_R16G16_UNORM, 0);
        verdict = rsf_role_verdict{};
        verdict.struct_size = sizeof(verdict);
        check(rsf_classify_texture(&table, &fixed, &full, &verdict) == 1 &&
                  verdict.role == RSF_ROLE_MOTION && verdict.confidence == RSF_ROLE_CANDIDATE,
              "The first matching rule must decide, and a fixed size must ignore the frame.");
        const rsf_texture_facts other_size = make_facts(641, 360, DXGI_FORMAT_R16G16_UNORM, 0);
        verdict = rsf_role_verdict{};
        verdict.struct_size = sizeof(verdict);
        check(rsf_classify_texture(&table, &other_size, &full, &verdict) == 1 &&
                  verdict.role == RSF_ROLE_DEPTH,
              "A rule that misses on size must fall through to the next one.");
    }

    return passed ? 0 : 1;
}
