// SPDX-License-Identifier: MIT
use super::*;

const LAYOUT: Layout = Layout {
    rt_offset: 0x578,
    zb_offset: 0x598,
    changed_offset: 0x980,
};
const SLOT_TWO: usize = LAYOUT.rt_offset as usize + 2 * size_of::<Target>();
const DIRTY: usize = LAYOUT.changed_offset as usize;

struct Backend([u8; 0x1000]);

impl Default for Backend {
    fn default() -> Self {
        Self([0; 0x1000])
    }
}

impl Backend {
    fn set_pointer(&mut self, offset: usize, value: usize) {
        self.0[offset..offset + size_of::<Target>()].copy_from_slice(&value.to_ne_bytes());
    }

    fn set_rt(&mut self, slot: usize, value: usize) {
        self.set_pointer(
            LAYOUT.rt_offset as usize + slot * size_of::<Target>(),
            value,
        );
    }

    fn bind(&mut self, position: usize, color: usize, motion: usize) {
        for (slot, value) in [position, color, motion].into_iter().enumerate() {
            self.set_rt(slot, value);
        }
        self.0[DIRTY] = 0;
    }

    fn slot_two(&self) -> usize {
        let mut bytes = [0; size_of::<Target>()];
        bytes.copy_from_slice(&self.0[SLOT_TWO..SLOT_TWO + size_of::<Target>()]);
        usize::from_ne_bytes(bytes)
    }

    fn dirty(&self) -> bool {
        self.0[DIRTY] != 0
    }
}

#[derive(Default)]
struct Host {
    binds: u32,
    clears: u32,
    targets: [usize; 4],
    depth: usize,
    refuse_all_writes: bool,
    refused_address: usize,
}

unsafe extern "C" fn read(_: Target, input: *const c_void, out: Target, size: usize) -> u32 {
    unsafe { ptr::copy_nonoverlapping(input.cast::<u8>(), out.cast(), size) };
    1
}

unsafe extern "C" fn write(host: Target, out: Target, input: *const c_void, size: usize) -> u32 {
    let host = unsafe { &*host.cast::<Host>() };
    if host.refuse_all_writes || host.refused_address == out.addr() {
        return 0;
    }
    unsafe { ptr::copy_nonoverlapping(input.cast::<u8>(), out.cast(), size) };
    1
}

unsafe extern "C" fn set_targets(host: Target, targets: *const Target, depth: Target) {
    let host = unsafe { &mut *host.cast::<Host>() };
    let targets = unsafe { targets.cast::<[Target; 4]>().read() };
    host.binds += 1;
    host.targets = targets.map(|target| target.addr());
    host.depth = depth.addr();
}

unsafe extern "C" fn clear_targets(host: Target) {
    unsafe { &mut *host.cast::<Host>() }.clears += 1;
}

static PLATFORM: Platform = Platform {
    read,
    write,
    set_targets,
    clear_targets,
};
const POSITION: usize = 0x1000;
const COLOR: usize = 0x2000;
const MOTION: usize = 0x3000;
const DEPTH: usize = 0x4000;
const OTHER: usize = 0x5000;

fn draw(state: &mut State, host: &mut Host, engine: &mut Backend, wanted: bool) -> i32 {
    rsf_anomaly_motion_bind_draw(
        state,
        &PLATFORM,
        ptr::from_mut(host).cast(),
        &LAYOUT,
        engine.0.as_mut_ptr(),
        wanted.into(),
        ptr::without_provenance(POSITION),
        ptr::without_provenance_mut(MOTION),
    )
}

fn close(state: &mut State, host: &mut Host, backend: Option<&mut Backend>) -> i32 {
    rsf_anomaly_motion_bind_close(
        state,
        &PLATFORM,
        ptr::from_mut(host).cast(),
        &LAYOUT,
        backend.map_or(ptr::null_mut(), |backend| backend.0.as_mut_ptr()),
        ptr::without_provenance_mut(MOTION),
    )
}

