// SPDX-License-Identifier: MIT
//! The native_ffx_vulkan_* fixtures (test-fixtures only): the FFX owner and its production Rust
//! SDK leaf over a fake FidelityFX 3 module (fake_sdk.rs) and fake Vulkan entries (fake_gpu.rs).
mod fake_gpu;
mod fake_sdk;

use self::fake_gpu::{route, state};
use self::fake_sdk::*;
use super::{
    Owner, rsf_ffxgvk_destroy, rsf_ffxgvk_drain, rsf_ffxgvk_enable, rsf_ffxgvk_get_status,
    rsf_ffxgvk_hudless, rsf_ffxgvk_images, rsf_ffxgvk_prepare, rsf_ffxgvk_present,
    rsf_ffxgvk_present_source, rsf_ffxgvk_submitted,
};
use crate::sys::{RSF_FGVK_ABI_VERSION, rsf_fgvk_frame, rsf_fgvk_status, rsf_srvk_image};
use crate::vulkan_fg_resources::Image as rsf_vkfg_image;
use crate::vulkan_fg_resources::{SAMPLED_TRANSFER, ensure_image, release_image};
use ash::vk::{self, Handle};
use rsf_game_sys::backend_result::{
    ABI_MISMATCH as FIXTURE_ABI_MISMATCH, FEATURE_FAILED as FIXTURE_FEATURE_FAILED,
    INIT_FAILED as FIXTURE_INIT_FAILED, INVALID_ARGUMENT as FIXTURE_INVALID_ARGUMENT,
    MISSING_ENTRY_POINT as FIXTURE_MISSING_ENTRY_POINT, NOT_COMPILED as FIXTURE_NOT_COMPILED,
    NOT_READY as FIXTURE_NOT_READY, NOT_SUPPORTED as FIXTURE_NOT_SUPPORTED, OK as FIXTURE_OK,
};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_RESET, rsf_frame_record,
};
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    ptr,
};

const VK_SUCCESS: i32 = 0;
const VK_INCOMPLETE: i32 = 5;
const VK_ERROR_INITIALIZATION_FAILED: i32 = -3;
const VK_ERROR_DEVICE_LOST: i32 = -4;
const VK_SUBOPTIMAL_KHR: i32 = 1_000_001_003;
const VK_ERROR_OUT_OF_DATE_KHR: i32 = -1_000_001_004;
const RGBA8: u32 = 37;

/// A field of the fake FidelityFX and Vulkan state; read only between provider calls.
macro_rules! f {
    ($field:ident) => {
        (*state()).$field
    };
}

fn cmd(address: usize) -> *mut c_void {
    ptr::without_provenance_mut(address)
}

fn record(session: u64, frame: u64, view: u32, generation: u32, flags: u32) -> rsf_frame_record {
    let mut result = rsf_frame_record {
        resource_generation: generation,
        flags,
        frame_time_ms: 16.667,
        ..rsf_test_support::frame_record(session, frame, view, [640, 360], [640, 360])
    };
    rsf_test_support::sr_camera(&mut result, 0.05, 100.0);
    result
}

fn frame(record: &rsf_frame_record) -> rsf_fgvk_frame {
    let image = |format| rsf_srvk_image {
        device: cmd(1),
        image: 12,
        format,
        width: 640,
        height: 360,
        layout: 5, // Shader read only.
        copy_source: 1,
    };
    rsf_fgvk_frame {
        struct_size: size_of::<rsf_fgvk_frame>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        record,
        depth: image(100), // R32_SFLOAT.
        motion: image(83), // R16G16_SFLOAT.
        hudless: image(RGBA8),
        motion_scale_x: 640.0,
        motion_scale_y: 360.0,
    }
}

fn status_record() -> rsf_fgvk_status {
    rsf_fgvk_status {
        struct_size: size_of::<rsf_fgvk_status>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        ..Default::default()
    }
}

unsafe fn complete(
    owner: *mut Owner,
    record: &rsf_frame_record,
    prepare: *mut c_void,
    hudless: *mut c_void,
) {
    let input = frame(record);
    unsafe {
        assert_eq!(rsf_ffxgvk_prepare(owner, prepare, &input), FIXTURE_OK);
        assert_eq!(rsf_ffxgvk_hudless(owner, hudless, &input), FIXTURE_OK);
    }
}

fn start(route: bool) -> *mut Owner {
    let mut owner = ptr::null_mut();
    let mut chain = 0;
    assert_eq!(
        unsafe { rsf_fixture_ffx_start(route, &mut owner, &mut chain) },
        FIXTURE_OK
    );
    assert!(!owner.is_null() && chain == 3);
    owner
}

