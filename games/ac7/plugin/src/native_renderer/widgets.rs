// SPDX-License-Identifier: MIT
//! Owned widget raster targets: the game-instance HUD canvases and the widget component render
//! targets at an integer density of the output size, with logical layout restored around them.
use super::frame::{INSIDE_ENGINE_TICK, SHARED_REPAINT_TARGETS, object_is_a};
use super::{Guard, Renderer, engine, hook, installed, key, method, original, read};
use rsf_ac7::discovery::Site;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::write_unaligned;

type TargetInit = unsafe extern "C" fn(*mut c_void, i32, i32, u8, u8);
type TargetUpdate = unsafe extern "C" fn(*mut c_void, u8);
type WidgetDraw =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, f32, u64, f32, u8);

/// Queue-local ownership avoids retaining UObject addresses across garbage collection.
#[derive(Clone, Copy)]
struct WidgetQueue {
    converter: u64,
    density: u32,
    shared: bool,
}

const NO_QUEUE: WidgetQueue = WidgetQueue {
    converter: 0,
    density: 1,
    shared: false,
};

thread_local! {
    static INITIALIZING_GAME_INSTANCE: Cell<u64> = const { Cell::new(0) };
    static WIDGET_QUEUE: Cell<WidgetQueue> = const { Cell::new(NO_QUEUE) };
}

fn widget_density(owner: Option<&Renderer>) -> u32 {
    let Some(config) = owner
        .filter(|owner| owner.active())
        .and_then(Renderer::config)
    else {
        return 1;
    };
    if config.enabled == 0 || config.output_width == 0 || config.output_height == 0 {
        return 1;
    }
    let density = 1u32
        .max(config.output_width.wrapping_add(1919) / 1920)
        .max(config.output_height.wrapping_add(1079) / 1080);
    if density <= 8 { density } else { 1 }
}

fn resize_widget_target(target: u64, width: i32, height: i32) -> bool {
    let mut extent = [0i32; 2];
    if !read(target, 0xd0, &mut extent) || (extent[0] == width && extent[1] == height) {
        return false;
    }
    let object = target as usize as *mut c_void;
    let init: TargetInit = unsafe { original(hook::TARGET_INIT) };
    unsafe { init(object, width, height, 2, 1) };
    unsafe { engine::<TargetUpdate>(Site::TargetUpdate)(object, 0) };
    true
}

fn shared_widget_target(converter: u64, target: &mut u64) -> bool {
    let get_world = method(converter, 0x138);
    if get_world == 0 {
        return false;
    }
    let get_world: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
        unsafe { super::function(get_world as usize as *mut c_void) };
    let world = key(unsafe { get_world(converter as usize as *mut c_void) });
    let mut instance = 0u64;
    let expected_class =
        key(unsafe { engine::<unsafe extern "C" fn() -> *mut c_void>(Site::WidgetStaticClass)() });
    if !read(world, 0x140, &mut instance) || !object_is_a(instance, expected_class) {
        return false;
    }
    let mut stereo = 0u8;
    read(converter, 0x79, &mut stereo)
        && read(instance, if stereo != 0 { 0x1f8 } else { 0x1f0 }, target)
        && *target != 0
}

pub(super) unsafe extern "C" fn hooked_game_instance_init(instance: *mut c_void) {
    let _entry = Guard::entry();
    let previous = INITIALIZING_GAME_INSTANCE.replace(key(instance));
    let forward: unsafe extern "C" fn(*mut c_void) = unsafe { original(hook::GAME_INSTANCE_INIT) };
    unsafe { forward(instance) };
    INITIALIZING_GAME_INSTANCE.set(previous);
}

pub(super) unsafe extern "C" fn hooked_target_init(
    target: *mut c_void,
    mut width: i32,
    mut height: i32,
    format: u8,
    linear: u8,
) {
    let _entry = Guard::entry();
    let instance = INITIALIZING_GAME_INSTANCE.get();
    let (mut hud, mut stereo) = (0u64, 0u64);
    if instance != 0
        && width == 1920
        && height == 1080
        && format == 2
        && linear == 1
        && read(instance, 0x1f0, &mut hud)
        && read(instance, 0x1f8, &mut stereo)
        && (key(target) == hud || key(target) == stereo)
    {
        let density = widget_density(installed());
        width = width.wrapping_mul(density as i32);
        height = height.wrapping_mul(density as i32);
    }
    let init: TargetInit = unsafe { original(hook::TARGET_INIT) };
    unsafe { init(target, width, height, format, linear) };
}

