// SPDX-License-Identifier: MIT
use super::*;
use crate::fake_code::Image;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ptr::without_provenance_mut;

const BASE: usize = 0x1000_0000;
/// A slot within rel32 reach of every site.
const NEAR: usize = BASE + 0x20_0000;
/// A slot beyond rel32 reach.
const FAR: usize = BASE + 0x1_0000_0000;

fn at(rva: u32) -> usize {
    BASE + rva as usize
}

fn unpatched() -> rsf_ac7_truesky_patches {
    rsf_ac7_truesky_patches {
        resolution_site: null_mut(),
        depth_branch: null_mut(),
        depth_format_site: null_mut(),
        depth_call: EMPTY_RELAY,
        motion_call: EMPTY_RELAY,
        motion_refused: false,
        resolution_refused: false,
    }
}

/// Which sites are patched and owned, in restore order. A relay counts by its call; its slot may
/// outlive the call (`restore_keeps_relay_memory_while_a_relay_call_runs`).
fn owned(state: &rsf_ac7_truesky_patches) -> [bool; 5] {
    [
        !state.resolution_site.is_null(),
        !state.motion_call.call.is_null(),
        !state.depth_call.call.is_null(),
        !state.depth_format_site.is_null(),
        !state.depth_branch.is_null(),
    ]
}

/// A scripted plugin over a code image of the fingerprinted TrueSky DLL at `BASE`.
struct Fake {
    image: Image,
    loaded: Cell<bool>,
    effect: Cell<bool>,
    ready: Cell<bool>,
    /// Next slot address; zero refuses relay memory.
    next_slot: Cell<usize>,
    slots: RefCell<Vec<(usize, Handler)>>,
    released: RefCell<Vec<usize>>,
    /// Outcomes of the next write attempts; a `false` refuses that write.
    outcomes: RefCell<VecDeque<bool>>,
    /// Addresses of successful writes, in order.
    written: RefCell<Vec<usize>>,
    releases: RefCell<Vec<&'static str>>,
    guards: Cell<u32>,
}

impl Fake {
    fn new() -> Self {
        let image = Image::default();
        image.put(at(RESOLUTION_RVA), &RESOLUTION_EXPECTED);
        image.put(at(DEPTH_CALL_RVA), &DEPTH_CALL_EXPECTED);
        image.put(
            at(DEPTH_BRANCH_RVA) - DEPTH_BRANCH_LEAD,
            &DEPTH_BRANCH_EXPECTED,
        );
        image.put(at(DEPTH_FORMAT_RVA), &DEPTH_FORMAT_EXPECTED);
        image.put(at(MOTION_CALL_RVA) - MOTION_BEFORE.len(), &MOTION_BEFORE);
        image.put(at(MOTION_CALL_RVA), &MOTION_CALL_EXPECTED);
        Self {
            image,
            loaded: Cell::new(true),
            effect: Cell::new(true),
            ready: Cell::new(false),
            next_slot: Cell::new(NEAR),
            slots: RefCell::default(),
            released: RefCell::default(),
            outcomes: RefCell::default(),
            written: RefCell::default(),
            releases: RefCell::default(),
            guards: Cell::new(0),
        }
    }

    fn slot(&self, handler: Handler) -> Option<usize> {
        self.slots
            .borrow()
            .iter()
            .find(|(_, owner)| *owner == handler)
            .map(|(slot, _)| *slot)
    }

    fn logged(&self, message: &str) -> usize {
        self.image
            .log
            .borrow()
            .iter()
            .filter(|line| *line == message)
            .count()
    }

    /// Every site holds its native bytes.
    fn native(&self) -> bool {
        self.image.has(at(RESOLUTION_RVA), &RESOLUTION_EXPECTED)
            && self.image.has(at(DEPTH_CALL_RVA), &DEPTH_CALL_EXPECTED)
            && self.image.has(
                at(DEPTH_BRANCH_RVA) - DEPTH_BRANCH_LEAD,
                &DEPTH_BRANCH_EXPECTED,
            )
            && self.image.has(at(DEPTH_FORMAT_RVA), &DEPTH_FORMAT_EXPECTED)
            && self.image.has(at(MOTION_CALL_RVA), &MOTION_CALL_EXPECTED)
    }

    /// Both relays, the depth route and, when ready, the full grid.
    fn installed(ready: bool) -> (Self, rsf_ac7_truesky_patches) {
        let fake = Self::new();
        fake.ready.set(ready);
        let mut state = unpatched();
        apply(&mut state, &fake);
        (fake, state)
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
        if self.outcomes.borrow_mut().pop_front() == Some(false) {
            return false;
        }
        let written = self.image.write(site, expected, replacement);
        if written {
            self.written.borrow_mut().push(site.addr());
        }
        written
    }
}

