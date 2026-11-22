// SPDX-License-Identifier: MIT
use super::{ConstPtr, Ptr, Seen, lock};
use std::sync::Mutex;

static FFX: Mutex<Seen> = Seen::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn ffxCreateContext(context: *mut Ptr, desc: Ptr, _: ConstPtr) -> u32 {
    let mut seen = lock(&FFX);
    seen.descriptor = desc as usize;
    seen.calls += 1;
    unsafe { context.write(0xf00d as Ptr) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn ffxDestroyContext(context: *mut Ptr, _: ConstPtr) -> u32 {
    let mut seen = lock(&FFX);
    seen.context = unsafe { context.read() } as usize;
    seen.calls += 1;
    0x43
}

#[unsafe(no_mangle)]
unsafe extern "C" fn ffxConfigure(context: *mut Ptr, desc: ConstPtr) -> u32 {
    lock(&FFX).record(unsafe { context.read() }, desc);
    0x41
}

#[unsafe(no_mangle)]
unsafe extern "C" fn ffxQuery(context: *mut Ptr, desc: Ptr) -> u32 {
    lock(&FFX).record(unsafe { context.read() }, desc);
    0x42
}

#[unsafe(no_mangle)]
unsafe extern "C" fn ffxDispatch(context: *mut Ptr, desc: ConstPtr) -> u32 {
    lock(&FFX).record(unsafe { context.read() }, desc);
    0x40
}

#[unsafe(no_mangle)]
unsafe extern "C" fn fixture_ffx_seen(context: *mut Ptr, desc: *mut ConstPtr, calls: *mut i32) {
    unsafe { lock(&FFX).publish(context, desc, calls) };
}
