// SPDX-License-Identifier: MIT
use rsf_game_sys::{RSF_GAME_RENDER_RESET, rsf_game_render_pass as Pass};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

static RESET_PENDING: AtomicBool = AtomicBool::new(false);
type Evaluate = unsafe extern "C" fn(*mut c_void, *const Pass) -> i32;

#[unsafe(no_mangle)]
extern "C" fn rsf_core_sr_request_reset() -> i32 {
    RESET_PENDING.store(true, Ordering::Release);
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_sr_clear_reset() {
    RESET_PENDING.store(false, Ordering::Release);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr_evaluate(
    context: *mut c_void,
    pass: *const Pass,
    evaluate: Evaluate,
) -> i32 {
    if context.is_null()
        || pass.is_null()
        || (unsafe { pass.cast::<u32>().read() } as usize) < size_of::<Pass>()
    {
        return -1;
    }
    // Claim before invoking the backend. A callback can request a new reset
    // even while this frame is successfully consuming the previous one.
    if !RESET_PENDING.swap(false, Ordering::AcqRel) {
        return unsafe { evaluate(context, pass) };
    }
    // Copy the known prefix as bytes. Foreign callers may leave unused fields
    // uninitialized or supply an extended structure.
    let mut copy = std::mem::MaybeUninit::<Pass>::uninit();
    let copy_ptr = copy.as_mut_ptr();
    let result = unsafe {
        copy_ptr.copy_from_nonoverlapping(pass, 1);
        (*copy_ptr).struct_size = size_of::<Pass>() as u32;
        (*copy_ptr).flags |= RSF_GAME_RENDER_RESET;
        evaluate(context, copy_ptr)
    };
    if result != 0 {
        RESET_PENDING.store(true, Ordering::Release);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe {
        observed: u32,
        size: u32,
        request_another: bool,
        result: i32,
    }

    unsafe extern "C" fn observe(context: *mut c_void, pass: *const Pass) -> i32 {
        let probe = unsafe { &mut *context.cast::<Probe>() };
        probe.observed = unsafe { (*pass).flags };
        probe.size = unsafe { (*pass).struct_size };
        if probe.request_another {
            rsf_core_sr_request_reset();
        }
        probe.result
    }

    #[test]
    fn reset_survives_refusal_and_another_request_during_evaluation() {
        #[repr(C)]
        struct Extended {
            pass: Pass,
            tail: u64,
        }
        let mut extended: Extended = unsafe { std::mem::zeroed() };
        extended.pass.struct_size = size_of::<Extended>() as u32;
        let pass = &extended.pass;
        let mut probe = Probe {
            observed: 0,
            size: 0,
            request_another: false,
            result: -8,
        };
        rsf_core_sr_request_reset();
        let run = |p: &mut Probe| unsafe {
            rsf_core_sr_evaluate(std::ptr::from_mut(p).cast(), pass, observe)
        };
        assert_eq!(run(&mut probe), -8);
        assert_eq!(probe.size, size_of::<Pass>() as u32);
        assert_eq!(probe.observed, RSF_GAME_RENDER_RESET);
        probe.result = 0;
        probe.request_another = true;
        assert_eq!(run(&mut probe), 0);
        assert_eq!(probe.observed, RSF_GAME_RENDER_RESET);
        probe.request_another = false;
        assert_eq!(run(&mut probe), 0);
        assert_eq!(probe.observed, RSF_GAME_RENDER_RESET);
        assert_eq!(run(&mut probe), 0);
        assert_eq!(probe.observed, 0);
        assert_eq!(probe.size, size_of::<Extended>() as u32);
        assert_eq!(pass.flags, 0);
    }
}
