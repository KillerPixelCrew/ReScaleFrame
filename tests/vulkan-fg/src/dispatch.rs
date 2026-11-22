// SPDX-License-Identifier: MIT
use crate::dispatch_drivers::{block_sleep, fixture_state, initialize, release_sleep, wait_sleep};
use crate::sys::*;
use rsf_game_sys::{RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record};
use std::ptr;

struct Lease {
    owner: *mut rsf_fgvk,
    closing: bool,
}

impl Lease {
    fn borrow() -> Self {
        let mut closing = 0;
        let owner = unsafe { rsf_core_fgvk_presentation_borrow(&mut closing) };
        Self {
            owner,
            closing: closing != 0,
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.owner.is_null() {
            unsafe { rsf_core_fgvk_presentation_return() };
        }
    }
}

fn host_options(intel_runtime_directory: *const u16) -> rsf_fgvk_host_options {
    rsf_fgvk_host_options {
        struct_size: size_of::<rsf_fgvk_host_options>() as u32,
        abi_version: RSF_FGVK_HOST_ABI_VERSION,
        window: ptr::without_provenance_mut(17),
        timeline_enabled: 1,
        intel_runtime_directory,
    }
}

fn verify_sleep_borrow(owner: *mut rsf_fgvk) {
    // Release the fake SDK's blocked sleep even if a main-thread assertion fails.
    struct ReleaseSleep;
    impl Drop for ReleaseSleep {
        fn drop(&mut self) {
            release_sleep();
        }
    }

    block_sleep();
    let address = owner as usize;
    std::thread::scope(|scope| {
        let sleeping = scope.spawn(move || {
            let lease = Lease::borrow();
            assert_eq!(
                lease.owner as usize, address,
                "successful creation binds latency context"
            );
            assert_eq!(
                unsafe { rsf_fgvk_latency_sleep(lease.owner, 42) },
                FIXTURE_OK
            );
        });
        let release = ReleaseSleep;
        assert!(wait_sleep(), "provider sleep entered");
        assert_eq!(unsafe { rsf_fgvk_drain(owner) }, FIXTURE_NOT_READY);
        assert_eq!(unsafe { rsf_fgvk_destroy(owner) }, FIXTURE_NOT_READY);
        assert!(
            Lease::borrow().owner.is_null(),
            "closing provider refuses new CPU users"
        );
        drop(release);
        sleeping.join().unwrap();
    });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_vulkan_dispatch_run(fsr4: bool, xess: bool) -> i32 {
    // Access the fixture record only between provider calls.
    initialize();
    let fixture = fixture_state();
    macro_rules! f {
        ($field:ident) => {
            (*fixture).$field
        };
    }
    unsafe {
        let mut identity = rsf_frame_record {
            struct_size: std::mem::offset_of!(rsf_frame_record, phase) as u32,
            ..std::mem::zeroed()
        };
        identity.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        identity.frame_id = 42;
        identity.session_id = 7;
        let mut setup = rsf_fgvk_setup {
            struct_size: size_of::<rsf_fgvk_setup>() as u32,
            abi_version: RSF_FGVK_ABI_VERSION,
            ..Default::default()
        };
        setup.graphics.struct_size = size_of_val(&setup.graphics) as u32;
        setup.graphics.abi_version = RSF_SRVK_ABI_VERSION;
        setup.graphics.backend = 5; // SR selection must not choose the FG provider.
        let mut owner = ptr::null_mut();
        let mut chain = 0;
        assert!(
            rsf_fgvk_create_for_backend(ptr::null(), 1, 1, &mut owner, &mut chain)
                == FIXTURE_INVALID_ARGUMENT
                && owner.is_null()
                && chain == 0,
            "null setup clears outputs"
        );
        setup.struct_size -= 1;
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_INVALID_ARGUMENT,
            "short setup refused"
        );
        setup.struct_size += 1;
        setup.abi_version += 1;
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_ABI_MISMATCH,
            "setup ABI refused"
        );
        setup.abi_version -= 1;
        let mut invalid = setup;
        invalid.abi_version += 1;
        invalid.graphics = Default::default();
        assert!(
            rsf_fgvk_create(&invalid, &mut owner, &mut chain) == FIXTURE_ABI_MISMATCH,
            "outer ABI mismatch takes precedence over embedded size"
        );
        invalid = setup;
        invalid.graphics.struct_size -= 1;
        invalid.graphics.abi_version += 1;
        assert!(
            rsf_fgvk_create(&invalid, &mut owner, &mut chain) == FIXTURE_INVALID_ARGUMENT,
            "embedded size takes precedence over embedded ABI"
        );
        invalid = setup;
        invalid.graphics.abi_version += 1;
        assert!(
            rsf_fgvk_create(&invalid, &mut owner, &mut chain) == FIXTURE_ABI_MISMATCH,
            "embedded ABI refused"
        );
        assert!(
            rsf_fgvk_create_for_backend(&setup, 2, 1, &mut owner, &mut chain)
                == FIXTURE_NOT_SUPPORTED
                && f!(ffx_creates) == 0
                && f!(dlss_creates) == 0,
            "unsupported FG provider never allocates a concrete owner"
        );
        assert!(
            rsf_fgvk_create_for_backend(&setup, 3, 2, &mut owner, &mut chain)
                == FIXTURE_NOT_SUPPORTED
                && f!(ffx_creates) == 0,
            "FFX unsupported MFG request never clamps or allocates"
        );
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_OK
                && !owner.is_null()
                && chain == 33
                && f!(borrowed_setup) == ptr::from_ref(&setup)
                && f!(enabled) == 0,
            "legacy create preserves FFX/count1 and borrowed setup"
        );
        let mut value = 0;
        assert!(
            rsf_fgvk_backend(owner, &mut value) == FIXTURE_OK && value == 3,
            "legacy FG independent of XeSS SR selection"
        );
        assert!(
            rsf_fgvk_image_layout(owner, &mut value) == FIXTURE_OK && value == 5,
            "FFX image layout"
        );
        assert!(
            rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_OK && value == 1,
            "FFX count limit"
        );
        let mut second = ptr::null_mut();
        let mut second_chain = 0;
        assert!(
            rsf_fgvk_create_for_backend(&setup, 1, 3, &mut second, &mut second_chain)
                == FIXTURE_NOT_READY
                && second.is_null()
                && second_chain == 0
                && f!(dlss_creates) == 0,
            "single admission spans both providers"
        );
        assert!(
            rsf_fgvk_enable_options(owner, 1, 2) == FIXTURE_NOT_SUPPORTED && f!(enabled) == 0,
            "invalid FFX options preserve request"
        );
        assert!(
            rsf_fgvk_enable(owner, 1) == FIXTURE_OK && f!(enabled) != 0 && f!(generated_count) == 1,
            "legacy enable preserves count1"
        );
        let command = ptr::without_provenance_mut(1);
        let frame = rsf_fgvk_frame::default();
        let info = ptr::from_ref(&frame).cast();
        assert!(
            rsf_fgvk_prepare(owner, command, &frame) == FIXTURE_OK
                && rsf_fgvk_hudless(owner, command, &frame) == FIXTURE_OK,
            "FFX recording routed"
        );
        rsf_fgvk_submitted(owner, &command, 1);
        assert!(
            rsf_fgvk_present(owner, info) == 1000001003 && f!(calls) == 15,
            "FFX submitted/Present and result routed"
        );
        assert!(
            rsf_fgvk_present_source(owner, info, &identity) == 1000001003
                && f!(presented_source) == ptr::from_ref(&identity),
            "FFX explicit identity prefix and WSI result forwarded"
        );
        let mut malformed = identity;
        malformed.struct_size -= 1;
        let before_invalid = f!(explicit_presents);
        assert!(
            rsf_fgvk_present_source(owner, info, &malformed) == -3
                && f!(explicit_presents) == before_invalid,
            "short source prefix never reaches WSI provider"
        );
        malformed = identity;
        malformed.abi_version += 1;
        assert!(
            rsf_fgvk_present_source(owner, info, &malformed) == -3
                && f!(explicit_presents) == before_invalid,
            "source ABI mismatch never reaches WSI provider"
        );
        assert!(
            rsf_fgvk_present_source(owner, info, ptr::null()) == 1000001003
                && f!(presented_source).is_null(),
            "unassociated output is passed explicitly, not replaced by a cached source"
        );
        assert!(
            rsf_fgvk_acquire(owner, 123, 4, 5, &mut value) == 2 && value == 2,
            "positive Vulkan timeout preserved"
        );
        let mut status = rsf_fgvk_status {
            struct_size: size_of::<rsf_fgvk_status>() as u32,
            abi_version: RSF_FGVK_ABI_VERSION,
            ..Default::default()
        };
        assert!(
            rsf_fgvk_get_status(owner, &mut status) == FIXTURE_OK && status.prepared == 30,
            "FFX status routed"
        );
        f!(refuse_destroy) = true;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_NOT_READY
                && rsf_fgvk_backend(owner, &mut value) == FIXTURE_OK
                && value == 3,
            "failed destroy retains concrete owner"
        );
        assert!(
            rsf_fgvk_create_for_backend(&setup, 1, 3, &mut second, &mut second_chain)
                == FIXTURE_NOT_READY
                && f!(dlss_creates) == 0,
            "failed destroy retains global admission"
        );
        f!(refuse_destroy) = false;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "FFX retirement retry succeeds"
        );
        assert!(
            rsf_fgvk_create_for_backend(&setup, 1, 3, &mut owner, &mut chain) == FIXTURE_OK
                && chain == 11
                && f!(generated_count) == 3,
            "DLSS creation forwards MFG count3"
        );
        assert!(
            rsf_fgvk_enable(owner, 1) == FIXTURE_OK && f!(enabled) != 0 && f!(generated_count) == 3,
            "legacy enable retains selected DLSS MFG count"
        );
        assert!(
            rsf_fgvk_enable_options(owner, 0, 4) == FIXTURE_NOT_SUPPORTED
                && f!(enabled) != 0
                && f!(generated_count) == 3,
            "unsupported DLSS options preserve both count and enabled state"
        );
        assert!(
            rsf_fgvk_image_layout(owner, &mut value) == FIXTURE_OK && value == 1000001002,
            "DLSS proxy image layout forwarded"
        );
        assert!(
            rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_OK && value == 3,
            "DLSS real reported limit forwarded"
        );
        status.prepared = 0;
        assert!(
            rsf_fgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.prepared == 10
                && status.abi_version == 1,
            "DLSS nested ABI1 status copied"
        );
        f!(refuse_state) = true;
        value = 77;
        assert!(
            rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_NOT_READY && value == 77,
            "failed capability query preserves output"
        );
        f!(refuse_state) = false;
        f!(calls) = 0;
        let _ = rsf_fgvk_prepare(owner, command, &frame);
        let _ = rsf_fgvk_hudless(owner, command, &frame);
        rsf_fgvk_submitted(owner, &command, 1);
        assert!(
            rsf_fgvk_present(owner, info) == -1000001004 && f!(calls) == 240,
            "DLSS recording/submission/WSI error forwarded"
        );
        assert!(
            rsf_fgvk_present_source(owner, info, &identity) == -1000001004
                && f!(presented_source) == ptr::from_ref(&identity),
            "DLSS explicit identity preserves original WSI failure"
        );
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "DLSS retirement succeeds"
        );
        f!(refuse_destroy) = true;
        owner = ptr::null_mut();
        assert!(
            rsf_fgvk_create_for_backend(&setup, 1, 4, &mut owner, &mut chain) == FIXTURE_NOT_READY
                && !owner.is_null()
                && chain == 11,
            "failed unsupported-count cleanup returns retained owner for retry"
        );
        assert!(
            rsf_fgvk_create(&setup, &mut second, &mut second_chain) == FIXTURE_NOT_READY
                && second.is_null(),
            "partial failed creation retains global admission"
        );
        f!(calls) = 0;
        assert!(
            rsf_core_fgvk_presentation_attach(owner) == FIXTURE_NOT_READY,
            "partial owner refuses latency attachment"
        );
        assert!(
            rsf_fgvk_prepare(owner, command, &frame) == FIXTURE_NOT_READY
                && rsf_fgvk_enable(owner, 1) == FIXTURE_NOT_READY
                && rsf_fgvk_present(owner, info) == -3
                && f!(calls) == 0,
            "partial owner refuses new provider work"
        );
        f!(refuse_destroy) = false;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "partial create cleanup retry succeeds"
        );
        f!(refuse_create) = true;
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_INIT_FAILED
                && owner.is_null()
                && chain == 0,
            "concrete create error releases clean generic admission"
        );
        f!(refuse_create) = false;
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_OK
                && rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "creation resumes after clean factory failure"
        );
        owner = ptr::null_mut();
        assert!(
            rsf_fgvk_create_for_backend(&setup, 4, 3, &mut owner, &mut chain)
                == FIXTURE_NOT_SUPPORTED
                && owner.is_null()
                && chain == 0,
            "FSR4 rejects unsupported MFG before concrete creation or fallback"
        );
        if fsr4 {
            assert!(
                rsf_fgvk_create_for_backend(&setup, 4, 1, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT
                    && f!(fsr4_creates) == 0,
                "FSR4 needs explicit host contract"
            );
            let mut fsr4_host = host_options(ptr::null());
            assert!(
                rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT
                    && f!(fsr4_creates) == 0,
                "FSR4 requires FidelityFX runtime directory rather than Intel host path"
            );
            setup.graphics.runtime_directory_utf8 = c"FidelityFX".as_ptr();
            fsr4_host.struct_size -= 1;
            assert!(
                rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT,
                "short FSR4 host refused"
            );
            fsr4_host.struct_size += 1;
            fsr4_host.abi_version += 1;
            assert!(
                rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_ABI_MISMATCH,
                "FSR4 host ABI refused"
            );
            fsr4_host.abi_version -= 1;
            fsr4_host.timeline_enabled = 0;
            assert!(
                rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT,
                "FSR4 external timeline must be enabled"
            );
            fsr4_host.timeline_enabled = 1;
            let before_fsr4_ffx = f!(ffx_creates);
            f!(fsr4_create_result) = RSF_FSR4FG_NOT_SUPPORTED;
            assert!(
                rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_NOT_SUPPORTED
                    && owner.is_null()
                    && chain == 0
                    && f!(ffx_creates) == before_fsr4_ffx,
                "missing exact FSR4 provider never falls back to FFX3"
            );
            f!(fsr4_create_result) = 0;
            assert!(
                rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_OK
                    && chain == 44
                    && f!(borrowed_setup) == ptr::from_ref(&setup)
                    && f!(enabled) == 0
                    && f!(generated_count) == 1,
                "FSR4 create is SR-independent and starts Off"
            );
            assert!(
                rsf_fgvk_backend(owner, &mut value) == FIXTURE_OK
                    && value == 4
                    && rsf_fgvk_image_layout(owner, &mut value) == FIXTURE_OK
                    && value == 5
                    && rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_OK
                    && value == 1,
                "FSR4 owner, shader-read layout and count-one limit"
            );
            assert!(
                rsf_fgvk_enable(owner, 1) == FIXTURE_OK
                    && f!(enabled) != 0
                    && f!(generated_count) == 1
                    && rsf_fgvk_enable_options(owner, 0, 2) == FIXTURE_NOT_SUPPORTED
                    && f!(enabled) != 0,
                "FSR4 invalid options retain prior active count-one state"
            );
            f!(calls) = 0;
            let _ = rsf_fgvk_prepare(owner, command, &frame);
            let _ = rsf_fgvk_hudless(owner, command, &frame);
            rsf_fgvk_submitted(owner, &command, 1);
            assert!(
                rsf_fgvk_present(owner, info) == 1000001003
                    && f!(calls) == 61440
                    && rsf_fgvk_acquire(owner, 123, 4, 5, &mut value) == 2
                    && value == 1
                    && rsf_fgvk_images(owner, &mut value, ptr::null_mut()) == 0
                    && value == 3,
                "FSR4 recording and all WSI routes preserve results"
            );
            assert!(
                rsf_fgvk_present_source(owner, info, &identity) == 1000001003
                    && f!(presented_source) == ptr::from_ref(&identity),
                "FSR4 explicit identity forwarded"
            );
            assert!(
                rsf_fgvk_get_status(owner, &mut status) == FIXTURE_OK
                    && status.prepared == 40
                    && status.result == -77,
                "FSR4 status preserves private SDK result"
            );
            f!(refuse_destroy) = true;
            assert!(
                rsf_fgvk_drain(owner) == FIXTURE_FEATURE_FAILED
                    && rsf_fgvk_destroy(owner) == FIXTURE_FEATURE_FAILED
                    && rsf_fgvk_create(&setup, &mut second, &mut second_chain) == FIXTURE_NOT_READY,
                "FSR4 failed retirement preserves owner and process admission"
            );
            f!(refuse_destroy) = false;
            assert!(
                rsf_fgvk_destroy(owner) == FIXTURE_OK,
                "FSR4 retirement retry succeeds"
            );
            f!(fsr4_create_result) = -77;
            f!(fsr4_partial) = true;
            f!(refuse_destroy) = true;
            assert!(
                rsf_fgvk_create_with_host(&setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_FEATURE_FAILED
                    && !owner.is_null()
                    && chain == 44,
                "FSR4 failed partial cleanup returns retained owner and chain"
            );
            f!(refuse_destroy) = false;
            assert!(
                rsf_fgvk_destroy(owner) == FIXTURE_OK,
                "FSR4 partial owner retires on retry"
            );
            f!(fsr4_partial) = false;
            f!(fsr4_create_result) = 0;
        } else {
            assert!(
                rsf_fgvk_create_for_backend(&setup, 4, 1, &mut owner, &mut chain)
                    == FIXTURE_NOT_COMPILED
                    && owner.is_null()
                    && chain == 0,
                "missing FSR4 build is explicit without provider fallback"
            );
        }
        if xess {
            assert!(
                rsf_fgvk_create_for_backend(&setup, 5, 3, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT
                    && f!(xess_creates) == 0,
                "legacy creation cannot select XeSS without its host contract"
            );
            let intel = [b'D' as u16, b':' as u16, 0];
            let mut host = host_options(intel.as_ptr());
            host.struct_size -= 1;
            assert!(
                rsf_fgvk_create_with_host(&setup, 5, 3, &host, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT,
                "short host contract refused"
            );
            host.struct_size += 1;
            host.abi_version += 1;
            assert!(
                rsf_fgvk_create_with_host(&setup, 5, 3, &host, &mut owner, &mut chain)
                    == FIXTURE_ABI_MISMATCH,
                "host ABI mismatch refused"
            );
            host.abi_version -= 1;
            setup.graphics.backend = 1;
            assert!(
                rsf_fgvk_create_with_host(&setup, 5, 3, &host, &mut owner, &mut chain)
                    == FIXTURE_OK
                    && chain == 55
                    && f!(generated_count) == 3,
                "explicit XeSS FG remains independent of DLSS SR and preserves requested count"
            );
            assert!(
                rsf_fgvk_backend(owner, &mut value) == FIXTURE_OK
                    && value == 5
                    && rsf_fgvk_image_layout(owner, &mut value) == FIXTURE_OK
                    && value == 5
                    && rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_OK
                    && value == 5,
                "XeSS owner/layout/native count dispatched"
            );
            assert!(
                rsf_fgvk_create(&setup, &mut second, &mut second_chain) == FIXTURE_NOT_READY,
                "XeSS shares process owner admission"
            );
            assert!(
                rsf_fgvk_enable(owner, 1) == FIXTURE_OK
                    && f!(generated_count) == 3
                    && f!(enabled) != 0,
                "legacy XeSS enable preserves MFG count"
            );
            assert!(
                rsf_fgvk_enable_options(owner, 0, 6) == FIXTURE_FEATURE_FAILED
                    && f!(enabled) != 0
                    && f!(generated_count) == 3,
                "XeSS unsupported count is never clamped"
            );
            f!(calls) = 0;
            let _ = rsf_fgvk_prepare(owner, command, &frame);
            let _ = rsf_fgvk_hudless(owner, command, &frame);
            rsf_fgvk_submitted(owner, &command, 1);
            assert!(
                rsf_fgvk_present(owner, info) == 1000001003
                    && f!(calls) == 3840
                    && rsf_fgvk_acquire(owner, 0, 0, 0, &mut value) == 2
                    && value == 2,
                "XeSS WSI results and submission routes preserved"
            );
            assert!(
                rsf_fgvk_present_source(owner, info, &identity) == 1000001003
                    && f!(presented_source) == ptr::from_ref(&identity),
                "XeSS explicit identity forwarded"
            );
            assert!(
                rsf_fgvk_get_status(owner, &mut status) == FIXTURE_OK
                    && status.prepared == 50
                    && status.result == -77,
                "native XeSS errors remain in bounded status"
            );
            assert!(
                rsf_fgvk_latency_options(owner, 2, 5000) == FIXTURE_NOT_SUPPORTED
                    && f!(xell).mode == 1,
                "XeLL Boost refused without state mutation"
            );
            assert!(
                rsf_fgvk_latency_options(owner, 0, 5000) == FIXTURE_OK && f!(xell).mode == 0,
                "XeLL Off options forwarded"
            );
            assert!(
                rsf_fgvk_latency_sleep(owner, 0) == FIXTURE_INVALID_ARGUMENT
                    && rsf_fgvk_latency_sleep(owner, u64::from(u32::MAX) + 1)
                        == FIXTURE_INVALID_ARGUMENT
                    && rsf_fgvk_latency_marker(owner, 42, 7) == FIXTURE_INVALID_ARGUMENT,
                "XeLL domain rejects invalid or truncated identities"
            );
            assert!(
                rsf_fgvk_latency_sleep(owner, 42) == FIXTURE_OK
                    && rsf_fgvk_latency_marker(owner, 42, 6) == FIXTURE_OK,
                "XeLL sleep and supported marker reach private owner"
            );
            f!(refuse_latency) = true;
            assert!(
                rsf_fgvk_latency_sleep(owner, 42) == FIXTURE_FEATURE_FAILED,
                "XeLL native sleep failure preserved as public refusal"
            );
            let mut latency = rsf_fgvk_latency_status {
                struct_size: size_of::<rsf_fgvk_latency_status>() as u32,
                abi_version: RSF_FGVK_LATENCY_ABI_VERSION,
                ..Default::default()
            };
            assert!(
                rsf_fgvk_latency_get_status(owner, &mut latency) == FIXTURE_OK
                    && latency.sleep_calls == 1
                    && latency.marker_calls == 1
                    && latency.result == -77
                    && latency.mode == 0,
                "XeLL counts cover accepted calls and retain exact native error"
            );
            f!(refuse_latency) = false;
            verify_sleep_borrow(owner);
            f!(refuse_destroy) = true;
            assert!(
                rsf_fgvk_drain(owner) == FIXTURE_FEATURE_FAILED
                    && rsf_fgvk_destroy(owner) == FIXTURE_FEATURE_FAILED
                    && rsf_fgvk_create(&setup, &mut second, &mut second_chain) == FIXTURE_NOT_READY,
                "XeSS failed retirement preserves process owner"
            );
            f!(refuse_destroy) = false;
            assert!(
                rsf_fgvk_destroy(owner) == FIXTURE_OK,
                "XeSS owner retry retires safely"
            );
        } else {
            assert!(
                rsf_fgvk_create_for_backend(&setup, 5, 3, &mut owner, &mut chain)
                    == FIXTURE_NOT_COMPILED
                    && owner.is_null()
                    && chain == 0,
                "XeSS missing SDK is explicit and does not allocate"
            );
        }
        let other_context = ptr::without_provenance_mut(99);
        assert!(
            rsf_core_fgvk_presentation_attach(other_context) == FIXTURE_OK,
            "independent existing latency binding retained"
        );
        owner = ptr::null_mut();
        chain = 0;
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_NOT_READY
                && owner.is_null()
                && chain == 0,
            "attachment collision rolls provider creation back"
        );
        f!(refuse_destroy) = true;
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) != FIXTURE_OK && !owner.is_null(),
            "failed rollback retains partial provider owner"
        );
        f!(refuse_destroy) = false;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "partial unattached owner cleanup retry"
        );
        {
            let retained = Lease::borrow();
            assert!(
                retained.owner == other_context,
                "partial-owner cleanup cannot release another active binding"
            );
        }
        assert!(
            rsf_core_fgvk_presentation_unbind() == FIXTURE_OK,
            "independent binding released by its owner"
        );
        f!(missing_owner) = true;
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_INIT_FAILED
                && owner.is_null()
                && chain == 0,
            "success without a provider is refused and does not publish the chain"
        );
        f!(missing_owner) = false;
        f!(fail_after_create) = true;
        let before_partial = f!(destroys);
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_INIT_FAILED
                && owner.is_null()
                && chain == 0
                && f!(destroys) == before_partial + 1,
            "failed create retires the partial provider before freeing the claim"
        );
        f!(fail_after_create) = false;
        assert!(
            rsf_fgvk_create(&setup, &mut owner, &mut chain) == FIXTURE_OK,
            "claim is reusable after early failures"
        );
        f!(fail_drain) = true;
        assert!(
            rsf_fgvk_drain(owner) == FIXTURE_FEATURE_FAILED,
            "drain failure is forwarded"
        );
        {
            let lease = Lease::borrow();
            assert!(
                lease.owner == owner && !lease.closing,
                "failed drain reopens CPU borrows"
            );
        }
        f!(fail_drain) = false;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "provider retires after a failed drain"
        );
        assert!(
            rsf_fgvk_destroy(ptr::null_mut()) == FIXTURE_OK
                && rsf_fgvk_drain(ptr::null_mut()) == FIXTURE_OK,
            "null retirement remains idempotent"
        );
    }
    0
}
