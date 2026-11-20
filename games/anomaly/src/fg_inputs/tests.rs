// SPDX-License-Identifier: MIT
use super::*;
use std::cell::Cell;

#[derive(Default)]
struct Context {
    builds: Cell<u32>,
    allocations: Cell<u32>,
    released_targets: Cell<u32>,
    submissions: Cell<u32>,
    fail_resize: Cell<bool>,
    last_motion: Cell<f32>,
}

struct Texture {
    owner: *mut c_void,
    desc: Description,
}

unsafe extern "C" fn device(context: *mut c_void) -> *mut c_void {
    context
}

unsafe extern "C" fn release(_: *mut c_void) {}

unsafe extern "C" fn describe(texture: *mut c_void, out: *mut Description) {
    unsafe { out.write((*texture.cast::<Texture>()).desc) };
}

unsafe extern "C" fn same_device(texture: *mut c_void, device: *mut c_void) -> u32 {
    u32::from(unsafe { (*texture.cast::<Texture>()).owner } == device)
}

unsafe extern "C" fn build(device: *mut c_void, _: Log, _: *mut c_void) -> *mut c_void {
    let state = unsafe { &*device.cast::<Context>() };
    state.builds.set(state.builds.get() + 1);
    device
}

unsafe extern "C" fn surfaces(device: *mut c_void, _: u32, _: u32, out: *mut Targets) {
    let state = unsafe { &*device.cast::<Context>() };
    state.allocations.set(state.allocations.get() + 1);
    if !state.fail_resize.get() {
        unsafe {
            out.write(Targets {
                resources: device,
                depth: NonNull::<u8>::dangling().as_ptr().cast(),
                motion: NonNull::<u16>::dangling().as_ptr().cast(),
            })
        };
    }
}

unsafe extern "C" fn release_surfaces(resources: *mut c_void) {
    let state = unsafe { &*resources.cast::<Context>() };
    state.released_targets.set(state.released_targets.get() + 1);
}

unsafe extern "C" fn motion_view(_: *mut c_void, texture: *mut c_void) -> *mut c_void {
    texture
}

unsafe extern "C" fn dispatch(
    context: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    motion: *mut c_void,
    values: *const Constants,
    groups_x: u32,
    groups_y: u32,
) -> u32 {
    let state = unsafe { &*context.cast::<Context>() };
    let values = unsafe { &*values };
    state.submissions.set(state.submissions.get() + 1);
    state.last_motion.set(values.has_motion);
    u32::from(
        (values.has_motion == 0.0) == motion.is_null()
            && groups_x == (values.size[0] as u32).div_ceil(8)
            && groups_y == (values.size[1] as u32).div_ceil(8),
    )
}

const PLATFORM: Platform = Platform {
    device: Some(device),
    release_interface: Some(release),
    describe: Some(describe),
    same_device: Some(same_device),
    build: Some(build),
    release_pipeline: Some(release),
    targets: Some(surfaces),
    release_targets: Some(release_surfaces),
    motion_view: Some(motion_view),
    dispatch: Some(dispatch),
    first_person_limit: 0.02,
    validity_threshold: 0.5,
};

fn make_depth(device: *mut c_void) -> Texture {
    Texture {
        owner: device,
        desc: Description {
            format: R24G8_TYPELESS,
            width: 9,
            height: 17,
            samples: 1,
            layers: 1,
            mips: 1,
        },
    }
}

#[test]
fn foreign_depth_refuses_the_copy_and_foreign_motion_falls_back_to_camera_motion() {
    let state = Context::default();
    let context = ptr::from_ref(&state).cast_mut().cast();
    let mut depth = make_depth(ptr::null_mut());
    let mut motion = make_depth(ptr::null_mut());
    motion.desc.format = RGBA16_FLOAT;
    let frame = Frame {
        depth: ptr::from_mut(&mut depth).cast(),
        motion: ptr::from_mut(&mut motion).cast(),
        clip_to_previous_clip: [0.0; 16],
        width: 9,
        height: 17,
    };
    let mut owner = ptr::null_mut();
    let mut run = || {
        rsf_anomaly_input_run(
            &mut owner,
            &PLATFORM,
            context,
            &frame,
            None,
            ptr::null_mut(),
        )
    };
    assert_eq!(run(), Result::Resources as u32);
    assert_eq!(state.submissions.get(), 0);
    depth.owner = context;
    // Refresh the pointer after modifying the borrowed texture.
    let mut frame = frame;
    frame.depth = ptr::from_mut(&mut depth).cast();
    assert_eq!(
        rsf_anomaly_input_run(
            &mut owner,
            &PLATFORM,
            context,
            &frame,
            None,
            ptr::null_mut()
        ),
        Result::Ok as u32
    );
    assert_eq!(state.submissions.get(), 1);
    assert_eq!(state.last_motion.get(), 0.0);
    assert!(!rsf_anomaly_input_output(unsafe { owner.as_ref() }, 0).is_null());
    unsafe { rsf_anomaly_input_release(owner) };
}

#[test]
fn failed_resize_keeps_the_previous_generation_for_retry_without_publishing_it() {
    let state = Context::default();
    let context = ptr::from_ref(&state).cast_mut().cast();
    let mut depth = make_depth(context);
    let mut frame = Frame {
        depth: ptr::from_mut(&mut depth).cast(),
        motion: ptr::null_mut(),
        clip_to_previous_clip: [0.0; 16],
        width: 9,
        height: 17,
    };
    let mut owner = ptr::null_mut();
    assert_eq!(
        rsf_anomaly_input_run(
            &mut owner,
            &PLATFORM,
            context,
            &frame,
            None,
            ptr::null_mut()
        ),
        Result::Ok as u32
    );
    frame.width = 12;
    depth.desc.width = frame.width;
    frame.depth = ptr::from_mut(&mut depth).cast();
    state.fail_resize.set(true);
    assert_eq!(
        rsf_anomaly_input_run(
            &mut owner,
            &PLATFORM,
            context,
            &frame,
            None,
            ptr::null_mut()
        ),
        Result::Resources as u32
    );
    assert!(rsf_anomaly_input_output(unsafe { owner.as_ref() }, 0).is_null());
    assert_eq!(state.released_targets.get(), 0);
    state.fail_resize.set(false);
    assert_eq!(
        rsf_anomaly_input_run(
            &mut owner,
            &PLATFORM,
            context,
            &frame,
            None,
            ptr::null_mut()
        ),
        Result::Ok as u32
    );
    assert_eq!(
        (
            state.builds.get(),
            state.allocations.get(),
            state.released_targets.get()
        ),
        (1, 3, 1)
    );
    unsafe { rsf_anomaly_input_release(owner) };
    assert_eq!(state.released_targets.get(), 2);
}
