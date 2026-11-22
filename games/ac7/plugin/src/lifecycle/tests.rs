// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::RSF_GAME_ABI_VERSION;
use std::cell::Cell;
use std::ffi::c_void;

/// Scripted native results.
struct Script {
    renderer: Renderer,
    prepare: bool,
    start: bool,
    active: bool,
    stop: bool,
    calls: Cell<u32>,
    options: Cell<Option<rsf_ac7_native_renderer_options>>,
}

impl Script {
    fn new() -> Self {
        Self {
            renderer: std::ptr::dangling_mut(),
            prepare: true,
            start: true,
            active: true,
            stop: true,
            calls: Cell::new(0),
            options: Cell::new(None),
        }
    }

    fn call<T>(&self, result: T) -> T {
        self.calls.set(self.calls.get() + 1);
        result
    }
}

impl Leaves for Script {
    fn prepare(&self, options: &rsf_ac7_native_renderer_options) -> (Renderer, bool) {
        self.options.set(Some(*options));
        (self.renderer, self.call(self.prepare))
    }
    fn start(&self, _: Renderer) -> bool {
        self.call(self.start)
    }
    fn is_active(&self, _: Renderer) -> bool {
        self.call(self.active)
    }
    fn quiesce(&self, _: Renderer) {
        self.call(());
    }
    fn stop(&self, _: Renderer) -> bool {
        self.call(self.stop)
    }
}

unsafe extern "C" fn log(_: *mut c_void, _: *const std::ffi::c_char) {}

fn host() -> rsf_game_host_services {
    // SAFETY: all-zero is the empty services table.
    let mut host: rsf_game_host_services = unsafe { std::mem::zeroed() };
    host.struct_size = size_of::<rsf_game_host_services>() as u32;
    host.abi_version = RSF_GAME_ABI_VERSION;
    host.session_id = 9;
    host.log = Some(log);
    host
}

/// The status result, prepared, active and the reason text it points at.
fn status(lifecycle: &Lifecycle, leaves: &Script) -> (rsf_result, u32, u32, String) {
    // SAFETY: all-zero is an empty status.
    let mut output: rsf_game_renderer_status = unsafe { std::mem::zeroed() };
    output.rendering_ready = 7;
    let result = lifecycle.status(leaves, &mut output);
    assert_eq!(output.rendering_ready, if result == RSF_OK { 0 } else { 7 });
    let reason = if output.reason.is_null() {
        String::new()
    } else {
        // SAFETY: every reason is one of this module's static strings.
        unsafe { CStr::from_ptr(output.reason) }
            .to_string_lossy()
            .into_owned()
    };
    (result, output.prepared, output.active, reason)
}

fn text(reason: &CStr) -> String {
    reason.to_string_lossy().into_owned()
}

#[test]
fn options_carry_the_host() {
    let host = host();
    let options = options(&host);
    assert_eq!(options.session_id, 9);
    assert!(options.log.is_some() && options.on_stage.is_none() && options.on_state.is_none());
}

#[test]
fn accepted_path_matches_the_former_plugin() {
    let lifecycle = Lifecycle::new();
    let leaves = Script::new();
    assert_eq!(
        status(&lifecycle, &leaves),
        (RSF_OK, 0, 0, text(NOT_PREPARED))
    );
    assert_eq!(lifecycle.start(&leaves), RSF_ERROR_NOT_READY);
    assert_eq!(leaves.calls.get(), 0);
    assert_eq!(lifecycle.prepare(&leaves, &host()), RSF_OK);
    assert_eq!(
        leaves.options.get().map(|options| options.session_id),
        Some(9)
    );
    assert_eq!(status(&lifecycle, &leaves), (RSF_OK, 1, 0, text(PREPARED)));
    assert_eq!(lifecycle.prepare(&leaves, &host()), RSF_ERROR_BUSY);
    assert_eq!(lifecycle.start(&leaves), RSF_OK);
    assert_eq!(status(&lifecycle, &leaves), (RSF_OK, 1, 1, text(STARTED)));
    assert_eq!(lifecycle.stop(&leaves), RSF_ERROR_BUSY);
    assert_eq!(lifecycle.quiesce(&leaves), RSF_OK);
    assert_eq!(status(&lifecycle, &leaves), (RSF_OK, 1, 0, text(QUIESCED)));
    assert_eq!(lifecycle.stop(&leaves), RSF_OK);
    assert_eq!(status(&lifecycle, &leaves), (RSF_OK, 0, 0, text(STOPPED)));
    // With no renderer, quiesce and stop complete without a native call.
    let calls = leaves.calls.get();
    assert_eq!(lifecycle.quiesce(&leaves), RSF_OK);
    assert_eq!(lifecycle.stop(&leaves), RSF_OK);
    assert_eq!(leaves.calls.get(), calls);
}

