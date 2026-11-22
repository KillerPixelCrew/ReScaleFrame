// SPDX-License-Identifier: MIT
use super::*;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

type Entry = (&'static str, u32);

#[derive(Default)]
struct Context {
    builds: Cell<u32>,
    generation: Cell<u32>,
    created: RefCell<Vec<Entry>>,
    released: RefCell<Vec<Entry>>,
    fail_at: Cell<Option<&'static str>>,
    submissions: Cell<u32>,
    last_motion: Cell<f32>,
}

struct Texture {
    owner: *mut c_void,
    desc: Description,
}

/// A fake pipeline or staging resource. Only these are counted when released.
struct Resource {
    context: *const Context,
    label: &'static str,
    generation: u32,
}

thread_local! {
    static LIVE: RefCell<HashSet<usize>> = RefCell::default();
}

fn context(device: *mut c_void) -> &'static Context {
    unsafe { &*device.cast::<Context>() }
}

fn label(object: *mut c_void) -> &'static str {
    if LIVE.with_borrow(|live| live.contains(&(object as usize))) {
        unsafe { (*object.cast::<Resource>()).label }
    } else {
        ""
    }
}

fn lease_resource(device: *mut c_void, label: &'static str) -> *mut c_void {
    let state = context(device);
    if state.fail_at.get() == Some(label) {
        return ptr::null_mut();
    }
    let generation = state.generation.get();
    state.created.borrow_mut().push((label, generation));
    let resource = Box::into_raw(Box::new(Resource {
        context: state,
        label,
        generation,
    }))
    .cast::<c_void>();
    LIVE.with_borrow_mut(|live| live.insert(resource as usize));
    resource
}

unsafe extern "C" fn device(context: *mut c_void) -> *mut c_void {
    context
}

unsafe extern "C" fn release(object: *mut c_void) {
    if LIVE.with_borrow_mut(|live| live.remove(&(object as usize))) {
        let resource = unsafe { Box::from_raw(object.cast::<Resource>()) };
        let state = unsafe { &*resource.context };
        state
            .released
            .borrow_mut()
            .push((resource.label, resource.generation));
    }
}

unsafe extern "C" fn describe(texture: *mut c_void, out: *mut Description) {
    unsafe { out.write((*texture.cast::<Texture>()).desc) };
}

unsafe extern "C" fn same_device(texture: *mut c_void, device: *mut c_void) -> u32 {
    u32::from(unsafe { (*texture.cast::<Texture>()).owner } == device)
}

unsafe extern "C" fn shader(device: *mut c_void, _: Log, _: *mut c_void) -> *mut c_void {
    let state = context(device);
    state.builds.set(state.builds.get() + 1);
    lease_resource(device, "shader")
}

unsafe extern "C" fn constant_buffer(device: *mut c_void) -> *mut c_void {
    lease_resource(device, "constants")
}

unsafe extern "C" fn texture(
    device: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
    bind: u32,
) -> *mut c_void {
    let state = context(device);
    let output = BIND_SHADER_RESOURCE | BIND_UNORDERED_ACCESS;
    let (label, expected) = match format {
        R24G8_TYPELESS => {
            state.generation.set(state.generation.get() + 1);
            ("depth_copy", BIND_SHADER_RESOURCE)
        }
        R32_FLOAT => ("depth", output),
        RG16_FLOAT => ("motion", output),
        _ => return ptr::null_mut(),
    };
    if bind != expected || width == 0 || height == 0 {
        return ptr::null_mut();
    }
    lease_resource(device, label)
}

unsafe extern "C" fn depth_view(device: *mut c_void, depth_copy: *mut c_void) -> *mut c_void {
    if label(depth_copy) != "depth_copy" {
        return ptr::null_mut();
    }
    lease_resource(device, "depth_view")
}

unsafe extern "C" fn unordered_view(device: *mut c_void, texture: *mut c_void) -> *mut c_void {
    match label(texture) {
        "depth" => lease_resource(device, "depth_uav"),
        "motion" => lease_resource(device, "motion_uav"),
        _ => ptr::null_mut(),
    }
}

unsafe extern "C" fn motion_view(_: *mut c_void, texture: *mut c_void) -> *mut c_void {
    texture
}

unsafe fn dispatch(
    _: StateScope,
    context: *mut c_void,
    pipeline: *const PipelineBindings,
    targets: *const Targets,
    _: *mut c_void,
    motion: *mut c_void,
    values: *const Constants,
    groups_x: u32,
    groups_y: u32,
) -> u32 {
    let state = self::context(context);
    let values = unsafe { &*values };
    let pipeline = unsafe { &*pipeline };
    let targets = unsafe { &*targets };
    let bound = [
        targets.depth_copy,
        targets.depth,
        targets.motion,
        targets.depth_view,
        targets.depth_uav,
        targets.motion_uav,
    ]
    .map(label);
    state.submissions.set(state.submissions.get() + 1);
    state.last_motion.set(values.has_motion);
    u32::from(
        label(pipeline.shader) == "shader"
            && label(pipeline.constants) == "constants"
            && bound == SET
            && (values.has_motion == 0.0) == motion.is_null()
            && groups_x == (values.size[0] as u32).div_ceil(8)
            && groups_y == (values.size[1] as u32).div_ceil(8),
    )
}

