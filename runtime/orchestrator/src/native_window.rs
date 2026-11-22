// SPDX-License-Identifier: MIT
use crate::scene_abi::{
    RSF_NATIVE_SCENE_PRESENT_DIRECT, rsf_native_present_record, rsf_native_scene_identity,
    rsf_observer_present_event,
};
use crate::{
    com_ref::ComRef,
    native_scene::{rsf_native_scene_classify_present, rsf_native_scene_for_window},
};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr::null_mut,
};
use rsf_game_sys::{RSF_GAME_RENDER_WINDOW, rsf_game_render_pass};
use windows_sys::Win32::{
    Foundation::S_OK,
    System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
};

use windows_sys::core::GUID;

const DXGI_PRESENT_TEST: u32 = 0x00000001;
const WINDOW_SCOPE_CAPACITY: usize = 32;

const IID_IUNKNOWN: GUID = GUID {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};

unsafe fn com_identity(object: *mut c_void) -> Option<ComRef> {
    type Query = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
    // IUnknown::QueryInterface is the first slot on every COM interface.
    let query = unsafe { **object.cast::<*const Query>() };
    let mut identity = null_mut();
    if unsafe { query(object, &IID_IUNKNOWN, &mut identity) } < 0 {
        return None;
    }
    unsafe { ComRef::from_owned(identity) }
}

struct WindowScopes {
    passes: [Option<rsf_game_render_pass>; WINDOW_SCOPE_CAPACITY],
    depth: usize,
    overflow: usize,
}

impl WindowScopes {
    const fn new() -> Self {
        Self {
            passes: [None; WINDOW_SCOPE_CAPACITY],
            depth: 0,
            overflow: 0,
        }
    }

    fn begin(&mut self, pass: &rsf_game_render_pass) -> bool {
        if self.overflow != 0 || self.depth == self.passes.len() {
            self.overflow += 1;
            return false;
        }
        self.passes[self.depth] = Some(rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            ..*pass
        });
        self.depth += 1;
        true
    }

    fn end(&mut self, pass: &rsf_game_render_pass) -> bool {
        if self.overflow != 0 {
            self.overflow -= 1;
            return false;
        }
        let Some(current) = self.current() else {
            return false;
        };
        if current.session_id != pass.session_id
            || current.scope_id != pass.scope_id
            || current.source_frame_id != pass.source_frame_id
            || current.window_key != pass.window_key
            || current.rhi_viewport_key != pass.rhi_viewport_key
            || current.viewport_key != pass.viewport_key
            || current.swapchain != pass.swapchain
        {
            // The end callback retires the lease. Drop every borrowed pass if nesting diverges.
            *self = Self::new();
            return false;
        }
        self.depth -= 1;
        self.passes[self.depth] = None;
        true
    }

    fn current(&self) -> Option<&rsf_game_render_pass> {
        if self.overflow != 0 || self.depth == 0 {
            return None;
        }
        self.passes[self.depth - 1].as_ref()
    }
}

thread_local! {
    static SCOPES: RefCell<WindowScopes> = const { RefCell::new(WindowScopes::new()) };
    static LAST_PRESENT: Cell<Option<rsf_native_present_record>> = const { Cell::new(None) };
}