#[test]
fn preparation_and_activation_refusals_are_distinct() {
    let lifecycle = Lifecycle::new();
    let mut leaves = Script::new();
    leaves.prepare = false;
    assert_eq!(
        lifecycle.prepare(&leaves, &host()),
        RSF_ERROR_NATIVE_REFUSED
    );
    // The native output is retained even on refusal, as the pinned partial activation needs.
    assert_eq!(status(&lifecycle, &leaves), (RSF_OK, 0, 0, text(REFUSED)));
    assert_eq!(lifecycle.start(&leaves), RSF_ERROR_NOT_READY);
    assert_eq!(lifecycle.prepare(&leaves, &host()), RSF_ERROR_BUSY);
    leaves.stop = false;
    assert_eq!(lifecycle.stop(&leaves), RSF_ERROR_BUSY);
    assert_eq!(status(&lifecycle, &leaves), (RSF_OK, 0, 0, text(DRAINING)));
    leaves.stop = true;
    assert_eq!(lifecycle.stop(&leaves), RSF_OK);

    let lifecycle = Lifecycle::new();
    let mut leaves = Script::new();
    leaves.renderer = std::ptr::null_mut();
    leaves.prepare = false;
    assert_eq!(
        lifecycle.prepare(&leaves, &host()),
        RSF_ERROR_NATIVE_REFUSED
    );
    assert_eq!(status(&lifecycle, &leaves), (RSF_OK, 0, 0, text(REFUSED)));
    assert_eq!(lifecycle.start(&leaves), RSF_ERROR_NOT_READY);

    let lifecycle = Lifecycle::new();
    let mut leaves = Script::new();
    leaves.start = false;
    assert_eq!(lifecycle.prepare(&leaves, &host()), RSF_OK);
    assert_eq!(lifecycle.start(&leaves), RSF_ERROR_NOT_READY);
    assert_eq!(
        status(&lifecycle, &leaves),
        (RSF_OK, 1, 0, text(START_REFUSED))
    );

    let lifecycle = Lifecycle::new();
    let mut leaves = Script::new();
    leaves.active = false;
    leaves.stop = false;
    assert_eq!(lifecycle.prepare(&leaves, &host()), RSF_OK);
    assert_eq!(lifecycle.start(&leaves), RSF_OK);
    assert_eq!(
        status(&lifecycle, &leaves),
        (RSF_OK, 1, 0, text(DEACTIVATED))
    );
    assert_eq!(lifecycle.quiesce(&leaves), RSF_OK);
    assert_eq!(lifecycle.stop(&leaves), RSF_ERROR_BUSY);
    assert_eq!(status(&lifecycle, &leaves), (RSF_OK, 1, 0, text(DRAINING)));
}

#[test]
fn reasons_are_the_former_texts() {
    assert_eq!(NOT_PREPARED, c"Native renderer has not been prepared.");
    assert_eq!(
        REFUSED,
        c"Native renderer refused: researched engine bytes are unavailable or differ."
    );
    assert_eq!(
        DEACTIVATED,
        c"Native renderer deactivated after command ownership refusal."
    );
}
