// SPDX-License-Identifier: MIT
//! AC7 UI classification over the frame tap's captured facts. The Runtime owns diversion.
use crate::ui_abi::*;
use std::ffi::{c_int, c_void};
use std::slice;

const SCENE: rsf_ac7_draw_class = 0;
const UI_SLATE: rsf_ac7_draw_class = 1;
const WIDGET_QUAD: rsf_ac7_draw_class = 2;
const UI_MODULATE: rsf_ac7_draw_class = 3;
const WIDGET_RASTER: rsf_ac7_draw_class = 4;
const UNKNOWN: rsf_ac7_draw_class = 5;
const SKIP: rsf_ac7_draw_class = 6;

const OTHER: rsf_ac7_layout_kind = 0;
const SLATE: rsf_ac7_layout_kind = 1;
const SLATE_INSTANCED: rsf_ac7_layout_kind = 2;
const CANVAS: rsf_ac7_layout_kind = 3;

const HUD_BYTES: u32 = 1104;
const HUD_CHECKSUM: [u8; 16] = [
    0x6c, 0xac, 0x06, 0x44, 0xb6, 0xa4, 0x9b, 0xe3, 0xef, 0x92, 0x58, 0x58, 0xa7, 0xd3, 0x8e, 0x63,
];
const HUD_CRC32C: u32 = 0xb2f7_719d;

type ShaderHash = unsafe extern "C" fn(*const c_void, u32) -> u32;

/// The caller lends a readable bytecode span and the Runtime's shader hash function.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_ui_is_hud_producer(
    bytecode: *const c_void,
    bytes: u32,
    shader_hash: ShaderHash,
) -> c_int {
    if bytecode.is_null() || bytes != HUD_BYTES {
        return 0;
    }
    let data = unsafe { slice::from_raw_parts(bytecode.cast::<u8>(), bytes as usize) };
    c_int::from(
        &data[..4] == b"DXBC"
            && data[4..20] == HUD_CHECKSUM
            && unsafe { shader_hash(bytecode, bytes) } == HUD_CRC32C,
    )
}

fn bgra(format: u32) -> bool {
    matches!(
        format,
        RSF_AC7_FORMAT_B8G8R8A8_TYPELESS
            | RSF_AC7_FORMAT_B8G8R8A8_UNORM
            | RSF_AC7_FORMAT_B8G8R8A8_UNORM_SRGB
    )
}

const fn element(
    semantic_index: u32,
    format: u32,
    input_slot: u32,
    byte_offset: u32,
    per_instance: u32,
) -> rsf_ac7_layout_element {
    rsf_ac7_layout_element {
        semantic_index,
        format,
        input_slot,
        byte_offset,
        per_instance,
    }
}

// FSlateVertex, stride 40. The instanced declaration appends one element.
const SLATE_ELEMENTS: [rsf_ac7_layout_element; 5] = [
    element(0, RSF_AC7_FORMAT_R32G32B32A32_FLOAT, 0, 0, 0),
    element(1, RSF_AC7_FORMAT_R32G32_FLOAT, 0, 16, 0),
    element(2, RSF_AC7_FORMAT_R32G32_FLOAT, 0, 24, 0),
    element(3, RSF_AC7_FORMAT_B8G8R8A8_UNORM, 0, 32, 0),
    element(4, RSF_AC7_FORMAT_R16G16_UINT, 0, 36, 0),
];
const INSTANCE_ELEMENT: rsf_ac7_layout_element =
    element(5, RSF_AC7_FORMAT_R32G32B32A32_FLOAT, 1, 0, 1);
// FSimpleElementVertex, stride 44.
const CANVAS_ELEMENTS: [rsf_ac7_layout_element; 4] = [
    element(0, RSF_AC7_FORMAT_R32G32B32A32_FLOAT, 0, 0, 0),
    element(1, RSF_AC7_FORMAT_R32G32_FLOAT, 0, 16, 0),
    element(2, RSF_AC7_FORMAT_R32G32B32A32_FLOAT, 0, 24, 0),
    element(3, RSF_AC7_FORMAT_B8G8R8A8_UNORM, 0, 40, 0),
];

