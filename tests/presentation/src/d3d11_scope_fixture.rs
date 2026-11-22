// SPDX-License-Identifier: MIT
//! The AC7 fixture queues real render markers on its synthetic native command list.
use crate::d3d11_marker::append_native;
use crate::d3d11_scope_fixture_abi::*;
use crate::present_fixture_loops_abi::*;
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
    direct: u32,
    expected: &mut u32,
) -> i32 {
    *expected = if direct != 0 {
        RSF_NATIVE_SCENE_PRESENT_DIRECT
    } else {
        RSF_NATIVE_SCENE_PRESENT_SAMPLED
    };
    let mut pass = source_copy(source);
    pass.role = RSF_GAME_RENDER_FRAME;
    pass.family_key = 0;
    pass.view_key = 0;
    unsafe { (calls.open.unwrap())(scopes, commands, &pass, frame) }
}

unsafe fn scope_scene(
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

#[allow(clippy::too_many_arguments)]
unsafe fn scope_window(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_record(
    packet: &rsf_ac7_fixture_commands,
    source: &rsf_game_render_pass,
    direct: u32,
    late_simulation: u32,
) -> i32 {
    let scopes = unsafe { &*packet.scopes_api };
    let mut submission = null_mut();
    if unsafe {
        scope_scene(
            scopes,
            packet.scopes,
            packet.commands,
            source,
            &mut submission,
        )
    } == 0
    {
        return 0;
    }
    unsafe {
        (*packet.frame).reset = u32::from(source.source_frame_id == 2);
        append_native(packet.commands, packet.evaluate);
    }
    // The native callback keeps both COM references through guarded command execution.
    let mut backbuffer = null_mut();
    if unsafe { (packet.draw.unwrap())(packet.draw_user, &mut backbuffer) } == 0 {
        return 0;
    }
    let mut window = null_mut();
    if unsafe {
        scope_window(
            scopes,
            packet.scopes,
            packet.commands,
            source,
            submission,
            backbuffer,
            packet.hudless,
            packet.window,
            packet.chain,
            direct,
            &mut window,
        )
    } == 0
    {
        return 0;
    }
    let close = scopes.close.unwrap();
    unsafe { append_native(packet.commands, packet.present) };
    if unsafe { close(window, packet.commands) } == 0 {
        return 0;
    }
    if late_simulation != 0 {
        unsafe { append_native(packet.commands, packet.simulation_end) };
    }
    if unsafe { close(packet.frame_ticket, packet.commands) } == 0 {
        return 0;
    }
    // Keep both leases through the synchronous, exception-guarded drain of the native list.
    unsafe { (packet.execute.unwrap())(packet.commands) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_queued_pass(
    context: Raw,
    expected: u32,
    pass: &rsf_game_render_pass,
    begin: u32,
) -> i32 {
    let accepted = unsafe {
        match pass.role {
            RSF_GAME_RENDER_SUBMISSION => {
                rsf_native_fg_submission(pass, begin);
                true
            }
            RSF_GAME_RENDER_FRAME => {
                rsf_native_fg_frame(pass, begin);
                true
            }
            RSF_GAME_RENDER_FINAL_SCENE => {
                let accepted = rsf_native_scene_pass(pass, begin) != 0;
                if begin == 0 && !context.is_null() {
                    rsf_native_fg_final(context, pass);
                }
                accepted
            }
            RSF_GAME_RENDER_TEXTURE_BINDING if begin == 0 => {
                rsf_native_scene_texture_binding(pass) != 0
            }
            RSF_GAME_RENDER_WINDOW => {
                let mut accepted = true;
                if begin == 0 {
                    rsf_native_fg_window_end(pass);
                } else {
                    let expected = if expected == RSF_NATIVE_SCENE_PRESENT_DIRECT {
                        expected
                    } else {
                        RSF_NATIVE_SCENE_PRESENT_NONE
                    };
                    accepted &= rsf_native_scene_present_path(pass) == expected;
                    let mut stale = source_copy(pass);
                    stale.source_frame_id = stale.source_frame_id.wrapping_add(1);
                    accepted &=
                        rsf_native_scene_present_path(&stale) == RSF_NATIVE_SCENE_PRESENT_NONE;
                    let mut other = source_copy(pass);
                    other.viewport_key = other.viewport_key.wrapping_add(1);
                    accepted &=
                        rsf_native_scene_present_path(&other) == RSF_NATIVE_SCENE_PRESENT_NONE;
                }
                accepted & (rsf_native_window_scope(pass, begin) != 0)
            }
            _ => true,
        }
    };
    i32::from(accepted)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_window_source(
    chain: Raw,
    expected: u32,
    source: &mut u64,
) -> i32 {
    let mut window: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    window.struct_size = size_of::<rsf_game_render_pass>() as u32;
    if unsafe { rsf_native_window_match(chain, &mut window) } == 0
        || unsafe { rsf_native_scene_present_path(&window) } != expected
    {
        return 0;
    }
    *source = window.source_frame_id;
    1
}