impl Plugin for Fake {
    fn module(&self) -> *mut u8 {
        if self.loaded.get() {
            without_provenance_mut(BASE)
        } else {
            null_mut()
        }
    }
    fn effect(&self) -> bool {
        self.effect.get()
    }
    fn relay_allocate(&self, _: *mut u8, handler: Handler) -> *mut c_void {
        let slot = self.next_slot.get();
        if slot == 0 {
            return null_mut();
        }
        self.next_slot.set(slot + 0x1000);
        self.slots.borrow_mut().push((slot, handler));
        without_provenance_mut(slot)
    }
    fn relay_release(&self, slot: *mut c_void) {
        let mut slots = self.slots.borrow_mut();
        let index = slots
            .iter()
            .position(|(live, _)| *live == slot.addr())
            .expect("only live relay memory is released");
        slots.remove(index);
        self.released.borrow_mut().push(slot.addr());
    }
    fn depth_ready(&self) -> bool {
        self.ready.get()
    }
    fn motion_release(&self) {
        self.releases.borrow_mut().push("motion");
    }
    fn depth_release(&self) {
        self.releases.borrow_mut().push("depth");
    }
    fn guards_held(&self) -> u32 {
        self.guards.get()
    }
}

/// `call [rip+rel32]` from `call` to `slot`.
fn relayed(call: usize, slot: usize) -> Vec<u8> {
    let relative = i32::try_from(slot as i64 - (call as i64 + 6)).unwrap();
    let mut bytes = vec![0xff, 0x15];
    bytes.extend_from_slice(&relative.to_le_bytes());
    bytes
}

#[test]
fn module_and_effect_fingerprints() {
    assert_eq!(
        String::from_utf16(&MODULE).unwrap(),
        "TrueSkyPluginRender_MT.dll\0"
    );
    assert_eq!(crc32_ieee(b"123456789"), 0xcbf4_3926);
    assert_eq!(crc32_ieee(&[]), 0);
    assert!(!effect_matches(&[]));
    assert!(!effect_matches(&[0; EFFECT_BYTES - 1]));
    assert!(EFFECT_PATH.ends_with(r"\Win64\mixed_resolution.fxo"));
}

#[test]
fn site_bytes_agree_with_their_guards() {
    assert_eq!(RESOLUTION_EXPECTED[..6], NATIVE_DIVISOR);
    assert_eq!(
        DEPTH_BRANCH_EXPECTED[DEPTH_BRANCH_LEAD..=DEPTH_BRANCH_LEAD],
        NATIVE_BRANCH
    );
    assert_eq!(
        DEPTH_FORMAT_EXPECTED[DEPTH_FORMAT_IMMEDIATE..DEPTH_FORMAT_IMMEDIATE + 4],
        NATIVE_DEPTH_FORMAT
    );
    assert_eq!(
        (RESOLUTION_RVA, DEPTH_CALL_RVA, DEPTH_BRANCH_RVA),
        (0x87f6d, 0xc003f, 0xbff0b)
    );
    assert_eq!(
        (
            DEPTH_FORMAT_RVA + DEPTH_FORMAT_IMMEDIATE as u32,
            MOTION_CALL_RVA
        ),
        (0xbfc09, 0xabc6b)
    );
    assert_eq!((DEPTH_CALL_BYTES, MOTION_CALL_BYTES), (7, 6));
}

#[test]
fn apply_waits_for_the_module() {
    let fake = Fake::new();
    fake.loaded.set(false);
    let mut state = unpatched();
    apply(&mut state, &fake);
    assert_eq!(owned(&state), [false; 5]);
    assert!(!state.resolution_refused && fake.written.borrow().is_empty());
    assert!(fake.image.log.borrow().is_empty());
}