// Each descriptor starts with the readable struct_size prefix, including output descriptors.
unsafe fn has_capacity<T>(descriptor: *const T) -> bool {
    !descriptor.is_null() && (unsafe { descriptor.cast::<u32>().read() } as usize) >= size_of::<T>()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_window_scope(pass: *const rsf_game_render_pass, begin: u32) -> i32 {
    if !unsafe { has_capacity(pass) } {
        return 0;
    }
    let pass = unsafe { &*pass };
    if pass.role != RSF_GAME_RENDER_WINDOW
        || pass.session_id == 0
        || pass.source_frame_id == 0
        || pass.scope_id == 0
        || pass.window_key == 0
        || pass.viewport_key == 0
        || pass.rhi_viewport_key == 0
        || pass.swapchain.is_null()
    {
        return 0;
    }
    i32::from(SCOPES.with_borrow_mut(|scopes| {
        if begin != 0 {
            scopes.begin(pass)
        } else {
            scopes.end(pass)
        }
    }))
}

unsafe fn match_window(swapchain: *mut c_void) -> Option<rsf_game_render_pass> {
    // The active scope leases its swapchain. Release the TLS borrow before calling COM.
    let current = SCOPES.with_borrow(|scopes| scopes.current().copied())?;
    let observed = unsafe { com_identity(swapchain) }?;
    let owned = unsafe { com_identity(current.swapchain) }?;
    (observed.as_ptr() == owned.as_ptr()).then_some(current)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_window_match(
    swapchain: *mut c_void,
    pass: *mut rsf_game_render_pass,
) -> i32 {
    if swapchain.is_null() || !unsafe { has_capacity(pass) } {
        return 0;
    }
    let Some(current) = (unsafe { match_window(swapchain) }) else {
        return 0;
    };
    unsafe { pass.write(current) };
    1
}

fn scene_for_window(window: &rsf_game_render_pass) -> Option<rsf_native_scene_identity> {
    let mut scene = rsf_native_scene_identity {
        struct_size: size_of::<rsf_native_scene_identity>() as u32,
        ..Default::default()
    };
    (unsafe { rsf_native_scene_for_window(window, &mut scene) } != 0).then_some(scene)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_window_present(event: *const rsf_observer_present_event) -> i32 {
    if !unsafe { has_capacity(event) } {
        return 0;
    }
    // The observer's result field is only valid on return from native Present.
    let (swapchain, completed, method, flags, sync_interval) = unsafe {
        (
            (*event).swapchain,
            (*event).completed,
            (*event).method,
            (*event).flags,
            (*event).sync_interval,
        )
    };
    if swapchain.is_null() || completed > 1 || method > 1 || flags & DXGI_PRESENT_TEST != 0 {
        return 0;
    }

    let (mut counter, mut frequency) = (0, 0);
    unsafe {
        QueryPerformanceCounter(&mut counter);
        QueryPerformanceFrequency(&mut frequency);
    }
    let window = unsafe { match_window(swapchain) };
    if completed == 0 {
        let mut record = rsf_native_present_record {
            struct_size: size_of::<rsf_native_present_record>() as u32,
            sync_interval,
            method,
            flags,
            begin_qpc: counter as u64,
            qpc_frequency: frequency as u64,
            ..Default::default()
        };
        if let Some(window) = window {
            record.matched = 1;
            record.session_id = window.session_id;
            record.source_frame_id = window.source_frame_id;
            record.scope_id = window.scope_id;
            record.viewport_key = window.viewport_key;
            record.window_key = window.window_key;
            if let Some(scene) = scene_for_window(&window) {
                record.scene_matched = 1;
                record.scene_submission_id = scene.submission_id;
                record.scene_texture_bound = u32::from(
                    scene.texture_bound != 0 && scene.bound_window_key == window.window_key,
                );
                record.scene_direct_present = u32::from(
                    unsafe { rsf_native_scene_classify_present(&window, &scene) }
                        == RSF_NATIVE_SCENE_PRESENT_DIRECT,
                );
                record.scene_view_key = scene.view_key;
                record.scene_native_frame = scene.native_frame;
            }
        }
        LAST_PRESENT.set(Some(record));
        return 1;
    }

    let Some(mut record) = LAST_PRESENT.take() else {
        return 0;
    };
    if record.completed != 0
        || record.sync_interval != sync_interval
        || record.flags != flags
        || record.method != method
        || (record.matched != 0) != window.is_some()
        || window.as_ref().is_some_and(|window| {
            record.session_id != window.session_id
                || record.source_frame_id != window.source_frame_id
                || record.scope_id != window.scope_id
        })
    {
        return 0;
    }
    if let Some(window) = window
        && record.scene_matched != 0
        && scene_for_window(&window).is_none_or(|scene| {
            scene.submission_id != record.scene_submission_id
                || scene.view_key != record.scene_view_key
                || scene.native_frame != record.scene_native_frame
        })
    {
        record.scene_matched = 0;
        record.scene_texture_bound = 0;
        record.scene_direct_present = 0;
    }
    record.result = unsafe { (*event).result };
    record.completed = 1;
    record.accepted = u32::from(record.result == S_OK);
    record.end_qpc = counter as u64;
    LAST_PRESENT.set(Some(record));
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_window_last_present(record: *mut rsf_native_present_record) -> i32 {
    if !unsafe { has_capacity(record) } {
        return 0;
    }
    let Some(last) = LAST_PRESENT.get().filter(|last| last.completed != 0) else {
        return 0;
    };
    unsafe { record.write(last) };
    1
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_native_window_reset() {
    SCOPES.with_borrow_mut(|scopes| *scopes = WindowScopes::new());
    LAST_PRESENT.set(None);
}
