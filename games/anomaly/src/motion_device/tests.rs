// SPDX-License-Identifier: MIT
use super::*;
use std::cell::Cell;
use std::ptr;

#[derive(Default)]
struct Calls {
    leases: Cell<u32>,
    reject_view: Cell<bool>,
    reject_upload: Cell<bool>,
    attempts: Cell<u32>,
    bound_stage: Cell<c_char>,
}

unsafe extern "C" fn retain(handle: *mut c_void) {
    let calls = unsafe { &*handle.cast::<Calls>() };
    calls.leases.set(calls.leases.get() + 1);
}

unsafe extern "C" fn release(handle: *mut c_void) {
    let calls = unsafe { &*handle.cast::<Calls>() };
    calls.leases.set(calls.leases.get() - 1);
}

unsafe extern "C" fn resource(device: *mut c_void, _: u32) -> *mut c_void {
    unsafe { retain(device) };
    device
}

unsafe extern "C" fn shader(device: *mut c_void, _: Log, _: *mut c_void) -> *mut c_void {
    unsafe { resource(device, 0) }
}

unsafe extern "C" fn texture(device: *mut c_void, _: u32, _: u32) -> *mut c_void {
    unsafe { resource(device, 0) }
}

unsafe extern "C" fn view(device: *mut c_void, _: *mut c_void) -> *mut c_void {
    let calls = unsafe { &*device.cast::<Calls>() };
    if calls.reject_view.get() {
        ptr::null_mut()
    } else {
        unsafe { resource(device, 0) }
    }
}

unsafe extern "C" fn upload(
    context: *mut c_void,
    _: *mut c_void,
    _: *const c_void,
    _: usize,
) -> u32 {
    let calls = unsafe { &*context.cast::<Calls>() };
    calls.attempts.set(calls.attempts.get() + 1);
    u32::from(!calls.reject_upload.get())
}

unsafe extern "C" fn bind(context: *mut c_void, _: *mut c_void, stage: c_char) -> u32 {
    let calls = unsafe { &*context.cast::<Calls>() };
    calls.bound_stage.set(stage);
    1
}

static PLATFORM: Platform = Platform {
    retain: retain,
    release: release,
    buffer: resource,
    zero_shader: shader,
    depth_state: resource,
    texture: texture,
    view: view,
    upload: upload,
    bind: bind,
};

fn make_owner(calls: &Calls) -> Box<Owner> {
    let mut owner = unsafe { Box::from_raw(rsf_anomaly_motion_device_new(&PLATFORM)) };
    let device = ptr::from_ref(calls).cast_mut().cast();
    assert_eq!(
        rsf_anomaly_motion_device_create(&mut owner, device, None, ptr::null_mut()),
        1
    );
    owner
}

#[test]
fn refused_view_releases_the_candidate_and_retry_publishes_a_complete_target() {
    let calls = Calls::default();
    let mut owner = make_owner(&calls);
    let device_leases = calls.leases.get();
    assert_eq!(
        rsf_anomaly_motion_device_target(Some(&mut owner), 20, 10, 1, 1),
        RSF_MOTION_CREATED
    );
    assert_eq!(calls.leases.get(), device_leases + 2);
    assert_eq!(
        rsf_anomaly_motion_device_target(Some(&mut owner), 20, 10, 1, 1),
        RSF_MOTION_READY
    );
    calls.reject_view.set(true);
    assert_eq!(
        rsf_anomaly_motion_device_target(Some(&mut owner), 40, 20, 1, 1),
        RSF_MOTION_FAILED
    );
    assert_eq!(calls.leases.get(), device_leases);
    let mut snapshot = MaybeUninit::uninit();
    rsf_anomaly_motion_device_get(Some(&owner), &mut snapshot);
    let snapshot = unsafe { snapshot.assume_init() };
    assert!(snapshot.texture.is_null() && snapshot.rtv.is_null());
    calls.reject_view.set(false);
    assert_eq!(
        rsf_anomaly_motion_device_target(Some(&mut owner), 40, 20, 1, 1),
        RSF_MOTION_CREATED
    );
    drop(owner);
    assert_eq!(calls.leases.get(), 0);
}

#[test]
fn a_failed_upload_invalidates_the_cache_and_the_next_success_rebinds() {
    let calls = Calls::default();
    let context = ptr::from_ref(&calls).cast_mut().cast();
    let mut owner = make_owner(&calls);
    let mut submit = |bytes: &[u8; TESS_BYTES]| {
        rsf_anomaly_motion_device_upload(
            Some(&mut owner),
            context,
            RSF_MOTION_TESS,
            bytes.as_ptr(),
            bytes.len(),
        )
    };
    let bytes = [1; TESS_BYTES];
    assert_eq!(submit(&bytes), 1);
    assert_eq!(calls.bound_stage.get(), b'd' as c_char);
    calls.bound_stage.set(0);
    assert_eq!(submit(&bytes), 1);
    assert_eq!(calls.bound_stage.get(), b'd' as c_char);
    assert_eq!(calls.attempts.get(), 1);
    calls.reject_upload.set(true);
    assert_eq!(submit(&[2; TESS_BYTES]), 0);
    calls.reject_upload.set(false);
    assert_eq!(submit(&bytes), 1);
    assert_eq!(calls.attempts.get(), 3);
    assert_eq!((owner.uploads, owner.upload_skips), (2, 1));
}
