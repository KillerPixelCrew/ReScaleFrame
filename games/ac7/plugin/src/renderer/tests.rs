// SPDX-License-Identifier: MIT
use super::*;
use crate::fake_code::Image;
use crate::patches::{SCENE_PRECISION_EXPECTED, SCENE_PRECISION_RVA, VELOCITY};
use crate::sys::{RSF_AC7_EVIDENCE_FIRST, RSF_AC7_EVIDENCE_LAST};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::ffi::OsString;
use std::ptr::dangling_mut;

/// A scripted native side over a code image of the exact build at address zero. Every call is
/// recorded by name; hooks keep their bound, created and enabled state as MinHook would.
struct Fake {
    image: Image,
    calls: RefCell<Vec<String>>,
    renderer: Renderer,
    installed: Cell<Renderer>,
    live: Cell<bool>,
    active: Cell<bool>,
    quiescing: Cell<bool>,
    scopes: Cell<bool>,
    accepting: Cell<bool>,
    capacity: Cell<u32>,
    targets: RefCell<[usize; HOOKS]>,
    created: RefCell<[bool; HOOKS]>,
    enabled: RefCell<[bool; HOOKS]>,
    patches: UnsafeCell<rsf_ac7_code_patches>,
    // Scripted results.
    create_fails: Option<u32>,
    enable_fails: Option<u32>,
    disable_fails: Cell<Option<u32>>,
    remove_fails: Cell<Option<u32>>,
    initialize: bool,
    environment: bool,
    entry_guards: Cell<u32>,
    outer_guards: Cell<u32>,
    outstanding: Cell<bool>,
    retired: Cell<bool>,
    unhook: Cell<bool>,
    controller: Cell<bool>,
    truesky: Cell<bool>,
    pinned: Cell<bool>,
}

impl Fake {
    fn new() -> Self {
        let image = Image::default();
        image.seed_admission(0);
        image.seed_temporal_patches(0);
        Self {
            image,
            calls: RefCell::default(),
            renderer: dangling_mut(),
            installed: Cell::new(null_mut()),
            live: Cell::new(false),
            active: Cell::new(false),
            quiescing: Cell::new(false),
            scopes: Cell::new(false),
            accepting: Cell::new(false),
            capacity: Cell::new(0),
            targets: RefCell::new([0; HOOKS]),
            created: RefCell::new([false; HOOKS]),
            enabled: RefCell::new([false; HOOKS]),
            patches: UnsafeCell::new(rsf_ac7_code_patches {
                velocity_module: null_mut(),
                velocity_applied: 0,
                scene_precision_site: null_mut(),
            }),
            create_fails: None,
            enable_fails: None,
            disable_fails: Cell::new(None),
            remove_fails: Cell::new(None),
            initialize: true,
            environment: false,
            entry_guards: Cell::new(0),
            outer_guards: Cell::new(0),
            outstanding: Cell::new(false),
            retired: Cell::new(false),
            unhook: Cell::new(true),
            controller: Cell::new(true),
            truesky: Cell::new(true),
            pinned: Cell::new(false),
        }
    }

    fn call(&self, name: &str) {
        self.calls.borrow_mut().push(name.to_owned());
    }

    fn called(&self, name: &str) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|call| *call == name)
            .count()
    }

    /// Calls made since the last `take`.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.borrow_mut())
    }

    fn created_count(&self) -> usize {
        self.created
            .borrow()
            .iter()
            .filter(|created| **created)
            .count()
    }

    fn enabled_count(&self) -> usize {
        self.enabled
            .borrow()
            .iter()
            .filter(|enabled| **enabled)
            .count()
    }

    fn options() -> rsf_ac7_native_renderer_options {
        // SAFETY: all-zero is an options record without callbacks.
        let mut options: rsf_ac7_native_renderer_options = unsafe { std::mem::zeroed() };
        options.struct_size = size_of::<rsf_ac7_native_renderer_options>() as u32;
        options.session_id = 7;
        options
    }

    /// A prepared and started renderer, quiesced again: ready for stop.
    fn quiesced() -> Self {
        let fake = Self::new();
        let (renderer, prepared) = prepare(&fake, &Self::options());
        assert!(prepared && renderer == fake.renderer);
        assert!(start(&fake, renderer));
        quiesce(&fake, renderer);
        fake.take();
        fake
    }
}

