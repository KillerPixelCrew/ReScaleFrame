// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INVALID_ARGUMENT, NOT_READY, NOT_SUPPORTED, OK,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

unsafe extern "C" fn wsi_callback(user: *mut c_void) -> i32 {
    unsafe { *user.cast::<i32>() }
}

extern "C" fn frame_lease(module: *mut c_void, token: usize, _: *mut c_void) -> i32 {
    verify(module == identity(1) && token_index(token) == Some(1));
    LEASED.fetch_add(1, Relaxed);
    FEATURE_FAILED
}

extern "C" fn non_frame_lease(module: *mut c_void, token: usize, _: *mut c_void) -> i32 {
    verify(!module.is_null() && token == 0);
    OK
}

fn begin(frame: u64) {
    report_with_registration(&clock_event(frame, BEGIN));
}

fn thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

fn post(message: u32) {
    assert_ne!(unsafe { PostThreadMessageW(thread_id(), message, 0, 0) }, 0);
}

fn peek(queue: &mut MSG, first: u32, last: u32, flags: u32) -> bool {
    unsafe { PeekMessageW(queue, THREAD_MESSAGES, first, last, flags) != 0 }
}

fn ping_and_sleep(setup: &rsf_latencyvk_setup) {
    // Exercise the Win32 thread queue and a message-only window. The SDK
    // exports and all graphics identities in this fixture are synthetic.
    unsafe {
        assert_eq!(rsf_latencyvk_initialize(setup), OK);
        let mut queued = MSG::default();
        peek(&mut queued, WM_USER, WM_USER, PM_NOREMOVE);
        while peek(&mut queued, PCL_MESSAGE, PCL_MESSAGE, PM_REMOVE) {}
        post(PCL_MESSAGE);
        let initial_binds = PCL_BINDS.load(Relaxed);
        begin(1);
        assert_eq!(PCL_BINDS.load(Relaxed), initial_binds + 1);
        assert_eq!(PCL_THREAD.load(Relaxed), thread_id());
        assert!(!peek(&mut queued, PCL_MESSAGE, PCL_MESSAGE, PM_NOREMOVE));
        let prior_markers = marker_count();
        begin(2);
        assert_eq!(marker_count(), prior_markers);
        post(PCL_MESSAGE);
        begin(3);
        begin(4);
        assert_eq!(marker_count(), prior_markers + 1);
        let last = *markers().last().unwrap();
        assert_eq!((last.frame, last.kind), (4, 8));
        post(0xc124);

        let class = [
            b'S' as u16,
            b'T' as u16,
            b'A' as u16,
            b'T' as u16,
            b'I' as u16,
            b'C' as u16,
            0,
        ];
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            [0_u16].as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        assert!(!window.is_null());
        struct MessageWindow(*mut c_void);
        impl Drop for MessageWindow {
            fn drop(&mut self) {
                unsafe { DestroyWindow(self.0) };
            }
        }
        let window = MessageWindow(window);
        assert_ne!(PostMessageW(window.0, PCL_MESSAGE, 0, 0), 0);
        for _ in 0..40 {
            post(PCL_MESSAGE);
        }
        begin(5);
        let mut remaining = 0;
        while peek(&mut queued, PCL_MESSAGE, PCL_MESSAGE, PM_REMOVE) {
            remaining += 1;
        }
        assert_eq!(remaining, 24);
        assert!(peek(&mut queued, 0xc124, 0xc124, PM_REMOVE) && queued.message == 0xc124);
        assert_ne!(
            PeekMessageW(&mut queued, window.0, PCL_MESSAGE, PCL_MESSAGE, PM_REMOVE),
            0
        );
        assert_eq!(queued.hwnd, window.0);
        drop(window);
        let before_flood = marker_count();
        begin(6);
        assert_eq!(marker_count(), before_flood + 1);
        PostQuitMessage(37);
        begin(7);
        assert!(
            peek(&mut queued, 0, 0, PM_REMOVE) && queued.message == WM_QUIT && queued.wParam == 37
        );
        post(PCL_MESSAGE);
        std::thread::spawn(|| {
            let mut queued = MSG::default();
            peek(&mut queued, WM_USER, WM_USER, PM_NOREMOVE);
            post(PCL_MESSAGE);
            PCL_BIND_FAIL.store(true, Relaxed);
            begin(8);
            assert!(peek(&mut queued, PCL_MESSAGE, PCL_MESSAGE, PM_NOREMOVE));
            let mut refused = status_request();
            assert_eq!(rsf_latencyvk_get_status(&mut refused), OK);
            assert_eq!(refused.last_result, -17);
            PCL_BIND_FAIL.store(false, Relaxed);
            begin(9);
            assert_eq!(PCL_THREAD.load(Relaxed), thread_id());
            assert!(!peek(&mut queued, PCL_MESSAGE, PCL_MESSAGE, PM_NOREMOVE));
        })
        .join()
        .unwrap();
        begin(10);
        assert_eq!(PCL_THREAD.load(Relaxed), thread_id());
        let before_return = marker_count();
        begin(11);
        assert_eq!(marker_count(), before_return);
        assert_eq!(rsf_latencyvk_options(1, 12345), OK);
        std::thread::spawn(|| {
            let sample = clock_event(11, SAMPLE);
            let binds_before = PCL_BINDS.load(Relaxed);
            for malformed in [
                Event {
                    struct_size: sample.struct_size - 1,
                    ..sample
                },
                Event {
                    timestamp_qpc: 0,
                    ..sample
                },
                Event {
                    flags: sample.flags | 0x8000_0000,
                    ..sample
                },
                Event {
                    stage: INPUT,
                    source_frame_id: 0,
                    flags: RSF_GAME_CPU_INPUT_UNFRAMED,
                    input_kind: 1,
                    ..sample
                },
            ] {
                rsf_latencyvk_cpu(&malformed);
            }
            rsf_latencyvk_render(11);
            assert_eq!(PCL_BINDS.load(Relaxed), binds_before);
            report_with_registration(&sample);
            assert_eq!(PCL_THREAD.load(Relaxed), thread_id());
        })
        .join()
        .unwrap();
        begin(12);
        post(PCL_MESSAGE);
        assert_eq!(rsf_latencyvk_shutdown(), OK);
        let mut stopped = status_request();
        assert_eq!(rsf_latencyvk_get_status(&mut stopped), NOT_READY);
        assert_eq!((stopped.pcl_message, stopped.report_available), (0, 0));
        let retired_binds = PCL_BINDS.load(Relaxed);
        assert_eq!(rsf_latencyvk_initialize(setup), OK);
        begin(1);
        assert_eq!(PCL_BINDS.load(Relaxed), retired_binds + 1);
        let after_restart = marker_count();
        begin(2);
        assert_eq!(marker_count(), after_restart);
        post(PCL_MESSAGE);
        begin(3);
        begin(4);
        assert_eq!(marker_count(), after_restart + 1);
        assert_eq!(markers().last().unwrap().kind, 8);
        assert_eq!(rsf_latencyvk_shutdown(), OK);
    }
    overlapping_sleep(setup);
}

