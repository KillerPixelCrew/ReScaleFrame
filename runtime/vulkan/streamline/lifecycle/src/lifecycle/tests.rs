// SPDX-License-Identifier: MIT
use super::*;
use std::cell::RefCell;

const MODULE: usize = 0x1000;
const PROC: usize = 0x2000;
const HANDLE: usize = 0x3000;

struct State {
    events: Vec<&'static str>,
    verified: bool,
    resolve: i32,
    init: i32,
    shutdown: i32,
    features: Vec<u32>,
    plugin: Vec<u16>,
    common: Vec<u16>,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            events: Vec::new(),
            verified: true,
            resolve: 0,
            init: 0,
            shutdown: 0,
            features: Vec::new(),
            plugin: Vec::new(),
            common: Vec::new(),
        })
    };
}

fn record(event: &'static str) {
    STATE.with_borrow_mut(|state| state.events.push(event));
}

fn take_events() -> Vec<&'static str> {
    STATE.with_borrow_mut(|state| std::mem::take(&mut state.events))
}

fn configure(update: impl FnOnce(&mut State)) {
    STATE.with_borrow_mut(update);
}

struct Fake;

impl System for Fake {
    unsafe fn load(&self, _: *const u16) -> *mut c_void {
        record("load");
        without_provenance_mut(MODULE)
    }

    unsafe fn free(&self, module: NonNull<c_void>) {
        assert_eq!(module.as_ptr().addr(), MODULE);
        record("free");
    }

    unsafe fn close(&self, handle: *mut c_void) {
        assert_eq!(handle.addr(), HANDLE);
        record("close");
    }

    unsafe fn symbol(&self, _: *mut c_void, _: *const c_char) -> *mut c_void {
        without_provenance_mut(PROC)
    }

    unsafe fn loaded(&self, path: *const u16) -> *mut c_void {
        let path = unsafe { terminated(path) };
        STATE.with_borrow_mut(|state| state.common = path);
        without_provenance_mut(MODULE)
    }

    fn verbose(&self) -> bool {
        false
    }
}

unsafe extern "C" fn open_verified(_: *const u16) -> *mut c_void {
    record("open");
    if STATE.with_borrow(|state| state.verified) {
        without_provenance_mut(HANDLE)
    } else {
        INVALID_HANDLE
    }
}

unsafe extern "C" fn resolve(_: *mut c_void) -> i32 {
    STATE.with_borrow(|state| state.resolve)
}

unsafe extern "C" fn init(preferences: *const rsf_slvk_sdk_preferences, version: u64) -> i32 {
    record("init");
    let preferences = unsafe { &*preferences };
    assert_eq!(version, sdk_records::VERSION);
    assert_eq!(preferences.base.version, 1);
    assert_eq!(preferences.render_api, 2);
    assert_eq!(preferences.flags, 132);
    assert_eq!(preferences.log_level, 1);
    assert!(!preferences.project.is_null());
    let plugin = unsafe { terminated(*preferences.plugin_paths) };
    let features = unsafe {
        std::slice::from_raw_parts(preferences.features, preferences.num_features as usize)
    };
    STATE.with_borrow_mut(|state| {
        state.features = features.to_vec();
        state.plugin = plugin;
        state.init
    })
}

unsafe extern "C" fn shutdown(result: *mut i32) -> u32 {
    record("shutdown");
    unsafe { *result = STATE.with_borrow(|state| state.shutdown) };
    1
}

unsafe extern "C" fn clear() {
    record("clear");
}

