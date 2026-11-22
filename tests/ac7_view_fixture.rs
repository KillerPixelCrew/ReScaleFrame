// SPDX-License-Identifier: MIT
//! Captured View layout used by the native graph fixture's synthetic 2x2 input.
use rsf_ac7::sys as ac7_graph_patch_abi;
mod ac7_graph_replay;
mod ac7_graph_replay_abi;

use ac7_graph_patch_abi::*;
use std::ptr::null_mut;

const VIEW_BYTES: usize = 4096;

/// Writes the fixture directly into the caller's float-aligned storage.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_graph_fixture_view(output: *mut f32, size: usize) -> bool {
    if output.is_null() || size != VIEW_BYTES {
        return false;
    }
    let output = unsafe { std::slice::from_raw_parts_mut(output, VIEW_BYTES / size_of::<f32>()) };
    output.fill(0.0);
    let mut put = |offset: usize, values: &[f32]| {
        let first = offset / size_of::<f32>();
        output[first..first + values.len()].copy_from_slice(values);
    };
    // Rotated camera basis in Unreal's order, where Z is forward.
    put(
        0x0c0,
        &[
            0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    );
    // Reversed-Z infinite projection with jitter, followed by its full inverse.
    put(
        0x180,
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.125, -0.25, 0.0, 1.0, 0.0, 0.0, 0.1, 0.0,
        ],
    );
    put(
        0x1c0,
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0, -0.125, 0.25, 1.0, 0.0,
        ],
    );
    put(0x300, &[1.0, 0.0, 0.0]);
    put(0x310, &[0.0, 0.0, 1.0]);
    put(0x320, &[0.0, 1.0, 0.0]);
    let position = [260_968.45, 107_399.9, 21_228.2];
    put(0x370, &position);
    put(0x3a0, &position.map(|value| -value));
    put(
        0x6e0,
        &[
            1.0, -0.00009, 0.0, 0.0, 0.00003, 1.0, 0.0, 0.0, -0.02808, -0.20304, 1.0, 17.49332,
            -0.00001, 0.00003, 0.0, 1.0,
        ],
    );
    put(0x720, &[0.125, -0.25]);
    for offset in [0x7f0, 0x800] {
        put(offset, &[2.0, 2.0, 0.5, 0.5]);
    }
    true
}

unsafe extern "C" {
    fn rsf_ac7_hook_rva(hook: u32) -> u32;
    fn rsf_ac7_hook_role(hook: u32) -> u32;
    fn rsf_ac7_hook_wanted(hook: u32, diagnostics: bool) -> bool;
}

/// Native enum indices supplied in the fixture's declared order.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_graph_fixture_hooks(
    hooks: *const u32,
    count: usize,
    output_role: u32,
) -> bool {
    if hooks.is_null() || count != 10 {
        return false;
    }
    let hooks = unsafe { std::slice::from_raw_parts(hooks, count) };
    let rvas = [
        (0, 0x3a3f70),
        (1, 0xebf050),
        (2, 0x1ab0200),
        (4, 0x10991e0),
        (5, 0xf4c620),
        (6, 0xe31e90),
    ];
    let evidence = |index: usize| unsafe {
        !rsf_ac7_hook_wanted(hooks[index], false) && rsf_ac7_hook_wanted(hooks[index], true)
    };
    rvas.iter()
        .all(|&(index, expected)| unsafe { rsf_ac7_hook_rva(hooks[index]) == expected })
        && unsafe { rsf_ac7_hook_role(hooks[3]) == output_role }
        && evidence(7)
        && evidence(5)
        && !evidence(8)
        && !evidence(9)
}

/// The image is borrowed writable memory. The leaves exercise the production compare and writer.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_graph_fixture_velocity_patches(
    image: *mut u8,
    size: usize,
    leaves: *const rsf_ac7_code_leaves,
) -> bool {
    let Some(matches) = (unsafe { leaves.as_ref() }).and_then(|leaves| leaves.matches) else {
        return false;
    };
    if image.is_null() {
        return false;
    }
    let mut sites = [rsf_ac7_code_patch {
        rva: 0,
        size: 0,
        native: [0; RSF_AC7_SITE_BYTES as usize],
        replacement: [0; RSF_AC7_SITE_BYTES as usize],
    }; RSF_AC7_VELOCITY_PATCHES as usize];
    for (index, site) in sites.iter_mut().enumerate() {
        if !unsafe { rsf_ac7_velocity_patch(index as u32, site) }
            || site.size == 0
            || site.size as usize > site.native.len()
            || site.rva as usize + site.size as usize > size
        {
            return false;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                site.native.as_ptr(),
                image.add(site.rva as usize),
                site.size as usize,
            )
        };
    }
    let mut owner = rsf_ac7_code_patches {
        velocity_module: null_mut(),
        velocity_applied: 0,
        scene_precision_site: null_mut(),
    };
    let matches_all = |replacement: bool| {
        sites.iter().all(|site| {
            let expected = if replacement {
                &site.replacement
            } else {
                &site.native
            };
            unsafe {
                matches(
                    image.add(site.rva as usize),
                    expected.as_ptr(),
                    site.size as usize,
                )
            }
        })
    };
    let history_gate = unsafe { image.add(sites[2].rva as usize) };
    unsafe { history_gate.write(0xcc) };
    let refused = !unsafe { rsf_ac7_velocity_apply(&mut owner, image, leaves) };
    unsafe { history_gate.write(0x77) };
    if !refused || owner.velocity_applied != 0 || !matches_all(false) {
        return false;
    }
    if !unsafe { rsf_ac7_velocity_apply(&mut owner, image, leaves) }
        || !unsafe { rsf_ac7_velocity_apply(&mut owner, image, leaves) }
        || !matches_all(true)
    {
        return false;
    }
    unsafe { history_gate.write(0xcc) };
    let retained =
        !unsafe { rsf_ac7_velocity_restore(&mut owner, leaves) } && owner.velocity_applied == 3;
    unsafe { history_gate.write(0xeb) };
    let restored = unsafe { rsf_ac7_velocity_restore(&mut owner, leaves) };
    retained && restored && matches_all(false)
}