extern "C" fn newer_lease(_: *mut c_void, token: usize, _: *mut c_void) -> i32 {
    if token_index(token) == Some(9) {
        OK
    } else {
        NOT_READY
    }
}

fn overlapping_sleep(setup: &rsf_latencyvk_setup) {
    unsafe {
        assert_eq!(rsf_latencyvk_initialize(setup), OK);
        assert_eq!(rsf_latencyvk_options(1, 12345), OK);
        let reflex_before_attachment = SLEPT.load(Relaxed);
        let first_context = identity(1);
        let next_context = identity::<rsf_fgvk>(2);
        assert_eq!(rsf_core_fgvk_presentation_attach(first_context), OK);
        XELL_MODE.store(0, Relaxed);
        begin(1);
        XELL_SLEEP.arm();
        std::thread::scope(|scope| {
            let sleeper = scope.spawn(|| rsf_latencyvk_cpu(&clock_event(1, SAMPLE)));
            assert!(XELL_SLEEP.entered());
            assert_eq!(XELL_MODE.load(Relaxed), 1);
            assert_eq!(SLEPT.load(Relaxed), reflex_before_attachment);
            let mut during_sleep = status_request();
            assert_eq!(rsf_latencyvk_get_status(&mut during_sleep), OK);
            assert_eq!(during_sleep.owner, RSF_PACING_XELL);
            assert_eq!(rsf_latencyvk_shutdown(), NOT_READY);
            {
                let refused = PresentationLease::borrow();
                assert!(refused.context.is_null() && refused.closing);
            }
            assert_eq!(rsf_latencyvk_presentation(null_mut()), NOT_READY);
            {
                let refused = PresentationLease::borrow();
                assert!(refused.context.is_null() && refused.closing);
            }
            assert_eq!(rsf_latencyvk_presentation(next_context.cast()), NOT_READY);
            assert_eq!(rsf_latencyvk_shutdown(), NOT_READY);
            XELL_SLEEP.release();
            sleeper.join().unwrap();
        });
        assert_eq!(rsf_latencyvk_presentation(null_mut()), OK);
        assert_eq!(rsf_core_fgvk_presentation_attach(next_context), OK);
        SHUTDOWN_ALLOWED.store(false, Relaxed);
        assert_eq!(rsf_latencyvk_shutdown(), FEATURE_FAILED);
        {
            let retained = PresentationLease::borrow();
            assert_eq!(retained.context, next_context);
        }
        SHUTDOWN_ALLOWED.store(true, Relaxed);
        assert_eq!(rsf_latencyvk_shutdown(), OK);
        assert_eq!(rsf_latencyvk_initialize(setup), OK);
        assert_eq!(rsf_latencyvk_options(1, 12345), OK);
        cpu(1, BEGIN);
        cpu(2, BEGIN);
        let pacing_token = CURRENT.load(Relaxed);
        REFLEX_SLEEP.arm();
        std::thread::scope(|scope| {
            let sleeper = scope.spawn(|| cpu(2, PACE));
            assert!(REFLEX_SLEEP.entered());
            assert_eq!(rsf_latencyvk_shutdown(), NOT_READY);
            cpu(3, BEGIN);
            assert_eq!(CURRENT.load(Relaxed), pacing_token);
            let before_present = marker_count();
            rsf_latencyvk_render(1);
            rsf_latencyvk_present(1);
            rsf_latencyvk_present(0);
            assert_eq!(marker_count(), before_present + 3);
            let last = *markers().last().unwrap();
            assert_eq!((last.frame, last.kind), (1, 5));
            REFLEX_SLEEP.release();
            sleeper.join().unwrap();
        });
        cpu(3, BEGIN);
        assert_eq!(token_index(CURRENT.load(Relaxed)), Some(3));
        cpu(9, BEGIN);
        cpu_expected(3, END, 0);
        let newer = rsf_slvk_call {
            struct_size: size_of::<rsf_slvk_call>() as u32,
            abi_version: RSF_SLVK_ABI_VERSION,
            session_id: 1,
            frame_id: 9,
            callback: Some(newer_lease),
            user: null_mut(),
        };
        assert_eq!(rsf_slvk_invoke(&newer), OK);
        assert_eq!(rsf_latencyvk_shutdown(), OK);

        // A checked failure with a nonzero out parameter still cannot lease.
        assert_eq!(rsf_latencyvk_initialize(setup), OK);
        FAIL_BEGIN.store(true, Relaxed);
        cpu(9, BEGIN);
        FAIL_BEGIN.store(false, Relaxed);
        assert_eq!(rsf_slvk_invoke(&newer), NOT_READY);
        let mut refused = status_request();
        assert_eq!(rsf_latencyvk_get_status(&mut refused), OK);
        assert_eq!((refused.last_result, refused.refused_calls), (-19, 1));
        assert_eq!(rsf_latencyvk_shutdown(), OK);
    }
}

