// SPDX-License-Identifier: MIT
use super::*;
use crate::texture_dump::{PackedRows, pixel_bytes};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_configure(
    directory: *const c_char,
    game: *const c_char,
    hash: *const c_char,
    log: rsf_backend_log_fn,
    user: *mut c_void,
) {
    let directory = unsafe { string(directory) }.map(PathBuf::from);
    let game = unsafe { string(game) }.unwrap_or_default().to_owned();
    let hash = unsafe { string(hash) }.unwrap_or_default().to_owned();
    owner(|owner| {
        owner.configure(
            directory,
            game,
            hash,
            Log {
                callback: log,
                user: user as usize,
            },
        )
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dump_request() {
    let now = clock();
    owner(|owner| {
        if owner.directory.is_none() {
            owner.poll(&now);
        }
        owner.request(&now);
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dump_cancel(session: u64) {
    owner(|owner| {
        if owner.capture.as_ref().is_some_and(|capture| {
            session == 0 || capture.session() == 0 || capture.session() == session
        }) {
            owner.finish("cancelled");
        }
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dump_poll() {
    let now = clock();
    owner(|owner| owner.poll(&now));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_begin_record(
    input: *const rsf_frame_record,
    backend: u32,
) -> i32 {
    let now = clock();
    owner(|owner| {
        owner.poll(&now);
        let Some(capture) = &mut owner.capture else {
            return 0;
        };
        let Some(frame) =
            (unsafe { record(input) }).filter(|frame| frame.session_id != 0 && frame.frame_id != 0)
        else {
            return 0;
        };
        if capture.has_target() {
            return i32::from(capture.wants(frame));
        }
        capture.record = Some(*frame);
        capture.backend = backend;
        capture.constants = metadata::frame(&capture.bundle, frame, backend).is_ok();
        owner.publish("capturing");
        1
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dump_wants(input: *const rsf_frame_record) -> i32 {
    i32::from(unsafe { record(input) }.is_some_and(|frame| {
        owner(|owner| {
            owner
                .capture
                .as_ref()
                .is_some_and(|capture| capture.wants(frame))
        })
    }))
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_dump_wants_frame(frame: u64) -> i32 {
    i32::from(owner(|owner| {
        owner
            .capture
            .as_ref()
            .and_then(|capture| capture.record)
            .is_some_and(|record| record.frame_id == frame)
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_current(output: *mut rsf_frame_record) -> i32 {
    let Some(selected) = owner(|owner| owner.capture.as_ref().and_then(|capture| capture.record))
    else {
        return 0;
    };
    if output.is_null()
        || (unsafe { output.cast::<u32>().read() } as usize) < size_of::<rsf_frame_record>()
    {
        return 0;
    }
    unsafe { output.write(selected) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dump_requested() -> i32 {
    i32::from(owner(|owner| owner.capture.is_some()))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dump_reserve(
    input: *const rsf_frame_record,
    name: *const c_char,
    description: *const c_char,
    view: u32,
    scale: f32,
) -> u64 {
    let (Some(frame), Some(name), Some(description)) =
        (unsafe { record(input) }, unsafe { string(name) }, unsafe {
            string(description)
        })
    else {
        return 0;
    };
    if !valid_name(name) || view > DENSE_MOTION || !scale.is_finite() {
        return 0;
    }
    owner(|owner| {
        let capture = owner
            .capture
            .as_mut()
            .filter(|capture| capture.wants(frame))?;
        capture.add_image(Image::new(
            name,
            description,
            view,
            scale,
            ImageState::Reserved,
        ))?;
        Some(capture.ticket)
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dump_failed(
    ticket: u64,
    name: *const c_char,
    result: i32,
) {
    let Some(name) = (unsafe { string(name) }) else {
        return;
    };
    owner(|owner| {
        let Some(capture) = &mut owner.capture else {
            return;
        };
        let Some(index) = capture.index(ticket, name) else {
            return;
        };
        let image = &mut capture.images[index];
        if image.state == ImageState::Reserved {
            let result = if result == 0 { INVALID } else { result };
            image.state = ImageState::Finished;
            image.encoding = Encoding {
                raw: result,
                preview: result,
            };
            owner.finish_ready();
        }
    });
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dump_pixels(
    ticket: u64,
    name: *const c_char,
    input: *const Pixels,
    api: u32,
    native_format: u32,
) -> i32 {
    let (Some(name), Some(pixels)) = (unsafe { string(name) }, unsafe { checked(input) }) else {
        return 0;
    };
    let Some(work) = owner(|owner| {
        let capture = owner.capture.as_mut()?;
        let index = capture.index(ticket, name)?;
        let image = &mut capture.images[index];
        if image.state != ImageState::Reserved {
            return None;
        }
        image.state = ImageState::Encoding;
        owner.work(index)
    }) else {
        return 0;
    };
    let view = preview_view(work.view, pixels.format, true);
    let mut encoding = Encoding::INVALID;
    let platform = platform();
    if let Some(rows) = unsafe { PackedRows::new(pixels, pixel_bytes(pixels.format)) }
        .filter(|rows| rows.packed_size() <= MAX_IMAGE_BYTES)
        && let Ok(prefix) = prefix(&work.prefix)
    {
        encoding.raw = match metadata::raw_pixels(
            &work.prefix,
            pixels,
            rows.iter(),
            rows.pitch(),
            api,
            native_format,
        ) {
            Ok(()) => 0,
            Err(_) => WRITE_FAILED,
        };
        encoding.preview =
            unsafe { (platform.preview_pixels)(pixels, &work.options(&prefix, view)) };
    }
    let result = ImageResult {
        surface: Surface {
            width: pixels.width,
            height: pixels.height,
            format: pixels.format,
        },
        api,
        native_format,
        view,
        encoding,
    };
    i32::from(owner(|owner| owner.finish_image(&work, result)))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_complete(input: *const rsf_frame_record, result: i32) {
    if let Some(frame) = unsafe { record(input) } {
        owner(|owner| {
            if owner
                .capture
                .as_ref()
                .is_some_and(|capture| capture.wants(frame))
            {
                owner.complete(result);
            }
        });
    }
}

// Preserve the native entry points. A frame record cannot identify a submitted batch.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_dump_gpu_submission(_: *const rsf_frame_record) -> u64 {
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dump_gpu_completed(_: u64, _: u32, _: *mut c_void) {}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dump_gpu_ready(_: u64, _: *const c_char, _: u32, _: *mut c_void) -> i32 {
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_texture_record(
    context: *mut c_void,
    input: *const rsf_frame_record,
    name: *const c_char,
    description: *const c_char,
    source: *mut c_void,
    view: u32,
    scale: f32,
) -> i32 {
    let (Some(frame), Some(name), Some(description)) =
        (unsafe { record(input) }, unsafe { string(name) }, unsafe {
            string(description)
        })
    else {
        return 0;
    };
    i32::from(unsafe {
        capture_texture(
            context,
            Texture::new(name, description, source, view, scale),
            |capture| capture.wants(frame),
            false,
        )
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_named_texture(
    context: *mut c_void,
    source: *mut c_void,
    request: *const NamedRequest,
    report: *mut NamedReport,
) -> i32 {
    let Some(request) = (unsafe { checked(request) }) else {
        return INVALID;
    };
    let (Some(directory), Some(name)) = (unsafe { string(request.directory) }, unsafe {
        string(request.name)
    }) else {
        return INVALID;
    };
    if context.is_null()
        || source.is_null()
        || directory.is_empty()
        || name.is_empty()
        || !request.scale.is_finite()
    {
        return INVALID;
    }
    if request.abi_version != 1 {
        return ABI_MISMATCH;
    }
    if !valid_name(name) || request.view > DENSE_MOTION {
        return INVALID;
    }
    let mut surface = Surface::default();
    let Some(texture) =
        (unsafe { ComRef::from_owned((platform().open_texture)(context, source, &mut surface)) })
    else {
        return INVALID;
    };
    let directory = Path::new(directory);
    if fs::create_dir_all(directory).is_err() {
        return WRITE_FAILED;
    }
    let Ok(prefix) = prefix(&directory.join(name)) else {
        return WRITE_FAILED;
    };
    let options = TextureOptions::new(
        &prefix,
        preview_view(request.view, surface.format, false),
        request.scale,
        Log::new(request.log, request.log_user),
    );
    let mut encoding = Encoding::INVALID;
    unsafe { (platform().encode_texture)(context, texture.as_ptr(), &options, 0, &mut encoding) };
    if encoding.preview == 0
        && !report.is_null()
        && unsafe { report.cast::<u32>().read() } as usize >= size_of::<NamedReport>()
    {
        unsafe {
            std::ptr::addr_of_mut!((*report).width).write(surface.width);
            std::ptr::addr_of_mut!((*report).height).write(surface.height);
            std::ptr::addr_of_mut!((*report).dxgi_format).write(surface.format);
        };
    }
    encoding.preview
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_pass_metadata(input: *const rsf_game_render_pass) {
    let Some(pass) = (unsafe { checked(input) }) else {
        return;
    };
    owner(|owner| {
        let Some(capture) = owner
            .capture
            .as_mut()
            .filter(|capture| capture.record.is_some() && capture.matches_pass(pass))
        else {
            return;
        };
        if capture
            .identity
            .is_some_and(|id| id.family != pass.family_key || id.scope != pass.scope_id)
        {
            return;
        }
        capture.identity = Some(pass.into());
        let depth_requested = unsafe { (platform().depth_domains_requested)(&pass.depth_domains) };
        let _ = metadata::engine_pass(&capture.bundle, pass, depth_requested);
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_result(
    input: *const rsf_frame_record,
    stage: *const c_char,
    backend: u32,
    result: i32,
) {
    let (Some(frame), Some(stage)) = (unsafe { record(input) }, unsafe { string(stage) }) else {
        return;
    };
    if !matches!(
        stage,
        "sr" | "fg" | "sequence_capture" | "automatic_sr_failure" | "spatial_recovery"
    ) {
        return;
    }
    owner(|owner| {
        if let Some(capture) = owner
            .capture
            .as_mut()
            .filter(|capture| capture.wants(frame))
        {
            if stage == "sr" {
                capture.sr_result = Some(result);
                if result == 0 {
                    capture.fill_sr_metadata(frame, backend);
                }
            } else if stage == "fg" {
                capture.fg_result = Some(result);
            }
            let _ = metadata::result(&capture.bundle, frame, stage, backend, result);
        }
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_sr_frame(
    input: *const rsf_sr_frame,
    backend: u32,
    input_sdr: u32,
) {
    let Some(frame) = (unsafe { checked(input) }) else {
        return;
    };
    let Some(record) = (unsafe { record(frame.record) }) else {
        return;
    };
    publish_sr_parameters(frame, record, backend, input_sdr);
}

fn publish_sr_parameters(
    frame: &rsf_sr_frame,
    record: &rsf_frame_record,
    backend: u32,
    input_sdr: u32,
) {
    owner(|owner| {
        if let Some(capture) = owner
            .capture
            .as_ref()
            .filter(|capture| capture.wants(record))
        {
            let _ = metadata::sr_parameters(&capture.bundle, frame, record, backend, input_sdr);
        }
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dump_capture_sr_parameters(
    input: *const rsf_frame_record,
    backend: u32,
    motion_x: f32,
    motion_y: f32,
    exposure: f32,
    reset: u32,
) {
    let Some(record) = (unsafe { record(input) }) else {
        return;
    };
    let frame = rsf_sr_frame {
        struct_size: size_of::<rsf_sr_frame>() as u32,
        record,
        jitter_x: record.camera.jitter_pixels[0],
        jitter_y: record.camera.jitter_pixels[1],
        motion_scale_x: motion_x,
        motion_scale_y: motion_y,
        pre_exposure: exposure,
        reset,
        view_space_to_meters: 1.0,
        ..unsafe { std::mem::zeroed() }
    };
    publish_sr_parameters(&frame, record, backend, 0);
}
