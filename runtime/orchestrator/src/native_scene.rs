// SPDX-License-Identifier: MIT
use crate::scene_abi::{
    RSF_NATIVE_SCENE_PRESENT_DIRECT, RSF_NATIVE_SCENE_PRESENT_NONE,
    RSF_NATIVE_SCENE_PRESENT_SAMPLED, rsf_native_scene_identity as Scene,
};
use rsf_game_sys::{
    RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_PRIMARY,
    RSF_GAME_RENDER_TEXTURE_BINDING, RSF_GAME_RENDER_WINDOW, rsf_game_render_pass as Pass,
};
use std::cell::RefCell;
use std::ffi::c_void;

const CAPACITY: usize = 64;
const EMPTY: Scene = unsafe { std::mem::zeroed() };
const PRIMARY: u32 = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;

thread_local! {
    // Only identity values survive the callback. The registry owns no COM references.
    static SCENES: RefCell<[Scene; CAPACITY]> = const { RefCell::new([EMPTY; CAPACITY]) };
}

type ResourceIdentity = unsafe extern "C" fn(*mut c_void) -> u64;
type BackbufferIdentity = unsafe extern "C" fn(*mut c_void, *mut u32, *mut u32) -> u64;

unsafe fn full_prefix<T>(value: *const T) -> bool {
    !value.is_null() && (unsafe { value.cast::<u32>().read() } as usize) >= size_of::<T>()
}

fn slot(source: u64) -> usize {
    (source % CAPACITY as u64) as usize
}

