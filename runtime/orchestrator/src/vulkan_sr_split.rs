// SPDX-License-Identifier: MIT
use crate::sr_session::has_capacity;
use crate::sys::*;
use rsf_game_sys::backend_result::{ABI_MISMATCH, INVALID_ARGUMENT, NOT_COMPILED, NOT_SUPPORTED};
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION};
use std::ffi::c_void;
use std::mem::offset_of;
use std::ptr;

type Handle = *mut c_void;
type Frame = rsf_srvk_frame;
type Setup = rsf_srvk_setup;
type Handoff = rsf_fsr4vk_handoff;
type Info = rsf_srvk_fsr4_provider_info;

const FIRST_RESERVED_QUEUE_FAMILY: u32 = 0xffff_fffd;
const FRAME_PREFIX: usize = offset_of!(Frame, bias_current_color);

/// The FSR4 owner's calls after admission: `vulkan_fsr4_sr::PROVIDER` in a build with the Vulkan
/// FSR4 provider, a stub in the tests below.
pub(crate) struct Provider {
    pub(crate) create: unsafe fn(*const Setup, *const Handoff, *mut Handle) -> i32,
    pub(crate) plan: unsafe fn(*const c_void, *mut u32, *mut u32) -> i32,
    pub(crate) evaluate: unsafe fn(Handle, Handle, *const Frame, *mut Handle) -> i32,
    pub(crate) destroy: unsafe fn(Handle) -> i32,
    pub(crate) retire: unsafe fn(Handle, Handle),
    pub(crate) submitted: unsafe fn(Handle, Handle) -> i32,
    pub(crate) info: unsafe fn(*const c_void, *mut Info) -> i32,
}