fn has_element(elements: &[rsf_ac7_layout_element], wanted: &rsf_ac7_layout_element) -> bool {
    elements.iter().any(|element| {
        (element.format == wanted.format
            || (wanted.format == RSF_AC7_FORMAT_B8G8R8A8_UNORM && bgra(element.format)))
            && element.semantic_index == wanted.semantic_index
            && element.input_slot == wanted.input_slot
            && element.byte_offset == wanted.byte_offset
            && (element.per_instance != 0) == (wanted.per_instance != 0)
    })
}

/// The caller lends the complete declaration or null.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_ui_classify_layout(
    elements: *const rsf_ac7_layout_element,
    count: u32,
) -> rsf_ac7_layout_kind {
    if elements.is_null() || count == 0 || count > RSF_AC7_UI_MAX_LAYOUT_ELEMENTS {
        return OTHER;
    }
    let elements = unsafe { slice::from_raw_parts(elements, count as usize) };
    let has_all =
        |wanted: &[rsf_ac7_layout_element]| wanted.iter().all(|item| has_element(elements, item));
    let count = elements.len();
    // Match exact counts and by element, irrespective of the declaration's order.
    match count {
        count if count == SLATE_ELEMENTS.len() && has_all(&SLATE_ELEMENTS) => SLATE,
        count
            if count == SLATE_ELEMENTS.len() + 1
                && has_all(&SLATE_ELEMENTS)
                && has_element(elements, &INSTANCE_ELEMENT) =>
        {
            SLATE_INSTANCED
        }
        count if count == CANVAS_ELEMENTS.len() && has_all(&CANVAS_ELEMENTS) => CANVAS,
        _ => OTHER,
    }
}

/// The caller lends a size-prefixed C record and its referenced data for the call.
unsafe fn facts<'a, T>(ptr: *const T) -> Option<&'a T> {
    if ptr.is_null() || (unsafe { ptr.cast::<u32>().read() } as usize) < size_of::<T>() {
        return None;
    }
    unsafe { ptr.as_ref() }
}

/// The caller lends size-prefixed texture facts and `pair_count` width/height pairs, or null.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_ui_is_widget_target(
    texture: *const rsf_ac7_texture_facts,
    draw_sizes: *const u32,
    pair_count: u32,
) -> c_int {
    let Some(texture) = (unsafe { facts(texture) }) else {
        return 0;
    };
    if texture.is_render_target == 0
        || texture.is_shader_resource == 0
        || texture.mip_levels != 1
        || texture.array_size != 1
        || texture.sample_count != 1
        || !bgra(texture.format)
        || draw_sizes.is_null()
        || pair_count == 0
    {
        return 0;
    }
    let sizes =
        unsafe { slice::from_raw_parts(draw_sizes.cast::<[u32; 2]>(), pair_count as usize) };
    c_int::from(sizes.contains(&[texture.width, texture.height]))
}

/// The registry lends the advertised pointers. They are identities, never dereferenced.
unsafe fn contains(set: *const *mut c_void, count: u32, value: *mut c_void) -> bool {
    !set.is_null()
        && !value.is_null()
        && unsafe { slice::from_raw_parts(set, count as usize) }.contains(&value)
}

unsafe fn named_shader(set: *const *mut c_void, count: u32, draw: &rsf_ac7_draw_facts) -> bool {
    unsafe { contains(set, count, draw.pixel_shader) || contains(set, count, draw.vertex_shader) }
}

unsafe fn named_layout(registry: &rsf_ac7_ui_registry, draw: &rsf_ac7_draw_facts) -> bool {
    unsafe {
        contains(
            registry.slate_layouts,
            registry.slate_layout_count,
            draw.input_layout,
        ) || contains(
            registry.canvas_layouts,
            registry.canvas_layout_count,
            draw.input_layout,
        )
    }
}

