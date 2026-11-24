// SPDX-License-Identifier: MIT
//! Synthetic Streamline session and SDK entries for the production DLSS-G
//! leaves. The fixture copies the
//! production tables, replaces the SDK initialize leaf with one that installs
//! these entries, and runs the present capture owner with its real Vulkan,
//! WSI and registration leaves. Only open, frame selection, readback recording
//! and file output are synthetic.
use super::vulkan::{self, State};
use super::{Handle, Platform, Setup};
use crate::dlss_vulkan_capture::leaves::fixture_native;
use crate::dlss_vulkan_capture::{Owner as Capture, Platform as CapturePlatform};
use crate::sys::{
    RSF_SLVK_ABI_VERSION, rsf_slvk_call, rsf_slvk_sdk_constants, rsf_slvk_sdk_fg_options,
    rsf_slvk_sdk_fg_state, rsf_slvk_sdk_resource, rsf_slvk_sdk_tag, rsf_slvk_sdk_viewport,
};
use crate::vulkan_dlss_fg_abi::{Sdk, SdkCall};
use rsf_game_sys::backend_result::{FEATURE_FAILED, NOT_READY, OK};
use std::ffi::c_void;
use std::process;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{LazyLock, OnceLock};

const SDK_OK: i32 = 0;
const SDK_INVALID_PARAMETER: i32 = 25;
const DLSS_G: u32 = 1000;
const PREPARE_LIST: usize = 10;
const COLOR_LIST: usize = 11;

static SDK_TOKEN: u32 = 42;

fn require(condition: bool, message: &str) {
    if !condition {
        eprintln!("{message}");
        process::exit(1);
    }
}

fn fixture() -> &'static mut State {
    unsafe { &mut *vulkan::state_ptr() }
}

fn sdk_token() -> *const c_void {
    ptr::from_ref(&SDK_TOKEN).cast()
}

unsafe extern "C-unwind" fn set_options(
    view: *const rsf_slvk_sdk_viewport,
    value: *const rsf_slvk_sdk_fg_options,
) -> i32 {
    let (view, value) = unsafe { (&*view, &*value) };
    require(
        view.value == 0 && value.buffers == 3,
        "FG uses root viewport zero and enumerated image count",
    );
    require(
        value.queue_parallelism == 1,
        "SDK never queues a future timeline wait ahead of client binary waits",
    );
    require(
        value.ui_recomposition == 0,
        "missing UI producer does not enable recomposition",
    );
    require(
        value.base.version == 5
            && value.reserved_boolean == 2
            && value.target_rate == 0.0
            && value.reserved_callback.is_some(),
        "packed options keep the pinned defaults and the API-error callback",
    );
    require(
        value.flags & 8 != 0,
        "Off keeps resources available for checked retirement",
    );
    let state = fixture();
    if state.refuse_options {
        return SDK_INVALID_PARAMETER;
    }
    if value.mode == 1 {
        state.on_calls += 1;
        require(
            value.generated == 3,
            "MFG forwards generated count excluding real frame",
        );
    } else {
        state.off_calls += 1;
        require(value.generated == 1, "Off publishes the SDK baseline count");
    }
    SDK_OK
}

unsafe extern "C-unwind" fn get_state(
    _: *const rsf_slvk_sdk_viewport,
    value: *mut rsf_slvk_sdk_fg_state,
    _: *const rsf_slvk_sdk_fg_options,
) -> i32 {
    let value = unsafe { &mut *value };
    let state = fixture();
    value.max_generated = state.max_generated;
    value.min_dimension = 128;
    value.actual_presents = state.sdk_presented;
    state.sdk_presented = 0;
    value.inputs_fence = ptr::without_provenance_mut(0xc9);
    value.inputs_fence_value = state.completion_value;
    SDK_OK
}

unsafe extern "C-unwind" fn set_constants(
    data: *const rsf_slvk_sdk_constants,
    token: *const c_void,
    _: *const rsf_slvk_sdk_viewport,
) -> i32 {
    let data = unsafe { &*data };
    require(
        token == sdk_token(),
        "constants borrow the original SDK token",
    );
    require(
        data.motion_scale == [0.5, -0.5],
        "motion scale uses the render dimensions",
    );
    require(
        data.base.version == 2
            && data.aspect == 1280.0 / 720.0
            && data.clip_to_lens[0] == f32::MAX
            && data.motion_includes_camera == 1,
        "packed camera keeps SDK defaults and the presented aspect ratio",
    );
    require(
        data.previous_to_clip[0] == 1.0 && data.previous_to_clip[15] == 1.0,
        "the inverse fills the packed camera before dispatch",
    );
    SDK_OK
}

unsafe extern "C-unwind" fn set_tags(
    token: *const c_void,
    _: *const rsf_slvk_sdk_viewport,
    tags: *const rsf_slvk_sdk_tag,
    count: u32,
    commands: Handle,
) -> i32 {
    require(
        token == sdk_token() && count == 2,
        "tags borrow the original SDK token",
    );
    let tags = unsafe { std::slice::from_raw_parts(tags, 2) };
    let first = tags[0].resource.cast::<rsf_slvk_sdk_resource>();
    require(
        tags[0].lifecycle == 0 && !first.is_null() && unsafe { (*first).image }.addr() >= 1000,
        "tag uses an owned Vulkan image, not the engine image",
    );
    if commands.addr() == PREPARE_LIST {
        require(
            tags[0].type_ == 0 && tags[1].type_ == 1,
            "prepare uses typed guide tags",
        );
        require(
            tags[0].width == 640
                && tags[0].height == 360
                && (tags[1].top, tags[1].left, tags[1].width, tags[1].height)
                    == (tags[0].top, tags[0].left, tags[0].width, tags[0].height)
                && tags[1].lifecycle == 0,
            "both staged guides carry the render extent and copy lifetime",
        );
    } else {
        require(
            commands.addr() == COLOR_LIST
                && tags[0].type_ == 2
                && tags[1].type_ == 23
                && tags[1].resource.is_null(),
            "HUD-less explicitly clears absent UI recomposition input",
        );
        require(
            tags[0].width == 1280
                && tags[0].height == 720
                && tags[1].lifecycle == 1
                && tags[1].width == 0
                && tags[1].height == 0,
            "HUD-less uses the presented extent and the cleared UI tag has no extent",
        );
    }
    if fixture().refuse_tags {
        SDK_INVALID_PARAMETER
    } else {
        SDK_OK
    }
}

