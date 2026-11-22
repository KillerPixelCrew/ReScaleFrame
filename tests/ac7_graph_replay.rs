// SPDX-License-Identifier: MIT
//! Synthetic AC7 engine records. Native leaves drive the actual typed engine paths.

use super::ac7_graph_replay_abi::*;
use crate::sys::rsf_ac7_truesky_patches;
use std::ffi::{CStr, c_void};
use std::ptr::from_mut;

fn put<const N: usize>(bytes: &mut [u8], offset: usize, value: [u8; N]) {
    bytes[offset..offset + N].copy_from_slice(&value);
}

// The fixtures store x64 engine addresses as integers before the native leaf reads them.
fn address(bytes: &mut [u8]) -> u64 {
    bytes.as_mut_ptr().expose_provenance() as u64
}

/// The writable module and the borrowed callbacks live for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_graph_replay_camera(
    module: *mut u8,
    size: usize,
    screen: rsf_ac7_graph_screen_fn,
    check: rsf_ac7_graph_check_fn,
) {
    let (Some(screen), Some(check)) = (screen, check) else {
        return;
    };
    if module.is_null() || size != 0x3cd0000 {
        unsafe { check(false, c"invalid native camera fixture image".as_ptr()) };
        return;
    }
    let mut engine = [0; 0x728];
    let mut client = [0; 0x90];
    let mut world = [0; 0xf8];
    let mut instance = [0; 0x48];
    let mut player = [0; 0x38];
    let mut controller = [0; 0x408];
    let mut pawn = [0; 0xe18];
    let mut camera = [0; 0x1ca0];
    let mut level = [0; 0x308];
    let mut settings = [0; 0x4f0];
    let mut actor = [0; 0x18];
    let mut plane_class = [0; 0xa0];
    let mut camera_class = [0; 0xa0];
    let plane_class_address = address(&mut plane_class);
    let camera_class_address = address(&mut camera_class);
    let mut plane_base = plane_class_address + 0x88;
    let mut camera_base = camera_class_address + 0x88;
    let mut player_pointer = address(&mut player);
    let player_address = player_pointer;
    let pawn_address = address(&mut pawn);
    let actor_address = address(&mut actor);
    {
        let image = unsafe { std::slice::from_raw_parts_mut(module, size) };
        put(image, 0x3cbbc28, address(&mut engine).to_le_bytes());
        put(image, 0x3a84660, plane_class_address.to_le_bytes());
        put(image, 0x3cc0150, camera_class_address.to_le_bytes());
    }
    put(
        &mut plane_class,
        0x88,
        (from_mut(&mut plane_base).expose_provenance() as u64).to_le_bytes(),
    );
    put(
        &mut camera_class,
        0x88,
        (from_mut(&mut camera_base).expose_provenance() as u64).to_le_bytes(),
    );
    put(&mut engine, 0x720, address(&mut client).to_le_bytes());
    put(&mut client, 0x80, address(&mut world).to_le_bytes());
    put(&mut client, 0x88, address(&mut instance).to_le_bytes());
    put(
        &mut instance,
        0x38,
        (from_mut(&mut player_pointer).expose_provenance() as u64).to_le_bytes(),
    );
    put(&mut instance, 0x40, 1_i32.to_le_bytes());
    put(&mut player, 0x30, address(&mut controller).to_le_bytes());
    put(&mut controller, 0x370, pawn_address.to_le_bytes());
    put(&mut controller, 0x400, address(&mut camera).to_le_bytes());
    put(&mut world, 0x30, address(&mut level).to_le_bytes());
    put(&mut level, 0x300, address(&mut settings).to_le_bytes());
    put(&mut pawn, 0x10, plane_class_address.to_le_bytes());
    pawn[0xe12] = 1;
    put(&mut camera, 0xfb0, pawn_address.to_le_bytes());

    let (mut reset, mut why, mut owner) = (0, 0, 0);
    let verdict = |ok, message: &CStr| unsafe { check(ok, message.as_ptr()) };
    verdict(
        unsafe { screen(&mut reset, &mut why, &mut owner) } == RSF_SCREEN_FLIGHT,
        c"normal possessed-plane view remains flight",
    );
    put(&mut actor, 0x10, camera_class_address.to_le_bytes());
    put(&mut camera, 0xfb0, actor_address.to_le_bytes());
    pawn[0xe12] = 0;
    verdict(
        unsafe { screen(&mut reset, &mut why, &mut owner) } == RSF_SCREEN_REPLAY
            && owner == actor_address,
        c"rendered cinematic camera is eligible without pawn camera input",
    );
    camera[0x1c99] = 4;
    reset = 0;
    verdict(
        unsafe { screen(&mut reset, &mut why, &mut owner) } == RSF_SCREEN_REPLAY && reset == 1,
        c"native cinematic cuts retain temporal reset",
    );
    put(&mut camera, 0x15a0, actor_address.to_le_bytes());
    verdict(
        unsafe { screen(&mut reset, &mut why, &mut owner) } == RSF_SCREEN_UNKNOWN,
        c"pending camera transition still refuses FG",
    );
    put(&mut camera, 0x15a0, 0_u64.to_le_bytes());
    put(&mut settings, 0x4e8, player_address.to_le_bytes());
    verdict(
        unsafe { screen(&mut reset, &mut why, &mut owner) } == RSF_SCREEN_UNKNOWN,
        c"paused cinematics still refuse FG",
    );
    put(&mut settings, 0x4e8, 0_u64.to_le_bytes());
    put(&mut pawn, 0x10, 0_u64.to_le_bytes());
    verdict(
        unsafe { screen(&mut reset, &mut why, &mut owner) } == RSF_SCREEN_UNKNOWN,
        c"generic camera without mission plane does not become gameplay",
    );
}