/// The provider of this build; null reports NOT_COMPILED (CMake sets `fsr4-vk` with the
/// FidelityFX DX12 SR runtime).
fn provider() -> *const Provider {
    if cfg!(feature = "fsr4-vk") {
        &crate::vulkan_fsr4_sr::PROVIDER
    } else {
        ptr::null()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_srvk_fsr4_create(
    setup: *const Setup,
    handoff: *const Handoff,
    owner: *mut Handle,
) -> i32 {
    unsafe { rsf_core_srvk_fsr4_create(provider(), setup, handoff, owner) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_srvk_fsr4_plan(
    owner: *const c_void,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe { rsf_core_srvk_fsr4_plan(provider(), owner, width, height) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_srvk_fsr4_evaluate(
    owner: Handle,
    commands: Handle,
    frame: *const Frame,
    continuation: *mut Handle,
) -> i32 {
    unsafe { rsf_core_srvk_fsr4_evaluate(provider(), owner, commands, frame, continuation) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_srvk_fsr4_destroy(owner: Handle) -> i32 {
    unsafe { rsf_core_srvk_fsr4_destroy(provider(), owner) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_srvk_fsr4_retire_commands(owner: Handle, commands: Handle) {
    unsafe { rsf_core_srvk_fsr4_retire_commands(provider(), owner, commands) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_srvk_fsr4_submitted(owner: Handle, commands: Handle) -> i32 {
    unsafe { rsf_core_srvk_fsr4_submitted(provider(), owner, commands) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_srvk_fsr4_info(owner: *const c_void, out: *mut Info) -> i32 {
    unsafe { rsf_core_srvk_fsr4_info(provider(), owner, out) }
}

fn public_result(result: i32) -> i32 {
    match result {
        RSF_FSR4_BRIDGE_INVALID_ARGUMENT => INVALID_ARGUMENT,
        RSF_FSR4_BRIDGE_ABI_MISMATCH => ABI_MISMATCH,
        _ => result,
    }
}

pub(super) unsafe fn admit_setup(
    setup: *const Setup,
    handoff: *const Handoff,
) -> Result<(Setup, Handoff), i32> {
    if !unsafe { has_capacity(setup) && has_capacity(handoff) } {
        return Err(INVALID_ARGUMENT);
    }
    let (facts, lease) = unsafe { (&*setup, &*handoff) };
    if facts.abi_version != RSF_SRVK_ABI_VERSION
        || !matches!(
            lease.abi_version,
            RSF_SRVK_SPLIT_ABI_VERSION | RSF_SRVK_SPLIT_TRACKED_ABI_VERSION
        )
    {
        return Err(ABI_MISMATCH);
    }
    if facts.backend != RSF_SR_FSR4 {
        return Err(NOT_SUPPORTED);
    }
    if facts.quality > 4
        || facts.instance.is_null()
        || facts.physical_device.is_null()
        || facts.device.is_null()
        || facts.instance_proc.is_none()
        || facts.device_proc.is_none()
        || facts.output_width == 0
        || facts.output_height == 0
        || facts.runtime_directory_utf8.is_null()
        || unsafe { *facts.runtime_directory_utf8 } == 0
        || lease.timeline_enabled != 1
        || lease.queue_family >= FIRST_RESERVED_QUEUE_FAMILY
        || lease.split_record.is_none()
        || facts.depth_inverted > 1
        || facts.depth_infinite > 1
        || facts.motion_jittered > 1
    {
        return Err(INVALID_ARGUMENT);
    }
    Ok((*facts, *lease))
}

/// The bridge code for a refused setup, as the state fixture's direct create reports it.
#[cfg(feature = "test-fixtures")]
pub(super) fn private_setup_result(error: i32) -> i32 {
    match error {
        ABI_MISMATCH => RSF_FSR4_BRIDGE_ABI_MISMATCH,
        _ => RSF_FSR4_BRIDGE_INVALID_ARGUMENT,
    }
}

unsafe fn rsf_core_srvk_fsr4_create(
    provider: *const Provider,
    setup: *const Setup,
    handoff: *const Handoff,
    owner: *mut Handle,
) -> i32 {
    if owner.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { owner.write(ptr::null_mut()) };
    let (setup, handoff) = match unsafe { admit_setup(setup, handoff) } {
        Ok(records) => records,
        Err(error) => return error,
    };
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.create)(&setup, &handoff, owner) })
}

unsafe fn rsf_core_srvk_fsr4_plan(
    provider: *const Provider,
    owner: *const c_void,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe {
        if !width.is_null() {
            width.write(0);
        }
        if !height.is_null() {
            height.write(0);
        }
    }
    if owner.is_null() || width.is_null() || height.is_null() {
        return INVALID_ARGUMENT;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.plan)(owner, width, height) })
}

pub(super) unsafe fn normalize_frame(frame: *const Frame) -> Result<Frame, i32> {
    if frame.is_null() {
        return Err(INVALID_ARGUMENT);
    }
    let size = unsafe { frame.cast::<u32>().read() } as usize;
    if size != FRAME_PREFIX && size < size_of::<Frame>() {
        return Err(INVALID_ARGUMENT);
    }
    if unsafe { (*frame).abi_version } != RSF_SRVK_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    let mut normalized = std::mem::MaybeUninit::<Frame>::zeroed();
    unsafe {
        ptr::copy_nonoverlapping(
            frame.cast::<u8>(),
            normalized.as_mut_ptr().cast::<u8>(),
            size.min(size_of::<Frame>()),
        );
        let mut frame = normalized.assume_init();
        frame.struct_size = size_of::<Frame>() as u32;
        Ok(frame)
    }
}

unsafe fn rsf_core_srvk_fsr4_evaluate(
    provider: *const Provider,
    owner: Handle,
    commands: Handle,
    frame: *const Frame,
    continuation: *mut Handle,
) -> i32 {
    if continuation.is_null() {
        return INVALID_ARGUMENT;
    }
    // Failed admission leaves the host's original command buffer recordable.
    unsafe { continuation.write(commands) };
    if owner.is_null() || commands.is_null() {
        return INVALID_ARGUMENT;
    }
    let frame = match unsafe { normalize_frame(frame) } {
        Ok(frame) => frame,
        Err(error) => return error,
    };
    if !unsafe { has_capacity(frame.record) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*frame.record).abi_version } != RSF_GAME_FRAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let camera = unsafe { ptr::addr_of!((*frame.record).camera) };
    if !unsafe { has_capacity(camera) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*camera).abi_version } != RSF_GAME_CAMERA_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if frame.reactive.image != 0 || frame.composition.image != 0 {
        return NOT_SUPPORTED;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    // The provider owns any committed split. Keep its continuation on error.
    public_result(unsafe { (provider.evaluate)(owner, commands, &frame, continuation) })
}

unsafe fn rsf_core_srvk_fsr4_destroy(provider: *const Provider, owner: Handle) -> i32 {
    if owner.is_null() {
        return 0;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.destroy)(owner) })
}

unsafe fn rsf_core_srvk_fsr4_retire_commands(
    provider: *const Provider,
    owner: Handle,
    commands: Handle,
) {
    if let Some(provider) = unsafe { provider.as_ref() } {
        unsafe { (provider.retire)(owner, commands) };
    }
}

unsafe fn rsf_core_srvk_fsr4_submitted(
    provider: *const Provider,
    owner: Handle,
    commands: Handle,
) -> i32 {
    if owner.is_null() || commands.is_null() {
        return INVALID_ARGUMENT;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.submitted)(owner, commands) })
}

unsafe fn rsf_core_srvk_fsr4_info(
    provider: *const Provider,
    owner: *const c_void,
    out: *mut Info,
) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    unsafe {
        (*out).adapter_luid = 0;
        (*out).provider_version = 0;
        (*out).vendor_id = 0;
        (*out).int8_compatibility = 0;
    }
    if unsafe { (*out).abi_version } != RSF_SRVK_FSR4_INFO_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if owner.is_null() {
        return INVALID_ARGUMENT;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.info)(owner, out) })
}

#[derive(Default)]
pub(super) struct SplitLease {
    tracked: bool,
    producer: Handle,
    continuation: Handle,
}

impl SplitLease {
    pub(super) fn new(version: u32) -> Self {
        Self {
            tracked: version == RSF_SRVK_SPLIT_TRACKED_ABI_VERSION,
            ..Self::default()
        }
    }

    pub(super) fn pending(&self) -> bool {
        self.tracked && (!self.producer.is_null() || !self.continuation.is_null())
    }

    pub(super) fn begin(&mut self, commands: Handle) {
        if self.tracked {
            self.producer = commands;
        }
    }

    pub(super) fn resume(&mut self, commands: Handle) {
        if self.tracked {
            self.continuation = commands;
        }
    }

    pub(super) fn submitted(&mut self, commands: Handle) -> bool {
        if !self.tracked || commands.is_null() {
            return false;
        }
        if commands == self.producer {
            self.producer = ptr::null_mut();
            return true;
        }
        if self.producer.is_null() && commands == self.continuation {
            self.continuation = ptr::null_mut();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rsf_game_sys::rsf_frame_record;

    unsafe extern "C" fn no_proc(_: Handle, _: *const std::ffi::c_char) -> rsf_vk_function {
        None
    }

    unsafe extern "C" fn split(
        _: Handle,
        _: Handle,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
        _: *mut Handle,
    ) -> i32 {
        0
    }

    fn records() -> (Setup, Handoff, rsf_frame_record) {
        let device = ptr::dangling_mut::<u8>().cast();
        let setup = Setup {
            struct_size: size_of::<Setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: device,
            physical_device: device,
            device,
            instance_proc: Some(no_proc),
            device_proc: Some(no_proc),
            backend: RSF_SR_FSR4,
            quality: 1,
            output_width: 640,
            output_height: 360,
            depth_inverted: 0,
            depth_infinite: 0,
            motion_jittered: 0,
            runtime_directory_utf8: c"C:/unused".as_ptr(),
        };
        let handoff = Handoff {
            struct_size: size_of::<Handoff>() as u32,
            abi_version: RSF_SRVK_SPLIT_ABI_VERSION,
            timeline_enabled: 1,
            queue_family: 0,
            user: ptr::null_mut(),
            split_record: Some(split),
        };
        let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<rsf_frame_record>() as u32;
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.camera.struct_size = size_of_val(&record.camera) as u32;
        record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        (setup, handoff, record)
    }

    fn frame(record: &rsf_frame_record) -> Frame {
        Frame {
            struct_size: size_of::<Frame>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            record,
            ..unsafe { std::mem::zeroed() }
        }
    }

    #[test]
    fn split_admission_and_disabled_outputs() {
        let disabled = ptr::null();
        let (setup, handoff, mut record) = records();
        let commands = ptr::dangling_mut::<u8>().cast();
        let mut owner = commands;
        let invalid_setups: [(Setup, i32); 4] = [
            (
                Setup {
                    struct_size: setup.struct_size - 1,
                    abi_version: 99,
                    ..setup
                },
                INVALID_ARGUMENT,
            ),
            (
                Setup {
                    abi_version: 99,
                    ..setup
                },
                ABI_MISMATCH,
            ),
            (
                Setup {
                    backend: RSF_SR_FSR3,
                    ..setup
                },
                NOT_SUPPORTED,
            ),
            (
                Setup {
                    depth_inverted: 2,
                    ..setup
                },
                INVALID_ARGUMENT,
            ),
        ];
        for (setup, expected) in invalid_setups {
            assert_eq!(
                unsafe { rsf_core_srvk_fsr4_create(disabled, &setup, &handoff, &mut owner) },
                expected
            );
            assert!(owner.is_null());
        }
        for (handoff, expected) in [
            (
                Handoff {
                    struct_size: handoff.struct_size - 1,
                    abi_version: 99,
                    ..handoff
                },
                INVALID_ARGUMENT,
            ),
            (
                Handoff {
                    abi_version: 99,
                    ..handoff
                },
                ABI_MISMATCH,
            ),
            (
                Handoff {
                    timeline_enabled: 0,
                    ..handoff
                },
                INVALID_ARGUMENT,
            ),
            (
                Handoff {
                    queue_family: 0xffff_fffe,
                    ..handoff
                },
                INVALID_ARGUMENT,
            ),
            (
                Handoff {
                    split_record: None,
                    ..handoff
                },
                INVALID_ARGUMENT,
            ),
        ] {
            assert_eq!(
                unsafe { rsf_core_srvk_fsr4_create(disabled, &setup, &handoff, &mut owner) },
                expected
            );
        }
        unsafe {
            assert_eq!(
                rsf_core_srvk_fsr4_create(disabled, &setup, &handoff, ptr::null_mut()),
                INVALID_ARGUMENT
            );
            assert_eq!(
                rsf_core_srvk_fsr4_create(disabled, &setup, &handoff, &mut owner),
                NOT_COMPILED
            );
            assert!(owner.is_null());

            let (mut width, mut height) = (9, 9);
            assert_eq!(
                rsf_core_srvk_fsr4_plan(disabled, ptr::null(), &mut width, &mut height),
                INVALID_ARGUMENT
            );
            assert_eq!((width, height), (0, 0));
            assert_eq!(
                rsf_core_srvk_fsr4_plan(disabled, commands, &mut width, &mut height),
                NOT_COMPILED
            );
            assert_eq!((width, height), (0, 0));
            assert_eq!(rsf_core_srvk_fsr4_destroy(disabled, ptr::null_mut()), 0);
            assert_eq!(rsf_core_srvk_fsr4_destroy(disabled, commands), NOT_COMPILED);
            assert_eq!(
                rsf_core_srvk_fsr4_submitted(disabled, commands, ptr::null_mut()),
                INVALID_ARGUMENT
            );
            assert_eq!(
                rsf_core_srvk_fsr4_submitted(disabled, ptr::null_mut(), commands),
                INVALID_ARGUMENT
            );
            assert_eq!(
                rsf_core_srvk_fsr4_submitted(disabled, commands, commands),
                NOT_COMPILED
            );

            let mut info = Info {
                struct_size: size_of::<Info>() as u32 - 1,
                abi_version: 99,
                adapter_luid: 7,
                provider_version: 2,
                vendor_id: 3,
                int8_compatibility: 4,
            };
            assert_eq!(
                rsf_core_srvk_fsr4_info(disabled, commands, &mut info),
                INVALID_ARGUMENT
            );
            assert_eq!(info.adapter_luid, 7);
            info.struct_size += 1;
            assert_eq!(
                rsf_core_srvk_fsr4_info(disabled, commands, &mut info),
                ABI_MISMATCH
            );
            assert_eq!(
                (
                    info.adapter_luid,
                    info.provider_version,
                    info.vendor_id,
                    info.int8_compatibility
                ),
                (0, 0, 0, 0)
            );
            info.abi_version = RSF_SRVK_FSR4_INFO_ABI_VERSION;
            assert_eq!(
                rsf_core_srvk_fsr4_info(disabled, commands, &mut info),
                NOT_COMPILED
            );

            let mut next = ptr::null_mut();
            let mut input = frame(&record);
            input.struct_size -= 1;
            input.abi_version = 99;
            assert_eq!(
                rsf_core_srvk_fsr4_evaluate(disabled, commands, commands, &input, &mut next),
                INVALID_ARGUMENT
            );
            assert_eq!(next, commands);
            input.struct_size += 1;
            assert_eq!(
                rsf_core_srvk_fsr4_evaluate(disabled, commands, commands, &input, &mut next),
                ABI_MISMATCH
            );
            input.abi_version = RSF_SRVK_ABI_VERSION;
            let probe = |record: &rsf_frame_record| {
                let mut next = ptr::null_mut();
                let result = rsf_core_srvk_fsr4_evaluate(
                    disabled,
                    commands,
                    commands,
                    &frame(record),
                    &mut next,
                );
                assert_eq!(next, commands);
                result
            };
            record.struct_size -= 1;
            assert_eq!(probe(&record), INVALID_ARGUMENT);
            record.struct_size += 1;
            record.abi_version = 99;
            assert_eq!(probe(&record), ABI_MISMATCH);
            record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
            record.camera.struct_size -= 1;
            assert_eq!(probe(&record), INVALID_ARGUMENT);
            record.camera.struct_size += 1;
            record.camera.abi_version = 99;
            assert_eq!(probe(&record), ABI_MISMATCH);
            record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
            assert_eq!(probe(&record), NOT_COMPILED);
            input.record = &record;
            input.reactive.image = 1;
            assert_eq!(
                rsf_core_srvk_fsr4_evaluate(disabled, commands, commands, &input, &mut next),
                NOT_SUPPORTED
            );
            assert_eq!(
                rsf_core_srvk_fsr4_evaluate(disabled, commands, commands, &input, ptr::null_mut()),
                INVALID_ARGUMENT
            );
        }
    }

    unsafe fn allocate(setup: *const Setup, handoff: *const Handoff, out: *mut Handle) -> i32 {
        let (setup, handoff) = unsafe { (&*setup, &*handoff) };
        assert_eq!((setup.output_width, handoff.timeline_enabled), (640, 1));
        let result = if handoff.abi_version == RSF_SRVK_SPLIT_TRACKED_ABI_VERSION {
            -71
        } else {
            0
        };
        unsafe { out.write(Box::into_raw(Box::new(result)).cast()) };
        result
    }

    static STUB: Provider = Provider {
        create: allocate,
        plan,
        evaluate,
        destroy,
        retire,
        submitted,
        info,
    };

    unsafe fn plan(_: *const c_void, w: *mut u32, h: *mut u32) -> i32 {
        unsafe {
            w.write(320);
            h.write(180);
        }
        0
    }

    unsafe fn evaluate(owner: Handle, _: Handle, frame: *const Frame, next: *mut Handle) -> i32 {
        assert_eq!(unsafe { (*frame).struct_size } as usize, size_of::<Frame>());
        unsafe { next.write(ptr::dangling_mut::<u16>().cast()) };
        unsafe { *owner.cast::<i32>() }
    }

    unsafe fn destroy(owner: Handle) -> i32 {
        let result = unsafe { *owner.cast::<i32>() };
        if result == 0 {
            unsafe { drop(Box::from_raw(owner.cast::<i32>())) };
        }
        result
    }

    unsafe fn retire(_: Handle, _: Handle) {}

    unsafe fn submitted(owner: Handle, _: Handle) -> i32 {
        unsafe { *owner.cast::<i32>() }
    }

    unsafe fn info(owner: *const c_void, out: *mut Info) -> i32 {
        let result = unsafe { *owner.cast::<i32>() };
        if result == 0 {
            let out = unsafe { &mut *out };
            out.adapter_luid = 0x123456789abcdef0;
            out.provider_version = 0x456789abcdef0123;
            out.vendor_id = 0x10de;
            out.int8_compatibility = 1;
        }
        result
    }

    #[test]
    fn split_routes_prefix_and_continuation_to_the_provider() {
        let (mut setup, mut handoff, record) = records();
        setup.struct_size += 8;
        handoff.struct_size += 8;
        let mut owner = ptr::null_mut();
        let commands = ptr::dangling_mut::<u8>().cast();
        let mut next = ptr::null_mut();
        let mut input = frame(&record);
        input.struct_size = FRAME_PREFIX as u32;
        #[repr(C)]
        struct Extended {
            info: Info,
            tail: u64,
        }
        let mut extended = Extended {
            info: Info {
                struct_size: size_of::<Extended>() as u32,
                abi_version: RSF_SRVK_FSR4_INFO_ABI_VERSION,
                ..unsafe { std::mem::zeroed() }
            },
            tail: 0xcafe,
        };
        unsafe {
            assert_eq!(
                rsf_core_srvk_fsr4_create(&STUB, &setup, &handoff, &mut owner),
                0
            );
            let (mut w, mut h) = (0, 0);
            assert_eq!(rsf_core_srvk_fsr4_plan(&STUB, owner, &mut w, &mut h), 0);
            assert_eq!((w, h), (320, 180));
            assert_eq!(rsf_core_srvk_fsr4_submitted(&STUB, owner, commands), 0);
            assert_eq!(rsf_core_srvk_fsr4_info(&STUB, owner, &mut extended.info), 0);
            assert_eq!(
                (
                    extended.info.adapter_luid,
                    extended.info.provider_version,
                    extended.info.vendor_id,
                    extended.info.int8_compatibility,
                    extended.tail
                ),
                (0x123456789abcdef0, 0x456789abcdef0123, 0x10de, 1, 0xcafe)
            );
            assert_eq!(
                rsf_core_srvk_fsr4_evaluate(&STUB, owner, commands, &input, &mut next),
                0
            );
            assert_eq!(rsf_core_srvk_fsr4_destroy(&STUB, owner), 0);

            handoff.abi_version = RSF_SRVK_SPLIT_TRACKED_ABI_VERSION;
            assert_eq!(
                rsf_core_srvk_fsr4_create(&STUB, &setup, &handoff, &mut owner),
                -71
            );
            assert!(!owner.is_null());
            assert_eq!(
                rsf_core_srvk_fsr4_info(&STUB, owner, &mut extended.info),
                -71
            );
            assert_eq!(
                (
                    extended.info.adapter_luid,
                    extended.info.provider_version,
                    extended.info.vendor_id,
                    extended.info.int8_compatibility,
                    extended.tail
                ),
                (0, 0, 0, 0, 0xcafe)
            );
            assert_eq!(rsf_core_srvk_fsr4_submitted(&STUB, owner, commands), -71);
            assert_eq!(
                rsf_core_srvk_fsr4_evaluate(&STUB, owner, commands, &input, &mut next),
                -71
            );
            assert_eq!(next, ptr::dangling_mut::<u16>().cast());
            assert_eq!(rsf_core_srvk_fsr4_destroy(&STUB, owner), -71);
            owner.cast::<i32>().write(0);
            assert_eq!(rsf_core_srvk_fsr4_destroy(&STUB, owner), 0);
        }
    }

    #[test]
    fn tracked_split_requires_ordered_submission_of_both_sections() {
        let mut lease = SplitLease::default();
        let producer = ptr::dangling_mut::<u32>().cast();
        let resumed = ptr::dangling_mut::<u64>().cast();
        lease.begin(producer);
        assert!(!lease.pending());
        assert!(!lease.submitted(producer));

        let mut lease = SplitLease::new(RSF_SRVK_SPLIT_TRACKED_ABI_VERSION);
        lease.begin(producer);
        lease.resume(resumed);
        assert!(lease.pending());
        assert!(!lease.submitted(resumed));
        assert!(!lease.submitted(ptr::dangling_mut::<u8>().cast()));
        assert!(lease.submitted(producer));
        assert!(lease.pending());
        assert!(!lease.submitted(producer));
        assert!(lease.submitted(resumed));
        assert!(!lease.pending());
        lease.begin(producer);
        assert!(lease.pending());
    }
}
