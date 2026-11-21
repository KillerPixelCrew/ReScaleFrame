// SPDX-License-Identifier: MIT
use super::{Frame, Handle};
use std::ptr::{from_ref, null};

#[repr(C)]
struct Constants<'a> {
    frame: &'a Frame,
    camera_pinhole: [f32; 2],
    motion_vectors_3d: u32,
}

#[repr(C)]
pub(super) struct Platform {
    owned_token: unsafe extern "C" fn(Handle, *const u32) -> Handle,
    host_token: unsafe extern "C" fn(Handle, u64) -> Handle,
    owned_constants: unsafe extern "C" fn(Handle, Handle, u32, &Constants) -> i32,
    host_constants: unsafe extern "C" fn(Handle, u64, u32, &Constants) -> i32,
}

const _: () = {
    assert!(size_of::<Constants>() == 24);
    assert!(std::mem::offset_of!(Constants, motion_vectors_3d) == 16);
    assert!(size_of::<Platform>() == 4 * size_of::<Handle>());
};

impl Platform {
    pub(super) fn token(
        &self,
        native: Handle,
        host: Handle,
        source: u64,
        frame_index: u32,
    ) -> Handle {
        if host.is_null() {
            // A zero index leaves frame counting to the SDK.
            let index = if frame_index == 0 {
                null()
            } else {
                from_ref(&frame_index)
            };
            unsafe { (self.owned_token)(native, index) }
        } else {
            unsafe { (self.host_token)(host, source) }
        }
    }

    pub(super) fn set_constants(
        &self,
        native: Handle,
        host: Handle,
        token: Handle,
        source: u64,
        slot: u32,
        frame: &Frame,
    ) -> i32 {
        let constants = Constants {
            frame,
            camera_pinhole: [0.0; 2],
            motion_vectors_3d: 0,
        };
        if host.is_null() {
            unsafe { (self.owned_constants)(native, token, slot, &constants) }
        } else {
            unsafe { (self.host_constants)(host, source, frame.viewport, &constants) }
        }
    }
}

#[cfg(test)]
#[test]
fn frame_routes_preserve_optional_index_and_original_host_viewport() {
    use std::ptr::{null_mut, without_provenance_mut};

    unsafe extern "C" fn owned_token(_: Handle, index: *const u32) -> Handle {
        if index.is_null() {
            return without_provenance_mut(1);
        }
        assert_eq!(unsafe { index.read() }, 9);
        without_provenance_mut(2)
    }

    unsafe extern "C" fn host_token(host: Handle, source: u64) -> Handle {
        assert_eq!((host.addr(), source), (3, 42));
        without_provenance_mut(4)
    }

    unsafe extern "C" fn owned_constants(
        _: Handle,
        token: Handle,
        slot: u32,
        constants: &Constants,
    ) -> i32 {
        assert_eq!((token.addr(), slot), (2, 0));
        assert_eq!(constants.frame.viewport, 7);
        assert_eq!(
            (constants.camera_pinhole, constants.motion_vectors_3d),
            ([0.0; 2], 0)
        );
        0
    }

    unsafe extern "C" fn host_constants(
        host: Handle,
        source: u64,
        viewport: u32,
        constants: &Constants,
    ) -> i32 {
        assert_eq!((host.addr(), source, viewport), (3, 42, 7));
        assert_eq!(
            (constants.camera_pinhole, constants.motion_vectors_3d),
            ([0.0; 2], 0)
        );
        0
    }

    let platform = Platform {
        owned_token,
        host_token,
        owned_constants,
        host_constants,
    };
    let mut frame: Frame = unsafe { std::mem::zeroed() };
    frame.viewport = 7;
    let slot = super::super::viewport_slot(frame.viewport);
    assert_eq!(platform.token(null_mut(), null_mut(), 0, 0).addr(), 1);
    let owned = platform.token(null_mut(), null_mut(), 0, 9);
    assert_eq!(
        platform.set_constants(null_mut(), null_mut(), owned, 0, slot, &frame),
        0
    );
    let host = without_provenance_mut(3);
    let shared = platform.token(null_mut(), host, 42, 0);
    assert_eq!(shared.addr(), 4);
    assert_eq!(
        platform.set_constants(null_mut(), host, shared, 42, slot, &frame),
        0
    );
}
