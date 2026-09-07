/* SPDX-License-Identifier: GPL-3.0-only */
/* Which draws in an AC7 frame are the interface.

   This is a game fact, so it lives here rather than in the runtime, and it is deliberately a pure
   function over facts the frame tap already shadows: no device calls, no allocation, no state of
   its own beyond the registries the caller fills at resource creation. The runtime performs the
   divert; this only says what a draw is.

   The rule exists because promotion could not work. AC7 rasterizes its interface at a hardcoded
   1920x1080 through `Nimbus.WidgetToTextureConverter` and, on the briefing and the hangar, draws it
   into the scene as world space widget quads with the scene's depth bound, into a render resolution
   layer of its own that the game composites before its upscale. There is no interface target whose
   promotion sharpens that. See docs/research/ac7-ui-composition.md for the frame that showed it and
   docs/research/ac7-ui-extraction.md for what is done about it.

   Every identification rule tried in this frame that had the form "the one that matches" was wrong
   at least once, because a running game leaves more bound than it reads. So the classifier compares
   against everything the shadow holds, and where a single fact would be a guess it says UNKNOWN and
   the run reports it rather than acting on it. */
#ifndef RSF_AC7_UI_RULES_H
#define RSF_AC7_UI_RULES_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* What a draw is. UNKNOWN is not a failure: it is the answer for a draw that reads something of
   interface shape but does not match any rule, and it is counted per screen so a classification
   gap is visible in the log instead of being diverted on a guess. */
typedef uint32_t rsf_ac7_draw_class;
#define RSF_AC7_DRAW_SCENE ((rsf_ac7_draw_class)0)
/* Slate or canvas geometry into the frame's own target: the interface drawn at native. */
#define RSF_AC7_DRAW_UI_SLATE ((rsf_ac7_draw_class)1)
/* A world space widget quad reading a converter target: the interface drawn as scene geometry. */
#define RSF_AC7_DRAW_UI_WIDGET_QUAD ((rsf_ac7_draw_class)2)
/* An interface draw whose blend cannot be represented in a premultiplied layer. Counted, never
   diverted: a modulate blend writes colour only, so a transparent layer has nothing to composite. */
#define RSF_AC7_DRAW_UI_MODULATE ((rsf_ac7_draw_class)3)
/* Slate rasterizing a widget into a converter's own render target. This is what builds the texture
   the quads read, so diverting it would empty the interface rather than move it. */
#define RSF_AC7_DRAW_WIDGET_RASTER ((rsf_ac7_draw_class)4)
/* Interface shaped and unexplained. The classification gap, reported per screen. */
#define RSF_AC7_DRAW_UNKNOWN ((rsf_ac7_draw_class)5)
/* Named by the override table as not interface, whatever the other rules say. */
#define RSF_AC7_DRAW_SKIP ((rsf_ac7_draw_class)6)

/* D3D11 blend factors, named here so this header does not include d3d11.h and stays testable
   without a device. The values are the D3D11_BLEND enumerators. */
#define RSF_AC7_BLEND_ZERO 1u
#define RSF_AC7_BLEND_ONE 2u
#define RSF_AC7_BLEND_SRC_COLOR 3u
#define RSF_AC7_BLEND_SRC_ALPHA 5u
#define RSF_AC7_BLEND_INV_SRC_ALPHA 6u

/* DXGI formats, named here for the same reason as the blend factors: this header stays free of
   d3d11.h so the rules can be tested without a device. */
#define RSF_AC7_FORMAT_R32G32B32A32_FLOAT 2u
#define RSF_AC7_FORMAT_R32G32_FLOAT 16u
#define RSF_AC7_FORMAT_R16G16_UINT 36u
#define RSF_AC7_FORMAT_B8G8R8A8_TYPELESS 90u
#define RSF_AC7_FORMAT_B8G8R8A8_UNORM 87u
#define RSF_AC7_FORMAT_B8G8R8A8_UNORM_SRGB 91u

#define RSF_AC7_UI_MAX_INPUTS 16u
#define RSF_AC7_UI_MAX_LAYOUT_ELEMENTS 16u

/* What produced a vertex declaration. Recognising this is what turns a pointer into a name, and it
   is the only part of identification that can be done at creation, before anything has drawn. */