const SET: [&str; 6] = [
    "depth_copy",
    "depth",
    "motion",
    "depth_view",
    "depth_uav",
    "motion_uav",
];
const PIPELINE_CREATED: [Entry; 2] = [("shader", 0), ("constants", 0)];
const PIPELINE_RELEASED: [Entry; 2] = [("constants", 0), ("shader", 0)];

pub(super) const BINDINGS: Bindings = Bindings {
    device,
    release_interface: release,
    describe,
    same_device,
    shader,
    constant_buffer,
    texture,
    depth_view,
    unordered_view,
    motion_view,
    dispatch,
};

fn created(generation: u32, count: usize) -> Vec<Entry> {
    SET[..count]
        .iter()
        .map(|&label| (label, generation))
        .collect()
}

fn released(generation: u32, count: usize) -> Vec<Entry> {
    let mut entries = created(generation, count);
    entries.reverse();
    entries
}

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
    let mut owner = Owner::with_bindings(BINDINGS, scope());
    assert_eq!(
        owner.run(context, &frame, None, ptr::null_mut()),
        Result::Resources
    );
    assert_eq!(state.submissions.get(), 0);
    assert!(state.created.borrow().is_empty());
    depth.owner = context;
    // Refresh the pointer after modifying the borrowed texture.
    let mut frame = frame;
    frame.depth = ptr::from_mut(&mut depth).cast();
    assert_eq!(
        owner.run(context, &frame, None, ptr::null_mut()),
        Result::Ok
    );
    assert_eq!(state.submissions.get(), 1);
    assert_eq!(state.last_motion.get(), 0.0);
    let outputs = owner.outputs().expect("published outputs");
    assert_eq!(outputs.map(label), ["depth", "motion"]);
    drop(owner);
    assert_eq!(
        *state.released.borrow(),
        [released(1, 6), PIPELINE_RELEASED.to_vec()].concat()
    );
}

#[test]
fn failed_resize_releases_the_partial_set_and_keeps_the_previous_one_for_retry() {
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
    let mut owner = Owner::with_bindings(BINDINGS, scope());
    assert_eq!(
        owner.run(context, &frame, None, ptr::null_mut()),
        Result::Ok
    );
    let mut expected_created = PIPELINE_CREATED.to_vec();
    expected_created.extend(created(1, 6));
    assert_eq!(*state.created.borrow(), expected_created);
    frame.width = 12;
    depth.desc.width = frame.width;
    frame.depth = ptr::from_mut(&mut depth).cast();
    state.fail_at.set(Some("motion_uav"));
    assert_eq!(
        owner.run(context, &frame, None, ptr::null_mut()),
        Result::Resources
    );
    assert!(owner.outputs().is_none());
    // Only the partial set drops, newest first; the older complete set stays leased.
    assert_eq!(*state.released.borrow(), released(2, 5));
    state.fail_at.set(None);
    assert_eq!(
        owner.run(context, &frame, None, ptr::null_mut()),
        Result::Ok
    );
    assert_eq!(state.builds.get(), 1);
    expected_created.extend(created(2, 5));
    expected_created.extend(created(3, 6));
    assert_eq!(*state.created.borrow(), expected_created);
    let mut expected_released = released(2, 5);
    expected_released.extend(released(1, 6));
    assert_eq!(*state.released.borrow(), expected_released);
    assert_eq!(
        owner.outputs().expect("published outputs").map(label),
        ["depth", "motion"]
    );
    drop(owner);
    expected_released.extend(released(3, 6));
    expected_released.extend(PIPELINE_RELEASED);
    assert_eq!(*state.released.borrow(), expected_released);
}

#[test]
fn refused_constant_buffer_releases_shader_and_retries_on_same_device() {
    let state = Context::default();
    let context = ptr::from_ref(&state).cast_mut().cast();
    let mut depth = make_depth(context);
    let frame = Frame {
        depth: ptr::from_mut(&mut depth).cast(),
        motion: ptr::null_mut(),
        clip_to_previous_clip: [0.0; 16],
        width: 9,
        height: 17,
    };
    let mut owner = Owner::with_bindings(BINDINGS, scope());
    state.fail_at.set(Some("constants"));
    assert_eq!(
        owner.run(context, &frame, None, ptr::null_mut()),
        Result::Resources
    );
    assert_eq!(*state.created.borrow(), [("shader", 0)]);
    assert_eq!(*state.released.borrow(), [("shader", 0)]);
    assert!(owner.outputs().is_none());
    state.fail_at.set(None);
    assert_eq!(
        owner.run(context, &frame, None, ptr::null_mut()),
        Result::Ok
    );
    assert_eq!(state.builds.get(), 2);
    assert_eq!(state.submissions.get(), 1);
}

#[cfg(windows)]
pub(crate) const STATE_SCOPE: rsf_anomaly_fg_state_scope = {
    unsafe extern "C" fn save(_: *mut c_void, _: *mut rsf_d3d11_state) -> u32 {
        0
    }
    unsafe extern "C" fn restore(_: *mut c_void, _: *mut rsf_d3d11_state) {}
    rsf_anomaly_fg_state_scope {
        save: Some(save),
        restore: Some(restore),
    }
};

fn scope() -> StateScope {
    #[cfg(windows)]
    return StateScope::bind(STATE_SCOPE).expect("fixture state scope");
    #[cfg(not(windows))]
    StateScope
}