const PLATFORM: Platform = Platform {
    open_verified,
    resolve,
    init,
    shutdown,
    clear,
    log: None,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn start(lifecycle: &Lifecycle, directory: &str) -> i32 {
    let directory = wide(directory);
    let features = feature_mask(true, true, false);
    unsafe {
        lifecycle.initialize(
            &Fake,
            &PLATFORM,
            directory.as_ptr(),
            c"project".as_ptr(),
            features,
        )
    }
}

#[test]
fn feature_mask_always_loads_pcl_and_reflex_for_frame_generation() {
    assert_eq!(feature_mask(false, false, false), FEATURE_PCL);
    assert_eq!(
        feature_mask(true, false, false),
        FEATURE_PCL | FEATURE_REFLEX
    );
    assert_eq!(
        feature_mask(false, true, false),
        FEATURE_PCL | FEATURE_REFLEX | FEATURE_DLSS_G
    );
    assert_eq!(feature_mask(false, false, true), FEATURE_PCL | FEATURE_DLSS);
}

#[test]
fn verified_handle_spans_the_load_and_keeps_the_plugin_path() {
    let lifecycle = Lifecycle::new();
    assert_eq!(start(&lifecycle, "C:\\sl"), 0);
    assert_eq!(take_events(), ["open", "load", "close", "init"]);
    assert_eq!(lifecycle.module().addr(), MODULE);
    STATE.with_borrow(|state| {
        assert_eq!(state.plugin, wide("C:\\sl").strip_suffix(&[0]).unwrap());
        assert_eq!(state.features, [4, 3, 1000]);
    });
    assert_eq!(start(&lifecycle, "C:\\sl"), 0);
    assert_eq!(start(&lifecycle, "C:\\other"), OTHER_DIRECTORY);
    let directory = wide("C:\\sl");
    let features = feature_mask(true, true, false);
    for (project, features) in [(c"other", features), (c"project", FEATURE_PCL)] {
        assert_eq!(
            unsafe {
                lifecycle.initialize(
                    &Fake,
                    &PLATFORM,
                    directory.as_ptr(),
                    project.as_ptr(),
                    features,
                )
            },
            OTHER_CONFIGURATION
        );
    }
    assert!(take_events().is_empty());
}

#[test]
fn unverified_or_overlong_interposer_is_never_loaded() {
    let lifecycle = Lifecycle::new();
    configure(|state| state.verified = false);
    assert_eq!(start(&lifecycle, "C:\\sl"), UNVERIFIED);
    assert_eq!(take_events(), ["open"]);
    configure(|state| state.verified = true);
    let long = "C:\\".to_owned() + &"d".repeat(MAX_PATH);
    assert_eq!(start(&lifecycle, &long), PATH_TOO_LONG);
    assert!(take_events().is_empty());
    assert!(lifecycle.module().is_null());
}

#[test]
fn shutdown_keeps_vulkan_dispatch_until_explicit_unload() {
    let lifecycle = Lifecycle::new();
    assert_eq!(lifecycle.with_sdk(-1, || 7), -1);
    assert_eq!(start(&lifecycle, "C:\\sl"), 0);
    take_events();
    assert_eq!(
        lifecycle.with_sdk(-1, || unsafe { lifecycle.shutdown(&PLATFORM) }),
        BUSY
    );
    assert!(take_events().is_empty());
    lifecycle.diagnostics.present_error(-3);
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, 0);
    assert_eq!(take_events(), ["shutdown", "clear"]);
    assert_eq!(lifecycle.diagnostics.take_present_error(), 0);
    assert_eq!(lifecycle.with_sdk(-1, || 7), -1);
    assert!(lifecycle.module().is_null());
    assert_eq!(lifecycle.instance_proc().addr(), PROC);
    assert_eq!(lifecycle.device_proc().addr(), PROC);
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, 0);
    assert!(take_events().is_empty());

    // A retained module is reinitialized only from the same directory.
    assert_eq!(start(&lifecycle, "C:\\other"), OTHER_DIRECTORY);
    assert_eq!(start(&lifecycle, "C:\\sl"), 0);
    assert_eq!(take_events(), ["init"]);
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, 0);
    take_events();

    assert_eq!(unsafe { lifecycle.unload(&Fake, &PLATFORM) }, 0);
    assert_eq!(take_events(), ["clear", "free"]);
    assert!(lifecycle.instance_proc().is_null());
    assert!(lifecycle.device_proc().is_null());
    assert!(lifecycle.published.module.load(Ordering::Acquire).is_null());
}

#[test]
fn failed_shutdown_is_sticky_and_refuses_unload() {
    let lifecycle = Lifecycle::new();
    assert_eq!(start(&lifecycle, "C:\\sl"), 0);
    take_events();
    configure(|state| state.shutdown = 5);
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, 5);
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, 5);
    assert_eq!(start(&lifecycle, "C:\\sl"), 5);
    assert_eq!(unsafe { lifecycle.unload(&Fake, &PLATFORM) }, 5);
    assert_eq!(take_events(), ["shutdown", "clear"]);
    assert_eq!(lifecycle.instance_proc().addr(), PROC);
}