impl CodeView for Fake {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        self.image.matches(address, expected)
    }
    fn log(&self, message: &str) {
        self.image.log(message);
    }
}

impl Code for Fake {
    fn write(&self, site: *mut u8, expected: &[u8], replacement: &[u8]) -> bool {
        self.image.write(site, expected, replacement)
    }
}

impl Native for Fake {
    fn base(&self) -> *mut u8 {
        null_mut()
    }
    fn installed(&self) -> Renderer {
        self.installed.get()
    }
    fn install(&self, renderer: Renderer) {
        self.call(if renderer.is_null() {
            "uninstall"
        } else {
            "install"
        });
        self.installed.set(renderer);
    }
    fn create(&self, _: &rsf_ac7_native_renderer_options) -> Renderer {
        self.call("create");
        self.live.set(true);
        self.renderer
    }
    fn release(&self, renderer: Renderer) {
        assert_eq!(renderer, self.renderer);
        self.call("release");
        self.live.set(false);
    }
    fn patches(&self, _: Renderer) -> *mut rsf_ac7_code_patches {
        self.patches.get()
    }
    fn active(&self, _: Renderer) -> bool {
        self.active.get()
    }
    fn set_active(&self, _: Renderer, active: bool) {
        self.active.set(active);
    }
    fn quiescing(&self, _: Renderer) -> bool {
        self.quiescing.get()
    }
    fn set_quiescing(&self, _: Renderer) {
        self.quiescing.set(true);
    }
    fn scopes_create(&self, _: Renderer, capacity: u32) -> bool {
        self.call("scopes_create");
        self.capacity.set(capacity);
        self.scopes.set(true);
        self.accepting.set(true);
        true
    }
    fn scopes_quiesce(&self, _: Renderer) {
        self.call("scopes_quiesce");
        self.accepting.set(false);
    }
    fn scopes_idle(&self, _: Renderer) -> bool {
        !self.scopes.get() || !self.accepting.get()
    }
    fn scopes_destroy(&self, _: Renderer) -> bool {
        self.call("scopes_destroy");
        if self.scopes.get() && self.accepting.get() {
            return false;
        }
        self.scopes.set(false);
        true
    }
    fn hooks_initialize(&self) -> bool {
        self.call("hooks_initialize");
        self.initialize
    }
    fn hook_bind(&self, hook: u32, target: *mut u8) {
        self.targets.borrow_mut()[hook as usize] = target.addr();
    }
    fn hook_create(&self, hook: u32) -> bool {
        assert_ne!(
            self.targets.borrow()[hook as usize],
            0,
            "only bound hooks are created"
        );
        if self.create_fails == Some(hook) {
            return false;
        }
        self.created.borrow_mut()[hook as usize] = true;
        true
    }
    fn hook_enable(&self, hook: u32) -> bool {
        assert!(self.created.borrow()[hook as usize]);
        if self.enable_fails == Some(hook) {
            return false;
        }
        self.enabled.borrow_mut()[hook as usize] = true;
        true
    }
    fn hook_created(&self, hook: u32) -> bool {
        self.created.borrow()[hook as usize]
    }
    fn hook_disable(&self, hook: u32) -> bool {
        assert!(
            self.created.borrow()[hook as usize],
            "only created hooks are disabled"
        );
        if self.disable_fails.get() == Some(hook) {
            return false;
        }
        self.enabled.borrow_mut()[hook as usize] = false;
        true
    }
    fn hook_remove(&self, hook: u32) -> bool {
        assert!(
            !self.enabled.borrow()[hook as usize],
            "hooks are disabled before removal"
        );
        if self.remove_fails.get() == Some(hook) {
            return false;
        }
        self.created.borrow_mut()[hook as usize] = false;
        true
    }
    fn pin(&self) {
        self.call("pin");
        self.pinned.set(true);
    }
    fn drain(&self, _: Renderer) {
        self.call("drain");
    }
    fn outstanding(&self, _: Renderer) -> bool {
        self.outstanding.get()
    }
    fn retired(&self, _: Renderer) -> bool {
        self.retired.get()
    }
    fn input_unhook(&self, _: Renderer) -> bool {
        self.call("input_unhook");
        self.unhook.get()
    }
    fn controller_disable(&self) {
        self.call("controller_disable");
    }
    fn controller_remove(&self) -> bool {
        self.call("controller_remove");
        self.controller.get()
    }
    fn truesky_restore(&self, _: Renderer) -> bool {
        self.call("truesky_restore");
        self.truesky.get()
    }
    fn guards_held(&self, outer_too: bool) -> u32 {
        // Hooks that are disabled let their bodies finish, as a forwarding call would.
        self.entry_guards.get()
            + if outer_too {
                self.outer_guards.get()
            } else {
                0
            }
    }
    fn pixel_diagnostics(&self) -> bool {
        self.environment
    }
}

