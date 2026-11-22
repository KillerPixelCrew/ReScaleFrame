// SPDX-License-Identifier: MIT
use super::{
    COUNTERS, Platform, capturing, engine_layout, read_bytes, read_pass,
    rsf_anomaly_geometry_binding,
};
use crate::native_geometry_abi::{
    Context, Engine, FIRST_PERSON, MAIN, SCOPE, UNSET, Visual, WORLD, Window,
};
use std::{cell::Cell, ptr};

// These nodes are pinned by their C++ stack guards. The pointers never leave
// this thread or outlive the corresponding guard, including on native unwind.
thread_local! {
    static CONTEXT: Cell<*mut Context> = const { Cell::new(ptr::null_mut()) };
    static VISUAL: Cell<*mut Visual> = const { Cell::new(ptr::null_mut()) };
    static WINDOW: Cell<*mut Window> = const { Cell::new(ptr::null_mut()) };
    static FACTORY: Cell<u32> = const { Cell::new(0) };
    static DETAIL: Cell<u32> = const { Cell::new(0) };
}

pub(super) fn context() -> (u8, u8) {
    unsafe { CONTEXT.get().as_ref() }.map_or((UNSET, UNSET), |scope| (scope.view, scope.domain))
}

pub(super) fn visual() -> *const Visual {
    VISUAL.get()
}

pub(super) fn in_factory() -> bool {
    FACTORY.get() != 0
}

pub(super) fn in_detail() -> bool {
    DETAIL.get() != 0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_factory_enter() {
    FACTORY.set(FACTORY.get() + 1);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_factory_leave() {
    leave_factory();
}

pub(super) fn leave_factory() {
    FACTORY.set(FACTORY.get() - 1);
}

fn view(platform: &Platform, engine: Engine) -> Option<u8> {
    let layout = engine_layout(engine)?;
    let viewport = engine.module.checked_add(layout.viewport as usize)?;
    let [active] = read_bytes(platform.read, viewport)?;
    let [delay] = read_bytes(platform.read, viewport.checked_add(1)?)?;
    let frame = u32::from_ne_bytes(read_bytes(
        platform.read,
        engine.module.checked_add(layout.frame as usize)?,
    )?);
    Some(
        if active != 0 && delay != 0 && frame.is_multiple_of(u32::from(delay)) {
            SCOPE
        } else {
            MAIN
        },
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_view(platform: &Platform, engine: Engine) -> u8 {
    view(platform, engine).unwrap_or(UNSET)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_context_enter(
    node: *mut Context,
    view: u8,
    domain: u8,
    inherit: bool,
) {
    let view = if inherit { context().0 } else { view };
    let previous = CONTEXT.replace(node);
    unsafe {
        node.write(Context {
            previous,
            view,
            domain,
        })
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_render_enter(
    platform: &Platform,
    node: *mut Context,
) -> bool {
    if !capturing() {
        return false;
    }
    let view = rsf_anomaly_geometry_view(platform, rsf_anomaly_geometry_binding());
    match view {
        MAIN => COUNTERS.views_main.increment(),
        SCOPE => COUNTERS.views_scope.increment(),
        _ => {}
    }
    // A nested Render has its own view identity even when the predicate bytes
    // cannot be read. Mask the previous view for this call.
    unsafe { rsf_anomaly_geometry_context_enter(node, view, WORLD, false) };
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_hud_enter(node: *mut Context) -> bool {
    if !capturing() {
        return false;
    }
    unsafe { rsf_anomaly_geometry_context_enter(node, UNSET, FIRST_PERSON, true) };
    COUNTERS.hud_scopes.increment();
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_context_leave(node: *mut Context) {
    let head = CONTEXT.get();
    let previous = unsafe { (*node).previous };
    if head == node {
        CONTEXT.set(previous);
        return;
    }
    // rmNormal can end a near interval while an explicit child scope is on
    // top. Unlink only that interval and preserve the live child.
    let mut current = head;
    while !current.is_null() {
        let next = unsafe { (*current).previous };
        if next == node {
            unsafe { (*current).previous = previous };
            return;
        }
        current = next;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_window_enter(node: *mut Window) {
    unsafe {
        (*node).previous = WINDOW.replace(node);
        (*node).near_active = false;
    }
}

fn close_near(node: *mut Window) {
    if !node.is_null() && unsafe { (*node).near_active } {
        unsafe {
            rsf_anomaly_geometry_context_leave(&raw mut (*node).near_context);
            (*node).near_active = false;
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_window_leave(node: *mut Window) {
    close_near(node);
    WINDOW.set(unsafe { (*node).previous });
}

pub(super) fn near_enter() {
    let node = WINDOW.get();
    if node.is_null() || !capturing() || unsafe { (*node).near_active } {
        return;
    }
    unsafe {
        rsf_anomaly_geometry_context_enter(
            &raw mut (*node).near_context,
            UNSET,
            FIRST_PERSON,
            true,
        );
        (*node).near_active = true;
    }
    COUNTERS.near_windows.increment();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_near_leave() {
    close_near(WINDOW.get());
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_detail_enter(platform: &Platform) {
    DETAIL.set(DETAIL.get() + 1);
    if capturing() {
        let pass = read_pass(platform, rsf_anomaly_geometry_binding());
        COUNTERS.detail_renders[pass as usize].increment();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_detail_leave() {
    DETAIL.set(DETAIL.get() - 1);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_visual_enter(node: *mut Visual) {
    unsafe { enter_visual(node) };
}

pub(super) unsafe fn enter_visual(node: *mut Visual) {
    let (view, domain) = context();
    unsafe {
        (*node).previous = VISUAL.replace(node);
        (*node).view = view;
        (*node).domain = domain;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_visual_leave(node: *const Visual) {
    VISUAL.set(unsafe { (*node).previous });
}