extern "C" fn rsf_fixture_ffx_refused_second_owner() -> i32 {
    let mut owner = ptr::null_mut();
    let mut chain = 0;
    let result = unsafe { rsf_fixture_ffx_start(false, &mut owner, &mut chain) };
    assert!(owner.is_null() && chain == 0);
    result
}

unsafe fn implicit(owner: *mut Owner, chain: u64) -> i32 {
    let chains = [vk::SwapchainKHR::from_raw(chain)];
    let info = vk::PresentInfoKHR::default().swapchains(&chains);
    unsafe { rsf_ffxgvk_present(owner, ptr::from_ref(&info).cast()) }
}
unsafe fn explicit(owner: *mut Owner, chain: u64, source: *const rsf_frame_record) -> i32 {
    let chains = [vk::SwapchainKHR::from_raw(chain)];
    let info = vk::PresentInfoKHR::default().swapchains(&chains);
    unsafe { rsf_ffxgvk_present_source(owner, ptr::from_ref(&info).cast(), source) }
}

/// The launcher entry (tests/fixture_main_args.c): argv[1] selects the scenario. Capture and timing
/// output stay disabled; the provider entries are the Runtime archive's (vulkan_ffx_fg.rs).
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_ffx_run(argc: c_int, argv: *const *const c_char) -> i32 {
    if argc != 2 {
        return 1;
    }
    // SAFETY: no other thread runs before the scenario starts.
    unsafe {
        std::env::remove_var("RSF_VULKAN_FRAME_CAPTURE_DIR");
        std::env::remove_var("RSF_VULKAN_TIMING_FILE");
    }
    let scenario = unsafe { CStr::from_ptr(*argv.add(1)) }.to_bytes();
    unsafe { (*state()).next = 100 };
    match scenario {
        b"0" => reset(),
        b"1" => submission(),
        b"2" => {
            wsi_route();
            creation();
        }
        _ => return 1,
    }
    0
}
fn reset() {
    unsafe {
        let owner = start(false);
        let chain = rsf_fixture_ffx_chain();
        let [a, b] = [cmd(10), cmd(11)];
        f!(check_reset) = true;

        assert!(
            rsf_ffxgvk_enable(owner, 1) == FIXTURE_OK,
            "explicit FG enable is accepted"
        );
        let mut invalid = record(7, 1, 0, 1, 0);
        invalid.camera.world_to_view[0] = f32::NAN;
        let mut invalid_input = frame(&invalid);
        assert!(
            rsf_ffxgvk_prepare(owner, a, &invalid_input) == FIXTURE_INVALID_ARGUMENT
                && f!(sdk_records) == 0,
            "FidelityFX retains its world-to-view check after sharing source validation"
        );
        invalid = record(7, 1, 0, 1, 0);
        invalid.camera.motion_jittered = 1;
        invalid_input = frame(&invalid);
        assert!(
            rsf_ffxgvk_prepare(owner, a, &invalid_input) == FIXTURE_INVALID_ARGUMENT
                && f!(sdk_records) == 0,
            "FidelityFX retains its setup-specific guide validation"
        );
        for frame in 1..=4 {
            // Frame 1 starts a producer, frame 3 carries an engine camera reset.
            let source = record(
                7,
                frame,
                0,
                1,
                if frame == 3 { RSF_FRAME_FLAG_RESET } else { 0 },
            );
            complete(owner, &source, a, a);
            rsf_ffxgvk_submitted(owner, &a, 1);
            f!(expected_reset) = frame % 2 == 1;
            assert!(
                implicit(owner, chain) == VK_SUCCESS && f!(enabled) && f!(generated_frame) == frame,
                "submitted reset and ordinary frames keep the presenter active"
            );
        }
        assert!(
            f!(configurations) == 4
                && f!(activations) == 1
                && f!(generated) == 4
                && f!(submits) == 8,
            "camera resets update per-frame SDK inputs without presenter reconfiguration"
        );

        assert!(
            rsf_ffxgvk_enable(owner, 0) == FIXTURE_OK,
            "explicit FG disable is accepted"
        );
        let disabled = record(7, 5, 0, 1, 0);
        complete(owner, &disabled, a, a);
        rsf_ffxgvk_submitted(owner, &a, 1);
        assert!(
            implicit(owner, chain) == VK_SUCCESS
                && !f!(enabled)
                && f!(activations) == 2
                && f!(generated) == 4,
            "explicit disable still retires presenter activation"
        );
        let mut status = status_record();
        assert!(
            rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.enabled == 0
                && status.generated_callbacks == 4
                && status.prepared == 4
                && status.tagged == 4,
            "status counts only enabled preparation and SDK generation"
        );

        assert!(rsf_ffxgvk_enable(owner, 1) == FIXTURE_OK, "FG can resume");
        let failed_wait = record(7, 6, 0, 1, RSF_FRAME_FLAG_RESET);
        let input = frame(&failed_wait);
        assert!(
            rsf_ffxgvk_prepare(owner, a, &input) == FIXTURE_OK,
            "split prepare accepted"
        );
        rsf_ffxgvk_submitted(owner, &a, 1);
        assert!(
            rsf_ffxgvk_hudless(owner, b, &input) == FIXTURE_OK,
            "split HUD-less accepted"
        );
        f!(reject_wait) = true;
        rsf_ffxgvk_submitted(owner, &b, 1);
        f!(reject_wait) = false;
        assert!(
            implicit(owner, chain) == VK_SUCCESS && !f!(enabled) && f!(generated) == 4,
            "unknown GPU completion never admits an input lease"
        );

        // The ledger marks a producer change as a reset; the callback sees the
        // leased record's flags for the exact source each Present names.
        let changed = record(7, 7, 2, 1, 0);
        let steady = record(7, 8, 2, 1, 0);
        complete(owner, &changed, a, a);
        rsf_ffxgvk_submitted(owner, &a, 1);
        complete(owner, &steady, b, b);
        rsf_ffxgvk_submitted(owner, &b, 1);
        f!(expected_reset) = true;
        assert!(
            explicit(owner, chain, &changed) == VK_SUCCESS
                && f!(enabled)
                && f!(generated_frame) == 7,
            "an older explicit source carries its view-change reset into generation"
        );
        f!(expected_reset) = false;
        assert!(
            explicit(owner, chain, &steady) == VK_SUCCESS
                && f!(enabled)
                && f!(generated_frame) == 8,
            "the next explicit source generates without a reset"
        );
        let next = record(7, 9, 2, 1, 0);
        complete(owner, &next, a, a);
        rsf_ffxgvk_submitted(owner, &a, 1);
        let mut other_view = next;
        other_view.view_id += 1;
        let generated = f!(generated);
        assert!(
            explicit(owner, chain, &other_view) == VK_SUCCESS
                && !f!(enabled)
                && f!(generated) == generated
                && rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.source_frame == 0,
            "a mismatched view identity never leases the stored source"
        );
        assert!(
            explicit(owner, chain, &next) == VK_SUCCESS
                && f!(enabled)
                && f!(generated_frame) == 9
                && rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.source_frame == 9,
            "the matching identity still generates after a refused one"
        );

        let refused = record(7, 10, 2, 1, 0);
        complete(owner, &refused, a, a);
        rsf_ffxgvk_submitted(owner, &a, 1);
        f!(reject_generate) = true;
        assert!(
            explicit(owner, chain, &refused) == VK_SUCCESS
                && rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.generated_callbacks == u64::from(generated) + 1,
            "a failed SDK dispatch does not increment the generation count"
        );

        assert!(
            rsf_ffxgvk_destroy(owner) == FIXTURE_OK && !f!(enabled) && routed().is_null(),
            "teardown disables generation before releasing the presenter"
        );
    }
}