fn evidence(hook: usize) -> bool {
    (RSF_AC7_EVIDENCE_FIRST as usize..=RSF_AC7_EVIDENCE_LAST as usize).contains(&hook)
}

#[test]
fn environment_flag_reads_as_the_former_eight_byte_buffer() {
    let flag = |text: &str| environment_flag(Some(&OsString::from(text)));
    assert!(!environment_flag(None));
    assert!(!flag(""));
    assert!(flag("1") && flag("yes") && flag("1234567"));
    assert!(!flag("0") && !flag("01"));
    // Eight characters did not fit the former buffer.
    assert!(!flag("12345678"));
}

#[test]
fn prepare_admits_the_build_before_anything_is_made() {
    let fake = Fake::new();
    fake.image.put(HOOK_SITES[3].rva as usize, &[0xcc]);
    assert_eq!(prepare(&fake, &Fake::options()), (null_mut(), false));
    assert!(fake.take().is_empty());
    assert!(fake.image.logged(&format!(
        "AC7 native renderer refused: expected entry bytes differ at RVA 0x{:x}",
        HOOK_SITES[3].rva
    )));
}

#[test]
fn prepare_binds_wanted_sites_and_publishes_before_hooking() {
    let fake = Fake::new();
    let (renderer, prepared) = prepare(&fake, &Fake::options());
    assert!(prepared && renderer == fake.renderer);
    assert_eq!(
        fake.take(),
        ["create", "scopes_create", "hooks_initialize", "install"]
    );
    assert_eq!(fake.capacity.get(), PENDING_CAPACITY);
    assert_eq!(fake.installed.get(), renderer);
    for (hook, site) in HOOK_SITES.iter().enumerate() {
        let target = fake.targets.borrow()[hook];
        assert_eq!(target, if evidence(hook) { 0 } else { site.rva as usize });
        assert_eq!(fake.created.borrow()[hook], !evidence(hook));
    }
    assert_eq!(fake.enabled_count(), fake.created_count());
    assert!(fake.image.logged(PREPARED) && !fake.image.logged(DIAGNOSTICS));
    assert!(!fake.active.get() && !fake.quiescing.get());
}

#[test]
fn diagnostics_bind_the_evidence_hooks() {
    let mut options = Fake::options();
    options.diagnostics = RSF_AC7_DIAG_PIXEL_BINDINGS;
    options.pending_capacity = 32;
    let fake = Fake::new();
    assert!(prepare(&fake, &options).1);
    assert_eq!(fake.capacity.get(), 32);
    assert_eq!(fake.created_count(), HOOKS);
    assert!(fake.image.logged(DIAGNOSTICS));

    let fake = Fake {
        environment: true,
        ..Fake::new()
    };
    assert!(prepare(&fake, &Fake::options()).1);
    assert_eq!(fake.created_count(), HOOKS);
}