typedef uint32_t rsf_ac7_layout_kind;
#define RSF_AC7_LAYOUT_OTHER ((rsf_ac7_layout_kind)0)
/* `FSlateVertexDeclaration`: the interface's own geometry. */
#define RSF_AC7_LAYOUT_SLATE ((rsf_ac7_layout_kind)1)
/* `FSlateInstancedVertexDeclaration`: the same five elements plus a per-instance transform. */
#define RSF_AC7_LAYOUT_SLATE_INSTANCED ((rsf_ac7_layout_kind)2)
/* `FSimpleElementVertexDeclaration`: the canvas, which is what `ANimbusHUD::DrawWidget*` and the
   engine's own debug drawing go through. */
#define RSF_AC7_LAYOUT_CANVAS ((rsf_ac7_layout_kind)3)

/* One `D3D11_INPUT_ELEMENT_DESC`, reduced to the fields that carry information.

   The semantic name is deliberately absent. Unreal's D3D11 RHI writes "ATTRIBUTE" for every element
   of every declaration in the engine (`D3D11VertexDeclaration.cpp:56`) and puts the element's index
   in the semantic index, so a name distinguishes nothing and only the index, format, slot and
   offset do. */
typedef struct rsf_ac7_layout_element {
    uint32_t semantic_index;
    uint32_t format;
    uint32_t input_slot;
    uint32_t byte_offset;
    uint32_t per_instance;
} rsf_ac7_layout_element;

/* Name a vertex declaration by its element signature.

   Verified against 4.18.3 at `0a14a8d537a3` rather than assumed from a later engine, which matters:
   UE5's `FSimpleElementVertex` carries a `FDFVector4` position and does not have this layout at all,
   so a fingerprint taken from a modern checkout would match nothing in this game.

     FSlateVertex, stride 40 (`RenderingCommon.h:140`, `SlateShaders.cpp:52-58, 73-82`)
       0  float TexCoords[4]        R32G32B32A32_FLOAT  slot 0
       16 FVector2D MaterialTexCoords R32G32_FLOAT      slot 0
       24 FVector2D Position        R32G32_FLOAT        slot 0
       32 FColor Color              B8G8R8A8_UNORM      slot 0
       36 uint16 PixelSize[2]       R16G16_UINT         slot 0
       and the instanced declaration appends, on slot 1 at offset 0, a per-instance
       R32G32B32A32_FLOAT with semantic index 5.

     FSimpleElementVertex, stride 44 (`BatchedElements.h:33-70`)
       0  FVector4 Position         R32G32B32A32_FLOAT  slot 0
       16 FVector2D TextureCoordinate R32G32_FLOAT      slot 0
       24 FLinearColor Color        R32G32B32A32_FLOAT  slot 0
       40 FColor HitProxyIdColor    B8G8R8A8_UNORM      slot 0

   Element order is not assumed: the engine adds them in the order above, but a match is by content
   so a reordered declaration with the same elements still resolves. Elements beyond
   RSF_AC7_UI_MAX_LAYOUT_ELEMENTS make the answer OTHER rather than a guess from a prefix. */
rsf_ac7_layout_kind rsf_ac7_ui_classify_layout(const rsf_ac7_layout_element* elements,
                                               uint32_t count);

/* What a texture was created as, reduced to what decides whether it holds a rasterized widget. */
typedef struct rsf_ac7_texture_facts {
    uint32_t struct_size;
    uint32_t width;
    uint32_t height;
    uint32_t mip_levels;
    uint32_t array_size;
    uint32_t sample_count;
    uint32_t format;
    uint32_t is_render_target;
    uint32_t is_shader_resource;
} rsf_ac7_texture_facts;

/* Whether a texture has the shape of a converter's widget target.

   `UWidgetToTextureConverter` creates these through `FWidgetRenderer::CreateTargetFor`, which asks
   for PF_B8G8R8A8 with a transparent clear, one mip, no array, no multisampling, bound as both a
   render target and a shader resource (`WidgetRenderer.cpp:68-117`). The size is the converter's
   `DrawSize`, which AC7 sets to a hardcoded 1920x1080 for the front end
   (`UWidgetToTextureConverter_Setup 0x1404d5c10`, constants `0x1425f1cac`/`0x1425f1ce4`).

   `draw_sizes` is width/height pairs, so a run can name a second size without a rebuild. A shape
   match is a candidate and nothing more: the frame tap confirms one by seeing a Slate draw write
   into it, because several things in a frame are 1920x1080 and only one of them is the interface.
   That distinction is the whole reason this returns a candidacy rather than an answer. */