fn submission() {
    unsafe {
        let owner = start(false);
        let mut second = ptr::null_mut();
        let mut second_chain = 0;
        assert!(
            rsf_fixture_ffx_start(false, &mut second, &mut second_chain) == FIXTURE_NOT_SUPPORTED
                && second.is_null()
                && second_chain == 0,
            "one FFX owner holds the process-wide WSI route"
        );
        assert!(rsf_ffxgvk_enable(owner, 1) == FIXTURE_OK, "FG enabled");

        let gpu = rsf_fixture_ffx_gpu();
        let mut cached = rsf_vkfg_image::default();
        assert!(
            ensure_image(
                rsf_fixture_ffx_platform(),
                gpu,
                &mut cached,
                [640, 360],
                RGBA8,
                SAMPLED_TRANSFER,
                false
            )
            .is_ok(),
            "initial private image is fully bound"
        );
        let initial = cached.image;
        f!(reject_bind) = true;
        assert!(
            ensure_image(
                rsf_fixture_ffx_platform(),
                gpu,
                &mut cached,
                [1280, 720],
                RGBA8,
                SAMPLED_TRANSFER,
                false
            )
            .is_err()
                && cached.image == initial
                && cached.width == 640
                && f!(destroyed_images) == 1
                && f!(freed_memories) == 1,
            "failed private image replacement rolls back without touching the committed cache"
        );
        f!(reject_bind) = false;
        release_image(rsf_fixture_ffx_platform(), gpu, &mut cached);
        f!(destroyed_images) = 0;
        f!(freed_memories) = 0;

        let mut image_handles = [99; 8];
        let mut image_count = 8;
        assert!(
            rsf_ffxgvk_images(owner, &mut image_count, image_handles.as_mut_ptr()) == VK_SUCCESS
                && image_count == 3
                && image_handles[0] == 1000
                && image_handles[2] == 1002
                && image_handles[3] == 99,
            "oversized image capacity reports only initialized SDK handles"
        );
        image_count = 2;
        assert!(
            rsf_ffxgvk_images(owner, &mut image_count, image_handles.as_mut_ptr()) == VK_INCOMPLETE
                && image_count == 2,
            "short image capacity preserves Vulkan incomplete semantics"
        );
        image_count = 0;
        assert!(
            rsf_ffxgvk_images(owner, &mut image_count, ptr::null_mut()) == VK_SUCCESS
                && image_count == 3,
            "count-only image enumeration preserved"
        );

        let chain = rsf_fixture_ffx_chain();
        assert!(
            implicit(owner, 77) == VK_ERROR_INITIALIZATION_FAILED && f!(present_calls) == 0,
            "a foreign chain never reaches the FFX presenter"
        );

        let [a, b, c] = [cmd(10), cmd(11), cmd(12)];
        let one = record(7, 1, 0, 1, 0);
        complete(owner, &one, a, a);
        let duplicate = [a, a, ptr::null_mut()];
        rsf_ffxgvk_submitted(owner, duplicate.as_ptr(), 3);
        assert!(
            f!(submits) == 1 && f!(resets) == 1,
            "one command buffer with both roles gets exactly one completion fence"
        );
        rsf_ffxgvk_submitted(owner, duplicate.as_ptr(), 3);
        assert!(
            f!(submits) == 1 && f!(resets) == 1,
            "repeated notification never rearms an accepted old slot"
        );
        let two = record(7, 2, 0, 1, 0);
        let mut input = frame(&two);
        assert!(
            rsf_ffxgvk_prepare(owner, a, &input) == FIXTURE_OK,
            "reused command address records another source"
        );
        rsf_ffxgvk_submitted(owner, &a, 1);
        assert!(
            f!(submits) == 2 && f!(resets) == 2,
            "prepare-only source is fenced without rearming the older accepted source"
        );
        let three = record(7, 3, 0, 1, 0);
        complete(owner, &three, b, b);
        let four = record(7, 4, 2, 1, 0);
        complete(owner, &four, b, b);
        rsf_ffxgvk_submitted(owner, &b, 1);
        assert!(
            f!(submits) == 3,
            "only the newest recording on a reused command buffer is fenced"
        );
        assert!(
            explicit(owner, chain, &three) == VK_SUCCESS && !f!(enabled),
            "an abandoned recording cannot generate"
        );
        assert!(
            implicit(owner, chain) == VK_SUCCESS
                && f!(enabled)
                && f!(generated) == 1
                && f!(generated_frame) == 4,
            "newest submitted source is presentation eligible and leased to the SDK callback"
        );
        assert!(
            f!(submits) == 4,
            "the generated source receives a consumer fence"
        );

        let five = record(7, 5, 0, 1, 0);
        input = frame(&five);
        f!(reject_prepare) = true;
        assert!(
            rsf_ffxgvk_prepare(owner, c, &input) == FIXTURE_FEATURE_FAILED,
            "SDK recording failure remains distinct from accepted FG preparation"
        );
        f!(reject_prepare) = false;
        assert!(
            rsf_ffxgvk_drain(owner) == FIXTURE_NOT_READY
                && rsf_ffxgvk_destroy(owner) == FIXTURE_NOT_READY
                && f!(drains) == 0
                && f!(effects_destroyed) == 0,
            "failed SDK recording retains its owner until the host submits the partial work"
        );
        let mut before = f!(submits);
        rsf_ffxgvk_submitted(owner, &c, 1);
        assert!(
            f!(submits) == before + 1,
            "partially recorded SDK failure still gets submission completion"
        );
        assert!(
            implicit(owner, chain) == VK_SUCCESS && !f!(enabled),
            "failed prepare does not replay the previous source"
        );

        let six = record(7, 6, 0, 1, 0);
        input = frame(&six);
        assert!(
            rsf_ffxgvk_prepare(owner, a, &input) == FIXTURE_OK,
            "split prepare accepted"
        );
        rsf_ffxgvk_submitted(owner, &a, 1);
        before = f!(submits);
        let earlier_waits = f!(waits);
        assert!(
            rsf_ffxgvk_hudless(owner, b, &input) == FIXTURE_OK,
            "split HUD-less accepted"
        );
        assert!(
            rsf_ffxgvk_drain(owner) == FIXTURE_NOT_READY,
            "submitted guides do not permit retirement of a later unsubmitted HUD-less copy"
        );
        rsf_ffxgvk_submitted(owner, &b, 1);
        assert!(
            f!(submits) == before + 1 && f!(waits) == earlier_waits + 1,
            "later role waits prior fence before establishing a new completion point"
        );
        rsf_ffxgvk_submitted(owner, &b, 1);
        assert!(
            f!(submits) == before + 1,
            "split duplicate submission is idempotent"
        );
        assert!(
            implicit(owner, chain) == VK_SUCCESS && f!(enabled),
            "split submitted roles can generate"
        );

        let seven = record(7, 7, 0, 1, 0);
        input = frame(&seven);
        assert!(
            rsf_ffxgvk_prepare(owner, a, &input) == FIXTURE_OK,
            "completed ring slot can be reused"
        );
        rsf_ffxgvk_submitted(owner, &a, 1);
        assert!(
            rsf_ffxgvk_hudless(owner, b, &input) == FIXTURE_OK,
            "later role recorded"
        );
        before = f!(resets);
        f!(reject_wait) = true;
        rsf_ffxgvk_submitted(owner, &b, 1);
        f!(reject_wait) = false;
        assert!(
            f!(resets) == before,
            "positive fence timeout preserves unknown completion without reset"
        );
        assert!(
            implicit(owner, chain) == VK_SUCCESS && !f!(enabled),
            "unknown completion refuses FG"
        );

        let new_session = record(99, 2, 8, 3, 0);
        input = frame(&new_session);
        assert!(
            rsf_ffxgvk_prepare(owner, c, &input) == FIXTURE_OK,
            "new session can restart original source IDs"
        );
        for kind in 0..3 {
            let mut wrong_source = new_session;
            if kind == 0 {
                wrong_source.session_id += 1;
            }
            if kind == 1 {
                wrong_source.view_id += 1;
            }
            if kind == 2 {
                wrong_source.resource_generation += 1;
            }
            let wrong = frame(&wrong_source);
            let previous_copies = f!(copies);
            assert!(
                rsf_ffxgvk_hudless(owner, c, &wrong) == FIXTURE_INVALID_ARGUMENT
                    && f!(copies) == previous_copies,
                "same numeric frame never joins another session, view or resource generation"
            );
        }
        assert!(
            rsf_ffxgvk_hudless(owner, c, &input) == FIXTURE_OK,
            "matching identity completes new session"
        );
        rsf_ffxgvk_submitted(owner, &c, 1);
        let mut status = status_record();
        assert!(
            implicit(owner, chain) == VK_SUCCESS
                && f!(enabled)
                && rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.enabled != 0
                && status.source_frame == 2,
            "presentation admission uses the local recording order, not old session frame IDs"
        );
        assert!(
            implicit(owner, chain) == VK_SUCCESS && !f!(enabled),
            "same recording cannot present twice"
        );

        let older = record(99, 3, 8, 3, 0);
        let newer = record(99, 4, 8, 3, 0);
        complete(owner, &older, a, a);
        rsf_ffxgvk_submitted(owner, &a, 1);
        complete(owner, &newer, b, b);
        rsf_ffxgvk_submitted(owner, &b, 1);
        assert!(
            explicit(owner, chain, &older) == VK_SUCCESS
                && f!(enabled)
                && f!(generated_frame) == 3
                && rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.source_frame == 3,
            "older output selects its own submitted guides without rewinding the producer"
        );
        assert!(
            explicit(owner, chain, &newer) == VK_SUCCESS && f!(enabled) && f!(generated_frame) == 4,
            "newer prepared source remains available after the older Present"
        );
        let mut invalid_source = newer;
        invalid_source.struct_size = 4;
        assert!(
            explicit(owner, chain, &invalid_source) == VK_ERROR_INITIALIZATION_FAILED,
            "short Present identity refused"
        );
        invalid_source = newer;
        invalid_source.abi_version += 1;
        assert!(
            explicit(owner, chain, &invalid_source) == VK_ERROR_INITIALIZATION_FAILED,
            "foreign Present identity ABI refused"
        );
        let mut foreign_source = record(99, 5, 8, 3, 0);
        complete(owner, &foreign_source, a, a);
        rsf_ffxgvk_submitted(owner, &a, 1);
        foreign_source.session_id += 1;
        assert!(
            explicit(owner, chain, &foreign_source) == VK_SUCCESS
                && !f!(enabled)
                && rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.source_frame == 0,
            "foreign identity retains ordinary output without selecting latest guides"
        );
        let forbidden_source = record(99, 6, 8, 3, RSF_FRAME_FLAG_NO_FG);
        complete(owner, &forbidden_source, b, b);
        rsf_ffxgvk_submitted(owner, &b, 1);
        let mut explicit_without_veto = forbidden_source;
        explicit_without_veto.flags = 0;
        assert!(
            explicit(owner, chain, &explicit_without_veto) == VK_SUCCESS && !f!(enabled),
            "a matching explicit identity cannot clear the prepared source's veto"
        );
        let ambiguous = record(98, 6, 8, 3, RSF_FRAME_FLAG_AMBIGUOUS_ID);
        complete(owner, &ambiguous, b, b);
        rsf_ffxgvk_submitted(owner, &b, 1);
        assert!(
            implicit(owner, chain) == VK_SUCCESS && !f!(enabled),
            "implicit presentation applies the prepared source's veto"
        );
        // Frame 7 maps to the slot whose HUD-less completion stayed unknown.
        let stuck = record(99, 7, 8, 3, 0);
        let records = f!(sdk_records);
        assert!(
            rsf_ffxgvk_prepare(owner, a, &frame(&stuck)) == FIXTURE_NOT_READY
                && f!(sdk_records) == records,
            "a slot with unknown completion is never reused"
        );
        let mut late_veto = record(99, 9, 8, 3, 0);
        complete(owner, &late_veto, a, a);
        rsf_ffxgvk_submitted(owner, &a, 1);
        late_veto.flags |= RSF_FRAME_FLAG_NO_FG;
        assert!(
            explicit(owner, chain, &late_veto) == VK_SUCCESS && !f!(enabled),
            "a matching output can veto interpolation after valid prepare"
        );
        assert!(
            implicit(owner, chain) == VK_SUCCESS && !f!(enabled),
            "ordinary presentation consumes the matched source even after a late veto"
        );
        assert!(
            explicit(owner, chain, ptr::null()) == VK_SUCCESS
                && !f!(enabled)
                && rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.source_frame == 0,
            "unassociated output never guesses a prepared source"
        );

        // presentInterpolated submits interpolation before it returns an earlier
        // asynchronous present result. That source still needs its consumer fence.
        let late = record(99, 8, 8, 3, 0);
        complete(owner, &late, c, c);
        rsf_ffxgvk_submitted(owner, &c, 1);
        before = f!(submits);
        f!(present_result) = VK_ERROR_OUT_OF_DATE_KHR;
        assert!(
            implicit(owner, chain) == VK_ERROR_OUT_OF_DATE_KHR
                && f!(enabled)
                && f!(submits) == before + 1
                && rsf_ffxgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.result == FIXTURE_FEATURE_FAILED
                && status.enabled != 0,
            "an error result after interpolation keeps the HUD-less consumer fence"
        );
        f!(present_result) = VK_SUCCESS;
        let waits_before_reuse = f!(waits);
        let reuse = record(99, 14, 8, 3, 0);
        input = frame(&reuse);
        assert!(
            rsf_ffxgvk_prepare(owner, a, &input) == FIXTURE_OK
                && f!(waits) == waits_before_reuse + 2,
            "slot reuse waits for both the source and the consumer completion"
        );
        rsf_ffxgvk_submitted(owner, &a, 1);
        assert!(
            f!(sdk_records) >= 8,
            "the scenarios exercised real SDK prepare callsites"
        );

        let failed_signal = record(99, 20, 8, 3, 0);
        complete(owner, &failed_signal, a, a);
        let submit_attempts = f!(submits);
        let generated = f!(generated);
        f!(reject_submit) = true;
        rsf_ffxgvk_submitted(owner, &a, 1);
        f!(reject_submit) = false;
        assert_eq!(f!(submits), submit_attempts + 1);
        assert_eq!(explicit(owner, chain, &failed_signal), VK_SUCCESS);
        assert!(
            !f!(enabled) && f!(generated) == generated,
            "a failed fence submission cannot certify a source"
        );

        // Teardown keeps every resource until each completion step succeeds.
        f!(reject_drain) = true;
        assert!(
            rsf_ffxgvk_destroy(owner) == FIXTURE_NOT_READY
                && f!(effects_destroyed) == 0
                && f!(chains_destroyed) == 0,
            "an unproven drain retains the owner and SDK contexts"
        );
        f!(reject_drain) = false;
        f!(reject_effect_destroy) = true;
        let drains = f!(drains);
        assert!(
            rsf_ffxgvk_destroy(owner) == FIXTURE_FEATURE_FAILED
                && f!(drains) == drains + 1
                && f!(chains_destroyed) == 0,
            "a refused effect destruction retains the chain and swapchain context"
        );
        image_count = 0;
        let later = record(99, 15, 8, 3, 0);
        input = frame(&later);
        assert!(
            implicit(owner, chain) == VK_ERROR_INITIALIZATION_FAILED
                && rsf_ffxgvk_images(owner, &mut image_count, ptr::null_mut())
                    == VK_ERROR_INITIALIZATION_FAILED
                && rsf_ffxgvk_prepare(owner, a, &input) == FIXTURE_INVALID_ARGUMENT,
            "a partially retired owner refuses new recording and presentation"
        );
        f!(reject_effect_destroy) = false;
        f!(reject_swapchain_destroy) = true;
        assert!(
            rsf_ffxgvk_destroy(owner) == FIXTURE_FEATURE_FAILED
                && f!(effects_destroyed) == 1
                && f!(chains_destroyed) == 1
                && routed() == rsf_fixture_ffx_native(),
            "the chain goes before a refused swapchain context, and the WSI route stays registered"
        );
        f!(reject_swapchain_destroy) = false;
        let fences_before = f!(destroyed_fences);
        assert!(
            rsf_ffxgvk_destroy(owner) == FIXTURE_OK
                && f!(effects_destroyed) == 1
                && f!(chains_destroyed) == 1
                && f!(swapchains_destroyed) == 1
                && f!(destroyed_images) == 6
                && f!(destroyed_fences) == fences_before + 12
                && routed().is_null(),
            "retry completes teardown without repeating released steps and releases the WSI route last"
        );
    }
}