#[test]
fn failed_initialisation_discards_the_renderer() {
    let fake = Fake {
        initialize: false,
        ..Fake::new()
    };
    assert_eq!(prepare(&fake, &Fake::options()), (null_mut(), false));
    assert_eq!(
        fake.take(),
        [
            "create",
            "scopes_create",
            "hooks_initialize",
            "scopes_quiesce",
            "scopes_destroy",
            "release"
        ]
    );
    assert!(!fake.live.get() && fake.installed.get().is_null());
}

#[test]
fn failed_installation_without_hook_bodies_removes_what_it_made() {
    for fake in [
        Fake {
            create_fails: Some(20),
            ..Fake::new()
        },
        Fake {
            enable_fails: Some(30),
            ..Fake::new()
        },
    ] {
        assert_eq!(prepare(&fake, &Fake::options()), (null_mut(), false));
        assert_eq!(fake.created_count(), 0);
        assert_eq!(fake.enabled_count(), 0);
        assert!(fake.installed.get().is_null() && !fake.live.get() && !fake.pinned.get());
        assert_eq!(fake.called("scopes_destroy"), 1);
    }
}

#[test]
fn partial_activation_keeps_trampolines_and_callbacks_until_stop() {
    let fake = Fake {
        enable_fails: Some(30),
        ..Fake::new()
    };
    // A native thread is inside an outer hook body when the installation fails.
    fake.outer_guards.set(1);
    let (renderer, prepared) = prepare(&fake, &Fake::options());
    assert!(!prepared && renderer == fake.renderer);
    // Disabled, but the trampolines a running body may still call stay created.
    assert_eq!(fake.enabled_count(), 0);
    assert_eq!(fake.created_count(), HOOKS - 6);
    assert_eq!(fake.installed.get(), renderer);
    assert!(fake.pinned.get() && fake.quiescing.get() && fake.live.get() && !fake.accepting.get());
    assert_eq!(fake.called("scopes_destroy"), 0);
    assert!(fake.image.logged(PINNED));
    // The pinned renderer cannot start.
    assert!(!start(&fake, renderer));

    // Stop waits for the body, then removes the hooks it kept.
    assert!(!stop(&fake, renderer));
    assert_eq!(fake.created_count(), HOOKS - 6);
    fake.outer_guards.set(0);
    assert!(stop(&fake, renderer));
    assert_eq!(fake.created_count(), 0);
    assert!(fake.installed.get().is_null() && !fake.live.get());
}

#[test]
fn start_requires_the_installed_unquiesced_renderer() {
    let fake = Fake::new();
    let (renderer, _) = prepare(&fake, &Fake::options());
    assert!(!start(&fake, null_mut()));
    assert!(!start(&fake, dangling_mut::<u64>().cast()));
    assert!(start(&fake, renderer));
    assert!(fake.active.get() && is_active(&fake, renderer));
    // Both engine patch groups are owned by the renderer record.
    // SAFETY: the record is only read here.
    let record = unsafe { &*fake.patches.get() };
    assert_eq!(record.velocity_applied, VELOCITY.len() as u32);
    assert_eq!(
        record.scene_precision_site.addr(),
        SCENE_PRECISION_RVA as usize
    );
    quiesce(&fake, renderer);
    assert!(!fake.active.get() && !is_active(&fake, renderer));
    assert!(!start(&fake, renderer));
}

#[test]
fn start_refusal_keeps_applied_patches_for_stop() {
    let fake = Fake::new();
    let (renderer, _) = prepare(&fake, &Fake::options());
    fake.image.put(VELOCITY[1].rva as usize, &[0xcc, 0xcc]);
    assert!(!start(&fake, renderer));
    assert!(!fake.active.get());
    // SAFETY: the record is only read here.
    assert!(
        !unsafe { &*fake.patches.get() }
            .scene_precision_site
            .is_null()
    );
    quiesce(&fake, renderer);
    assert!(stop(&fake, renderer));
    assert!(
        fake.image
            .has(SCENE_PRECISION_RVA as usize, &SCENE_PRECISION_EXPECTED)
    );
}

#[test]
fn quiesce_disables_producers_in_the_former_order() {
    let fake = Fake::new();
    quiesce(&fake, null_mut());
    assert!(fake.take().is_empty());
    let (renderer, _) = prepare(&fake, &Fake::options());
    fake.take();
    quiesce(&fake, renderer);
    assert_eq!(fake.take(), ["controller_disable", "scopes_quiesce"]);
    assert!(fake.quiescing.get() && !fake.active.get());
}