#[repr(C, align(8))]
struct Short(u32);

pub(super) fn run() {
    let setup = legacy_tokens();
    shared_features();
    keyed_sources(&setup);
    ping_and_sleep(&setup);
}

fn legacy_tokens() -> rsf_latencyvk_setup {
    // Calls use fixture-owned packets and opaque synthetic handles.
    unsafe {
        let short_cpu = Short(4);
        rsf_latencyvk_cpu(ptr::from_ref(&short_cpu.0).cast());
        assert!(
            rsf_slvk_without_dlssg(None, null_mut()) == -3,
            "missing foreign WSI callback refused"
        );
        let mut foreign_wsi = 7_i32;
        assert!(
            rsf_slvk_without_dlssg(Some(wsi_callback), (&raw mut foreign_wsi).cast()) == 7,
            "foreign WSI callback result preserved"
        );
        let lease = rsf_slvk_call {
            struct_size: size_of::<rsf_slvk_call>() as u32,
            abi_version: RSF_SLVK_ABI_VERSION,
            session_id: 1,
            frame_id: 1,
            callback: Some(frame_lease),
            user: null_mut(),
        };
        assert!(
            rsf_slvk_invoke(null()) == INVALID_ARGUMENT && rsf_slvk_invoke(&lease) == NOT_READY,
            "no SDK lease before initialization"
        );
        let mut malformed = lease;
        malformed.struct_size -= 1;
        assert!(
            rsf_slvk_invoke(&malformed) == INVALID_ARGUMENT,
            "short SDK lease refused"
        );
        malformed = lease;
        malformed.abi_version += 1;
        assert!(
            rsf_slvk_invoke(&malformed) == ABI_MISMATCH,
            "SDK lease ABI mismatch refused"
        );
        malformed = lease;
        malformed.callback = None;
        assert!(
            rsf_slvk_invoke(&malformed) == INVALID_ARGUMENT,
            "null SDK callback refused"
        );
        malformed = lease;
        malformed.session_id = 0;
        assert!(
            rsf_slvk_invoke(&malformed) == INVALID_ARGUMENT,
            "partial frame identity refused"
        );
        assert!(
            rsf_latencyvk_initialize(null()) == INVALID_ARGUMENT,
            "null setup refused"
        );
        let mut setup = rsf_latencyvk_setup {
            struct_size: size_of::<rsf_latencyvk_setup>() as u32 - 1,
            abi_version: RSF_LATENCYVK_ABI_VERSION,
            runtime_directory_utf8: c"unused".as_ptr(),
            project_id: c"synthetic".as_ptr(),
        };
        assert!(
            rsf_latencyvk_initialize(&setup) == INVALID_ARGUMENT,
            "short setup refused"
        );
        setup.struct_size = size_of_val(&setup) as u32;
        setup.abi_version = 999;
        assert!(
            rsf_latencyvk_initialize(&setup) == ABI_MISMATCH,
            "ABI mismatch refused"
        );
        setup.abi_version = RSF_LATENCYVK_ABI_VERSION;
        assert!(
            rsf_latencyvk_initialize(&setup) == OK,
            "synthetic initialization"
        );
        assert!(
            SR.load(Relaxed) == 0 && FG.load(Relaxed) == 0,
            "legacy latency ABI does not load SR or FG"
        );
        assert!(
            rsf_latencyvk_options(3, 0) == INVALID_ARGUMENT,
            "unknown mode refused"
        );
        assert!(rsf_latencyvk_options(1, 12345) == OK, "options queued");
        cpu(1, BEGIN);
        cpu(1, PACE);
        cpu(1, PACE);
        assert!(
            rsf_slvk_invoke(&lease) == FEATURE_FAILED && LEASED.load(Relaxed) == 1,
            "callback failure preserved"
        );
        let mut foreign = lease;
        foreign.session_id = 2;
        assert!(
            rsf_slvk_invoke(&foreign) == NOT_READY && LEASED.load(Relaxed) == 1,
            "foreign session lease refused"
        );
        cpu(1, SIM_BEGIN);
        cpu(1, SIM_BEGIN);
        cpu(1, SIM_END);
        assert!(
            SLEPT.load(Relaxed) == 1 && OPTIONS.load(Relaxed) == 1,
            "one pacing call per CPU frame"
        );
        let before_input = marker_count();
        assert!(
            rsf_latencyvk_input(1, 1, 0) == OK
                && rsf_latencyvk_input(1, 2, 0) == OK
                && marker_count() == before_input,
            "keyboard/mouse cannot spoof controller or deprecated markers"
        );
        assert!(
            rsf_latencyvk_input(1, 4, 0xc124) == INVALID_ARGUMENT && marker_count() == before_input,
            "an unrelated window message cannot partially emit a controller marker"
        );
        assert!(
            rsf_latencyvk_input(1, 4, 0) == OK
                && rsf_latencyvk_input(1, 4, 0) == OK
                && marker_count() == before_input + 1,
            "controller sampled once per source"
        );
        assert!(
            rsf_latencyvk_input(1, 0, 0xc124) == INVALID_ARGUMENT,
            "foreign Windows message refused"
        );
        assert!(
            rsf_latencyvk_input(1, 0, 0xc123) == OK,
            "registered latency ping"
        );
        rsf_latencyvk_render(1);
        rsf_latencyvk_present(1);
        rsf_latencyvk_present(0);
        let completed = marker_count();
        rsf_latencyvk_present(1);
        rsf_latencyvk_present(0);
        assert!(
            marker_count() == completed,
            "secondary Present cannot reuse completed source identity"
        );
        assert!(
            markers().first().unwrap().frame == 1
                && markers().first().unwrap().kind == 0
                && markers().last().unwrap().kind == 5,
            "simulation through Present on one source token"
        );
        let before_oversize = CURRENT.load(Relaxed);
        cpu(u64::from(u32::MAX) + 2, BEGIN);
        assert!(
            CURRENT.load(Relaxed) == before_oversize,
            "an out-of-range source cannot replace a 32-bit SDK token"
        );
        cpu(2, BEGIN);
        cpu(2, PACE);
        cpu_expected(2, END, 0);
        let newest = CURRENT.load(Relaxed);
        assert!(
            rsf_slvk_invoke(&lease) == FEATURE_FAILED && CURRENT.load(Relaxed) == newest,
            "older render lease cannot rewind pacing token"
        );
        assert!(
            SLEPT.load(Relaxed) == 2 && OPTIONS.load(Relaxed) == 2,
            "unchanged options refreshed for a possible swapchain successor"
        );
        rsf_latencyvk_render(2);
        rsf_latencyvk_present(1);
        rsf_latencyvk_present(0);
        assert!(marker_count() == completed, "nonrendered frame aborted");
        for frame in 3..10 {
            cpu(frame, BEGIN);
        }
        assert!(
            rsf_slvk_invoke(&lease) == NOT_READY && LEASED.load(Relaxed) == 2,
            "expired render lease refused"
        );
        cpu(8, PACE);
        assert!(
            SLEPT.load(Relaxed) == 2,
            "late pacing event cannot sleep with a newer frame token"
        );
        rsf_latencyvk_render(1);
        rsf_latencyvk_present(1);
        rsf_latencyvk_present(0);
        assert!(
            marker_count() == completed,
            "expired SDK ring token refused"
        );
        assert!(
            rsf_latencyvk_input(1, 1, 0) == NOT_READY,
            "stale input identity refused"
        );
        let mut status = status_request();
        status.abi_version = 999;
        assert!(
            rsf_latencyvk_get_status(&mut status) == ABI_MISMATCH,
            "status ABI checked"
        );
        status.abi_version = RSF_LATENCYVK_ABI_VERSION;
        assert!(
            rsf_latencyvk_get_status(&mut status) == OK
                && status.sleep_calls == 2
                && status.marker_calls == completed as u64,
            "accepted SDK calls counted"
        );
        assert!(
            rsf_latencyvk_shutdown() == OK && rsf_latencyvk_shutdown() == OK,
            "idempotent shutdown"
        );
        assert!(
            rsf_latencyvk_options(1, 0) == NOT_SUPPORTED,
            "no latency claim after shutdown"
        );
        assert!(
            rsf_latencyvk_initialize(&setup) == OK && rsf_latencyvk_options(1, 12345) == OK,
            "fresh session initialized"
        );
        cpu(1, BEGIN);
        cpu(1, PACE);
        assert!(
            SLEPT.load(Relaxed) == 3 && OPTIONS.load(Relaxed) == 3,
            "options reapplied after SDK reinitialization"
        );
        assert!(
            rsf_latencyvk_get_status(&mut status) == OK
                && status.sleep_calls == 1
                && status.marker_calls == 0,
            "fresh session counters"
        );
        let overlap = marker_count();
        rsf_latencyvk_render(1);
        rsf_latencyvk_present(1);
        cpu(2, BEGIN);
        rsf_latencyvk_render(2);
        rsf_latencyvk_present(1);
        rsf_latencyvk_present(0);
        rsf_latencyvk_present(1);
        rsf_latencyvk_present(0);
        assert!(
            marker_count() == overlap + 6
                && markers()[overlap + 3].frame == 1
                && markers()[overlap + 3].kind == 5
                && markers()[overlap + 4].frame == 2
                && markers()[overlap + 4].kind == 4
                && markers()[overlap + 5].frame == 2
                && markers()[overlap + 5].kind == 5,
            "legacy Present end keeps a newer render queued and duplicate begin cannot replace its source"
        );
        assert!(rsf_latencyvk_shutdown() == OK, "fresh session retired");
        LOW_LATENCY.store(0, Relaxed);
        assert!(
            rsf_latencyvk_initialize(&setup) == OK,
            "stats-only session initialized"
        );
        assert!(
            rsf_latencyvk_options(1, 0) == NOT_SUPPORTED
                && rsf_latencyvk_options(0, 12345) == NOT_SUPPORTED,
            "unavailable Reflex mode and limiter refused"
        );
        assert!(
            rsf_latencyvk_options(0, 0) == OK && rsf_latencyvk_shutdown() == OK,
            "stats-only PCL remains available"
        );
        setup
    }
}

