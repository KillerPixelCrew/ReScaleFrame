// SPDX-License-Identifier: MIT
//! Frame state of the shared Unity D3D12 render bridge. bridge.c is linked into the game plugin
//! and the Unity package, and both link this archive. This module owns packet admission and
//! storage, the engine policy and its generation, frame admission into the command ring, source
//! submission bookkeeping and retirement. The C11 leaf bridge.c keeps the Unity interface pointers
//! and their SDK calls.
//!
//! The Unity package's D3D11 and Vulkan bridges (d3d11_state.rs, vulkan_state.rs) share the
//! activity flag, managed stage, callback count, packet store, policy and config: a process has
//! one Unity renderer, so only one bridge uses this state. That bridge does not use the D3D12
//! ring or sources. It applies policy changes through `pending_change` and `settle_change`.
//! D3D11 and Vulkan each borrow a Runtime submission ring through `bridge_interop.rs`.
//!
//! The D3D11 and D3D12 routes retain native textures at admission and hold the packet
//! through its callback, with AddRef and Release on each texture. Vulkan carries borrowed
//! image identities and returns its packet slot when taken.
//!
//! A fence completion of 0 stands for "no fence": every recorded submission is then in flight.
use crate::sys::{
    RSF_UNITY_BRIDGE_ABI_VERSION, RSF_UNITY_FRAMES_PACKETS, RSF_UNITY_FRAMES_SLOTS,
    RSF_UNITY_FRAMES_SOURCES, RSF_UNITY_PACKET_HUDLESS, RSF_UNITY_PACKET_INPUTS_ONLY,
    RSF_UNITY_PACKET_PROBE, RSF_UNITY_PACKET_WINDOW, rsf_unity_config, rsf_unity_packet,
};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_INPUT_KEYED,
    RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_SUBMISSION, rsf_camera_frame, rsf_game_cpu_event,
    rsf_game_host_services, rsf_game_render_config, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

const PACKETS: usize = RSF_UNITY_FRAMES_PACKETS as usize;
const SLOTS: usize = RSF_UNITY_FRAMES_SLOTS as usize;
const SOURCES: usize = RSF_UNITY_FRAMES_SOURCES as usize;
const MAX_EXTENT: u32 = 16384;

// SAFETY: all-zero is a valid packet: integers, floats and null pointers.
const EMPTY_PACKET: rsf_unity_packet = unsafe { std::mem::zeroed() };
const NO_SOURCE: Source = Source {
    session_id: 0,
    source_frame_id: 0,
    view_key: 0,
};

/// A reconstruction frame admitted into command ring slot `slot`. A nonzero generation
/// identifies a pending change to acknowledge after delivery.
pub(crate) struct Admission {
    pub(crate) slot: u32,
    pub(crate) pending_generation: u32,
}

/// A recorded source frame whose engine submission is not yet confirmed to the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Source {
    pub(crate) session_id: u64,
    pub(crate) source_frame_id: u64,
    pub(crate) view_key: u64,
}

/// The references a stored packet holds on its colour, depth, motion and output resources, in
/// that order. A null resource holds nothing.
#[derive(Clone, Copy)]
struct Lease {
    resources: [*mut c_void; 4],
}