int rsf_ac7_ui_is_widget_target(const rsf_ac7_texture_facts* texture, const uint32_t* draw_sizes,
                                uint32_t pair_count);

typedef struct rsf_ac7_draw_input {
    uint32_t slot;
    void* texture;
    uint32_t width;
    uint32_t height;
} rsf_ac7_draw_input;

/* Everything the rule is allowed to look at, all of it already in the tap's shadow. Anything not
   here is a fact the classifier must not need, because reading it would cost a device call on the
   game's hottest path. */
typedef struct rsf_ac7_draw_facts {
    uint32_t struct_size;
    /* Identity of the pipeline, by pointer. Compared against registries, never dereferenced. */
    void* input_layout;
    void* vertex_shader;
    void* pixel_shader;
    /* The render target this draw writes, and what it is. */
    void* render_target;
    uint32_t target_width;
    uint32_t target_height;
    uint32_t target_count;
    uint32_t depth_bound;
    uint32_t uav_bound;
    /* Geometry shape. A world space widget quad is two triangles. */
    uint32_t indexed;
    uint32_t element_count;
    uint32_t vertex_stride;
    /* Colour blend, which separates an over blend from a modulate. */
    uint32_t blend_enabled;
    uint32_t src_blend;
    uint32_t dest_blend;
    /* Pixel stage inputs, in slot order, truncated at RSF_AC7_UI_MAX_INPUTS. */
    uint32_t input_count;
    const rsf_ac7_draw_input* inputs;
} rsf_ac7_draw_facts;

/* What the caller learned at resource creation. Zero-initialize; the runtime fills it as the game
   creates layouts, shaders and textures, and the classifier only reads it.

   Membership is by pointer for pipeline objects and by pointer for textures, because a descriptor
   alone has never been enough in this frame: the widget targets are recognised by descriptor when
   they are created, and confirmed by a Slate draw writing into one. */
typedef struct rsf_ac7_ui_registry {
    uint32_t struct_size;
    /* Input layouts whose element signature matches FSlateVertex (five elements, stride 40). The
       instanced variant adds a sixth per-instance element and is registered too. */
    void* const* slate_layouts;
    uint32_t slate_layout_count;
    /* Input layouts matching FSimpleElementVertex, the canvas HUD's declaration. */
    void* const* canvas_layouts;
    uint32_t canvas_layout_count;
    /* Textures a converter rasterizes a widget into: B8G8R8A8, render target and shader resource,
       one mip, one sample, at a configured draw size (1920x1080 by default). */
    void* const* widget_targets;
    uint32_t widget_target_count;
    /* The frame's own target, so a Slate draw into it is the interface rather than a widget being
       rasterized. Null while the tail has not been identified, which makes Slate draws UNKNOWN
       instead of being classified on half a fact. */
    void* back_buffer;
    /* Shader pointers the settings name explicitly. Force wins over every rule; skip loses to none. */
    void* const* force_shaders;
    uint32_t force_shader_count;
    void* const* skip_shaders;
    uint32_t skip_shader_count;
} rsf_ac7_ui_registry;

/* Classify one draw. Pure: no allocation, no device calls, no writes through either pointer.
   A null argument or a short struct_size is RSF_AC7_DRAW_SCENE, which is the answer that changes
   nothing. */
rsf_ac7_draw_class rsf_ac7_ui_classify(const rsf_ac7_ui_registry* registry,
                                       const rsf_ac7_draw_facts* draw);

/* Whether a draw is worth classifying at all, by pointer comparison only.

   The classifier runs on the game's hottest path, so this is the prefilter that the tens of
   thousands of scene draws in a frame fail: a draw is a candidate when its layout is registered,
   when it reads a registered widget target, or when a setting named its shader. */
int rsf_ac7_ui_is_candidate(const rsf_ac7_ui_registry* registry, const rsf_ac7_draw_facts* draw);

#ifdef __cplusplus
} /* extern "C" */
#endif
#endif