#[test]
fn apply_installs_both_relays_then_waits_for_readiness() {
    let (fake, mut state) = Fake::installed(false);
    assert_eq!(owned(&state), [false, true, true, true, true]);
    assert!(!state.resolution_refused);
    let motion = fake.slot(Handler::CompositeDraw).unwrap();
    let depth = fake.slot(Handler::DepthDispatch).unwrap();
    assert!(
        fake.image
            .has(at(MOTION_CALL_RVA), &relayed(at(MOTION_CALL_RVA), motion))
    );
    // Only the six call bytes change; the next instruction stays.
    assert!(
        fake.image
            .has(at(MOTION_CALL_RVA) + 6, &MOTION_CALL_EXPECTED[6..])
    );
    let mut depth_call = relayed(at(DEPTH_CALL_RVA), depth);
    depth_call.push(0x90);
    assert!(fake.image.has(at(DEPTH_CALL_RVA), &depth_call));
    assert!(fake.image.has(at(DEPTH_BRANCH_RVA), &SUPPORTED_BRANCH));
    assert!(fake.image.has(
        at(DEPTH_FORMAT_RVA) + DEPTH_FORMAT_IMMEDIATE,
        &PRECISE_DEPTH_FORMAT
    ));
    assert!(fake.image.has(at(RESOLUTION_RVA), &NATIVE_DIVISOR));
    // Motion relay, then the selector, depth call and format, as the former install.
    assert_eq!(
        *fake.written.borrow(),
        [
            at(MOTION_CALL_RVA),
            at(DEPTH_BRANCH_RVA),
            at(DEPTH_CALL_RVA),
            at(DEPTH_FORMAT_RVA) + DEPTH_FORMAT_IMMEDIATE
        ]
    );
    for message in [MOTION_INSTALLED, DEPTH_FORMAT, DEPTH_INSTALLED] {
        assert_eq!(fake.logged(message), 1);
    }

    // Unready frames change nothing.
    apply(&mut state, &fake);
    assert_eq!(fake.written.borrow().len(), 4);
    assert_eq!(fake.slots.borrow().len(), 2);

    fake.ready.set(true);
    apply(&mut state, &fake);
    assert!(fake.image.has(at(RESOLUTION_RVA), &FULL_DIVISOR));
    assert_eq!(state.resolution_site.addr(), at(RESOLUTION_RVA));
    assert_eq!(fake.logged(RESOLUTION), 1);
    apply(&mut state, &fake);
    assert_eq!(fake.written.borrow().len(), 5);
}

#[test]
fn depth_admission_refuses_before_any_depth_write() {
    let refusals: [&dyn Fn(&Fake); 4] = [
        &|fake: &Fake| fake.image.put(at(DEPTH_CALL_RVA) + 3, &[0]),
        &|fake: &Fake| fake.image.put(at(DEPTH_BRANCH_RVA) - 1, &[3]),
        &|fake: &Fake| fake.image.put(at(DEPTH_FORMAT_RVA) + 15, &[1]),
        &|fake: &Fake| fake.effect.set(false),
    ];
    for refuse in refusals {
        let fake = Fake::new();
        fake.ready.set(true);
        refuse(&fake);
        let mut state = unpatched();
        apply(&mut state, &fake);
        assert!(state.resolution_refused);
        assert_eq!(owned(&state), [false, true, false, false, false]);
        assert_eq!(*fake.written.borrow(), [at(MOTION_CALL_RVA)]);
        assert_eq!(fake.slot(Handler::DepthDispatch), None);
        assert_eq!(fake.logged(DEPTH_REFUSED), 1);
        // The refusal holds for this renderer.
        apply(&mut state, &fake);
        assert_eq!(fake.logged(DEPTH_REFUSED), 1);
        assert!(restore(&mut state, &fake));
        assert!(fake.image.has(at(MOTION_CALL_RVA), &MOTION_CALL_EXPECTED));
        assert!(fake.slots.borrow().is_empty());
    }
}

#[test]
fn a_motion_fingerprint_refusal_does_not_block_depth() {
    let fake = Fake::new();
    fake.image.put(at(MOTION_CALL_RVA) - 1, &[0]);
    let mut state = unpatched();
    apply(&mut state, &fake);
    apply(&mut state, &fake);
    assert_eq!(fake.logged(MOTION_REFUSED), 1);
    assert!(state.motion_refused);
    // The depth route does not depend on it.
    assert_eq!(owned(&state), [false, false, true, true, true]);
}

#[test]
fn motion_write_retry_is_independent_of_both_terminal_grid_states() {
    for depth_supported in [false, true] {
        let fake = Fake::new();
        fake.ready.set(true);
        fake.effect.set(depth_supported);
        // First guarded write is the motion call. Later writes use the default success.
        fake.outcomes.borrow_mut().push_back(false);
        let mut state = unpatched();
        apply(&mut state, &fake);
        assert!(state.motion_call.slot.is_null());
        assert!(!state.motion_refused);
        assert_eq!(!state.resolution_site.is_null(), depth_supported);
        assert_eq!(state.resolution_refused, !depth_supported);
        apply(&mut state, &fake);
        assert!(!state.motion_call.slot.is_null());
        assert_eq!(fake.logged(MOTION_INSTALLED), 1);
        assert!(restore(&mut state, &fake));
    }
}

