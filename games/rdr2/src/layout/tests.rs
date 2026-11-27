// SPDX-License-Identifier: MIT
use super::*;

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
}

#[test]
fn fsr_description_normalizes_to_the_dlss_descriptor() {
    let mut bytes = [0u8; fsr::DESCRIPTION_BYTES];
    put(&mut bytes, fsr::COMMAND_LIST, &0x1000usize.to_le_bytes());
    let records = [
        (fsr::COLOR, 0x2000usize, fsr::STATE_COMPUTE_READ),
        (fsr::DEPTH, 0x3000, fsr::STATE_COMPUTE_READ),
        (fsr::MOTION, 0x4000, fsr::STATE_COMPUTE_READ),
        (fsr::OUTPUT, 0x5000, fsr::STATE_UNORDERED_ACCESS),
    ];
    for (offset, resource, state) in records {
        put(&mut bytes, offset, &resource.to_le_bytes());
        put(
            &mut bytes,
            offset + fsr::RESOURCE_STATE,
            &state.to_le_bytes(),
        );
    }
    put(&mut bytes, fsr::JITTER, &0.25f32.to_le_bytes());
    put(&mut bytes, fsr::JITTER + 4, &(-0.5f32).to_le_bytes());
    put(&mut bytes, fsr::MOTION_SCALE, &1706.0f32.to_le_bytes());
    put(&mut bytes, fsr::MOTION_SCALE + 4, &1066.0f32.to_le_bytes());
    put(&mut bytes, fsr::RENDER_SIZE, &1706u32.to_le_bytes());
    put(&mut bytes, fsr::RENDER_SIZE + 4, &1066u32.to_le_bytes());
    put(&mut bytes, fsr::PRE_EXPOSURE, &1.0f32.to_le_bytes());
    bytes[fsr::RESET] = 7;
    let dispatch = fsr_dispatch(&bytes);
    let d = &dispatch.descriptor;
    assert_eq!(dispatch.command_list as usize, 0x1000);
    assert_eq!(d.color as usize, 0x2000);
    assert_eq!(d.depth as usize, 0x3000);
    assert_eq!(d.motion as usize, 0x4000);
    assert_eq!(d.output as usize, 0x5000);
    assert!(d.exposure.is_null());
    assert_eq!(d.jitter, [0.25, -0.5]);
    assert_eq!(d.motion_scale, [1706.0, 1066.0]);
    assert_eq!((d.width, d.height, d.reset), (1706, 1066, 1));
    assert_eq!(d.pre_exposure, 1.0);
    assert!(d.origins.iter().all(|&origin| origin == 0));
    assert!(dispatch.states_valid);
    let npsr = d3d::NON_PIXEL_SHADER_RESOURCE;
    assert_eq!(
        dispatch.states,
        [npsr, npsr, npsr, d3d::UNORDERED_ACCESS, 0]
    );
}

#[test]
fn unknown_fsr_resource_state_refuses() {
    let mut bytes = [0u8; fsr::DESCRIPTION_BYTES];
    put(&mut bytes, fsr::DEPTH, &0x3000usize.to_le_bytes());
    put(
        &mut bytes,
        fsr::DEPTH + fsr::RESOURCE_STATE,
        &3u32.to_le_bytes(),
    );
    assert!(!fsr_dispatch(&bytes).states_valid);
    // A null resource's state is not consulted.
    put(&mut bytes, fsr::DEPTH, &0usize.to_le_bytes());
    assert!(fsr_dispatch(&bytes).states_valid);
}

#[test]
fn call_targets_decode_only_rel32_calls() {
    let (site, target) = (0x5c_aafc, 0x5f_1e08);
    let mut call = [0xe8, 0, 0, 0, 0];
    call[1..].copy_from_slice(&((target as i64 - site as i64 - 5) as i32).to_le_bytes());
    assert_eq!(call_target(site, &call), Some(target));
    call[0] = 0xe9;
    assert_eq!(call_target(site, &call), None);
}