fn wsi_route() {
    let route = route();
    macro_rules! r {
        ($field:ident) => {
            (*route).$field
        };
    }
    unsafe {
        let owner = start(true);
        let native = rsf_fixture_ffx_native();
        assert!(
            routed() == native && r!(registered) == 1,
            "Rust publishes the registered leaf to the WSI route"
        );

        let [device, foreign] = [1, 2];
        let mut chain = 0;
        let mut index = 0;
        let mut count = 0;
        assert!(
            rsf_fixture_wsi_create(foreign, &mut chain) == VK_ERROR_INITIALIZATION_FAILED
                && rsf_fixture_wsi_acquire(foreign, chain, &mut index)
                    == VK_ERROR_INITIALIZATION_FAILED
                && rsf_fixture_wsi_images(foreign, chain, &mut count)
                    == VK_ERROR_INITIALIZATION_FAILED
                && rsf_fixture_wsi_device_proc(foreign) == 0,
            "a foreign device is not routed"
        );
        rsf_fixture_wsi_destroy(foreign, 40);
        assert!(
            r!(creates) == 0
                && r!(destroys) == 0
                && r!(acquires) == 0
                && r!(enumerations) == 0
                && r!(lookups) == 0,
            "no foreign call was forwarded"
        );
        assert!(
            rsf_fixture_wsi_create(device, &mut chain) == VK_SUCCESS
                && chain == 40
                && rsf_fixture_wsi_acquire(device, chain, &mut index) == VK_SUCCESS
                && index == 2
                && rsf_fixture_wsi_images(device, chain, &mut count) == VK_INCOMPLETE
                && count == 3,
            "device hooks forward their Vulkan results unchanged"
        );
        assert!(
            rsf_fixture_wsi_device_proc(device) == 1 && r!(lookups) == 2,
            "the backend lookup falls back to the core alias"
        );
        let submits = f!(submits);
        assert!(
            rsf_fixture_wsi_graphics_submit(77) == VK_SUCCESS && f!(submits) == submits + 1,
            "the game-queue submit reaches the host"
        );

        assert!(
            rsf_fixture_wsi_present(22) == VK_SUBOPTIMAL_KHR && r!(presents) == 1,
            "present forwards its result"
        );

        r!(destroy_inside) = true;
        assert!(
            rsf_fixture_wsi_present(22) == VK_SUBOPTIMAL_KHR && r!(presents) == 2,
            "the borrowed present still returns its result"
        );
        assert!(
            r!(destroyed_inside) == FIXTURE_NOT_READY
                && r!(routed_inside).is_null()
                && r!(created_inside) == FIXTURE_NOT_SUPPORTED
                && r!(unregistered) == 0,
            "a hook inside its callback keeps the leaf, its registration and the admission"
        );
        assert!(
            f!(effects_destroyed) == 1
                && f!(chains_destroyed) == 1
                && f!(swapchains_destroyed) == 1,
            "the SDK contexts were released before the route refused the close"
        );
        r!(destroy_inside) = false;
        assert!(
            rsf_fixture_wsi_present(22) == VK_ERROR_INITIALIZATION_FAILED
                && rsf_fixture_wsi_graphics_submit(77) == VK_ERROR_DEVICE_LOST
                && r!(presents) == 2,
            "a closing route forwards no new callback"
        );

        r!(refuse_unregister) = true;
        assert!(
            rsf_ffxgvk_destroy(owner) == FIXTURE_FEATURE_FAILED
                && r!(unregistered) == 0
                && rsf_fixture_ffx_refused_second_owner() == FIXTURE_NOT_SUPPORTED,
            "a refused unregister retains admission and the leaf for retry"
        );
        r!(refuse_unregister) = false;
        assert!(
            rsf_ffxgvk_destroy(owner) == FIXTURE_OK && r!(unregistered) == 1 && routed().is_null(),
            "the retry closes the leaf once, after its last borrow"
        );
        assert!(
            f!(effects_destroyed) == 1
                && f!(chains_destroyed) == 1
                && f!(swapchains_destroyed) == 1,
            "the retry repeats no SDK release"
        );
        assert!(
            rsf_fixture_wsi_present(22) == VK_ERROR_INITIALIZATION_FAILED
                && rsf_fixture_wsi_device_proc(device) == 0,
            "a late hook after retirement touches no freed leaf"
        );

        let successor = start(true);
        assert!(
            routed() == rsf_fixture_ffx_native()
                && rsf_ffxgvk_destroy(successor) == FIXTURE_OK
                && routed().is_null(),
            "the released route admits a successor"
        );
    }
}