#[test]
fn stop_admission_changes_nothing_while_work_is_outstanding() {
    let fake = Fake::quiesced();
    assert!(stop(&fake, null_mut()));
    fake.active.set(true);
    assert!(!stop(&fake, fake.renderer));
    assert!(fake.take().is_empty());
    fake.active.set(false);
    let refusals: [&dyn Fn(&Fake); 3] = [
        &|fake: &Fake| fake.entry_guards.set(1),
        &|fake: &Fake| fake.outstanding.set(true),
        &|fake: &Fake| fake.accepting.set(true),
    ];
    for refuse in refusals {
        refuse(&fake);
        assert!(!stop(&fake, fake.renderer));
        assert_eq!(fake.take(), ["drain"]);
        assert_eq!(fake.enabled_count(), fake.created_count());
        fake.entry_guards.set(0);
        fake.outstanding.set(false);
        fake.accepting.set(false);
    }
}

#[test]
fn stop_retires_in_the_former_order() {
    let fake = Fake::quiesced();
    assert!(stop(&fake, fake.renderer));
    assert_eq!(
        fake.take(),
        [
            "drain",
            "input_unhook",
            "controller_remove",
            "truesky_restore",
            "scopes_destroy",
            "drain",
            "uninstall",
            "release"
        ]
    );
    assert_eq!(fake.created_count(), 0);
    assert!(
        fake.image
            .has(SCENE_PRECISION_RVA as usize, &SCENE_PRECISION_EXPECTED)
    );
    assert!(
        VELOCITY
            .iter()
            .all(|patch| fake.image.has(patch.rva as usize, patch.native))
    );
}

#[test]
fn stop_retains_renderer_after_refused_disable_or_remove() {
    let fake = Fake::quiesced();
    fake.disable_fails.set(Some(0));
    assert!(!stop(&fake, fake.renderer));
    assert_eq!(fake.installed.get(), fake.renderer);
    assert!(fake.live.get());

    fake.disable_fails.set(None);
    fake.remove_fails.set(Some(0));
    assert!(!stop(&fake, fake.renderer));
    assert_eq!(fake.created_count(), 1);
    assert_eq!(fake.installed.get(), fake.renderer);
    assert!(fake.live.get());

    fake.remove_fails.set(None);
    assert!(stop(&fake, fake.renderer));
    assert!(!fake.live.get());
}

#[test]
fn stop_retries_continue_from_the_refused_step() {
    let fake = Fake::quiesced();
    fake.unhook.set(false);
    assert!(!stop(&fake, fake.renderer));
    assert!(
        fake.enabled_count() > 0,
        "hooks stay enabled until the input hook is gone"
    );
    fake.unhook.set(true);

    // A body still running after the hooks are disabled.
    fake.outer_guards.set(1);
    assert!(!stop(&fake, fake.renderer));
    assert_eq!(fake.enabled_count(), 0);
    assert_eq!(fake.called("controller_remove"), 0);
    fake.outer_guards.set(0);

    fake.truesky.set(false);
    assert!(!stop(&fake, fake.renderer));
    // The engine patches were restored once and stay restored on the retry.
    assert!(
        fake.image
            .has(SCENE_PRECISION_RVA as usize, &SCENE_PRECISION_EXPECTED)
    );
    fake.truesky.set(true);

    // A lease retired while the scopes were destroyed: the scopes are gone, the hooks stay.
    fake.retired.set(true);
    fake.take();
    assert!(!stop(&fake, fake.renderer));
    assert!(!fake.scopes.get() && fake.created_count() > 0 && fake.live.get());
    fake.retired.set(false);
    let writes = fake.image.writes.get();
    assert!(stop(&fake, fake.renderer));
    assert_eq!(fake.image.writes.get(), writes, "nothing is restored twice");
    assert!(fake.installed.get().is_null() && !fake.live.get() && fake.created_count() == 0);
}