fn check(condition: bool, reason: &'static str) -> Result<(), &'static str> {
    if condition { Ok(()) } else { Err(reason) }
}

fn exercise() -> Result<(), &'static str> {
    let mut state = State::default();
    let mut host = Host::default();
    let mut engine = Backend::default();
    engine.set_pointer(LAYOUT.zb_offset as usize, DEPTH);
    engine.bind(POSITION, COLOR, 0);
    check(
        draw(&mut state, &mut host, &mut engine, true) == BOUND,
        "first G-buffer draw binds",
    )?;
    check(
        engine.slot_two() == MOTION && !engine.dirty(),
        "motion occupies a clean slot two",
    )?;
    check(
        host.binds == 1 && host.targets == [POSITION, COLOR, MOTION, 0] && host.depth == DEPTH,
        "the OM bind preserves the other targets and depth",
    )?;
    check(
        draw(&mut state, &mut host, &mut engine, true) == ALREADY_BOUND && host.binds == 1,
        "repeated bind leaves OM alone",
    )?;
    engine.bind(POSITION, COLOR, 0);
    check(
        draw(&mut state, &mut host, &mut engine, true) == BOUND && host.binds == 2,
        "engine bind that dropped RTV2 triggers a rebind",
    )?;
    engine.set_rt(0, OTHER);
    check(
        draw(&mut state, &mut host, &mut engine, true) == UNBOUND
            && engine.slot_two() == 0
            && host.targets[0] == OTHER
            && host.targets[2] == 0,
        "foreign RT0 unbinds",
    )?;
    check(
        draw(&mut state, &mut host, &mut engine, true) == NONE,
        "no owned slot to remove",
    )?;
    engine.bind(POSITION, COLOR, OTHER);
    check(
        draw(&mut state, &mut host, &mut engine, true) == CONFLICT
            && engine.slot_two() == OTHER
            && state.bound == 0,
        "foreign RTV2 is preserved",
    )?;
    engine.bind(POSITION, COLOR, 0);
    draw(&mut state, &mut host, &mut engine, true);
    check(
        draw(&mut state, &mut host, &mut engine, false) == UNBOUND && engine.slot_two() == 0,
        "unwanted draw drops motion",
    )?;
    draw(&mut state, &mut host, &mut engine, true);
    let clears = host.clears;
    check(
        close(&mut state, &mut host, None) == FAULT && engine.slot_two() == MOTION,
        "missing cache cannot prove cleanup",
    )?;
    let mut foreign = Backend::default();
    check(
        close(&mut state, &mut host, Some(&mut foreign)) == FAULT
            && state.cache_may_hold_motion != 0
            && engine.slot_two() == MOTION,
        "foreign cache cannot authorize retirement",
    )?;
    let mut new_context = Host::default();
    check(
        close(&mut state, &mut new_context, Some(&mut engine)) == FAULT
            && engine.slot_two() == MOTION
            && new_context.clears == 0,
        "foreign context cannot clear a live borrow",
    )?;
    check(
        close(&mut state, &mut host, Some(&mut engine)) == UNBOUND
            && engine.slot_two() == 0
            && engine.dirty()
            && host.clears == clears + 1,
        "close dirties a clean cache and unbinds",
    )?;
    engine.bind(POSITION, COLOR, 0);
    draw(&mut state, &mut host, &mut engine, true);
    engine.0[DIRTY] = 1;
    check(
        close(&mut state, &mut host, Some(&mut engine)) == UNBOUND && host.clears == clears + 1,
        "already dirty requires no extra OM clear",
    )?;
    check(
        close(&mut state, &mut host, Some(&mut engine)) == NONE,
        "close with nothing bound",
    )?;

    engine.bind(POSITION, COLOR, 0);
    host.refuse_all_writes = true;
    check(
        draw(&mut state, &mut host, &mut engine, true) == FAULT && engine.slot_two() == 0,
        "refused write leaves the slot untouched",
    )?;
    host.refuse_all_writes = false;
    engine.0[DIRTY] = 1;
    let binds = host.binds;
    let base = engine.0.as_mut_ptr().addr();
    for offset in [DIRTY, SLOT_TWO] {
        host.refused_address = base + offset;
        check(
            draw(&mut state, &mut host, &mut engine, true) == FAULT
                && engine.slot_two() == 0
                && engine.dirty()
                && host.binds == binds,
            "refused dirty-byte or slot write preserves the engine binding",
        )?;
    }
    host.refused_address = 0;
    check(
        draw(&mut state, &mut host, &mut engine, true) == BOUND,
        "retry binds",
    )?;
    host.refused_address = base + DIRTY;
    let clears = host.clears;
    check(
        close(&mut state, &mut host, Some(&mut engine)) == FAULT
            && engine.slot_two() == MOTION
            && !engine.dirty()
            && host.clears == clears
            && state.cache_may_hold_motion != 0,
        "refused close retains the borrowed target",
    )?;
    host.refused_address = 0;
    check(
        close(&mut state, &mut host, Some(&mut engine)) == UNBOUND
            && engine.slot_two() == 0
            && engine.dirty()
            && state.cache_may_hold_motion == 0
            && host.clears == clears + 1,
        "close retry verifies cleanup",
    )?;
    engine.bind(POSITION, COLOR, 0);
    draw(&mut state, &mut host, &mut engine, true);
    engine.bind(POSITION, COLOR, 0);
    check(
        close(&mut state, &mut new_context, Some(&mut engine)) == NONE
            && state.cache_may_hold_motion == 0
            && new_context.clears == 0,
        "empty original slot permits retirement after a context change",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_binder_fixture() -> bool {
    if let Err(error) = exercise() {
        eprintln!("anomaly motion binder: {error}");
        return false;
    }
    true
}