fn shared_features() {
    unsafe {
        let mut shared = rsf_slvk_setup {
            struct_size: size_of::<rsf_slvk_setup>() as u32,
            abi_version: RSF_SLVK_ABI_VERSION,
            runtime_directory_utf8: c"unused".as_ptr(),
            project_id: c"synthetic".as_ptr(),
            features: RSF_SLVK_DLSS_SR,
        };
        let mut invalid_setup = shared;
        invalid_setup.struct_size -= 1;
        assert!(
            rsf_slvk_initialize(&invalid_setup) == INVALID_ARGUMENT,
            "short shared SDK setup refused"
        );
        invalid_setup = shared;
        invalid_setup.abi_version += 1;
        assert!(
            rsf_slvk_initialize(&invalid_setup) == ABI_MISMATCH,
            "shared SDK ABI mismatch refused"
        );
        invalid_setup = shared;
        invalid_setup.features = 4;
        assert!(
            rsf_slvk_initialize(&invalid_setup) == INVALID_ARGUMENT,
            "unknown shared SDK features refused"
        );
        assert!(
            rsf_slvk_initialize(&shared) == OK && SR.load(Relaxed) == 1,
            "SR requested on shared SDK session"
        );
        let lease = rsf_slvk_call {
            struct_size: size_of::<rsf_slvk_call>() as u32,
            abi_version: RSF_SLVK_ABI_VERSION,
            session_id: 0,
            frame_id: 0,
            callback: Some(non_frame_lease),
            user: null_mut(),
        };
        assert!(
            rsf_slvk_invoke(&lease) == OK && rsf_latencyvk_shutdown() == OK,
            "non-frame SDK operations serialized"
        );
        shared.features = RSF_SLVK_DLSS_SR | RSF_SLVK_DLSS_FG;
        assert!(
            rsf_slvk_initialize(&shared) == OK && SR.load(Relaxed) == 1 && FG.load(Relaxed) == 1,
            "SR and FG requested together on shared SDK session"
        );
        assert!(
            rsf_latencyvk_shutdown() == OK,
            "shared SR and FG session retired"
        );
    }
}

