// SPDX-License-Identifier: MIT
use crate::com_ref;
use crate::sr_session::has_capacity;
use crate::sys::{
    RSF_NATIVE_SCENE_PRESENT_DIRECT, RSF_NATIVE_SCENE_PRESENT_NONE,
    RSF_NATIVE_SCENE_PRESENT_SAMPLED, rsf_native_scene_identity as Scene,
};
use rsf_game_sys::{
    RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_PRIMARY,
    RSF_GAME_RENDER_TEXTURE_BINDING, RSF_GAME_RENDER_WINDOW, rsf_game_render_pass as Pass,
};
use std::cell::RefCell;
use std::ffi::c_void;
use windows::Win32::Graphics::{
    Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Texture2D},
    Dxgi::IDXGISwapChain,
};
use windows::core::Interface;

const CAPACITY: usize = 64;
const EMPTY: Scene = unsafe { std::mem::zeroed() };
const PRIMARY: u32 = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;

thread_local! {
    // Only identity values survive the callback. The registry owns no COM references.
    static SCENES: RefCell<[Scene; CAPACITY]> = const { RefCell::new([EMPTY; CAPACITY]) };
}

fn slot(source: u64) -> usize {
    (source % CAPACITY as u64) as usize
}

fn same_source(scene: &Scene, pass: &Pass) -> bool {
    scene.session_id == pass.session_id && scene.source_frame_id == pass.source_frame_id
}

fn resource_identity(resource: *mut c_void) -> u64 {
    unsafe { com_ref::identity(resource) }.map_or(0, |identity| identity.as_raw() as usize as u64)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_scene_pass(pass: *const Pass, begin: u32) -> i32 {
    if !unsafe { has_capacity(pass) } {
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
                surface_key: resource_identity(pass.scene_surface),
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
            || scene.surface_key != resource_identity(pass.scene_surface)
        {
            scene.ambiguous = 1;
            return 0;
        }
        scene.complete = 1;
        1
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_scene_texture_binding(pass: *const Pass) -> i32 {
    if !unsafe { has_capacity(pass) } {
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
            || scene.surface_key != resource_identity(pass.sampled_texture)
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

pub(crate) fn find_window(window: &Pass) -> Option<Scene> {
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
pub(super) unsafe extern "C" fn rsf_native_scene_for_window(
    window: *const Pass,
    out: *mut Scene,
) -> i32 {
    if !unsafe { has_capacity(window) && has_capacity(out) } {
        return 0;
    }
    let Some(scene) = find_window(unsafe { &*window }) else {
        return 0;
    };
    unsafe { out.write(scene) };
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_scene_present_path(window: *const Pass) -> u32 {
    if !unsafe { has_capacity(window) } {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    }
    let window = unsafe { &*window };
    let Some(scene) = find_window(window) else {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    };
    unsafe { rsf_native_scene_classify_present(window, &scene) }
}

pub(super) unsafe extern "C" fn rsf_native_scene_classify_present(
    window: *const Pass,
    scene: *const Scene,
) -> u32 {
    if !unsafe { has_capacity(window) && has_capacity(scene) } {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    }
    let (window, scene) = unsafe { (&*window, &*scene) };
    if scene.texture_bound != 0 && scene.bound_window_key == window.window_key {
        return RSF_NATIVE_SCENE_PRESENT_SAMPLED;
    }
    if window.swapchain.is_null() || scene.surface_key == 0 {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    }
    // FSceneViewport can render straight into its backbuffer, and Slate then draws over it
    // without sampling the scene. Compare live COM identity with the final-pass identity.
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&window.swapchain) }) else {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    };
    let Ok(buffer) = (unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }) else {
        return RSF_NATIVE_SCENE_PRESENT_NONE;
    };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { buffer.GetDesc(&mut desc) };
    let full = [0, 0, desc.Width as i32, desc.Height as i32];
    if resource_identity(buffer.as_raw()) == scene.surface_key && scene.output_rect == full {
        return RSF_NATIVE_SCENE_PRESENT_DIRECT;
    }
    RSF_NATIVE_SCENE_PRESENT_NONE
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_native_scene_reset() {
    SCENES.with(|scenes| {
        if let Ok(mut scenes) = scenes.try_borrow_mut() {
            *scenes = [EMPTY; CAPACITY];
        }
    });
}
