// SPDX-License-Identifier: MIT
use super::super::*;
use crate::motion_device::{Owner, rsf_anomaly_motion_device_upload};
use crate::motion_device_abi::{RSF_MOTION_CORE, RSF_MOTION_PIXEL, RSF_MOTION_TESS};
use std::ffi::{CStr, c_void};

/// Uploads VS (and DS) b13 for a skinned mover through the production policy and packers.
#[allow(clippy::too_many_arguments)]
pub(super) fn model_upload(
    owner: &mut Owner,
    context: *mut c_void,
    view: &[f64; 16],
    projection: &[f64; 16],
    previous_bone: &[f64; 16],
    history: bool,
    tessellated: bool,
    skin: i32,
    extent: u32,
) -> Result<(), &'static CStr> {
    let camera = CameraSample {
        view: Mat4(*view),
        projection: Mat4(*projection),
    };
    let bone = Mat4(*previous_bone);
    let world = if skin == 0 { bone } else { Mat4::IDENTITY };
    let bones = [bone; CORE_MAX_BONES];
    let input = CoreDrawInput {
        family: CORE_MODEL,
        skin,
        in_visual: true,
        verdict: IDENTIFIED,
        kind: if skin == 0 {
            SINGLE_BONE
        } else {
            HARDWARE_SKINNED
        },
        context: true,
        camera_conflict: false,
        previous_camera: &camera,
        previous: DrawSnapshotView {
            world: &world,
            bones: if skin == 0 {
                PaletteView::default()
            } else {
                PaletteView {
                    data: bones.as_ptr(),
                    count: bones.len(),
                }
            },
        },
    };
    let Ok(decision) = (unsafe { decide_core(&input) }) else {
        return Err(c"policy accepts the skinned mover");
    };
    let (mut vertex, mut domain, mut pixel) = (
        MaybeUninit::uninit(),
        MaybeUninit::uninit(),
        MaybeUninit::uninit(),
    );
    if history {
        if unsafe {
            pack_core(
                &decision.previous_wvp,
                decision.bones.data,
                decision.bones.count,
                &decision.flags,
                &mut vertex,
            )
        }
        .is_err()
        {
            return Err(c"pack VS b13");
        }
        if tessellated
            && !rsf_anomaly_motion_pack_tess(
                &decision.previous_wv,
                &decision.previous_projection,
                true,
                &mut domain,
            )
        {
            return Err(c"pack DS b13");
        }
    } else {
        rsf_anomaly_motion_pack_core_invalid(&mut vertex);
        if tessellated {
            rsf_anomaly_motion_pack_tess_invalid(&mut domain);
        }
    }
    if !rsf_anomaly_motion_pack_pixel(
        &PixelInputs {
            viewport: [0., 0., extent as f32, extent as f32],
            jitter: [0.; 4],
        },
        &mut pixel,
    ) {
        return Err(c"pack PS b13");
    }
    let mut upload = |kind, bytes: &[u8]| {
        rsf_anomaly_motion_device_upload(
            Some(&mut *owner),
            context,
            kind,
            bytes.as_ptr(),
            bytes.len(),
        ) != 0
    };
    if !upload(RSF_MOTION_CORE, unsafe { vertex.assume_init_ref() })
        || (tessellated && !upload(RSF_MOTION_TESS, unsafe { domain.assume_init_ref() }))
        || !upload(RSF_MOTION_PIXEL, unsafe { pixel.assume_init_ref() })
    {
        return Err(c"b13 uploads");
    }
    Ok(())
}