unsafe fn reads_widget(registry: &rsf_ac7_ui_registry, draw: &rsf_ac7_draw_facts) -> bool {
    let count = draw.input_count.min(RSF_AC7_UI_MAX_INPUTS) as usize;
    !draw.inputs.is_null()
        && unsafe { slice::from_raw_parts(draw.inputs, count) }
            .iter()
            .any(|input| unsafe {
                contains(
                    registry.widget_targets,
                    registry.widget_target_count,
                    input.texture,
                )
            })
}

fn retargetable(draw: &rsf_ac7_draw_facts) -> bool {
    !draw.render_target.is_null() && draw.target_count == 1 && draw.uav_bound == 0
}

fn blends_over(draw: &rsf_ac7_draw_facts) -> bool {
    draw.blend_enabled != 0
        && draw.dest_blend == RSF_AC7_BLEND_INV_SRC_ALPHA
        && matches!(draw.src_blend, RSF_AC7_BLEND_SRC_ALPHA | RSF_AC7_BLEND_ONE)
}

fn modulates(draw: &rsf_ac7_draw_facts) -> bool {
    draw.blend_enabled != 0 && draw.dest_blend == RSF_AC7_BLEND_SRC_COLOR
}

fn slate_draw(draw: &rsf_ac7_draw_facts) -> rsf_ac7_draw_class {
    if modulates(draw) {
        UI_MODULATE
    } else {
        UI_SLATE
    }
}

unsafe fn draw_facts<'a>(
    registry: *const rsf_ac7_ui_registry,
    draw: *const rsf_ac7_draw_facts,
) -> Option<(&'a rsf_ac7_ui_registry, &'a rsf_ac7_draw_facts)> {
    if registry.is_null() || draw.is_null() {
        return None;
    }
    Some((unsafe { facts(registry) }?, unsafe { facts(draw) }?))
}

/// The caller lends size-prefixed records and their referenced facts for one draw.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_ui_is_candidate(
    registry: *const rsf_ac7_ui_registry,
    draw: *const rsf_ac7_draw_facts,
) -> c_int {
    let Some((registry, draw)) = (unsafe { draw_facts(registry, draw) }) else {
        return 0;
    };
    c_int::from(unsafe {
        named_shader(registry.force_shaders, registry.force_shader_count, draw)
            || named_layout(registry, draw)
            || reads_widget(registry, draw)
    })
}

/// The caller lends size-prefixed records and their referenced facts for one draw.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_ui_classify(
    registry: *const rsf_ac7_ui_registry,
    draw: *const rsf_ac7_draw_facts,
) -> rsf_ac7_draw_class {
    let Some((registry, draw)) = (unsafe { draw_facts(registry, draw) }) else {
        return SCENE;
    };
    if unsafe { named_shader(registry.skip_shaders, registry.skip_shader_count, draw) } {
        return SKIP;
    }
    let forced = unsafe { named_shader(registry.force_shaders, registry.force_shader_count, draw) };
    let widget_target = unsafe {
        contains(
            registry.widget_targets,
            registry.widget_target_count,
            draw.render_target,
        )
    };
    let back_buffer = !registry.back_buffer.is_null() && registry.back_buffer == draw.render_target;
    if unsafe { named_layout(registry, draw) } {
        // The converter must populate its texture before a world-space quad can read it.
        if widget_target {
            return WIDGET_RASTER;
        }
        return if back_buffer {
            slate_draw(draw)
        } else {
            UNKNOWN
        };
    }

    if unsafe { reads_widget(registry, draw) } {
        if retargetable(draw) && !widget_target && !back_buffer {
            if modulates(draw) {
                return UI_MODULATE;
            }
            if draw.indexed != 0 && draw.element_count == 6 && blends_over(draw) {
                return WIDGET_QUAD;
            }
        }
        return if forced { WIDGET_QUAD } else { UNKNOWN };
    }
    if forced && retargetable(draw) {
        slate_draw(draw)
    } else {
        SCENE
    }
}