unsafe extern "C-unwind" fn free_resources(
    feature: u32,
    view: *const rsf_slvk_sdk_viewport,
) -> i32 {
    require(
        feature == DLSS_G && unsafe { (*view).value } == 0,
        "release addresses the shared viewport",
    );
    let state = fixture();
    state.release_calls += 1;
    if state.refuse_release {
        SDK_INVALID_PARAMETER
    } else {
        SDK_OK
    }
}

/// Installs the synthetic entries at the start of the production native owner.
unsafe extern "C" fn initialize(_: Handle, _: usize, user: Handle) -> i32 {
    let call = unsafe { &*user.cast::<SdkCall>() };
    let sdk = unsafe { &mut *call.native.cast::<Sdk>() };
    *sdk = Sdk {
        set_constants: Some(set_constants),
        set_tags: Some(set_tags),
        free_resources: Some(free_resources),
        set_options: Some(set_options),
        get_state: Some(get_state),
    };
    OK
}

/// The Streamline session invoke: versioned call, outside the host queue lock.
unsafe extern "C" fn invoke(call: *const rsf_slvk_call) -> i32 {
    let call = unsafe { &*call };
    require(
        !vulkan::locked()
            && call.struct_size as usize == size_of::<rsf_slvk_call>()
            && call.abi_version == RSF_SLVK_ABI_VERSION,
        "worker receives versioned call outside the host queue lock",
    );
    let state = fixture();
    if call.session_id != 0
        && (call.session_id != state.lease_session || call.frame_id != state.lease_frame)
    {
        return NOT_READY;
    }
    let token = if call.frame_id != 0 {
        sdk_token().addr()
    } else {
        0
    };
    unsafe { (call.callback.unwrap())(ptr::without_provenance_mut(1), token, call.user) }
}

struct Tables([Platform; 3]);
// The tables hold static code and data addresses only.
unsafe impl Sync for Tables {}
unsafe impl Send for Tables {}

unsafe extern "C" fn prepare_with_capture(
    native: Handle,
    enabled: *mut u32,
    gpu: *mut Handle,
) -> i32 {
    let result = unsafe { (super::leaves::LEAVES.prepare_chain)(native, enabled, gpu) };
    if result == OK {
        unsafe { enabled.write(1) };
    }
    result
}

unsafe extern "C" fn refuse_capture(_: Handle) -> i32 {
    FEATURE_FAILED
}

fn tables() -> &'static [Platform; 3] {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    &TABLES
        .get_or_init(|| {
            // The production leaves with the synthetic SDK entries, session invoke and worker epoch.
            let synthetic = Platform {
                initialize: Some(initialize),
                invoke,
                timeout_epoch: vulkan::timeout_epoch,
                ..super::leaves::LEAVES
            };
            Tables([
                synthetic,
                Platform {
                    prepare_chain: prepare_with_capture,
                    ..synthetic
                },
                Platform {
                    start_capture: refuse_capture,
                    ..synthetic
                },
            ])
        })
        .0
}

#[derive(Clone, Copy)]
pub(super) enum Variant {
    Normal,
    CaptureRequired,
    CaptureRefused,
}

/// The provider borrows the selected platform table until destruction.
pub(super) unsafe fn create(
    variant: Variant,
    setup: *const Setup,
    owner: *mut *mut super::Owner,
    chain: *mut u64,
) -> i32 {
    let tables = tables();
    let selected = match variant {
        Variant::Normal => &tables[0],
        Variant::CaptureRequired => &tables[1],
        Variant::CaptureRefused => &tables[2],
    };
    unsafe { super::rsf_core_dlgvk_create(Some(selected), setup, owner, chain) }
}

static SYNTHETIC_CAPTURE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

unsafe extern "C" fn open_capture(_: *const Setup, out: *mut Handle) -> i32 {
    unsafe { out.write(SYNTHETIC_CAPTURE.load(Ordering::Acquire)) };
    OK
}

unsafe extern "C" fn wants(_: Handle, frame: u64) -> u32 {
    u32::from(frame == 42)
}

/// A synthetic open and frame selection with the fixture's readback recording and file output.
static CAPTURE_TABLE: LazyLock<CapturePlatform> = LazyLock::new(|| CapturePlatform {
    open: open_capture,
    wants,
    readbacks: vulkan::readbacks,
    record: vulkan::record,
    flush: vulkan::flush,
    failure: vulkan::failure,
    append: vulkan::append,
});

pub(super) unsafe fn capture_open(
    setup: *const Setup,
    capture: *mut *mut Capture,
    calls: &mut vulkan::CaptureCalls,
) -> i32 {
    *calls = vulkan::capture_calls();
    let native = unsafe { fixture_native(setup, vulkan::register_capture) };
    require(
        !native.is_null(),
        "synthetic capture resolves its command and fence operations",
    );
    SYNTHETIC_CAPTURE.store(native, Ordering::Release);
    vulkan::reset_waited_fence();
    unsafe {
        crate::dlss_vulkan_capture::rsf_core_dlss_capture_open(&CAPTURE_TABLE, setup, capture)
    }
}