fn load_word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn normal_tag(word: u32) -> u32 {
    0x2000_0000 | (word & 0x1fff_ffff) | ((word & 0x2000_0000) << 2)
}

/// The replay supplies native detours with their original function signatures.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_graph_replay_particles(replay: *const rsf_ac7_graph_particle_replay) {
    let Some(replay) = (unsafe { replay.as_ref() }) else {
        return;
    };
    let Some(check) = replay.check else {
        return;
    };
    let verdict = |ok, message: &CStr| unsafe { check(ok, message.as_ptr()) };
    let (Some(tag_beam), Some(tag_trail), Some(beam_originals), Some(trail_originals)) = (
        replay.tag_beam,
        replay.tag_trail,
        replay.beam_originals,
        replay.trail_originals,
    ) else {
        verdict(false, c"missing native particle replay callback");
        return;
    };
    let mut source = [0; 0x160];
    let mut fill = [0; 0xa0];
    let mut particles = [0; 512];
    let mut vertices: [u8; 1600] = std::array::from_fn(|i| i as u8);
    let native = vertices;
    let particle_address = address(&mut particles);
    put(&mut source, 0x44, 2_i32.to_le_bytes());
    put(&mut source, 0x48, 256_i32.to_le_bytes());
    put(&mut source, 0x60, particle_address.to_le_bytes());
    put(&mut source, 0x12c, 128_i32.to_le_bytes());
    put(&mut source, 0x154, 2_i32.to_le_bytes());
    put(&mut fill, 0x88, 20_i32.to_le_bytes());
    put(&mut fill, 0x8c, 80_i32.to_le_bytes());
    put(&mut fill, 0x90, address(&mut vertices).to_le_bytes());
    let (first, second) = ((3_u32 << 20) | 55, (12_u32 << 20) | 99);
    put(&mut particles, 0x5c, first.to_le_bytes());
    put(&mut particles, 256 + 0x5c, second.to_le_bytes());
    put(&mut particles, 128 + 0x58, 2_i32.to_le_bytes());
    put(&mut particles, 256 + 128 + 0x58, 4_i32.to_le_bytes());
    let beam_source = source.as_mut_ptr().cast::<c_void>();
    verdict(
        unsafe { tag_beam(beam_source, fill.as_mut_ptr().cast()) },
        c"tag exact native beam ranges across particles and sheets",
    );
    for i in 0..20 {
        let (pairs, start, counter) = if i < 8 { (2, 0, first) } else { (3, 8, second) };
        let point = (i - start) / 2;
        let (sheet, sample) = (point / pairs, point % pairs);
        let start = i as usize * 80;
        let vertex = &vertices[start..start + 80];
        verdict(
            load_word(vertex, 12) == normal_tag(counter | ((sheet & 3) << 26))
                && load_word(vertex, 28) == normal_tag(counter | ((sheet >> 2) << 26))
                && load_word(vertex, 44) == normal_tag(sample | ((pairs - 1) << 12)),
            c"beam counter/sheet/sample tag follows producer ordering",
        );
        verdict(
            [0..12, 16..28, 32..44, 48..80]
                .into_iter()
                .all(|range| vertex[range.clone()] == native[start..start + 80][range]),
            c"beam tagging preserves all native geometry/material fields",
        );
    }
    put(&mut fill, 0x88, 19_i32.to_le_bytes());
    verdict(
        !unsafe { tag_beam(beam_source, fill.as_mut_ptr().cast()) },
        c"mismatched native beam count refuses partial identity writes",
    );
    verdict(
        f32::from_bits(load_word(&vertices, 44)) == 0.0,
        c"refused beam fill invalidates stale pooled tags",
    );
    put(&mut fill, 0x88, 20_i32.to_le_bytes());
    verdict(
        unsafe { beam_originals(replay.owner, beam_source, fill.as_mut_ptr().cast()) },
        c"all native beam producer detours preserve the original call and return",
    );

    let mut trail_source = [0; 0x150];
    let mut particle_indices = [0_u16, 1];
    put(&mut trail_source, 0x64, 2_i32.to_le_bytes());
    put(&mut trail_source, 0x68, 256_i32.to_le_bytes());
    put(&mut trail_source, 0x80, particle_address.to_le_bytes());
    put(
        &mut trail_source,
        0x88,
        (particle_indices.as_mut_ptr().expose_provenance() as u64).to_le_bytes(),
    );
    put(&mut trail_source, 0x138, 128_i32.to_le_bytes());
    put(&mut fill, 0x88, 8_i32.to_le_bytes());
    let (head, tail) = (0x4000_0001_u32, 0x2000_3fff_u32);
    put(&mut particles, 128, head.to_le_bytes());
    put(&mut particles, 256 + 128, tail.to_le_bytes());
    put(&mut particles, 256 + 128 + 0x1c, 3_i32.to_le_bytes());
    let trail_source = trail_source.as_mut_ptr().cast::<c_void>();
    verdict(
        unsafe { tag_trail(trail_source, fill.as_mut_ptr().cast(), 1) },
        c"tag linked trail endpoints and interpolated parent pairs",
    );
    verdict(
        load_word(&vertices, 2 * 80 + 12) == normal_tag(second | (1 << 28)),
        c"trail interpolated point retains current particle counter",
    );
    verdict(
        load_word(&vertices, 2 * 80 + 28) == normal_tag(first),
        c"trail interpolation identifies its prior linked particle",
    );
    verdict(
        load_word(&vertices, 2 * 80 + 44) == normal_tag(2 | (3 << 12)),
        c"trail sample fraction follows native reverse interpolation loop",
    );
    put(&mut particles, 256 + 128, head.to_le_bytes());
    verdict(
        !unsafe { tag_trail(trail_source, fill.as_mut_ptr().cast(), 2) },
        c"cyclic trail payload refuses identity production",
    );
    put(&mut particles, 256 + 128, tail.to_le_bytes());
    verdict(
        unsafe { trail_originals(replay.owner, trail_source, fill.as_mut_ptr().cast()) },
        c"native ribbon/animated-trail detours preserve original calls and returns",
    );
}