/// Creation: admission refuses before the module loads, a refused rollback returns the owner for a
/// destroy retry, and a failed open releases the WSI route.
fn creation() {
    let create = |host: bool, edit: Edit| {
        let (mut owner, mut chain) = (ptr::null_mut(), 0);
        let result = unsafe { rsf_fixture_ffx_create(host, edit, &mut owner, &mut chain) };
        (result, owner, chain)
    };
    unsafe {
        let loads = f!(loads);
        let refusals: [(Edit, i32); 4] = [
            (
                |setup, _| setup.queues[3] = setup.queues[0],
                FIXTURE_INVALID_ARGUMENT,
            ),
            (|setup, _| setup.abi_version += 1, FIXTURE_ABI_MISMATCH),
            (|_, info| info.image_array_layers = 2, FIXTURE_NOT_SUPPORTED),
            (
                |_, info| info.old_swapchain = vk::SwapchainKHR::from_raw(5),
                FIXTURE_NOT_SUPPORTED,
            ),
        ];
        for (edit, expected) in refusals {
            let (result, owner, chain) = create(true, edit);
            assert!(
                result == expected && owner.is_null() && chain == 0,
                "admission refuses a duplicate queue, another ABI and an unsupported swapchain"
            );
        }
        let (result, owner, chain) = create(false, |_, _| {});
        assert!(
            result == FIXTURE_NOT_COMPILED && owner.is_null() && chain == 0,
            "the build without the SDK headers validates, then reports NOT_COMPILED"
        );
        assert!(f!(loads) == loads, "refused admission loads no SDK module");

        let (effects, chains, swapchains) = (
            f!(effects_destroyed),
            f!(chains_destroyed),
            f!(swapchains_destroyed),
        );
        let (configurations, drains) = (f!(configurations), f!(drains));
        let unregistered = (*route()).unregistered;
        f!(reject_provider) = true;
        f!(reject_swapchain_destroy) = true;
        let (result, owner, chain) = create(true, |_, _| {});
        f!(reject_provider) = false;
        assert!(
            result == FIXTURE_INIT_FAILED && !owner.is_null() && chain == 0,
            "a refused rollback returns the owner for a destroy retry"
        );
        assert!(
            f!(configurations) == configurations
                && f!(drains) == drains
                && f!(effects_destroyed) == effects + 1
                && f!(chains_destroyed) == chains + 1
                && f!(swapchains_destroyed) == swapchains
                && (*route()).unregistered == unregistered,
            "an owner that never became available releases what it can without a disable or drain"
        );
        assert!(
            rsf_ffxgvk_drain(owner) == FIXTURE_OK && f!(drains) == drains,
            "a retiring owner has nothing left to drain"
        );
        f!(reject_swapchain_destroy) = false;
        assert!(
            rsf_ffxgvk_destroy(owner) == FIXTURE_OK
                && f!(effects_destroyed) == effects + 1
                && f!(chains_destroyed) == chains + 1
                && f!(swapchains_destroyed) == swapchains + 1
                && (*route()).unregistered == unregistered + 1
                && routed().is_null(),
            "the retry finishes the rollback without repeating a release"
        );

        f!(missing_export) = true;
        let (result, owner, chain) = create(true, |_, _| {});
        f!(missing_export) = false;
        assert!(
            result == FIXTURE_MISSING_ENTRY_POINT && owner.is_null() && chain == 0,
            "a failed open closes its partial leaf"
        );
        let successor = start(false);
        assert!(
            rsf_ffxgvk_destroy(successor) == FIXTURE_OK,
            "the failed open released the WSI route"
        );
    }
}