pub(super) unsafe extern "C" fn hooked_widget_queue(converter: *mut c_void, delta: f32) {
    let _entry = Guard::entry();
    let saved = WIDGET_QUEUE.replace(NO_QUEUE);
    let owner = installed();
    let mut borrowed = 1u8;
    let address = key(converter);
    let mut shared = 0u64;
    if owner.is_some()
        && read(address, 0x78, &mut borrowed)
        && (borrowed == 0 || shared_widget_target(address, &mut shared))
    {
        let queue = WidgetQueue {
            converter: address,
            density: widget_density(owner),
            shared: borrowed != 0,
        };
        WIDGET_QUEUE.set(queue);
        if borrowed != 0 {
            let replaced = resize_widget_target(
                shared,
                1920i32.wrapping_mul(queue.density as i32),
                1080i32.wrapping_mul(queue.density as i32),
            );
            let mut targets = SHARED_REPAINT_TARGETS.get();
            if replaced && INSIDE_ENGINE_TICK.get() {
                if let Some(canvas) = targets
                    .iter_mut()
                    .find(|canvas| **canvas == 0 || **canvas == shared)
                {
                    *canvas = shared;
                }
                SHARED_REPAINT_TARGETS.set(targets);
            }
            // Every contributor queued in the replacement input frame must repaint the cleared
            // canvas. Resume the engine RefreshFPS policy on subsequent input frames.
            if replaced || targets.contains(&shared) {
                unsafe { *converter.cast::<u8>().add(0xe0) = 1 };
            }
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void, f32) = unsafe { original(hook::WIDGET_QUEUE) };
    unsafe { forward(converter, delta) };
    WIDGET_QUEUE.set(saved);
}

pub(super) unsafe extern "C" fn hooked_widget_targets(converter: *mut c_void) {
    let _entry = Guard::entry();
    let forward: unsafe extern "C" fn(*mut c_void) = unsafe { original(hook::WIDGET_TARGETS) };
    let address = key(converter);
    let queue = WIDGET_QUEUE.get();
    let mut logical = [0i32; 2];
    if queue.converter != address || !read(address, 0x28, &mut logical) {
        unsafe { forward(converter) };
        return;
    }
    // Borrowed targets are game-instance canvases with a verified 1920x1080 logical size.
    // Stock first binding copies their physical extent into DrawSize; restore logical layout.
    if queue.shared {
        logical = [1920, 1080];
    }
    if logical[0] <= 0
        || logical[1] <= 0
        || logical[0] as u64 * u64::from(queue.density) > 16384
        || logical[1] as u64 * u64::from(queue.density) > 16384
    {
        unsafe { forward(converter) };
        return;
    }
    let physical = [
        logical[0] * queue.density as i32,
        logical[1] * queue.density as i32,
    ];
    let (mut primary, mut previous) = (0u64, [0i32; 2]);
    let resized = !read(address, 0x48, &mut primary)
        || !read(primary, 0xd0, &mut previous)
        || previous != physical;
    let layout = converter.cast::<u8>().wrapping_add(0x28).cast::<[i32; 2]>();
    unsafe { write_unaligned(layout, physical) };
    unsafe { forward(converter) };
    unsafe { write_unaligned(layout, logical) };
    // A replacement must be painted this queue even when RefreshFPS would skip it.
    if resized {
        unsafe { *converter.cast::<u8>().add(0xe0) = 1 };
    }
    // Stock PrepareTargets resizes the primary target, but only creates its glow siblings once.
    let mut glow = 0u8;
    if !read(address, 0x80, &mut glow) || glow == 0 {
        return;
    }
    for offset in [0xc0usize, 0xc8, 0xd0, 0xd8] {
        let (mut target, mut extent) = (0u64, [0i32; 2]);
        let divisor = if offset == 0xd8 { 1 } else { 2 };
        let desired = [1.max(physical[0] / divisor), 1.max(physical[1] / divisor)];
        if !read(address, offset, &mut target)
            || !read(target, 0xd0, &mut extent)
            || extent == desired
        {
            continue;
        }
        let object = target as usize as *mut c_void;
        unsafe {
            engine::<TargetInit>(hook::TARGET_INIT as usize)(object, desired[0], desired[1], 2, 1)
        };
        unsafe { engine::<TargetUpdate>(Site::TargetUpdate)(object, 0) };
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe extern "C" fn hooked_widget_draw(
    renderer: *mut c_void,
    target: *mut c_void,
    hit_grid: *mut c_void,
    window: *mut c_void,
    mut scale: f32,
    mut packed_size: u64,
    delta: f32,
    defer: u8,
) {
    let _entry = Guard::entry();
    let queue = WIDGET_QUEUE.get();
    let (mut owned, mut extent) = (0u64, [0i32; 2]);
    let mut size = [
        f32::from_bits(packed_size as u32),
        f32::from_bits((packed_size >> 32) as u32),
    ];
    let density = queue.density as f32;
    if queue.converter != 0
        && read(queue.converter, 0x48, &mut owned)
        && owned == key(target)
        && read(owned, 0xd0, &mut extent)
        && scale.is_finite()
        && scale > 0.0
        && size[0].is_finite()
        && size[1].is_finite()
        && size[0] > 0.0
        && size[1] > 0.0
        && extent[0] == (size[0] * density) as i32
        && extent[1] == (size[1] * density) as i32
    {
        size[0] *= density;
        size[1] *= density;
        scale *= density;
        packed_size = u64::from(size[0].to_bits()) | (u64::from(size[1].to_bits()) << 32);
    }
    let forward: WidgetDraw = unsafe { original(hook::WIDGET_DRAW) };
    unsafe {
        forward(
            renderer,
            target,
            hit_grid,
            window,
            scale,
            packed_size,
            delta,
            defer,
        )
    };
}
