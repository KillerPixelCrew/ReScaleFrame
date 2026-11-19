// SPDX-License-Identifier: MIT
use rsf_game_sys::{
    RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_NO_FG,
    RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RESET, rsf_game_render_pass as Pass,
};
use std::cell::Cell;

const EMPTY: Pass = unsafe { std::mem::zeroed() };
const PASS_SIZE: u32 = size_of::<Pass>() as u32;
const SOURCE_FLAGS: u32 = RSF_GAME_RENDER_RESET
    | RSF_GAME_RENDER_PRIMARY
    | RSF_GAME_RENDER_AFTER_SIMULATION
    | RSF_GAME_RENDER_NO_FG;

#[derive(Clone, Copy)]
struct Source {
    session: u64,
    frame: u64,
    submission: u64,
    view: u64,
    family: u64,
    viewport: u64,
    scope: u64,
    native_frame: u64,
    screen: u32,
    flags: u32,
}

impl Source {
    fn from_pass(pass: &Pass) -> Option<Self> {
        (pass.source_frame_id != 0).then_some(Self {
            session: pass.session_id,
            frame: pass.source_frame_id,
            submission: pass.submission_id,
            view: pass.view_key,
            family: pass.family_key,
            viewport: pass.viewport_key,
            scope: pass.scope_id,
            native_frame: pass.native_frame,
            screen: pass.screen,
            flags: pass.flags & SOURCE_FLAGS,
        })
    }

    fn apply(self, pass: &mut Pass) {
        pass.session_id = self.session;
        pass.source_frame_id = self.frame;
        pass.submission_id = self.submission;
        pass.view_key = self.view;
        pass.family_key = self.family;
        pass.viewport_key = self.viewport;
        pass.scope_id = self.scope;
        pass.native_frame = self.native_frame;
        pass.screen = self.screen;
        pass.flags = (pass.flags & !RSF_GAME_RENDER_NO_FG) | self.flags;
    }
}

thread_local! {
    static BOUND: Cell<Option<Source>> = const { Cell::new(None) };
    // The exact scene is valid only during the enclosing graphics lease.
    static SCENE: Cell<Pass> = const { Cell::new(EMPTY) };
}

fn apply_bound(pass: &mut Pass) {
    if let Some(bound) = BOUND.get() {
        bound.apply(pass);
    }
}

pub(super) fn with_scene<R>(pass: &Pass, call: impl FnOnce() -> R) -> R {
    struct Restore(Pass);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCENE.set(self.0);
        }
    }
    let mut scene = Pass {
        struct_size: PASS_SIZE,
        ..*pass
    };
    if scene.source_frame_id == 0 {
        apply_bound(&mut scene);
    }
    let _restore = Restore(SCENE.replace(scene));
    call()
}

unsafe fn full_pass(pass: *const Pass) -> bool {
    !pass.is_null() && unsafe { pass.cast::<u32>().read() } >= PASS_SIZE
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_bind_source(identity: *const Pass) {
    let source = if unsafe { full_pass(identity) } {
        Source::from_pass(unsafe { &*identity })
    } else {
        None
    };
    BOUND.set(source);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_scene_swap(in_out: *mut Pass) -> i32 {
    if !unsafe { full_pass(in_out) } {
        return 0;
    }
    let current = Pass {
        struct_size: PASS_SIZE,
        ..unsafe { in_out.read() }
    };
    let previous = SCENE.replace(current);
    unsafe {
        in_out.write(Pass {
            struct_size: PASS_SIZE,
            ..previous
        })
    };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_scene_set(
    pass: *const Pass,
    refuse_held_inputs: unsafe extern "C" fn(*const Pass),
) {
    if pass.is_null() {
        SCENE.set(EMPTY);
        return;
    }
    if !unsafe { full_pass(pass) } {
        return;
    }
    let mut scene = Pass {
        struct_size: PASS_SIZE,
        ..unsafe { pass.read() }
    };
    if scene.source_frame_id == 0 {
        apply_bound(&mut scene);
    } else if scene.role == RSF_GAME_RENDER_FG_INPUTS && scene.flags & RSF_GAME_RENDER_NO_FG != 0 {
        // Only a refusal reported with the original explicit lease may mark held inputs.
        unsafe { refuse_held_inputs(pass) };
    }
    SCENE.set(scene);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_scene_read(out: *mut Pass) {
    unsafe { out.write(SCENE.get()) };
}