#[test]
fn failed_initialization_releases_only_a_module_it_loaded() {
    let lifecycle = Lifecycle::new();
    configure(|state| state.init = 3);
    assert_eq!(start(&lifecycle, "C:\\sl"), 3);
    assert_eq!(
        take_events(),
        ["open", "load", "close", "init", "clear", "free"]
    );
    assert!(lifecycle.instance_proc().is_null());

    configure(|state| state.init = 0);
    assert_eq!(start(&lifecycle, "C:\\sl"), 0);
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, 0);
    take_events();
    configure(|state| state.init = 3);
    assert_eq!(start(&lifecycle, "C:\\sl"), 3);
    assert_eq!(take_events(), ["init", "clear"]);
    assert_eq!(
        lifecycle.published.module.load(Ordering::Acquire).addr(),
        MODULE
    );

    configure(|state| state.resolve = -1);
    assert_eq!(start(&lifecycle, "C:\\sl"), MISSING_EXPORT);
    assert_eq!(take_events(), ["clear"]);
    assert_eq!(lifecycle.device_proc().addr(), PROC);
}

#[test]
fn overlapping_lifecycle_call_is_refused_without_waiting() {
    let lifecycle = Lifecycle::new();
    let held = lifecycle.session.lock().unwrap();
    assert_eq!(start(&lifecycle, "C:\\sl"), BUSY);
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, BUSY);
    assert_eq!(unsafe { lifecycle.unload(&Fake, &PLATFORM) }, BUSY);
    drop(held);
    assert!(take_events().is_empty());
    assert_eq!(start(&lifecycle, "C:\\sl"), 0);
}

#[cfg(windows)]
#[test]
fn reflex_token_borrow_allows_older_present_and_defers_shutdown() {
    use std::{sync::mpsc, thread, time::Duration};

    let lifecycle = Lifecycle::new();
    let tokens = &lifecycle.frame_token;
    assert_eq!(start(&lifecycle, "C:\\sl"), 0);
    assert_eq!(
        tokens.begin(|token| {
            *token = 2;
            0
        }),
        Ok(2)
    );
    assert_eq!(tokens.begin(|_| 7), Err(7));
    take_events();

    thread::scope(|scope| {
        assert_eq!(
            tokens.with_token(0, -1, |current| {
                assert_eq!(current, 2);
                let (send, receive) = mpsc::channel();
                scope.spawn(move || {
                    let marker = tokens.with_token(1, -1, |older| {
                        assert_eq!(older, 1);
                        assert_eq!(tokens.current(), current);
                        0
                    });
                    let _ = send.send(marker);
                });
                assert_eq!(receive.recv_timeout(Duration::from_secs(2)), Ok(0));
                assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, BUSY);
                assert_eq!(unsafe { lifecycle.unload(&Fake, &PLATFORM) }, BUSY);
                assert_eq!(lifecycle.module().addr(), MODULE);
                assert!(take_events().is_empty());
                0
            }),
            0
        );
    });
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, 0);
    assert_eq!(take_events(), ["shutdown", "clear"]);
    assert_eq!(tokens.current(), 0);
    assert_eq!(tokens.with_token(1, -1, |_| 0), -1);
}

#[test]
fn symbol_lookups_follow_the_published_modules() {
    let lifecycle = Lifecycle::new();
    let name = c"rsfSlVulkanWsiCall".as_ptr();
    assert!(unsafe { lifecycle.interposer_symbol(&Fake, name) }.is_null());
    assert!(unsafe { lifecycle.common_symbol(&Fake, name) }.is_null());
    assert_eq!(start(&lifecycle, "C:/sl"), 0);
    assert!(lifecycle.initialized());
    assert_eq!(
        unsafe { lifecycle.interposer_symbol(&Fake, name) }.addr(),
        PROC
    );
    assert_eq!(unsafe { lifecycle.common_symbol(&Fake, name) }.addr(), PROC);
    STATE.with_borrow(|state| {
        assert_eq!(
            state.common,
            wide("C:\\sl\\sl.common.dll").strip_suffix(&[0]).unwrap()
        )
    });
    assert_eq!(unsafe { lifecycle.shutdown(&PLATFORM) }, 0);
    assert_eq!(unsafe { lifecycle.unload(&Fake, &PLATFORM) }, 0);
    assert!(unsafe { lifecycle.interposer_symbol(&Fake, name) }.is_null());
}
