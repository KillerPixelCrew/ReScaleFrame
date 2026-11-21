// SPDX-License-Identifier: MIT
// Binding scopes behind rescaleframe/d3d11_state.h. This owns the snapshots and every reference
// they hold, the per-thread nesting, snapshot reuse and the restore order; the leaves in
// runtime/graphics/src/d3d11_state.cpp only issue typed context calls.
use crate::com_ref::ComRef;
use crate::d3d11_output_abi::rsf_d3d11_state as State;
use std::alloc::{Layout, alloc_zeroed};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::{self, NonNull, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

type Handle = *mut c_void;

// D3D11 slot counts; the layout sizes below are asserted beside the native leaves.
const RESOURCES: usize = 128;
const SAMPLERS: usize = 16;
const CONSTANTS: usize = 14;
const INTERFACES: usize = 256;
const VERTEX_BUFFERS: usize = 32;
const UAVS: usize = 64;
const STREAM_TARGETS: usize = 4;
const VIEWPORTS: usize = 16;
const TARGETS: usize = 8;

type Viewport = [f32; 6];
type Rect = [i32; 4];

// The snapshots below are filled and bound by the native leaves. Each interface is one reference
// the save took, so dropping a snapshot releases exactly what its save retained.
#[allow(dead_code)]
#[repr(C)]
struct Stage {
    resources: [Option<ComRef>; RESOURCES],
    samplers: [Option<ComRef>; SAMPLERS],
    constants: [Option<ComRef>; CONSTANTS],
    first_constants: [u32; CONSTANTS],
    constant_counts: [u32; CONSTANTS],
    classes: [Option<ComRef>; INTERFACES],
    class_count: u32,
}

#[allow(dead_code)]
#[repr(C)]
struct Snapshot {
    context1: Option<ComRef>,
    uav_count: u32,
    constant_ranges: u32,
    layout: Option<ComRef>,
    index_buffer: Option<ComRef>,
    topology: u32,
    index_format: u32,
    index_offset: u32,
    vertex_buffers: [Option<ComRef>; VERTEX_BUFFERS],
    vertex_strides: [u32; VERTEX_BUFFERS],
    vertex_offsets: [u32; VERTEX_BUFFERS],
    // Vertex, hull, domain, geometry, pixel and compute, in that order.
    shaders: [Option<ComRef>; 6],
    stages: [Stage; 6],
    compute_uavs: [Option<ComRef>; UAVS],
    pixel_uavs: [Option<ComRef>; UAVS],
    stream_targets: [Option<ComRef>; STREAM_TARGETS],
    predicate: Option<ComRef>,
    predicate_value: i32,
    raster: Option<ComRef>,
    viewport_count: u32,
    viewports: [Viewport; VIEWPORTS],
    scissor_count: u32,
    scissors: [Rect; VIEWPORTS],
    targets: [Option<ComRef>; TARGETS],
    depth_view: Option<ComRef>,
    blend: Option<ComRef>,
    blend_factor: [f32; 4],
    blend_mask: u32,
    depth_state: Option<ComRef>,
    stencil_reference: u32,
}

#[allow(dead_code)]
#[repr(C)]
struct DepthSnapshot {
    targets: [Option<ComRef>; TARGETS],
    depth: Option<ComRef>,
    state: Option<ComRef>,
    reference: u32,
    shader: Option<ComRef>,
    classes: [Option<ComRef>; INTERFACES],
    class_count: u32,
}

#[allow(dead_code)]
#[repr(C)]
struct DrawSnapshot {
    targets: [Option<ComRef>; TARGETS],
    depth_view: Option<ComRef>,
    vertex_shader: Option<ComRef>,
    pixel_shader: Option<ComRef>,
    layout: Option<ComRef>,
    topology: u32,
    resource: Option<ComRef>,
    sampler: Option<ComRef>,
    constants: Option<ComRef>,
    blend: Option<ComRef>,
    blend_factor: [f32; 4],
    blend_mask: u32,
    depth_state: Option<ComRef>,
    stencil_reference: u32,
    raster: Option<ComRef>,
    viewport_count: u32,
    viewports: [Viewport; VIEWPORTS],
    scissor_count: u32,
    scissors: [Rect; VIEWPORTS],
}

#[repr(C)]
#[derive(Default)]
struct Features {
    uav_count: u32,
    constant_ranges: u32,
}

#[repr(C)]
struct Platform {
    immediate: unsafe extern "C" fn(Handle) -> u32,
    device: unsafe extern "C" fn(Handle) -> Handle,
    features: unsafe extern "C" fn(Handle, &mut Features),
    context1: unsafe extern "C" fn(Handle) -> Handle,
    read: unsafe extern "C" fn(Handle, &mut Snapshot),
    clear: unsafe extern "C" fn(Handle, u32),
    write: unsafe extern "C" fn(Handle, &Snapshot, u32),
    read_depth: unsafe extern "C" fn(Handle, *mut DepthSnapshot),
    write_depth: unsafe extern "C" fn(Handle, *const DepthSnapshot),
    read_draw: unsafe extern "C" fn(Handle, *mut DrawSnapshot),
    write_draw: unsafe extern "C" fn(Handle, *const DrawSnapshot),
}

// What the full scope keeps in the caller's storage. The storage may move; it must not be copied.
#[repr(C)]
struct Scope {
    // Null for a save nested under an open scope on the same context, which took nothing.
    snapshot: *mut Snapshot,
    // This save opened the thread's scope.
    outermost: bool,
}

const _: () = {
    assert!(size_of::<Stage>() == 3432);
    assert!(size_of::<Snapshot>() == 23048);
    assert!(std::mem::offset_of!(Snapshot, stages) == 608);
    assert!(std::mem::offset_of!(Snapshot, targets) == 22928);
    assert!(size_of::<DepthSnapshot>() == 2152 && size_of::<DrawSnapshot>() == 832);
    assert!(size_of::<Features>() == 8);
    assert!(size_of::<Platform>() == 11 * size_of::<Handle>());
    // The narrow snapshots and the full scope live in the caller's storage.
    assert!(size_of::<DepthSnapshot>() <= size_of::<State>());
    assert!(size_of::<DrawSnapshot>() <= size_of::<State>());
    assert!(size_of::<Scope>() <= size_of::<State>());
    assert!(align_of::<Snapshot>() <= align_of::<State>());
};

#[derive(Clone, Copy)]
struct Traits {
    // Compared only, never dereferenced.
    device: usize,
    uav_count: u32,
    constant_ranges: bool,
}

thread_local! {
    // The context a save is open on for this thread. A pass running inside another's scope would
    // only capture what the outer restore already puts back, at the cost of reading and rebinding
    // every slot of six stages, so it takes nothing and its restore does nothing. The passes that
    // nest set every binding they use and leave the outer scope to restore the game's.
    static OPEN: Cell<usize> = const { Cell::new(0) };
    // What a device can do does not change for its lifetime, so the feature queries are made once
    // per device and thread.
    static TRAITS: Cell<Traits> = const {
        Cell::new(Traits {
            device: 0,
            uav_count: 0,
            constant_ranges: false,
        })
    };
}

// One snapshot is kept for reuse, so a frame of passes does not allocate and free ~23 KB each
// time. A thread that finds it taken allocates its own.
static SPARE: AtomicPtr<Snapshot> = AtomicPtr::new(null_mut());

fn take_snapshot() -> Option<Box<Snapshot>> {
    let spare = SPARE.swap(null_mut(), Ordering::Acquire);
    if !spare.is_null() {
        return Some(unsafe { Box::from_raw(spare) });
    }
    // All zero is an empty snapshot: no references and no counts.
    let raw = unsafe { alloc_zeroed(Layout::new::<Snapshot>()) }.cast::<Snapshot>();
    NonNull::new(raw).map(|raw| unsafe { Box::from_raw(raw.as_ptr()) })
}

fn give_back(mut snapshot: Box<Snapshot>) {
    let raw: *mut Snapshot = &mut *snapshot;
    // Release every reference the save took and leave the snapshot empty again. Nothing between
    // the two can unwind.
    unsafe {
        ptr::drop_in_place(raw);
        raw.write_bytes(0, 1);
    }
    let previous = SPARE.swap(Box::into_raw(snapshot), Ordering::AcqRel);
    if !previous.is_null() {
        drop(unsafe { Box::from_raw(previous) });
    }
}

unsafe fn traits(platform: &Platform, context: Handle) -> Traits {
    let device = unsafe { ComRef::from_owned((platform.device)(context)) };
    let address = device.as_ref().map_or(0, |device| device.as_ptr() as usize);
    let cached = TRAITS.get();
    if cached.device == address {
        return cached;
    }
    let mut features = Features::default();
    if let Some(device) = &device {
        unsafe { (platform.features)(device.as_ptr(), &mut features) };
    }
    let traits = Traits {
        device: address,
        uav_count: features.uav_count,
        constant_ranges: features.constant_ranges != 0,
    };
    TRAITS.set(traits);
    traits
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d11_state_save(
    context: Handle,
    storage: *mut State,
    independent: u32,
    platform: &Platform,
) -> u32 {
    if context.is_null() || storage.is_null() {
        return 0;
    }
    unsafe { storage.write_bytes(0, 1) };
    if unsafe { (platform.immediate)(context) } == 0 {
        return 0;
    }
    let open = OPEN.get();
    if open == context as usize && independent == 0 {
        // The zeroed storage is a scope that restores nothing.
        return 1;
    }
    let Some(mut snapshot) = take_snapshot() else {
        return 0;
    };
    let outermost = open == 0;
    if outermost {
        OPEN.set(context as usize);
    }
    let traits = unsafe { traits(platform, context) };
    snapshot.uav_count = traits.uav_count;
    if traits.constant_ranges {
        snapshot.context1 = unsafe { ComRef::from_owned((platform.context1)(context)) };
        snapshot.constant_ranges = snapshot.context1.is_some().into();
    }
    unsafe {
        (platform.read)(context, &mut snapshot);
        storage.cast::<Scope>().write(Scope {
            snapshot: Box::into_raw(snapshot),
            outermost,
        });
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d11_state_restore(
    context: Handle,
    storage: *mut State,
    platform: &Platform,
) {
    if context.is_null() || storage.is_null() {
        return;
    }
    let scope = unsafe { storage.cast::<Scope>().read() };
    if !scope.snapshot.is_null() {
        let snapshot = unsafe { Box::from_raw(scope.snapshot) };
        // Output-merger targets and UAVs share slots, so pixel UAVs resume after the last target.
        let targets = snapshot
            .targets
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |last| last + 1);
        // Foreign inputs and outputs go first so no saved binding conflicts with one.
        unsafe {
            (platform.clear)(context, snapshot.uav_count);
            (platform.write)(context, &snapshot, targets as u32);
        }
        if scope.outermost {
            OPEN.set(0);
        }
        give_back(snapshot);
    }
    unsafe { storage.write_bytes(0, 1) };
}

// The narrow scopes live in the caller's storage, work on any context and do not nest.
unsafe fn save_inline<T>(
    context: Handle,
    storage: *mut State,
    read: unsafe extern "C" fn(Handle, *mut T),
) -> u32 {
    if context.is_null() || storage.is_null() {
        return 0;
    }
    let snapshot = storage.cast::<T>();
    unsafe {
        snapshot.write_bytes(0, 1);
        read(context, snapshot);
    }
    1
}

unsafe fn restore_inline<T>(
    context: Handle,
    storage: *mut State,
    write: unsafe extern "C" fn(Handle, *const T),
) {
    if context.is_null() || storage.is_null() {
        return;
    }
    let snapshot = storage.cast::<T>();
    unsafe {
        write(context, snapshot);
        // Releases what the save retained.
        ptr::drop_in_place(snapshot);
        storage.write_bytes(0, 1);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d11_depth_state_save(
    context: Handle,
    storage: *mut State,
    platform: &Platform,
) -> u32 {
    unsafe { save_inline(context, storage, platform.read_depth) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d11_depth_state_restore(
    context: Handle,
    storage: *mut State,
    platform: &Platform,
) {
    unsafe { restore_inline(context, storage, platform.write_depth) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d11_draw_state_save(
    context: Handle,
    storage: *mut State,
    platform: &Platform,
) -> u32 {
    unsafe { save_inline(context, storage, platform.read_draw) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d11_draw_state_restore(
    context: Handle,
    storage: *mut State,
    platform: &Platform,
) {
    unsafe { restore_inline(context, storage, platform.write_draw) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{Fake, fake, raw};

    const CONTEXT: usize = 0x1000;
    const DEFERRED: usize = 0x2000;

    thread_local! {
        static OBJECT: Cell<Handle> = const { Cell::new(null_mut()) };
        static READS: Cell<u32> = const { Cell::new(0) };
        static WRITES: Cell<u32> = const { Cell::new(0) };
        // Target count and UAV count of the last full write.
        static WRITTEN: Cell<[u32; 2]> = const { Cell::new([0; 2]) };
    }

    // Tests may share a thread, so each starts from its own object and counts.
    fn start(object: &Fake) {
        OBJECT.set(raw(object));
        READS.set(0);
        WRITES.set(0);
    }

    fn retained() -> Option<ComRef> {
        Some(unsafe { ComRef::retain(OBJECT.get()) })
    }

    unsafe extern "C" fn immediate(context: Handle) -> u32 {
        (context as usize != DEFERRED).into()
    }

    unsafe extern "C" fn device(_: Handle) -> Handle {
        retained().map_or(null_mut(), ComRef::into_raw)
    }

    unsafe extern "C" fn features(_: Handle, out: &mut Features) {
        out.uav_count = 8;
    }

    unsafe extern "C" fn context1(_: Handle) -> Handle {
        null_mut()
    }

    unsafe extern "C" fn read(_: Handle, snapshot: &mut Snapshot) {
        snapshot.targets[1] = retained();
        snapshot.stages[4].resources[3] = retained();
        READS.set(READS.get() + 1);
    }

    unsafe extern "C" fn clear(_: Handle, _: u32) {}

    unsafe extern "C" fn write(_: Handle, snapshot: &Snapshot, target_count: u32) {
        WRITES.set(WRITES.get() + 1);
        WRITTEN.set([target_count, snapshot.uav_count]);
    }

    unsafe extern "C" fn read_depth(_: Handle, snapshot: *mut DepthSnapshot) {
        unsafe { (*snapshot).depth = retained() };
    }

    unsafe extern "C" fn write_depth(_: Handle, _: *const DepthSnapshot) {
        WRITES.set(WRITES.get() + 1);
    }

    unsafe extern "C" fn read_draw(_: Handle, snapshot: *mut DrawSnapshot) {
        unsafe { (*snapshot).raster = retained() };
    }

    unsafe extern "C" fn write_draw(_: Handle, _: *const DrawSnapshot) {
        WRITES.set(WRITES.get() + 1);
    }

    static PLATFORM: Platform = Platform {
        immediate,
        device,
        features,
        context1,
        read,
        clear,
        write,
        read_depth,
        write_depth,
        read_draw,
        write_draw,
    };

    fn empty(state: &State) -> bool {
        state.opaque.iter().all(|word| *word == 0)
    }

    fn save(context: usize, state: *mut State, independent: u32) -> u32 {
        unsafe { rsf_core_d3d11_state_save(context as Handle, state, independent, &PLATFORM) }
    }

    fn restore(context: usize, state: &mut State) {
        unsafe { rsf_core_d3d11_state_restore(context as Handle, state, &PLATFORM) }
    }

    #[test]
    fn nested_saves_take_nothing_and_restores_release_every_reference() {
        let object = fake();
        start(&object);
        let [mut outer, mut inner, mut independent] = [(); 3].map(|()| State::default());
        assert_eq!(save(CONTEXT, &mut outer, 0), 1);
        assert_eq!(object.refs.get(), 3);
        assert_eq!(save(CONTEXT, &mut inner, 0), 1);
        assert!(empty(&inner) && READS.get() == 1);
        assert_eq!(save(CONTEXT, &mut independent, 1), 1);
        assert_eq!(READS.get(), 2);

        restore(CONTEXT, &mut inner);
        assert_eq!(WRITES.get(), 0);
        restore(CONTEXT, &mut independent);
        assert_eq!((WRITES.get(), WRITTEN.get()), (1, [2, 8]));
        assert!(empty(&independent) && object.refs.get() == 3);
        // The independent scope did not open the thread's scope, so it did not close it.
        assert_eq!(save(CONTEXT, &mut inner, 0), 1);
        assert_eq!(READS.get(), 2);
        restore(CONTEXT, &mut inner);

        restore(CONTEXT, &mut outer);
        assert_eq!((WRITES.get(), object.refs.get()), (2, 1));
        // Closed: the next save takes a real snapshot again.
        assert_eq!(save(CONTEXT, &mut outer, 0), 1);
        assert_eq!(READS.get(), 3);
        restore(CONTEXT, &mut outer);
        assert_eq!(object.refs.get(), 1);
    }

    #[test]
    fn refused_saves_leave_nothing_to_restore() {
        let object = fake();
        start(&object);
        let mut state = State::default();
        assert_eq!(save(DEFERRED, &mut state, 0), 0);
        assert_eq!(save(0, &mut state, 0), 0);
        assert_eq!(save(CONTEXT, null_mut(), 0), 0);
        restore(CONTEXT, &mut state);
        assert!(empty(&state));
        assert_eq!((READS.get(), WRITES.get(), object.refs.get()), (0, 0, 1));
    }

    #[test]
    fn narrow_scopes_release_what_they_saved() {
        let object = fake();
        start(&object);
        let context = CONTEXT as Handle;
        let deferred = DEFERRED as Handle;
        let mut state = State::default();
        unsafe {
            assert_eq!(
                rsf_core_d3d11_depth_state_save(context, &mut state, &PLATFORM),
                1
            );
            assert_eq!(object.refs.get(), 2);
            rsf_core_d3d11_depth_state_restore(context, &mut state, &PLATFORM);
            assert!(empty(&state) && object.refs.get() == 1);

            // A deferred context is allowed here.
            assert_eq!(
                rsf_core_d3d11_draw_state_save(deferred, &mut state, &PLATFORM),
                1
            );
            assert_eq!(object.refs.get(), 2);
            rsf_core_d3d11_draw_state_restore(deferred, &mut state, &PLATFORM);
            assert!(empty(&state) && object.refs.get() == 1);
            assert_eq!(
                rsf_core_d3d11_draw_state_save(null_mut(), &mut state, &PLATFORM),
                0
            );
        }
        assert_eq!(WRITES.get(), 2);
    }
}