fn keyed_sources(setup: &rsf_latencyvk_setup) {
    let mut status = status_request();
    unsafe {
        LOW_LATENCY.store(1, Relaxed);
        assert!(
            rsf_latencyvk_initialize(setup) == OK,
            "keyed session initialized"
        );
        assert!(
            rsf_latencyvk_presentation(identity(1)) == OK,
            "core binds required XeLL owner"
        );
        assert!(
            rsf_latencyvk_options(2, 10000) == OK && XELL_MODE.load(Relaxed) == 1,
            "core maps Boost to required XeLL On"
        );
        let reflex_sleeps = SLEPT.load(Relaxed);
        let mut fact = event(77, 100, BEGIN, 1000, 1000, V1, KEYED);
        rsf_latencyvk_cpu(&fact);
        // This sink has no prospective owner/token contract. Source v2 must remain
        // outside native_cpu adoption and cannot acquire a late pace or marker.
        let before_prospective_sleep = SLEPT.load(Relaxed);
        let before_prospective_xell = XELL_SLEEPS.load(Relaxed);
        let before_prospective_markers = xell_marker_count();
        let unsupported = rsf_game_cpu_source_event {
            header: Event {
                struct_size: size_of::<rsf_game_cpu_source_event>() as u32,
                render_expected: 0,
                ..event(
                    77,
                    101,
                    RSF_GAME_CPU_SOURCE_RESERVE,
                    1001,
                    1000,
                    RSF_GAME_CPU_EVENT_VERSION_SOURCE,
                    0,
                )
            },
            predecessor_source_id: 0,
            native_generation: 1,
            reason: 0,
            reserved: 0,
            poll_begin_qpc: 0,
        };
        rsf_latencyvk_cpu(ptr::from_ref(&unsupported).cast());
        let mut absent = rsf_native_cpu_reservation {
            struct_size: size_of::<rsf_native_cpu_reservation>() as u32,
            ..Default::default()
        };
        assert!(
            rsf_native_cpu_reservation_read(77, 101, &mut absent) == 0
                && SLEPT.load(Relaxed) == before_prospective_sleep
                && XELL_SLEEPS.load(Relaxed) == before_prospective_xell
                && xell_marker_count() == before_prospective_markers,
            "Vulkan prospective source remains refused without its owner contract"
        );
        fact.stage = SAMPLE;
        fact.timestamp_qpc = 1010;
        rsf_latencyvk_cpu(&fact);
        rsf_latencyvk_cpu(&fact);
        fact.stage = SIM_BEGIN;
        fact.timestamp_qpc = 1011;
        rsf_latencyvk_cpu(&fact);
        fact.stage = INPUT;
        fact.timestamp_qpc = 1012;
        fact.input_kind = 7;
        rsf_latencyvk_cpu(&fact);
        rsf_latencyvk_cpu(&fact);
        fact.stage = SIM_END;
        fact.timestamp_qpc = 1020;
        fact.input_kind = 0;
        rsf_latencyvk_cpu(&fact);
        assert!(
            XELL_SLEEPS.load(Relaxed) == 1 && SLEPT.load(Relaxed) == reflex_sleeps,
            "exactly one selected pacer, no Reflex sleep stacked on XeLL"
        );
        let markers = xell_markers();
        assert!(
            markers.len() == 3
                && markers[0].kind == 6
                && markers[1].kind == 0
                && markers[2].kind == 1,
            "shared accepted input facts reach XeLL once in order"
        );
        let mut execution = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            session_id: 77,
            source_frame_id: 100,
            role: RSF_GAME_RENDER_PRESENT,
            ..Default::default()
        };
        let short_execution = Short(4);
        assert!(
            rsf_latencyvk_execution(ptr::from_ref(&short_execution.0).cast(), 1)
                == INVALID_ARGUMENT,
            "size-only execution prefix is refused without reading its body"
        );
        assert!(
            rsf_latencyvk_execution(null(), 1) == INVALID_ARGUMENT
                && rsf_latencyvk_execution(&execution, 1) == NOT_READY,
            "explicit Present cannot invent a missing submission"
        );
        execution.role = RSF_GAME_RENDER_SUBMISSION;
        let mut foreign_execution = execution;
        foreign_execution.session_id = 76;
        assert!(
            rsf_latencyvk_execution(&foreign_execution, 1) == NOT_READY,
            "foreign execution session refused"
        );
        assert!(
            rsf_latencyvk_execution(&execution, 1) == OK
                && rsf_latencyvk_execution(&execution, 1) == OK
                && rsf_latencyvk_execution(&execution, 0) == OK
                && xell_marker_count() == 5,
            "actual submission end closes its interval; duplicate begins emit once"
        );
        fact = event(77, 101, BEGIN, 2000, 1000, V1, KEYED);
        rsf_latencyvk_cpu(&fact);
        fact.stage = SAMPLE;
        fact.timestamp_qpc = 2010;
        rsf_latencyvk_cpu(&fact);
        fact.stage = SIM_BEGIN;
        fact.timestamp_qpc = 2011;
        rsf_latencyvk_cpu(&fact);
        fact.stage = SIM_END;
        fact.timestamp_qpc = 2020;
        rsf_latencyvk_cpu(&fact);
        execution.source_frame_id = 101;
        assert!(
            rsf_latencyvk_execution(&execution, 1) == OK,
            "newer rendering may overlap previous presentation"
        );
        execution.role = RSF_GAME_RENDER_PRESENT;
        assert!(
            rsf_latencyvk_execution(&execution, 1) == NOT_READY,
            "newer Present cannot synthesize the missing actual submission end"
        );
        execution.source_frame_id = 100;
        execution.role = RSF_GAME_RENDER_PRESENT;
        assert!(
            rsf_latencyvk_execution(&execution, 0) == NOT_READY,
            "unpaired explicit Present end refused"
        );
        assert!(
            rsf_latencyvk_execution(&execution, 1) == OK
                && rsf_latencyvk_execution(&execution, 0) == OK,
            "previous output presents with its own source"
        );
        let markers = xell_markers();
        let n = markers.len();
        assert!(
            n >= 2
                && markers[n - 2].frame == 100
                && markers[n - 2].kind == 4
                && markers[n - 1].frame == 100
                && markers[n - 1].kind == 5,
            "Present never takes the newer render token"
        );
        assert!(
            rsf_latencyvk_execution(&execution, 1) == OK
                && rsf_latencyvk_execution(&execution, 0) == OK
                && xell_marker_count() == n,
            "repeated explicit presentation is deduplicated"
        );
        execution.role = RSF_GAME_RENDER_SUBMISSION;
        assert!(
            rsf_latencyvk_execution(&execution, 1) == NOT_READY,
            "late rendering cannot reopen a presented source"
        );
        let duplicate_begin = event(77, 100, BEGIN, 1000, 1000, 0, 0);
        rsf_latencyvk_cpu(&duplicate_begin);
        assert!(
            rsf_latencyvk_execution(&execution, 1) == NOT_READY,
            "a duplicate legacy CPU begin cannot reopen an already presented keyed source"
        );
        execution.source_frame_id = 101;
        XELL_RESULT.store(FEATURE_FAILED, Relaxed);
        assert!(
            rsf_latencyvk_execution(&execution, 0) == OK,
            "submission end fact is admitted"
        );
        assert!(
            rsf_latencyvk_get_status(&mut status) == OK
                && status.owner == RSF_PACING_XELL
                && status.marker_calls == n as u64
                && status.last_result == FEATURE_FAILED
                && status.refused_calls == 1,
            "XeLL status reports the selected provider marker refusal"
        );
        XELL_RESULT.store(OK, Relaxed);
        assert!(
            rsf_latencyvk_presentation(null_mut()) == OK && rsf_latencyvk_shutdown() == OK,
            "keyed owner retired"
        );
        assert!(
            rsf_latencyvk_initialize(setup) == OK,
            "new keyed session initialized"
        );
        assert!(
            rsf_latencyvk_options(1, 12345) == OK,
            "new session owns pacing"
        );
        fact = event(78, 1, BEGIN, 2000, 1000, V1, KEYED);
        rsf_latencyvk_cpu(&fact);
        let mut restarted = rsf_native_cpu_frame {
            struct_size: size_of::<rsf_native_cpu_frame>() as u32,
            ..Default::default()
        };
        assert!(
            rsf_native_cpu_read(78, 1, &mut restarted) != 0,
            "shutdown releases previous CPU session before a new source starts at one"
        );
        SHUTDOWN_ALLOWED.store(false, Relaxed);
        assert!(
            rsf_latencyvk_shutdown() == FEATURE_FAILED
                && rsf_native_latency_has_claim(2) != 0
                && rsf_native_cpu_read(78, 1, &mut restarted) != 0,
            "failed SDK shutdown retains pacing lease and CPU session"
        );
        SHUTDOWN_ALLOWED.store(true, Relaxed);
        assert!(
            rsf_latencyvk_shutdown() == OK && rsf_native_latency_has_claim(2) == 0,
            "new keyed session retired"
        );
        assert!(
            rsf_native_latency_claim(1) != 0,
            "another graphics orchestrator owns process pacing"
        );
        assert!(
            rsf_latencyvk_initialize(setup) == OK,
            "contending Vulkan session initialized"
        );
        assert!(
            rsf_latencyvk_presentation(identity(1)) == OK,
            "presentation facts can be borrowed without claiming pacing"
        );
        assert!(
            rsf_latencyvk_options(1, 10000) == NOT_SUPPORTED,
            "required XeLL cannot take another owner's lease"
        );
        let denied_markers = xell_marker_count();
        rsf_latencyvk_render(999);
        rsf_latencyvk_submit_end(999);
        rsf_latencyvk_present(1);
        rsf_latencyvk_present(0);
        assert!(
            xell_marker_count() == denied_markers && XELL_SLEEPS.load(Relaxed) == 2,
            "contending owner emits no XeLL marker or sleep"
        );
        assert!(rsf_latencyvk_shutdown() == OK, "contending session retired");
        rsf_native_latency_release_claim(1);
    }
}