#[test]
fn relay_memory_out_of_reach_is_released() {
    let fake = Fake::new();
    fake.next_slot.set(FAR);
    let mut state = unpatched();
    apply(&mut state, &fake);
    assert!(state.resolution_refused);
    assert_eq!(owned(&state), [false; 5]);
    assert!(fake.slots.borrow().is_empty());
    assert_eq!(fake.released.borrow().len(), 2);
    assert!(fake.written.borrow().is_empty() && fake.native());
    assert_eq!(fake.logged(DEPTH_REFUSED), 1);

    let fake = Fake::new();
    fake.next_slot.set(0);
    let mut state = unpatched();
    apply(&mut state, &fake);
    assert!(state.resolution_refused && fake.native());
}

#[test]
fn a_refused_depth_relay_puts_the_selector_back() {
    let fake = Fake::new();
    // Motion relay, selector, then the depth call refuses.
    fake.outcomes.borrow_mut().extend([true, true, false]);
    let mut state = unpatched();
    apply(&mut state, &fake);
    assert!(state.resolution_refused);
    assert_eq!(owned(&state), [false, true, false, false, false]);
    assert!(fake.image.has(at(DEPTH_BRANCH_RVA), &NATIVE_BRANCH));
    assert_eq!(fake.slot(Handler::DepthDispatch), None);
}

#[test]
fn a_selector_that_cannot_be_put_back_stays_owned() {
    let fake = Fake::new();
    // Motion relay, selector, refused depth call, refused selector rollback.
    fake.outcomes
        .borrow_mut()
        .extend([true, true, false, false]);
    let mut state = unpatched();
    apply(&mut state, &fake);
    assert!(state.resolution_refused);
    assert_eq!(state.depth_branch.addr(), at(DEPTH_BRANCH_RVA));
    assert!(fake.image.has(at(DEPTH_BRANCH_RVA), &SUPPORTED_BRANCH));
    assert!(restore(&mut state, &fake));
    assert!(fake.native() && owned(&state) == [false; 5]);
}

#[test]
fn a_refused_format_keeps_the_route_owned_for_stop() {
    let fake = Fake::new();
    fake.ready.set(true);
    fake.image
        .protected
        .set(Some(at(DEPTH_FORMAT_RVA) + DEPTH_FORMAT_IMMEDIATE));
    let mut state = unpatched();
    apply(&mut state, &fake);
    assert!(state.resolution_refused);
    assert_eq!(owned(&state), [false, true, true, false, true]);
    assert_eq!(fake.logged(DEPTH_FORMAT_REFUSED), 1);
    assert_eq!(fake.logged(DEPTH_REFUSED), 1);
    assert!(fake.image.has(at(RESOLUTION_RVA), &NATIVE_DIVISOR));
    fake.image.protected.set(None);
    assert!(restore(&mut state, &fake) && fake.native());
}

#[test]
fn a_refused_divisor_keeps_native_cloud_resolution() {
    let fake = Fake::new();
    fake.ready.set(true);
    fake.image.put(at(RESOLUTION_RVA) + 15, &[0]);
    let mut state = unpatched();
    apply(&mut state, &fake);
    assert!(state.resolution_refused && state.resolution_site.is_null());
    assert_eq!(fake.logged(RESOLUTION_REFUSED), 1);

    let fake = Fake::new();
    fake.ready.set(true);
    fake.image.protected.set(Some(at(RESOLUTION_RVA)));
    let mut state = unpatched();
    apply(&mut state, &fake);
    assert!(state.resolution_refused && state.resolution_site.is_null());
    assert_eq!(fake.logged(RESOLUTION_REFUSED), 1);
}

