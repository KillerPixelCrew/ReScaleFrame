// SPDX-License-Identifier: MIT
//! The AC7 fixture queues real render markers on its synthetic native command list.
use crate::d3d11_scope_fixture_abi::*;
use rsf_game_sys::*;
use std::ffi::c_void;
use std::ptr::{self, null_mut};

type Raw = *mut c_void;

// These plain C records contain no owned resources. The fixture retains each input.
fn source_copy(source: &rsf_game_render_pass) -> rsf_game_render_pass {
    unsafe { ptr::read(source) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_scope_begin(
    calls: &rsf_d3d11_scope_fixture_calls,
    scopes: *mut rsf_ac7_render_scopes,
    commands: Raw,
    source: &rsf_game_render_pass,
    frame: *mut *mut rsf_ac7_render_ticket,
) -> i32 {
    let mut pass = source_copy(source);
    pass.role = RSF_GAME_RENDER_FRAME;
    pass.family_key = 0;
    pass.view_key = 0;
    unsafe { (calls.open.unwrap())(scopes, commands, &pass, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_scope_scene(
    calls: &rsf_d3d11_scope_fixture_calls,
    scopes: *mut rsf_ac7_render_scopes,
    commands: Raw,
    source: &rsf_game_render_pass,
    submission: *mut *mut rsf_ac7_render_ticket,
) -> i32 {
    let mut pass = source_copy(source);
    pass.role = RSF_GAME_RENDER_SUBMISSION;
    unsafe { (calls.open.unwrap())(scopes, commands, &pass, submission) }
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn rsf_d3d11_fixture_scope_window(
    calls: &rsf_d3d11_scope_fixture_calls,
    scopes: *mut rsf_ac7_render_scopes,
    commands: Raw,
    source: &rsf_game_render_pass,
    submission: *mut rsf_ac7_render_ticket,
    backbuffer: Raw,
    hudless: Raw,
    window: Raw,
    chain: Raw,
    direct: u32,
    output: *mut *mut rsf_ac7_render_ticket,
) -> i32 {
    let open = calls.open.unwrap();
    let close = calls.close.unwrap();
    let mut final_pass = source_copy(source);
    final_pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    final_pass.scene_surface = if direct != 0 { backbuffer } else { hudless };
    let mut final_ticket = null_mut();
    if unsafe { open(scopes, commands, &final_pass, &mut final_ticket) } == 0
        || unsafe { close(final_ticket, commands) } == 0
        || unsafe { close(submission, commands) } == 0
    {
        return 0;
    }

    // Slate packets carry source and window ownership independently of the scene's view keys.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.session_id = source.session_id;
    pass.source_frame_id = source.source_frame_id;
    pass.viewport_key = source.viewport_key;
    pass.pass_key = source.source_frame_id;
    pass.flags = source.flags;
    pass.role = RSF_GAME_RENDER_WINDOW;
    pass.window_key = window as usize as u64;
    pass.rhi_viewport_key = 4;
    pass.swapchain = chain;
    if unsafe { open(scopes, commands, &pass, output) } == 0 {
        return 0;
    }
    if direct == 0 {
        pass.role = RSF_GAME_RENDER_TEXTURE_BINDING;
        pass.sampled_texture = hudless;
        pass.rhi_viewport_key = 0;
        pass.swapchain = null_mut();
        let mut binding = null_mut();
        if unsafe { open(scopes, commands, &pass, &mut binding) } == 0
            || unsafe { close(binding, commands) } == 0
        {
            return 0;
        }
    }
    1
}
