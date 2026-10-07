/* SPDX-License-Identifier: GPL-3.0-only */
/* Pure AC7 UI classification from frame-tap shadow facts and caller-owned registries. No device
   calls, allocation or resource ownership. Widget rasterization, scene-space quads and final
   Slate/canvas draws are distinct producers; UNKNOWN preserves ambiguous draws for diagnostics.
   Evidence and limits: docs/research/ac7-ui-composition.md, ac7-ui-extraction.md and
   ac7-consumer-session.md. The runtime applies the selected rendering policy. */
#ifndef RSF_AC7_UI_RULES_H
#define RSF_AC7_UI_RULES_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Classification result. UNKNOWN denotes a UI candidate lacking sufficient routing evidence. */
typedef uint32_t rsf_ac7_draw_class;
#define RSF_AC7_DRAW_SCENE ((rsf_ac7_draw_class)0)
/* Slate or canvas geometry into the frame's own target: the interface drawn at native. */
#define RSF_AC7_DRAW_UI_SLATE ((rsf_ac7_draw_class)1)
/* A world space widget quad reading a converter target: the interface drawn as scene geometry. */
#define RSF_AC7_DRAW_UI_WIDGET_QUAD ((rsf_ac7_draw_class)2)
/* Modulate blend depends on destination colour and cannot be diverted to a transparent layer. */
#define RSF_AC7_DRAW_UI_MODULATE ((rsf_ac7_draw_class)3)
/* Slate/canvas writes the widget texture sampled by subsequent scene-space quads. */
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

/* Input-layout signature recognized at resource creation. */
typedef uint32_t rsf_ac7_layout_kind;
#define RSF_AC7_LAYOUT_OTHER ((rsf_ac7_layout_kind)0)
/* `FSlateVertexDeclaration`: the interface's own geometry. */
#define RSF_AC7_LAYOUT_SLATE ((rsf_ac7_layout_kind)1)
/* `FSlateInstancedVertexDeclaration`: the same five elements plus a per-instance transform. */
#define RSF_AC7_LAYOUT_SLATE_INSTANCED ((rsf_ac7_layout_kind)2)
/* `FSimpleElementVertexDeclaration`: the canvas, which is what `ANimbusHUD::DrawWidget*` and the
   engine's own debug drawing go through. */
#define RSF_AC7_LAYOUT_CANVAS ((rsf_ac7_layout_kind)3)

/* Relevant D3D11_INPUT_ELEMENT_DESC fields. UE4.18 uses "ATTRIBUTE" for every semantic name
   (D3D11VertexDeclaration.cpp:56), so identity depends on semantic index, format, slot and offset. */
typedef struct rsf_ac7_layout_element {
    uint32_t semantic_index;
    uint32_t format;
    uint32_t input_slot;
    uint32_t byte_offset;
    uint32_t per_instance;
} rsf_ac7_layout_element;

/* Match borrowed elements against UE4.18.3 signatures at revision `0a14a8d537a3`:

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

   Order is ignored; the count and complete element set must match. Null, empty, oversized or
   unmatched declarations return OTHER. Newer engine layouts are outside this contract. */
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

/* Return nonzero for a widget-target descriptor candidate: B8G8R8A8, one mip/sample/slice,
   RTV+SRV bindings and one configured size. draw_sizes contains pair_count width/height pairs,
   borrowed for the call. A later Slate write confirms identity; descriptor similarity is insufficient.
   Source: WidgetRenderer.cpp:68-117. AC7 front-end DrawSize is 1920x1080 at setup 0x1404d5c10,
   constants 0x1425f1cac/0x1425f1ce4. Invalid/short inputs or no configured size return zero. */
int rsf_ac7_ui_is_widget_target(const rsf_ac7_texture_facts* texture, const uint32_t* draw_sizes,
                                uint32_t pair_count);

/* Borrowed sampled-texture identity and pixel extent. Pointer values are compared, never read. */
typedef struct rsf_ac7_draw_input {
    uint32_t slot;
    void* texture;
    uint32_t width;
    uint32_t height;
} rsf_ac7_draw_input;

/* Borrowed draw snapshot. Set struct_size; inputs points to input_count shadow entries.
   Classification uses these facts only and never queries live D3D11 state. */
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

/* Caller-owned pointer registries; zero-initialize and set struct_size before use. Arrays and
   resource identities remain borrowed. Populate widget_targets only after a Slate write confirms
   a descriptor candidate. Remove retired identities before address reuse. */
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
    /* Explicit shader overrides. Skip has highest priority. Force applies to unmatched draws;
       recognized widget producers and Slate draws retain their more specific classification. */
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

/* Cheap prefilter for registered layouts, sampled widget targets or forced shaders.
   Skip-only shader membership does not admit a candidate. Invalid/short inputs return zero. */
int rsf_ac7_ui_is_candidate(const rsf_ac7_ui_registry* registry, const rsf_ac7_draw_facts* draw);

/* Captured flight HUD producer: exact DXBC container checksum, CRC32C and size.
   This names a producer for resolution tracking, not a draw to divert into a transparent layer. */
int rsf_ac7_ui_is_hud_producer(const void* bytecode, uint32_t bytes);

#ifdef __cplusplus
} /* extern "C" */
#endif
#endif
