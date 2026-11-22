// SPDX-License-Identifier: MIT
use super::*;
use windows::core::Interface;

unsafe fn capture_batch(
    context: *mut c_void,
    ticket: u64,
    images: impl IntoIterator<Item = Texture<'static>>,
) {
    for image in images {
        unsafe { capture_texture(context, image, |capture| capture.ticket == ticket, true) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_input_dump_pass(
    context: *mut c_void,
    input: *const rsf_game_render_pass,
    begin: u32,
) {
    let Some(pass) = (unsafe { checked(input) }) else {
        return;
    };
    let begin = begin != 0;
    let Some(ticket) = owner(|owner| {
        let capture = owner.capture.as_mut()?;
        let selecting = pass.role == RSF_GAME_RENDER_SR
            && begin
            && !capture.has_target()
            && pass.session_id != 0
            && pass.source_frame_id != 0
            && pass.flags & RSF_GAME_RENDER_PRIMARY != 0;
        if selecting {
            capture.identity = Some(pass.into());
            capture.legacy_camera = Some(pass.camera);
        }
        let matched = capture.matches_pass(pass);
        if pass.role == RSF_GAME_RENDER_SR {
            capture.in_sr = begin && matched;
        }
        let ticket = matched.then_some(capture.ticket);
        if selecting {
            owner.publish("capturing");
        }
        ticket
    }) else {
        return;
    };
    match (pass.role, begin) {
        (RSF_GAME_RENDER_UI_COMPOSITE, true) => unsafe {
            capture_batch(
                context,
                ticket,
                [
                    Texture::new(
                        "hudless_color",
                        "Tonemapped scene immediately before native HUD composition",
                        pass.color_input,
                        HDR,
                        1.0,
                    ),
                    Texture::new(
                        "hud_color",
                        "Native HUD raster RGB; source alpha is preserved in raw data",
                        pass.ui_input,
                        RAW,
                        1.0,
                    ),
                    Texture::new(
                        "hud_alpha",
                        "Stored native HUD alpha; translucent/additive producers can leave it zero",
                        pass.ui_input,
                        ALPHA,
                        1.0,
                    ),
                ],
            )
        },
        (RSF_GAME_RENDER_UI_COMPOSITE, false) => {
            let source = if pass.color_output_readable.is_null() {
                pass.color_output
            } else {
                pass.color_output_readable
            };
            unsafe {
                capture_batch(
                    context,
                    ticket,
                    [Texture::new(
                        "hud_composed_color",
                        "Native HUD composition output",
                        source,
                        HDR,
                        1.0,
                    )],
                )
            };
        }
        (RSF_GAME_RENDER_FINAL_SCENE, false) => unsafe {
            capture_batch(
                context,
                ticket,
                [Texture::new(
                    "fg_scene_color",
                    "Actual completed FG scene input; game UI can already be composed here",
                    pass.scene_surface,
                    HDR,
                    1.0,
                )],
            )
        },
        _ => {}
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_input_dump_texture(
    context: *mut c_void,
    name: *const c_char,
    description: *const c_char,
    source: *mut c_void,
    view: u32,
    scale: f32,
) {
    if let (Some(name), Some(description)) =
        (unsafe { string(name) }, unsafe { string(description) })
    {
        unsafe {
            capture_texture(
                context,
                Texture::new(name, description, source, view, scale),
                |capture| capture.in_sr,
                true,
            )
        };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_input_dump_active() -> i32 {
    i32::from(owner(|owner| {
        owner.capture.as_ref().is_some_and(|capture| capture.in_sr)
    }))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_input_dump_sr(
    context: *mut c_void,
    input: *const rsf_dlss_frame,
    backend: u32,
) {
    let Some(frame) = (unsafe { checked(input) }) else {
        return;
    };
    let Some(ticket) = owner(|owner| {
        owner
            .capture
            .as_mut()
            .filter(|capture| capture.in_sr)
            .map(|capture| {
                capture.backend = backend;
                capture.ticket
            })
    }) else {
        return;
    };
    let (color_description, color_view) = if frame.color_encoded != 0 {
        ("SR backend colour: bounded display-range transport", RAW)
    } else {
        ("SR backend colour: jittered linear HDR", HDR)
    };
    let (motion_view, motion_scale) = if frame.camera_motion_included != 0 {
        (DENSE_MOTION, 1.0 / 32.0)
    } else {
        (DECODED_MOTION, 8.0)
    };
    unsafe {
        capture_batch(
            context,
            ticket,
            [
                Texture::new(
                    "sr_color",
                    color_description,
                    frame.color_in,
                    color_view,
                    1.0,
                ),
                Texture::new(
                    "sr_depth",
                    "SR backend device depth",
                    frame.depth,
                    DEPTH,
                    1.0,
                ),
                Texture::new(
                    "sr_motion",
                    "SR backend motion; scales and camera inclusion in sr_constants.json",
                    frame.motion,
                    motion_view,
                    motion_scale,
                ),
                Texture::new(
                    "sr_exposure",
                    "Exposure texture supplied to the SR backend; absent when colour transport uses auto exposure",
                    frame.exposure,
                    SCALAR,
                    1.0,
                ),
            ],
        )
    };
    owner(|owner| {
        if let Some(capture) = owner
            .capture
            .as_mut()
            .filter(|capture| capture.ticket == ticket)
        {
            let written = metadata::sr_constants(capture, frame, backend).is_ok();
            if capture.record.is_none() {
                capture.constants = written;
            }
        }
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_input_dump_sr_result(result: i32) {
    owner(|owner| {
        if let Some(capture) = owner.capture.as_mut().filter(|capture| capture.in_sr) {
            capture.sr_result = Some(result);
        }
    });
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_input_dump_fg(
    context: *mut c_void,
    pass: *const rsf_game_render_pass,
    input: *const rsf_dlss_frame,
) {
    let (Some(pass), Some(frame)) = (unsafe { checked(pass) }, unsafe { checked(input) }) else {
        return;
    };
    if let Some(ticket) = owner(|owner| {
        owner
            .capture
            .as_ref()
            .filter(|capture| capture.matches_pass(pass))
            .map(|capture| capture.ticket)
    }) {
        unsafe {
            capture_batch(
                context,
                ticket,
                [
                    Texture::new(
                        "fg_depth",
                        "Normalized depth offered to FG for this source frame",
                        frame.depth,
                        DEPTH,
                        1.0,
                    ),
                    Texture::new(
                        "fg_motion",
                        "Dense previous-minus-current pixel motion offered to FG",
                        frame.motion,
                        DENSE_MOTION,
                        1.0 / 32.0,
                    ),
                ],
            )
        };
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_input_dump_fg_prepare(
    pass: *const rsf_game_render_pass,
    input: *const rsf_fg_frame,
    result: i32,
) {
    let (Some(pass), Some(frame)) = (unsafe { checked(pass) }, unsafe { checked(input) }) else {
        return;
    };
    let Some(record) = (unsafe { record(frame.record) }) else {
        return;
    };
    // The FG record belongs to its own resource pool. Join its actual native pass
    // identity and retain the FG generation in the metadata.
    if pass.source_frame_id == 0
        || record.session_id != pass.session_id
        || record.frame_id != pass.source_frame_id
        || record.view_id != pass.view_key as u32
    {
        return;
    }
    owner(|owner| {
        if let Some(capture) = owner
            .capture
            .as_mut()
            .filter(|capture| capture.matches_pass(pass))
        {
            capture.fg_result = Some(result);
            capture.fg_constants =
                metadata::fg_constants(&capture.bundle, frame, record, result).is_ok();
        }
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_input_dump_present(
    chain: *mut c_void,
    context: *mut c_void,
    input: *const rsf_game_render_pass,
) {
    let now = clock();
    let pass = unsafe { checked(input) };
    let (configured, ticket) = owner(|owner| {
        if owner.directory.is_none() {
            return (false, None);
        }
        let Some(capture) = &mut owner.capture else {
            return (true, None);
        };
        if let Some(pass) = pass.filter(|pass| capture.matches_window(pass)) {
            if capture.window.is_none() {
                capture.window = Some(pass.into());
                return (true, Some(capture.ticket));
            }
        } else if now.tick.saturating_sub(capture.started) > 15000 {
            owner.finish("incomplete_no_matched_frame");
        }
        (true, None)
    });
    if let Some(ticket) = ticket
        && let Some(buffer) = unsafe { com_ref::owned(native::backbuffer(chain)) }
    {
        unsafe {
            capture_batch(
                context,
                ticket,
                [Texture::new(
                    "final_color",
                    "Final real frame with game HUD, before the RSF overlay and interpolation",
                    buffer.as_raw(),
                    HDR,
                    1.0,
                )],
            )
        };
    }
    if configured {
        owner(|owner| owner.poll(&now));
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_input_dump_presented(input: *const rsf_game_render_pass, result: i32) {
    let Some(pass) = (unsafe { checked(input) }) else {
        return;
    };
    owner(|owner| {
        if owner
            .capture
            .as_ref()
            .is_some_and(|capture| capture.window == Some(pass.into()))
        {
            owner.complete(result);
        }
    });
}