#[test]
fn refused_full_grid_restores_the_native_divisor() {
    let (fake, mut state) = Fake::installed(true);
    refuse_full_grid(&mut state, &fake);
    assert!(state.resolution_refused && state.resolution_site.is_null());
    assert!(fake.image.has(at(RESOLUTION_RVA), &NATIVE_DIVISOR));
    assert_eq!(fake.logged(FULL_GRID_RETIRED), 1);
    // Not reactivated, and a second refusal has nothing to restore.
    apply(&mut state, &fake);
    refuse_full_grid(&mut state, &fake);
    assert!(fake.image.has(at(RESOLUTION_RVA), &NATIVE_DIVISOR));
    assert_eq!(fake.logged(FULL_GRID_RETIRED), 1);
    assert_eq!(fake.logged(FULL_GRID_RETAINED), 0);

    let (fake, mut state) = Fake::installed(true);
    fake.image.protected.set(Some(at(RESOLUTION_RVA)));
    refuse_full_grid(&mut state, &fake);
    assert!(state.resolution_refused && !state.resolution_site.is_null());
    assert_eq!(fake.logged(FULL_GRID_RETAINED), 1);
    fake.image.protected.set(None);
    assert!(restore(&mut state, &fake) && fake.native());
}

#[test]
fn restore_follows_the_former_order_and_releases_last() {
    let (fake, mut state) = Fake::installed(true);
    fake.written.borrow_mut().clear();
    let motion = fake.slot(Handler::CompositeDraw).unwrap();
    let depth = fake.slot(Handler::DepthDispatch).unwrap();
    assert!(restore(&mut state, &fake));
    assert_eq!(
        *fake.written.borrow(),
        [
            at(RESOLUTION_RVA),
            at(MOTION_CALL_RVA),
            at(DEPTH_CALL_RVA),
            at(DEPTH_FORMAT_RVA) + DEPTH_FORMAT_IMMEDIATE,
            at(DEPTH_BRANCH_RVA)
        ]
    );
    assert!(fake.native());
    assert_eq!(*fake.released.borrow(), [motion, depth]);
    assert_eq!(*fake.releases.borrow(), ["motion", "depth"]);
    assert_eq!(owned(&state), [false; 5]);
    assert!(state.motion_call.call.is_null() && state.depth_call.call.is_null());
}

#[test]
fn restore_of_an_unpatched_plugin_releases_only_the_depth_state() {
    let fake = Fake::new();
    let mut state = unpatched();
    assert!(restore(&mut state, &fake));
    assert!(fake.written.borrow().is_empty());
    assert_eq!(*fake.releases.borrow(), ["depth"]);
}

#[test]
fn restore_retries_from_the_refused_step() {
    let (fake, mut state) = Fake::installed(true);
    let steps = [
        (at(RESOLUTION_RVA), RESOLUTION_KEPT),
        (at(MOTION_CALL_RVA), MOTION_KEPT),
        (at(DEPTH_CALL_RVA), DEPTH_CALL_KEPT),
        (
            at(DEPTH_FORMAT_RVA) + DEPTH_FORMAT_IMMEDIATE,
            DEPTH_FORMAT_KEPT,
        ),
        (at(DEPTH_BRANCH_RVA), DEPTH_BRANCH_KEPT),
    ];
    for (index, (site, message)) in steps.into_iter().enumerate() {
        fake.image.protected.set(Some(site));
        assert!(!restore(&mut state, &fake));
        assert_eq!(fake.logged(message), 1);
        // Earlier steps are forgotten, this one and later ones stay owned, nothing is released.
        let owned = owned(&state);
        assert!(owned[..index].iter().all(|owned| !owned));
        assert!(owned[index..].iter().all(|owned| *owned));
        assert!(fake.released.borrow().is_empty() && fake.releases.borrow().is_empty());
    }
    fake.image.protected.set(None);
    let writes = fake.image.writes.get();
    assert!(restore(&mut state, &fake));
    assert_eq!(
        fake.image.writes.get(),
        writes + 1,
        "only the selector was left"
    );
    assert!(fake.native() && fake.slots.borrow().is_empty());
}

#[test]
fn restore_keeps_relay_memory_while_a_relay_call_runs() {
    let (fake, mut state) = Fake::installed(true);
    fake.guards.set(1);
    assert!(!restore(&mut state, &fake));
    assert_eq!(fake.logged(RELAY_RUNNING), 1);
    // The code is native again, but the slots and cloud resources stay for the running call.
    assert!(fake.native());
    assert_eq!(fake.slots.borrow().len(), 2);
    assert!(fake.releases.borrow().is_empty());
    assert!(!state.motion_call.slot.is_null() && state.motion_call.call.is_null());

    fake.guards.set(0);
    let writes = fake.image.writes.get();
    assert!(restore(&mut state, &fake));
    assert_eq!(fake.image.writes.get(), writes, "nothing is restored twice");
    assert!(fake.slots.borrow().is_empty());
    assert_eq!(*fake.releases.borrow(), ["motion", "depth"]);
    assert_eq!(owned(&state), [false; 5]);
}