impl Lease {
    /// Drops every held reference. Runs outside the state mutex.
    fn end(self) {
        for resource in self.resources {
            if !resource.is_null() {
                unsafe { release_texture(resource) };
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Pending {
    packet: rsf_unity_packet,
    used: bool,
    /// Taken by its render event, which still reads the leased resources. The slot stays used and
    /// its address cannot be taken again until `end_packet`.
    consuming: bool,
    lease: Option<Lease>,
}

const FREE: Pending = Pending {
    packet: EMPTY_PACKET,
    used: false,
    consuming: false,
    lease: None,
};

/// The engine policy the managed adapter reads through config and stamps into packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Policy {
    engine_spatial: u32,
    quality: u32,
    generation: u32,
    /// A change the next admitted frame applies as a reconfiguration once the ring is idle.
    pending: bool,
}

struct State {
    pending: [Pending; PACKETS],
    /// Unity completion value of each command ring slot's last submission; 0 for none.
    complete: [u64; SLOTS],
    next: usize,
    sources: [Source; SOURCES],
    /// Latest completion of a copy recorded on Unity's own list.
    inline_complete: u64,
    input: (u32, u32),
    output: (u32, u32),
    /// A submission returned no completion: its resources may still be in use.
    submission_unknown: bool,
    callbacks: u32,
    closed: bool,
    policy: Policy,
}

// Packets hold Unity resource addresses. This module only passes leased resources to typed leaves.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State::new());
static ACTIVE: AtomicBool = AtomicBool::new(false);
static D3D12_RECORDING: AtomicBool = AtomicBool::new(false);
static MANAGED_STAGE: AtomicU32 = AtomicU32::new(0);

thread_local! {
    // The C02 host queries this only while processing the reconfiguration pass.
    static DELIVERED_GENERATION: Cell<u32> = const { Cell::new(0) };
}

fn lock() -> MutexGuard<'static, State> {
    lock_or_recover(&STATE)
}

fn source_of(packet: &rsf_unity_packet) -> Source {
    Source {
        session_id: packet.session_id,
        source_frame_id: packet.frame_id,
        view_key: packet.view_key,
    }
}

fn in_flight(complete: u64, completed: u64) -> bool {
    complete != 0 && (completed == u64::MAX || completed < complete)
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn valid_camera(camera: &rsf_camera_frame) -> bool {
    let extents = [
        camera.render_width,
        camera.render_height,
        camera.output_width,
        camera.output_height,
    ];
    camera.struct_size == size_of::<rsf_camera_frame>() as u32
        && camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
        && extents
            .iter()
            .all(|&extent| extent != 0 && extent <= MAX_EXTENT)
        && camera.near_plane.is_finite()
        && camera.far_plane.is_finite()
        && camera.near_plane > 0.0
        && camera.far_plane > camera.near_plane
        && finite(&camera.view_to_clip)
        && finite(&camera.clip_to_view)
        && finite(&camera.clip_to_previous_clip)
        && finite(&camera.jitter_pixels)
}

/// The packet admission rules: this ABI and session, a frame and view, the resources each event
/// kind reads, and a finite camera with bounded extents.
fn valid(packet: &rsf_unity_packet, session_id: u64) -> bool {
    let flags = packet.flags;
    let has = |resource: *mut c_void| !resource.is_null();
    if packet.struct_size != size_of::<rsf_unity_packet>() as u32
        || packet.abi_version != RSF_UNITY_BRIDGE_ABI_VERSION
        || packet.session_id != session_id
        || packet.frame_id == 0
        || packet.view_key == 0
        || (flags & (RSF_UNITY_PACKET_WINDOW | RSF_UNITY_PACKET_HUDLESS) == 0 && !has(packet.color))
    {
        return false;
    }
    if flags & (RSF_UNITY_PACKET_PROBE | RSF_UNITY_PACKET_WINDOW) == 0
        && !(has(packet.depth) && has(packet.motion) && has(packet.output))
    {
        return false;
    }
    if flags & RSF_UNITY_PACKET_INPUTS_ONLY != 0 && !(has(packet.depth) && has(packet.motion)) {
        return false;
    }
    valid_camera(&packet.camera)
}

impl State {
    const fn new() -> Self {
        Self {
            pending: [FREE; PACKETS],
            complete: [0; SLOTS],
            next: 0,
            sources: [NO_SOURCE; SOURCES],
            inline_complete: 0,
            input: (0, 0),
            output: (0, 0),
            submission_unknown: false,
            callbacks: 0,
            closed: false,
            policy: Policy {
                engine_spatial: 0,
                quality: 1,
                generation: 1,
                pending: false,
            },
        }
    }

    fn admit_packet(&mut self, packet: &rsf_unity_packet, session_id: u64) -> Option<usize> {
        if !valid(packet, session_id) {
            return None;
        }
        let slot = self.pending.iter().position(|entry| !entry.used)?;
        self.pending[slot] = Pending {
            packet: *packet,
            used: true,
            ..FREE
        };
        Some(slot)
    }

    /// Admits like `admit_packet`, then retains each non-null resource of the stored copy under
    /// the state mutex, so the address is never returned, and the packet never taken, without its
    /// lease. A refused packet retains nothing.
    fn admit_leased(&mut self, packet: &rsf_unity_packet, session_id: u64) -> Option<usize> {
        let slot = self.admit_packet(packet, session_id)?;
        let entry = &mut self.pending[slot];
        let stored = &entry.packet;
        let resources = [stored.color, stored.depth, stored.motion, stored.output];
        for resource in resources {
            if !resource.is_null() {
                unsafe { retain_texture(resource) };
            }
        }
        entry.lease = Some(Lease { resources });
        Some(slot)
    }

    fn packet_address(&self, slot: usize) -> *mut c_void {
        ptr::from_ref(&self.pending[slot].packet).cast_mut().cast()
    }

    /// Copies out the queued packet stored at `address`. An unleased packet's slot is freed at
    /// once; a leased one stays with its lease, consuming, until `end_packet`.
    fn take_packet(&mut self, address: *const c_void) -> Option<(usize, rsf_unity_packet)> {
        let slot = (0..PACKETS).find(|&slot| {
            let entry = &self.pending[slot];
            entry.used
                && !entry.consuming
                && ptr::eq(self.packet_address(slot).cast_const(), address)
        })?;
        let entry = &mut self.pending[slot];
        let packet = entry.packet;
        if entry.lease.is_some() {
            entry.consuming = true;
        } else {
            *entry = FREE;
        }
        Some((slot, packet))
    }

    /// Frees a consuming slot and hands its lease out for release outside the mutex. Any other
    /// slot, including one queued since an unleased take freed it, is left alone.
    fn end_packet(&mut self, slot: usize) -> Option<Lease> {
        let entry = self.pending.get_mut(slot)?;
        if !entry.consuming {
            return None;
        }
        let lease = entry.lease;
        *entry = FREE;
        lease
    }

    fn any_in_flight(&self, completed: u64) -> bool {
        self.complete
            .iter()
            .any(|&complete| in_flight(complete, completed))
    }

    fn set_policy(&mut self, engine_spatial: u32, quality: u32, active: bool) -> bool {
        if engine_spatial > 1 || quality > 5 {
            return false;
        }
        let policy = &mut self.policy;
        policy.engine_spatial = engine_spatial;
        policy.quality = quality;
        policy.generation = policy.generation.wrapping_add(1).max(1);
        if active {
            policy.pending = true;
        }
        true
    }

    /// The generation of the pending policy change, for the package's D3D11 and Vulkan events,
    /// which apply it outside `admit_frame`.
    fn pending_change(&self) -> Option<u32> {
        self.policy.pending.then_some(self.policy.generation)
    }

    /// The pending change once no ring slot is in flight. Only the ring records with the host's
    /// SR contexts; the pre-UI copies on Unity's own list do not, so they never hold it back.
    fn idle_change(&self, completed: u64) -> Option<u32> {
        let idle =
            !self.submission_unknown && completed != u64::MAX && !self.any_in_flight(completed);
        self.pending_change().filter(|_| idle)
    }

    /// Clears the pending change once its generation is applied. A newer change stays pending.
    fn settle_change(&mut self, generation: u32) -> bool {
        let settled = self.policy.pending && self.policy.generation == generation;
        if settled {
            self.policy.pending = false;
        }
        settled
    }

    /// Render extents, a policy change and output extents change only with the ring idle; a
    /// packet from an older generation is refused unless it carries the pending change. The
    /// state accepted by an earlier step is kept when a later one refuses.
    fn admit_frame(&mut self, packet: &rsf_unity_packet, completed: u64) -> Option<Admission> {
        if self.submission_unknown
            || completed == u64::MAX
            || !self.can_record_source(&source_of(packet))
        {
            return None;
        }
        let busy = self.any_in_flight(completed) || in_flight(self.inline_complete, completed);
        let camera = &packet.camera;
        let hudless = packet.flags & RSF_UNITY_PACKET_HUDLESS != 0;
        let input = (camera.render_width, camera.render_height);
        if !hudless && self.input != input {
            if busy {
                return None;
            }
            self.input = input;
        }
        let pending_generation = self.pending_change();
        if pending_generation.is_some() {
            if busy || hudless {
                return None;
            }
        } else if packet.generation != self.policy.generation {
            return None;
        }
        let output = (camera.output_width, camera.output_height);
        if self.output != output {
            if busy {
                return None;
            }
            self.output = output;
        }
        // Reuse only after Unity's completion fence.
        if in_flight(self.complete[self.next], completed) {
            return None;
        }
        Some(Admission {
            slot: self.next as u32,
            pending_generation: pending_generation.unwrap_or(0),
        })
    }

    fn submitted(&mut self, admission: &Admission, complete: u64, source: &Source) -> bool {
        let slot = admission.slot as usize;
        if slot >= SLOTS {
            return false;
        }
        self.complete[slot] = complete;
        // An unavailable or device-removed completion leaves the slot owned.
        if matches!(complete, 0 | u64::MAX) {
            self.submission_unknown = true;
            return false;
        }
        self.next = (slot + 1) % SLOTS;
        self.record_source(source, 0)
    }

    fn can_record_source(&self, source: &Source) -> bool {
        source.source_frame_id != 0
            && self
                .sources
                .iter()
                .any(|entry| entry.source_frame_id == 0 || entry == source)
    }

    fn record_source(&mut self, source: &Source, inline_fence: u64) -> bool {
        if !self.can_record_source(source) {
            return false;
        }
        if !self.sources.iter().any(|entry| entry == source) {
            let free = self
                .sources
                .iter_mut()
                .find(|entry| entry.source_frame_id == 0)
                .expect("checked receipt capacity");
            *free = *source;
        }
        self.inline_complete = self.inline_complete.max(inline_fence);
        true
    }

    fn take_flushed(
        &mut self,
        session_id: u64,
        source_frame_id: u64,
    ) -> ([Source; SOURCES], usize) {
        let mut flushed = [NO_SOURCE; SOURCES];
        let mut remaining = [NO_SOURCE; SOURCES];
        let (mut count, mut retained) = (0, 0);
        for source in self
            .sources
            .into_iter()
            .filter(|source| source.source_frame_id != 0)
        {
            if source.session_id == session_id && source.source_frame_id <= source_frame_id {
                flushed[count] = source;
                count += 1;
            } else {
                remaining[retained] = source;
                retained += 1;
            }
        }
        self.sources = remaining;
        (flushed, count)
    }

    fn drained(&self, completed: u64) -> bool {
        if completed == u64::MAX
            || self.submission_unknown
            || self.pending.iter().any(|entry| entry.used)
        {
            return false;
        }
        !in_flight(self.inline_complete, completed) && !self.any_in_flight(completed)
    }

    fn try_close(&mut self, active: bool) -> bool {
        if active || self.callbacks != 0 || self.pending.iter().any(|entry| entry.used) {
            return false;
        }
        self.closed = true;
        true
    }

    #[cfg(any(test, feature = "mono-plugin"))]
    fn try_close_completed(&mut self, active: bool, completed: u64) -> bool {
        completed != u64::MAX && self.drained(completed) && self.try_close(active)
    }

    /// Forgets the store and returns the leases of any packet still queued or consuming, which
    /// release cancels outside the mutex.
    fn release(&mut self) -> [Option<Lease>; PACKETS] {
        let leases = self.pending.map(|entry| entry.lease);
        let policy = self.policy;
        // Preserve the admission state across retirement.
        let callbacks = self.callbacks;
        let closed = self.closed;
        *self = Self::new();
        self.policy = policy;
        self.callbacks = callbacks;
        self.closed = closed;
        leases
    }

    fn config(
        &self,
        active: bool,
        host: &rsf_game_render_config,
        output_width: u32,
        output_height: u32,
    ) -> rsf_unity_config {
        let policy = self.policy;
        let matches = host.output_width == output_width && host.output_height == output_height;
        rsf_unity_config {
            struct_size: size_of::<rsf_unity_config>() as u32,
            abi_version: RSF_UNITY_BRIDGE_ABI_VERSION,
            enabled: if active && !policy.pending && matches {
                host.enabled
            } else {
                0
            },
            engine_spatial: policy.engine_spatial,
            quality: policy.quality,
            generation: policy.generation,
            render_width: host.render_width,
            render_height: host.render_height,
            output_width,
            output_height,
        }
    }
}

/// render_bridge.c deactivates the bridge when its Unity interfaces unload.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_unity_frames_activate(enabled: u32) {
    let mut state = lock();
    if enabled != 0 {
        state.closed = false;
    }
    ACTIVE.store(enabled != 0, Ordering::SeqCst);
}

pub(crate) fn rsf_unity_frames_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

/// The managed adapter's stage report (rsf_unity_native_api::managed_state).
pub(crate) extern "C" fn rsf_unity_frames_report_managed(stage: u32) {
    let state = lock();
    if !state.closed {
        MANAGED_STAGE.store(stage, Ordering::SeqCst);
    }
}

pub fn rsf_unity_frames_managed_stage() -> u32 {
    MANAGED_STAGE.load(Ordering::SeqCst)
}

/// The admission both entry points share: the stored slot and the address Unity hands back.
///
/// # Safety
/// `packet` is null or readable for its leading size field, and for a whole packet when that
/// field matches. A leased packet's non-null resources must be live COM objects.
unsafe fn admit(packet: *const rsf_unity_packet, session_id: u64, leased: bool) -> *mut c_void {
    if packet.is_null() {
        return ptr::null_mut();
    }
    // Only the size field is readable from a short packet.
    if unsafe { packet.cast::<u32>().read_unaligned() } != size_of::<rsf_unity_packet>() as u32 {
        return ptr::null_mut();
    }
    let packet = unsafe { packet.read_unaligned() };
    let mut state = lock();
    if state.closed || !ACTIVE.load(Ordering::SeqCst) {
        return ptr::null_mut();
    }
    let slot = if leased {
        state.admit_leased(&packet, session_id)
    } else {
        state.admit_packet(&packet, session_id)
    };
    slot.map_or(ptr::null_mut(), |slot| state.packet_address(slot))
}

/// Stores a packet whose resources are borrowed identities.
#[cfg(not(feature = "mono-plugin"))]
pub(crate) unsafe fn rsf_unity_frames_admit_packet(
    packet: *const rsf_unity_packet,
    session_id: u64,
) -> *mut c_void {
    unsafe { admit(packet, session_id, false) }
}

/// Stores a packet and retains its native textures. The returned address already holds the lease.
#[cfg(windows)]
pub(crate) unsafe fn rsf_unity_frames_admit_leased_packet(
    packet: *const rsf_unity_packet,
    session_id: u64,
) -> *mut c_void {
    unsafe { admit(packet, session_id, true) }
}

/// Copies out the queued packet stored at `address` with its slot.
pub(crate) fn rsf_unity_frames_take_packet(
    address: *const c_void,
) -> Option<(u32, rsf_unity_packet)> {
    let (slot, packet) = lock().take_packet(address)?;
    Some((slot as u32, packet))
}

/// A CPU event of `session_id` for `source`, stamped `(timestamp_qpc, qpc_frequency)`. The input
/// kind and message are zero for a stage and set for received input.
pub(crate) fn build_cpu_event(
    session_id: u64,
    stage: u32,
    source: u64,
    (timestamp_qpc, qpc_frequency): (u64, u64),
    input_kind: u32,
    message_id: u32,
    flags: u32,
) -> rsf_game_cpu_event {
    rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage,
        session_id,
        source_frame_id: source,
        timestamp_qpc,
        qpc_frequency,
        render_expected: 0,
        input_kind,
        message_id,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags,
    }
}

pub(crate) fn rsf_unity_frames_cpu_stage(
    services: &rsf_game_host_services,
    stage: u32,
    source: u64,
    time: (u64, u64),
) {
    if source == 0 || !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    if let Some(deliver) = services.cpu_event {
        let flags = RSF_GAME_CPU_INPUT_KEYED;
        let event = build_cpu_event(services.session_id, stage, source, time, 0, 0, flags);
        unsafe { deliver(services.user, &event) };
    }
}

/// The render event that took a leased packet from `slot` has ended: free the slot, then release
/// the lease outside the state mutex.
pub(crate) fn rsf_unity_frames_end_packet(slot: u32) {
    let lease = lock().end_packet(slot as usize);
    if let Some(lease) = lease {
        lease.end();
    }
}

/// Fills the managed adapter's config from the host's render config and the current policy.
///
/// # Safety
/// `output` is null or the managed adapter's config record, readable for its header.
pub(crate) unsafe fn rsf_unity_frames_config(
    services: &rsf_game_host_services,
    output_width: u32,
    output_height: u32,
    output: *mut rsf_unity_config,
) -> c_int {
    if output.is_null() {
        return 0;
    }
    let header = output.cast::<u32>();
    if unsafe { header.read() } != size_of::<rsf_unity_config>() as u32
        || unsafe { header.add(1).read() } != RSF_UNITY_BRIDGE_ABI_VERSION
    {
        return 0;
    }
    let Some(render_config) = services.render_config else {
        return 0;
    };
    let mut host = rsf_game_render_config::request();
    // The host call runs without the state lock.
    if unsafe { render_config(services.user, &mut host) } == 0 {
        return 0;
    }
    let config = lock().config(
        ACTIVE.load(Ordering::SeqCst),
        &host,
        output_width,
        output_height,
    );
    unsafe { output.write(config) };
    1
}

pub(crate) fn rsf_unity_frames_set_policy(engine_spatial: u32, quality: u32) -> bool {
    lock().set_policy(engine_spatial, quality, ACTIVE.load(Ordering::SeqCst))
}

/// The Mono host's policy request (rsf_game_unity_policy::request_policy).
///
/// # Safety
///
/// `generation` is null or writable.
pub unsafe extern "C" fn rsf_unity_mono_request_policy(
    engine_spatial: u32,
    quality: u32,
    generation: *mut u32,
) -> c_int {
    if generation.is_null() {
        return 0;
    }
    let mut state = lock();
    if !state.set_policy(engine_spatial, quality, ACTIVE.load(Ordering::SeqCst)) {
        return 0;
    }
    unsafe { generation.write(state.policy.generation) };
    1
}

/// # Safety
///
/// `render_width` and `render_height` are null or writable.
pub unsafe extern "C" fn rsf_unity_mono_spatial_extent(
    quality: u32,
    width: u32,
    height: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> c_int {
    // The Mono URP camera adapter applies these extents as its FSR1 render scale.
    const RATIOS: [f32; 6] = [1.0, 1.5, 1.7, 2.0, 3.0, 1.3];
    let Some(&ratio) = RATIOS.get(quality as usize) else {
        return 0;
    };
    if render_width.is_null() || render_height.is_null() {
        return 0;
    }
    unsafe {
        render_width.write((width as f32 / ratio) as u32);
        render_height.write((height as f32 / ratio) as u32);
    }
    1
}

/// The generation of the pending policy change, for the package's D3D11 and Vulkan events.
#[cfg(not(feature = "mono-plugin"))]
pub(crate) fn rsf_unity_frames_pending_change() -> Option<u32> {
    lock().pending_change()
}

#[cfg(not(feature = "mono-plugin"))]
pub(crate) fn rsf_unity_frames_settle_change(generation: u32) -> bool {
    lock().settle_change(generation)
}

/// The pending change the Runtime host may apply at the window event, once Unity's ring is idle.
pub(crate) fn rsf_unity_frames_idle_change(completed: u64) -> Option<u32> {
    lock().idle_change(completed)
}

pub(crate) fn rsf_unity_frames_admit_frame(
    packet: &rsf_unity_packet,
    completed: u64,
) -> Option<Admission> {
    lock().admit_frame(packet, completed)
}

/// Records a submission. One that gives no completion stops the bridge.
pub(crate) fn rsf_unity_frames_submitted(
    admission: &Admission,
    complete: u64,
    source: &Source,
) -> bool {
    if lock().submitted(admission, complete, source) {
        return true;
    }
    ACTIVE.store(false, Ordering::SeqCst);
    false
}

pub(crate) fn rsf_unity_frames_can_record_source(source: &Source) -> bool {
    lock().can_record_source(source)
}

pub(crate) fn rsf_unity_frames_record_source(source: &Source, inline_fence: u64) {
    let recorded = lock().record_source(source, inline_fence);
    debug_assert!(
        recorded,
        "the event checked inline receipt capacity before recording"
    );
}

pub(crate) fn rsf_unity_frames_flush_sources(
    services: &rsf_game_host_services,
    queue: *mut c_void,
    window: &rsf_game_render_pass,
) {
    let Some(render) = services.render_pass else {
        return;
    };
    if queue.is_null() {
        return;
    }
    let (flushed, count) = lock().take_flushed(window.session_id, window.source_frame_id);
    for source in &flushed[..count] {
        // The submission pass carries identity only. Unity has already flushed its lists.
        let submission = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            role: RSF_GAME_RENDER_SUBMISSION,
            session_id: source.session_id,
            source_frame_id: source.source_frame_id,
            view_key: source.view_key,
            ..Default::default()
        };
        unsafe { render(services.user, queue, &submission, 0) };
    }
}

pub(crate) fn rsf_unity_frames_dispatch_d3d12(
    services: &rsf_game_host_services,
    commands: *mut c_void,
    pass: &mut rsf_game_render_pass,
    generation: u32,
) -> bool {
    let Some(render) = services.render_pass else {
        return false;
    };
    if commands.is_null() || pass.session_id != services.session_id {
        return false;
    }
    if generation != 0 {
        if lock().pending_change() != Some(generation) {
            return false;
        }
        pass.flags |= RSF_GAME_RENDER_RECONFIGURE;
        pass.camera_valid = 0;
    }
    DELIVERED_GENERATION.set(generation);
    unsafe { render(services.user, commands, pass, 1) };
    DELIVERED_GENERATION.set(0);
    if generation != 0 {
        lock().settle_change(generation);
    }
    true
}

/// The Mono host's query for the policy generation being delivered on this thread.
///
/// # Safety
///
/// `generation` is null or writable.
pub unsafe extern "C" fn rsf_unity_mono_drained_generation(generation: *mut u32) -> c_int {
    let delivered = DELIVERED_GENERATION.get();
    if generation.is_null() || delivered == 0 {
        return 0;
    }
    unsafe { generation.write(delivered) };
    1
}

pub(crate) fn rsf_unity_frames_begin_d3d12_event() -> bool {
    D3D12_RECORDING
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
}

pub(crate) fn rsf_unity_frames_end_d3d12_event() {
    D3D12_RECORDING.store(false, Ordering::Release);
}

pub(crate) fn rsf_unity_frames_callback_enter() -> bool {
    let mut state = lock();
    if state.closed {
        return false;
    }
    state.callbacks += 1;
    true
}

pub(crate) fn rsf_unity_frames_callback_leave() {
    lock().callbacks -= 1;
}

#[cfg(feature = "mono-plugin")]
pub(crate) fn rsf_unity_frames_open_callbacks() {
    lock().closed = false;
}

/// Keep queued packet delivery open until all packets have been taken
/// and every leased packet's render event has ended.
#[cfg(not(feature = "mono-plugin"))]
pub(crate) fn rsf_unity_frames_try_close() -> bool {
    lock().try_close(ACTIVE.load(Ordering::SeqCst))
}

#[cfg(feature = "mono-plugin")]
pub(crate) fn rsf_unity_frames_try_close_completed(completed: u64) -> bool {
    lock().try_close_completed(ACTIVE.load(Ordering::SeqCst), completed)
}

#[cfg(not(feature = "mono-plugin"))]
pub(crate) fn rsf_unity_frames_drained(completed: u64) -> bool {
    let state = lock();
    state.callbacks == 0 && state.drained(completed)
}

/// A line to the host's log, guarded against a foreign exception.
pub(crate) fn log(host: &rsf_game_host_services, text: &std::ffi::CStr) {
    unsafe { rsf_game_sys::rsf_game_log_guarded(host.log, host.user, text.as_ptr()) };
}

pub(crate) fn rsf_unity_frames_release() {
    let leases = lock().release();
    MANAGED_STAGE.store(0, Ordering::SeqCst);
    for lease in leases.into_iter().flatten() {
        lease.end();
    }
}

#[cfg(test)]
use tests::{release_texture, retain_texture};

/// Takes one more COM reference on a non-null texture.
#[cfg(not(test))]
unsafe fn retain_texture(texture: *mut c_void) {
    use windows::core::{IUnknown, Interface};
    if let Some(resource) = unsafe { IUnknown::from_raw_borrowed(&texture) } {
        let _ = resource.clone().into_raw();
    }
}

/// Drops the reference `retain_texture` took.
#[cfg(not(test))]
unsafe fn release_texture(texture: *mut c_void) {
    use windows::core::{IUnknown, Interface};
    if !texture.is_null() {
        drop(unsafe { IUnknown::from_raw(texture) });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::{RSF_UNITY_PACKET_NO_INPUTS, RSF_UNITY_PACKET_RESET};

    const SESSION: u64 = 7;

    fn resource(value: usize) -> *mut c_void {
        ptr::without_provenance_mut(value)
    }

    fn camera() -> rsf_camera_frame {
        rsf_camera_frame {
            struct_size: size_of::<rsf_camera_frame>() as u32,
            abi_version: RSF_GAME_CAMERA_ABI_VERSION,
            render_width: 1280,
            render_height: 720,
            output_width: 2560,
            output_height: 1440,
            near_plane: 0.1,
            far_plane: 1000.0,
            ..Default::default()
        }
    }

    fn packet(frame_id: u64) -> rsf_unity_packet {
        rsf_unity_packet {
            struct_size: size_of::<rsf_unity_packet>() as u32,
            abi_version: RSF_UNITY_BRIDGE_ABI_VERSION,
            session_id: SESSION,
            frame_id,
            view_key: 3,
            generation: 1,
            flags: RSF_UNITY_PACKET_RESET,
            color: resource(0x10),
            depth: resource(0x20),
            motion: resource(0x30),
            output: resource(0x40),
            camera: camera(),
            previous_clip_to_clip: [0.0; 16],
        }
    }

    fn source(frame_id: u64) -> Source {
        Source {
            session_id: SESSION,
            source_frame_id: frame_id,
            view_key: 3,
        }
    }

    #[test]
    fn mono_closes_only_after_callbacks_and_gpu_work_retire() {
        let mut state = State::new();
        state.callbacks = 1;
        assert!(!state.try_close_completed(false, 10));
        assert!(!state.closed);
        state.callbacks = 0;
        state.inline_complete = 11;
        assert!(!state.try_close_completed(false, 10));
        assert!(!state.try_close_completed(false, u64::MAX));
        assert!(!state.try_close_completed(true, 11));
        assert!(state.try_close_completed(false, 11));
        assert!(state.closed);
        let _ = state.release();
        assert!(state.closed);
    }

    #[test]
    fn packets_need_their_event_resources_and_a_finite_camera() {
        assert!(valid(&packet(1), SESSION));
        assert!(!valid(&packet(1), SESSION + 1));
        assert!(!valid(&packet(0), SESSION));

        let mut window = packet(1);
        window.flags = RSF_UNITY_PACKET_WINDOW;
        window.color = ptr::null_mut();
        window.depth = ptr::null_mut();
        assert!(valid(&window, SESSION));

        let mut probe = packet(1);
        probe.flags = RSF_UNITY_PACKET_PROBE;
        probe.output = ptr::null_mut();
        assert!(valid(&probe, SESSION));
        probe.flags |= RSF_UNITY_PACKET_INPUTS_ONLY;
        probe.motion = ptr::null_mut();
        assert!(!valid(&probe, SESSION));

        let mut reconstruction = packet(1);
        reconstruction.flags = RSF_UNITY_PACKET_NO_INPUTS;
        reconstruction.output = ptr::null_mut();
        assert!(!valid(&reconstruction, SESSION));

        let mut hudless = packet(1);
        hudless.flags = RSF_UNITY_PACKET_HUDLESS;
        hudless.color = ptr::null_mut();
        assert!(valid(&hudless, SESSION));

        let mut bad = packet(1);
        bad.camera.jitter_pixels[1] = f32::NAN;
        assert!(!valid(&bad, SESSION));
        bad = packet(1);
        bad.camera.output_width = MAX_EXTENT + 1;
        assert!(!valid(&bad, SESSION));
        bad = packet(1);
        bad.camera.far_plane = bad.camera.near_plane;
        assert!(!valid(&bad, SESSION));
        bad = packet(1);
        bad.camera.abi_version += 1;
        assert!(!valid(&bad, SESSION));
    }

    #[test]
    fn packets_are_stored_until_their_event_takes_them() {
        let mut state = State::new();
        let mut addresses = Vec::new();
        for frame in 1..=PACKETS as u64 {
            let slot = state.admit_packet(&packet(frame), SESSION).unwrap();
            addresses.push(state.packet_address(slot).cast_const());
        }
        assert_eq!(state.admit_packet(&packet(99), SESSION), None);
        assert!(!state.drained(0));

        let (slot, taken) = state.take_packet(addresses[4]).unwrap();
        assert_eq!((slot, taken.frame_id), (4, 5));
        assert!(state.take_packet(addresses[4]).is_none());
        assert!(state.take_packet(ptr::null()).is_none());
        assert_eq!(state.admit_packet(&packet(100), SESSION), Some(4));
    }

    thread_local! {
        /// Lease leaf calls of the current test, in order. Tests run on their own threads.
        static LEASE_CALLS: std::cell::RefCell<Vec<(&'static str, usize)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    pub(super) unsafe fn retain_texture(resource: *mut c_void) {
        LEASE_CALLS.with(|calls| calls.borrow_mut().push(("retain", resource.addr())));
    }

    pub(super) unsafe fn release_texture(resource: *mut c_void) {
        LEASE_CALLS.with(|calls| calls.borrow_mut().push(("release", resource.addr())));
    }

    fn lease_calls() -> Vec<(&'static str, usize)> {
        LEASE_CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
    }

    #[test]
    fn a_leased_packet_holds_its_textures_until_its_event_ends() {
        let mut state = State::new();
        let mut first = packet(1);
        first.motion = ptr::null_mut();
        first.flags = RSF_UNITY_PACKET_PROBE;
        let slot = state.admit_leased(&first, SESSION).unwrap();
        // Retained before the address exists; a null resource holds nothing.
        assert_eq!(
            lease_calls(),
            [("retain", 0x10), ("retain", 0x20), ("retain", 0x40)]
        );
        let address = state.packet_address(slot).cast_const();

        // The render event takes it; the slot stays with its lease and cannot be taken again.
        let (taken, copy) = state.take_packet(address).unwrap();
        assert_eq!((taken, copy.frame_id), (slot, 1));
        assert!(state.take_packet(address).is_none());
        assert!(!state.drained(0));
        assert_eq!(state.admit_packet(&packet(2), SESSION), Some(slot + 1));
        assert!(lease_calls().is_empty());

        // The event ends: the slot is free and the lease is handed out for release.
        state.end_packet(slot).unwrap().end();
        assert_eq!(
            lease_calls(),
            [("release", 0x10), ("release", 0x20), ("release", 0x40)]
        );
        assert!(state.end_packet(slot).is_none());
        assert_eq!(state.admit_packet(&packet(3), SESSION), Some(slot));
    }

    #[test]
    fn unleased_packets_are_freed_when_taken_and_never_ended() {
        let mut state = State::new();
        let slot = state.admit_packet(&packet(1), SESSION).unwrap();
        let address = state.packet_address(slot).cast_const();
        assert!(state.take_packet(address).is_some());
        // A queued packet that reused the freed slot is not ended by the earlier event.
        assert_eq!(state.admit_packet(&packet(2), SESSION), Some(slot));
        assert!(state.end_packet(slot).is_none());
        assert!(state.end_packet(PACKETS).is_none());
        assert!(state.take_packet(address).is_some());
        assert!(lease_calls().is_empty());
    }

    #[test]
    fn a_refused_leased_admission_retains_nothing() {
        let mut state = State::new();
        assert!(state.admit_leased(&packet(1), SESSION + 1).is_none());
        for frame in 1..=PACKETS as u64 {
            assert!(state.admit_packet(&packet(frame), SESSION).is_some());
        }
        assert!(state.admit_leased(&packet(99), SESSION).is_none());
        assert!(lease_calls().is_empty());
    }

    #[test]
    fn release_cancels_queued_and_consuming_leases() {
        let mut state = State::new();
        let queued = state.admit_leased(&packet(1), SESSION).unwrap();
        let consuming = state.admit_leased(&packet(2), SESSION).unwrap();
        state.admit_packet(&packet(3), SESSION).unwrap();
        let address = state.packet_address(consuming).cast_const();
        assert!(state.take_packet(address).is_some());
        lease_calls();
        let leases = state.release();
        assert_eq!(leases.iter().flatten().count(), 2);
        assert!(leases[queued].is_some() && leases[consuming].is_some());
        assert!(lease_calls().is_empty());
        leases.into_iter().flatten().for_each(Lease::end);
        assert_eq!(lease_calls().len(), 8);
        assert!(state.drained(0));
        assert!(state.end_packet(consuming).is_none());
    }

    #[test]
    fn frames_wait_for_the_ring_before_changing_extents_or_policy() {
        let mut state = State::new();
        let first = state.admit_frame(&packet(1), 0).unwrap();
        assert_eq!((first.slot, first.pending_generation), (0, 0));
        assert!(state.submitted(&first, 10, &source(1)));

        // Same extents and generation: the next slot is free.
        let second = state.admit_frame(&packet(2), 0).unwrap();
        assert_eq!(second.slot, 1);
        assert!(state.submitted(&second, 11, &source(2)));

        // A size change waits for both submissions.
        let mut resized = packet(3);
        resized.camera.render_width = 960;
        assert!(state.admit_frame(&resized, 10).is_none());
        assert_eq!(state.input, (1280, 720));
        assert_eq!(state.admit_frame(&resized, 11).unwrap().slot, 2);
        assert_eq!(state.input, (960, 720));

        // A policy change advances the generation; stale packets wait for it.
        assert!(!state.set_policy(2, 1, true));
        assert!(state.set_policy(1, 3, true));
        assert_eq!(state.policy.generation, 2);
        let mut admitted = state.admit_frame(&resized, 11).unwrap();
        assert_eq!(admitted.pending_generation, 2);
        assert!(state.settle_change(admitted.pending_generation));
        assert!(state.submitted(&admitted, 12, &source(3)));
        assert!(state.admit_frame(&resized, 12).is_none());
        resized.generation = 2;
        admitted = state.admit_frame(&resized, 12).unwrap();
        assert_eq!((admitted.slot, admitted.pending_generation), (0, 0));

        // An inactive bridge records the change without a pending reconfiguration.
        assert!(state.set_policy(0, 1, false));
        assert!(!state.policy.pending);
    }

    #[test]
    fn a_settled_change_clears_only_its_own_generation() {
        let mut state = State::new();
        assert_eq!(state.pending_change(), None);
        assert!(state.set_policy(1, 2, true));
        let first = state.pending_change().unwrap();
        assert_eq!(first, 2);
        // A newer change arrives while the first is being applied.
        assert!(state.set_policy(0, 2, true));
        assert!(!state.settle_change(first));
        assert_eq!(state.pending_change(), Some(3));
        assert!(state.settle_change(3));
        assert_eq!(state.pending_change(), None);
        assert!(!state.settle_change(3));
    }

    #[test]
    fn a_change_is_idle_once_the_ring_retires_whatever_the_pre_ui_copies_do() {
        let mut state = State::new();
        let first = state.admit_frame(&packet(1), 0).unwrap();
        assert!(state.submitted(&first, 10, &source(1)));
        assert_eq!(state.idle_change(10), None, "nothing pending");
        assert!(state.set_policy(1, 1, true));
        let pending = state.pending_change();
        assert_eq!(state.idle_change(9), None, "the ring slot is in flight");
        // A pre-UI copy recorded every frame keeps a scene packet busy, not the window delivery.
        assert!(state.record_source(&source(2), 20));
        assert!(state.admit_frame(&packet(2), 10).is_none());
        assert_eq!(state.idle_change(10), pending);
        assert_eq!(state.idle_change(u64::MAX), None, "device removed");
        state.submission_unknown = true;
        assert_eq!(state.idle_change(10), None);
    }

    #[test]
    fn admission_keeps_the_generation_pending_until_delivery() {
        let mut state = State::new();
        assert!(state.set_policy(0, 1, true));
        let old = state.pending_change().unwrap();
        let mut hudless = packet(1);
        hudless.flags |= RSF_UNITY_PACKET_HUDLESS;
        assert!(state.admit_frame(&hudless, 0).is_none());
        assert_eq!(
            state.admit_frame(&packet(1), 0).unwrap().pending_generation,
            old
        );
        assert_eq!(state.pending_change(), Some(old));
        assert!(state.set_policy(1, 5, true));
        assert!(!state.settle_change(old));
        let latest = state.pending_change().unwrap();
        assert!(state.settle_change(latest));
    }

    #[test]
    fn a_busy_slot_is_not_reused() {
        let mut state = State::new();
        for (frame, complete) in [(1, 5), (2, 6), (3, 7)] {
            let admitted = state.admit_frame(&packet(frame), 0).unwrap();
            assert!(state.submitted(&admitted, complete, &source(frame)));
        }
        assert!(state.admit_frame(&packet(4), 4).is_none());
        assert_eq!(state.admit_frame(&packet(4), 5).unwrap().slot, 0);
        // Without a fence every recorded submission is in flight.
        assert!(state.admit_frame(&packet(4), 0).is_none());
        // Device removal gives no completion proof for the recorded slots.
        assert!(state.admit_frame(&packet(4), u64::MAX).is_none());
        assert!(!state.drained(u64::MAX));
    }

    #[test]
    fn a_missing_completion_blocks_retirement() {
        let mut state = State::new();
        let admitted = state.admit_frame(&packet(1), 0).unwrap();
        assert!(!state.submitted(&admitted, 0, &source(1)));
        assert_eq!(state.next, 0);
        assert!(!state.drained(u64::MAX));
        state.release();
        assert!(state.drained(0));
    }

    #[test]
    fn sources_flush_by_session_frame_and_view_in_recording_order() {
        let mut state = State::new();
        let alternate_view = Source {
            view_key: 99,
            ..source(9)
        };
        assert!(state.record_source(&source(9), 0));
        assert!(state.record_source(&alternate_view, 0));
        assert!(state.record_source(&source(1), 0));
        assert!(state.record_source(&source(9), 0));
        state.record_source(&source(3), 0);
        state.record_source(&source(12), 0);
        state.record_source(
            &Source {
                session_id: SESSION + 1,
                ..source(5)
            },
            0,
        );
        let (flushed, count) = state.take_flushed(SESSION, 10);
        let frames: Vec<u64> = flushed[..count].iter().map(|s| s.source_frame_id).collect();
        assert_eq!(frames, [9, 9, 1, 3]);
        assert_eq!(flushed[1].view_key, 99);
        let (flushed, count) = state.take_flushed(SESSION, 20);
        assert_eq!((count, flushed[0].source_frame_id), (1, 12));
        assert_eq!(state.take_flushed(SESSION, 20).1, 0);
        assert_eq!(state.take_flushed(SESSION + 1, 5).1, 1);
    }

    #[test]
    fn a_delayed_flush_refuses_new_sources_at_capacity() {
        let mut state = State::new();
        for frame in 1..=SOURCES as u64 {
            assert!(state.record_source(&source(frame), 0));
        }
        assert!(state.can_record_source(&source(1)));
        let next = SOURCES as u64 + 1;
        assert!(!state.record_source(&source(next), 0));
        assert!(state.admit_frame(&packet(next), 0).is_none());
        let (flushed, count) = state.take_flushed(SESSION, 1);
        assert_eq!((count, flushed[0].source_frame_id), (1, 1));
        assert!(state.admit_frame(&packet(next), 0).is_some());
        assert!(state.record_source(&source(next), 0));
        let (flushed, count) = state.take_flushed(SESSION, next);
        assert_eq!((count, flushed[0].source_frame_id), (SOURCES, 2));
        assert_eq!(flushed[SOURCES - 1].source_frame_id, next);
    }

    #[test]
    fn inline_copies_retire_at_their_fence() {
        let mut state = State::new();
        state.record_source(&source(1), 40);
        state.record_source(&source(2), 30);
        assert_eq!(state.inline_complete, 40);
        assert!(!state.drained(0));
        assert!(!state.drained(39));
        assert!(!state.drained(u64::MAX));
        assert!(state.drained(40));
    }

    #[test]
    fn release_keeps_the_policy() {
        let mut state = State::new();
        assert!(state.set_policy(1, 4, true));
        let admitted = state.admit_frame(&packet(1), 0).unwrap();
        // Delivery settles the change the admission carried.
        assert!(state.settle_change(admitted.pending_generation));
        assert!(state.submitted(&admitted, 3, &source(1)));
        state.release();
        assert_eq!(
            state.policy,
            Policy {
                engine_spatial: 1,
                quality: 4,
                generation: 2,
                pending: false,
            }
        );
        assert_eq!(
            (state.next, state.complete, state.input),
            (0, [0; SLOTS], (0, 0))
        );
    }

    #[test]
    fn config_reports_the_policy_and_gates_enablement() {
        let mut state = State::new();
        let host = rsf_game_render_config {
            struct_size: size_of::<rsf_game_render_config>() as u32,
            enabled: 1,
            output_width: 2560,
            output_height: 1440,
            render_width: 1280,
            render_height: 720,
            ..rsf_game_render_config::request()
        };
        let config = state.config(true, &host, 2560, 1440);
        assert_eq!(config.enabled, 1);
        assert_eq!((config.quality, config.generation), (1, 1));
        assert_eq!((config.render_width, config.output_width), (1280, 2560));
        assert_eq!(state.config(false, &host, 2560, 1440).enabled, 0);
        assert_eq!(state.config(true, &host, 1920, 1080).enabled, 0);
        assert!(state.set_policy(0, 2, true));
        let config = state.config(true, &host, 2560, 1440);
        assert_eq!(
            (config.enabled, config.quality, config.generation),
            (0, 2, 2)
        );
    }
}