fn same_source(scene: &Scene, pass: &Pass) -> bool {
    scene.session_id == pass.session_id && scene.source_frame_id == pass.source_frame_id
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_scene_pass(
    pass: *const Pass,
    begin: u32,
    identity: ResourceIdentity,
) -> i32 {
    if !unsafe { full_prefix(pass) } {
        return 0;
    }
    let pass = unsafe { &*pass };
    if pass.role != RSF_GAME_RENDER_FINAL_SCENE
        || pass.flags & PRIMARY != PRIMARY
        || pass.session_id == 0
        || pass.source_frame_id == 0
        || pass.viewport_key == 0
        || pass.submission_id == 0
        || pass.view_key == 0
        || pass.scope_id == 0
    {
        return 0;
    }
    SCENES.with(|scenes| {
        let Ok(mut scenes) = scenes.try_borrow_mut() else {
            return 0;
        };
        let scene = &mut scenes[slot(pass.source_frame_id)];
        let same = same_source(scene, pass);
        if begin != 0 {
            if same {
                // One source cannot declare two primary compositions.
                scene.ambiguous = 1;
                return 0;
            }
            if scene.source_frame_id != 0 && scene.complete == 0 && scene.ambiguous == 0 {
                return 0;
            }
            *scene = Scene {
                struct_size: size_of::<Scene>() as u32,
                session_id: pass.session_id,
                source_frame_id: pass.source_frame_id,
                viewport_key: pass.viewport_key,
                submission_id: pass.submission_id,
                family_key: pass.family_key,
                view_key: pass.view_key,
                native_frame: pass.native_frame,
                scope_id: pass.scope_id,
                flags: pass.flags,
                surface_key: unsafe { identity(pass.scene_surface) },
                output_rect: pass.output_rect,
                ..EMPTY
            };
            return 1;
        }
        if !same || scene.ambiguous != 0 {
            return 0;
        }
        if scene.scope_id != pass.scope_id
            || scene.submission_id != pass.submission_id
            || scene.family_key != pass.family_key
            || scene.view_key != pass.view_key
            || scene.viewport_key != pass.viewport_key
            || scene.native_frame != pass.native_frame
            || scene.output_rect != pass.output_rect
            || scene.surface_key == 0
            || scene.surface_key != unsafe { identity(pass.scene_surface) }
        {
            scene.ambiguous = 1;
            return 0;
        }
        scene.complete = 1;
        1
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_scene_texture_binding(
    pass: *const Pass,
    identity: ResourceIdentity,
) -> i32 {
    if !unsafe { full_prefix(pass) } {
        return 0;
    }
    let pass = unsafe { &*pass };
    if pass.role != RSF_GAME_RENDER_TEXTURE_BINDING
        || pass.sampled_texture.is_null()
        || pass.source_frame_id == 0
        || pass.window_key == 0
    {
        return 0;
    }
    SCENES.with(|scenes| {
        let Ok(mut scenes) = scenes.try_borrow_mut() else {
            return 0;
        };
        let scene = &mut scenes[slot(pass.source_frame_id)];
        if scene.complete == 0
            || scene.ambiguous != 0
            || !same_source(scene, pass)
            || scene.viewport_key != pass.viewport_key
            || scene.surface_key != unsafe { identity(pass.sampled_texture) }
        {
            return 0;
        }
        if scene.bound_window_key != 0 && scene.bound_window_key != pass.window_key {
            scene.ambiguous = 1;
            return 0;
        }
        scene.texture_bound = 1;
        scene.bound_window_key = pass.window_key;
        1
    })
}

fn find_window(window: &Pass) -> Option<Scene> {
    if window.role != RSF_GAME_RENDER_WINDOW
        || window.session_id == 0
        || window.source_frame_id == 0
        || window.viewport_key == 0
        || window.window_key == 0
        || window.flags & PRIMARY != PRIMARY
    {
        return None;
    }
    SCENES.with(|scenes| {
        let scene = scenes.try_borrow().ok()?[slot(window.source_frame_id)];
        (same_source(&scene, window)
            && scene.viewport_key == window.viewport_key
            && scene.complete != 0
            && scene.ambiguous == 0)
            .then_some(scene)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_scene_for_window(window: *const Pass, out: *mut Scene) -> i32 {
    if !unsafe { full_prefix(window) && full_prefix(out) } {
        return 0;
    }
    let Some(scene) = find_window(unsafe { &*window }) else {
        return 0;
    };
    unsafe { out.write(scene) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_scene_present_path(
    window: *const Pass,
    backbuffer: BackbufferIdentity,
) -> u32 {
    if !unsafe { full_prefix(window) } {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    }
    let window = unsafe { &*window };
    let Some(scene) = find_window(window) else {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    };
    if scene.texture_bound != 0 && scene.bound_window_key == window.window_key {
        return RSF_NATIVE_SCENE_PRESENT_SAMPLED;
    }
    if window.swapchain.is_null() || scene.surface_key == 0 {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    }
    let (mut width, mut height) = (0, 0);
    let identity = unsafe { backbuffer(window.swapchain, &mut width, &mut height) };
    // A direct path uses the current full backbuffer, without a sampled scene.
    if identity == scene.surface_key && scene.output_rect == [0, 0, width as i32, height as i32] {
        return RSF_NATIVE_SCENE_PRESENT_DIRECT;
    }
    RSF_NATIVE_SCENE_PRESENT_NONE
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_scene_reset() {
    SCENES.with(|scenes| {
        if let Ok(mut scenes) = scenes.try_borrow_mut() {
            *scenes = [EMPTY; CAPACITY];
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn resource(resource: *mut c_void) -> u64 {
        resource as usize as u64
    }

    unsafe extern "C" fn backbuffer(chain: *mut c_void, width: *mut u32, height: *mut u32) -> u64 {
        unsafe {
            width.write(128);
            height.write(64);
            resource(chain)
        }
    }

    #[test]
    fn scene_path_requires_a_complete_unambiguous_source() {
        rsf_native_scene_reset();
        let surface = std::ptr::dangling_mut::<c_void>();
        let pass = Pass {
            struct_size: size_of::<Pass>() as u32,
            role: RSF_GAME_RENDER_FINAL_SCENE,
            flags: PRIMARY,
            session_id: 17,
            source_frame_id: 2,
            viewport_key: 3,
            submission_id: 4,
            view_key: 5,
            scope_id: 6,
            output_rect: [0, 0, 128, 64],
            scene_surface: surface,
            ..unsafe { std::mem::zeroed() }
        };
        let window = Pass {
            role: RSF_GAME_RENDER_WINDOW,
            window_key: 8,
            swapchain: surface,
            ..pass
        };
        let colliding = Pass {
            source_frame_id: pass.source_frame_id + CAPACITY as u64,
            ..pass
        };
        let bind = Pass {
            role: RSF_GAME_RENDER_TEXTURE_BINDING,
            sampled_texture: surface,
            ..window
        };
        unsafe {
            assert_eq!(rsf_core_native_scene_pass(&pass, 1, resource), 1);
            assert_eq!(rsf_core_native_scene_pass(&colliding, 1, resource), 0);
            assert_eq!(
                rsf_core_native_scene_present_path(&window, backbuffer),
                RSF_NATIVE_SCENE_PRESENT_NONE
            );
            assert_eq!(rsf_core_native_scene_pass(&pass, 0, resource), 1);
            assert_eq!(
                rsf_core_native_scene_present_path(&window, backbuffer),
                RSF_NATIVE_SCENE_PRESENT_DIRECT
            );
            assert_eq!(rsf_core_native_scene_texture_binding(&bind, resource), 1);
            assert_eq!(
                rsf_core_native_scene_present_path(&window, backbuffer),
                RSF_NATIVE_SCENE_PRESENT_SAMPLED
            );
            assert_eq!(
                rsf_core_native_scene_texture_binding(
                    &Pass {
                        window_key: 9,
                        ..bind
                    },
                    resource
                ),
                0
            );
            assert_eq!(
                rsf_core_native_scene_present_path(&window, backbuffer),
                RSF_NATIVE_SCENE_PRESENT_NONE
            );
        }
        rsf_native_scene_reset();
    }
}