/// Seeds the synthetic plugin call window and checks the native allocation and restore effects.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_graph_replay_truesky(
    image: *mut u8,
    size: usize,
    owner: *mut c_void,
    patches: *const rsf_ac7_truesky_patches,
    handler: *mut c_void,
    apply: rsf_ac7_graph_sky_apply_fn,
    restore: rsf_ac7_graph_sky_restore_fn,
    check: rsf_ac7_graph_check_fn,
) {
    let (Some(apply), Some(restore), Some(check)) = (apply, restore, check) else {
        return;
    };
    let verdict = |ok, message: &CStr| unsafe { check(ok, message.as_ptr()) };
    const CALL: usize = 0xabc6b;
    const BEFORE: [u8; 16] = [
        0x49, 0x8b, 0x8e, 0x28, 0x02, 0x00, 0x00, 0x45, 0x8b, 0xc4, 0x49, 0x8b, 0xd7, 0x48, 0x8b,
        0x01,
    ];
    const DRAW: [u8; 16] = [
        0xff, 0x90, 0x30, 0x01, 0x00, 0x00, 0x49, 0x8b, 0x9e, 0x60, 0x02, 0x00, 0x00, 0x48, 0x8d,
        0x0d,
    ];
    if image.is_null() || size < CALL + DRAW.len() || owner.is_null() || patches.is_null() {
        verdict(false, c"invalid synthetic TrueSky plugin image");
        return;
    }
    {
        let bytes = unsafe { std::slice::from_raw_parts_mut(image, size) };
        put(bytes, CALL - BEFORE.len(), BEFORE);
        put(bytes, CALL, DRAW);
    }
    let call = unsafe { image.add(CALL) };
    unsafe { apply(owner) };
    let installed = unsafe { patches.read() };
    verdict(
        installed.resolution_refused
            && installed.depth_call.slot.is_null()
            && installed.depth_branch.is_null()
            && installed.motion_call.call == call,
        c"depth route refuses without its code and effect; the composite relay installs",
    );
    {
        let patched = unsafe { std::slice::from_raw_parts(call, DRAW.len()) };
        verdict(
            patched[..2] == [0xff, 0x15] && patched[6..] == DRAW[6..],
            c"relay call is patched in over six bytes",
        );
        let relative = i32::from_le_bytes(patched[2..6].try_into().unwrap());
        let slot = installed.motion_call.slot;
        verdict(
            !slot.is_null()
                && call.wrapping_offset(6 + relative as isize).cast() == slot
                && unsafe { slot.cast::<*mut c_void>().read_unaligned() } == handler,
            c"relay slot holds the handler",
        );
    }
    let restored = unsafe { restore(owner) };
    let final_state = unsafe { patches.read() };
    let live = unsafe { std::slice::from_raw_parts(call, DRAW.len()) };
    verdict(
        restored
            && live == DRAW
            && final_state.motion_call.call.is_null()
            && final_state.motion_call.slot.is_null(),
        c"restore puts back the native call and releases the slot",
    );
}
