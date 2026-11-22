// SPDX-License-Identifier: MIT
// Dedicated DLSS-G present capture owner. Rust decides admission, owns the
// per-queue-family command pools and fences, orders submission and retires
// the readbacks. The production leaves for typed Vulkan, WSI, Streamline
// registration and file output are in leaves.rs.
use crate::allocate_handle;
use crate::dlss_vulkan_capture_ledger::{AMBIGUOUS, Ledger};
use crate::sys::rsf_srvk_image as Image;
use crate::sys::{
    rsf_dlss_capture_source as Source, rsf_dlssgvk_status as SdkStatus, rsf_fgvk_setup as Setup,
};
use rsf_game_sys::rsf_frame_record as Record;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

const OK: i32 = 0;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const VK_SUCCESS: i32 = 0;
const VK_INITIALIZATION_FAILED: i32 = -3;
const VK_FORMAT_NOT_SUPPORTED: i32 = -11;
const TRANSFER_SOURCE: u32 = 1;
const RGBA8: u32 = 37;
const BGRA8: u32 = 44;
const PRESENT_LAYOUT: u32 = 1_000_001_002;
const WAIT_NS: u64 = 15_000_000_000;
const PACKET_ABI_VERSION: u32 = 1;

type Handle = *mut c_void;
type Callback = unsafe extern "C" fn(Handle, *const Packet) -> i32;

// rsf_sl_present_capture_packet from runtime/vulkan/streamline/present-capture.h.
#[repr(C)]
pub(crate) struct Packet {
    struct_size: u32,
    abi_version: u32,
    device: Handle,
    queue: Handle,
    present_info: *const c_void,
    swapchain_info: *const c_void,
    native_present: usize,
    source_frame: u64,
    present_frame: u32,
    generated_index: i32,
    boundary_valid: u32,
    queue_family: u32,
    native_images: usize,
    native_submit: usize,
    native_wsi_call: usize,
}

const _: () = {
    assert!(size_of::<Packet>() == 96);
    assert!(std::mem::offset_of!(Packet, source_frame) == 48);
    assert!(std::mem::offset_of!(Packet, queue_family) == 68);
    assert!(std::mem::offset_of!(Packet, native_wsi_call) == 88);
};

#[repr(C)]
#[derive(Default)]
struct Target {
    chain: u64,
    index: u32,
    selected: u32,
    usage: u32,
    layers: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
pub(crate) struct Platform {
    // Writes a null native handle and succeeds when capture is disabled.
    open: unsafe extern "C" fn(*const Setup, *mut Handle) -> i32,
    close: unsafe extern "C" fn(Handle),
    // A null callback clears the registration for the same device and user.
    register: unsafe extern "C" fn(Handle, Option<Callback>, Handle) -> i32,
    create_pool: unsafe extern "C" fn(Handle, u32, *mut u64) -> i32,
    destroy_pool: unsafe extern "C" fn(Handle, u64),
    allocate: unsafe extern "C" fn(Handle, u64, *mut Handle) -> i32,
    reset_pool: unsafe extern "C" fn(Handle, u64) -> i32,
    begin: unsafe extern "C" fn(Handle, Handle) -> i32,
    end: unsafe extern "C" fn(Handle, Handle) -> i32,
    create_fence: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    destroy_fence: unsafe extern "C" fn(Handle, u64),
    reset_fence: unsafe extern "C" fn(Handle, u64) -> i32,
    wait_fence: unsafe extern "C" fn(Handle, u64, u64) -> i32,
    complete_readback: unsafe extern "C" fn(Handle, Handle),
    wants: unsafe extern "C" fn(Handle, u64) -> u32,
    readbacks: unsafe extern "C" fn(Handle) -> u64,
    target: unsafe extern "C" fn(*const Packet, *mut Target),
    images: unsafe extern "C" fn(*const Packet, u64, *mut u32, *mut u64) -> i32,
    record: unsafe extern "C" fn(Handle, Handle, *const Image, *const Record, *const c_char) -> u32,
    // Submits the copy behind the original Present wait semaphores.
    submit: unsafe extern "C" fn(*const Packet, Handle, u64) -> i32,
    // Zero waits presents after the copy fence consumed the original waits.
    present: unsafe extern "C" fn(*const Packet, u32) -> i32,
    flush: unsafe extern "C" fn(Handle) -> u32,
    failure: unsafe extern "C" fn(Handle, u64, *const c_char, i32),
    append: unsafe extern "C" fn(Handle, *const c_char, *const u8, usize),
}

const _: () = {
    assert!(size_of::<Target>() == 40 && align_of::<Target>() == 8);
    assert!(std::mem::offset_of!(Target, index) == 8);
    assert!(std::mem::offset_of!(Target, height) == 32);
    assert!(size_of::<Platform>() == 24 * size_of::<Handle>());
};

impl Platform {
    fn present_image(&self, packet: &Packet) -> Result<(Image, Target), (&'static CStr, i32)> {
        let mut target = Target::default();
        unsafe { (self.target)(packet, &mut target) };
        if target.usage & TRANSFER_SOURCE == 0
            || target.selected == 0
            || target.layers != 1
            || !matches!(target.format, BGRA8 | RGBA8)
        {
            return Err((c"real-chain-copy-contract", VK_FORMAT_NOT_SUPPORTED));
        }
        let mut count = 0;
        let result = unsafe { (self.images)(packet, target.chain, &mut count, ptr::null_mut()) };
        if result != VK_SUCCESS || target.index >= count {
            return Err((c"real-images-count", result));
        }
        let mut images = Vec::new();
        images
            .try_reserve_exact(count as usize)
            .map_err(|_| (c"real-images", VK_INITIALIZATION_FAILED))?;
        images.resize(count as usize, 0);
        let result =
            unsafe { (self.images)(packet, target.chain, &mut count, images.as_mut_ptr()) };
        if result != VK_SUCCESS {
            return Err((c"real-images", result));
        }
        if count as usize > images.len() || target.index >= count {
            return Err((c"real-images", VK_INITIALIZATION_FAILED));
        }
        let image = images[target.index as usize];
        if image == 0 {
            return Err((c"real-images", VK_INITIALIZATION_FAILED));
        }
        let image = Image {
            device: packet.device,
            image,
            format: target.format,
            width: target.width,
            height: target.height,
            layout: PRESENT_LAYOUT,
            copy_source: 1,
        };
        Ok((image, target))
    }
}

// The command buffer is freed with its pool.
struct Family {
    pool: u64,
    commands: Handle,
    fence: u64,
}

impl Default for Family {
    fn default() -> Self {
        Self {
            pool: 0,
            commands: ptr::null_mut(),
            fence: 0,
        }
    }
}

struct State {
    ledger: Ledger,
    families: BTreeMap<u32, Family>,
}

pub(crate) struct Owner {
    ops: &'static Platform,
    native: Handle,
    // Changed only by open and detach, outside the presenter callback.
    registered: AtomicBool,
    state: Mutex<State>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        // The FG owner drains the presenter and device and detaches first.
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        for family in state.families.values() {
            unsafe {
                if family.fence != 0 {
                    (self.ops.destroy_fence)(self.native, family.fence);
                }
                if family.pool != 0 {
                    (self.ops.destroy_pool)(self.native, family.pool);
                }
            }
        }
        unsafe { (self.ops.close)(self.native) };
    }
}

impl Owner {
    fn append(&self, file: &CStr, line: &str) {
        unsafe { (self.ops.append)(self.native, file.as_ptr(), line.as_ptr(), line.len()) };
    }

    fn trace(&self, packet: &Packet, observed: &Source, found: bool) {
        let sdk = &observed.sdk;
        self.append(
            c"dlss-present-boundaries.jsonl",
            &format!(
                concat!(
                    "{{\"capture_anchor_frame\":{},\"capture_anchor_session\":{},",
                    "\"sdk_source_frame\":{},\"sdk_present_frame\":{},\"sdk_generated_index\":{},",
                    "\"boundary_valid\":{},\"source_found\":{},\"configured_on\":{},",
                    "\"requested_generated_count\":{},\"sdk_status\":{},\"sdk_result\":{},",
                    "\"sdk_actual_presents\":{},\"max_generated\":{},\"min_dimension\":{},",
                    "\"vsync_supported\":{},\"dynamic_mfg_supported\":{},",
                    "\"state_sample\":\"previous_sdk_query\"}}\n",
                ),
                observed.record.frame_id,
                observed.record.session_id,
                packet.source_frame,
                packet.present_frame,
                packet.generated_index,
                packet.boundary_valid != 0,
                found,
                observed.configured != 0,
                observed.generated_count,
                sdk.sdk_status,
                sdk.sdk_result,
                sdk.presentation.last_present_count,
                sdk.max_generated_frames,
                sdk.min_dimension,
                sdk.vsync_supported,
                sdk.dynamic_mfg_supported,
            ),
        );
    }

    fn failure(&self, source: u64, reason: &CStr, result: i32) {
        unsafe { (self.ops.failure)(self.native, source, reason.as_ptr(), result) };
    }

    fn wants(&self, frame: u64) -> bool {
        unsafe { (self.ops.wants)(self.native, frame) != 0 }
    }

    fn readbacks(&self) -> u64 {
        unsafe { (self.ops.readbacks)(self.native) }
    }

    // A family's single command buffer is reused only after its own fence
    // proves the previous copy finished.
    fn commands(&self, state: &mut State, family: u32) -> Option<(Handle, u64)> {
        let (ops, native) = (self.ops, self.native);
        let work = state.families.entry(family).or_default();
        if state.ledger.pending(family) {
            if unsafe { (ops.wait_fence)(native, work.fence, WAIT_NS) } != VK_SUCCESS {
                return None;
            }
            state.ledger.completed(family);
            unsafe { (ops.complete_readback)(native, work.commands) };
        }
        let created = (work.pool != 0
            || unsafe { (ops.create_pool)(native, family, &mut work.pool) } == VK_SUCCESS)
            && (!work.commands.is_null()
                || unsafe { (ops.allocate)(native, work.pool, &mut work.commands) } == VK_SUCCESS)
            && (work.fence != 0
                || unsafe { (ops.create_fence)(native, &mut work.fence) } == VK_SUCCESS);
        if !created
            || unsafe { (ops.reset_pool)(native, work.pool) } != VK_SUCCESS
            || unsafe { (ops.reset_fence)(native, work.fence) } != VK_SUCCESS
        {
            return None;
        }
        Some((work.commands, work.fence))
    }

    fn remember(&self, record: &Record, generated_count: u32, sdk: &SdkStatus, configured: bool) {
        let wanted = self.wants(record.frame_id);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let source = Source {
            record: *record,
            sdk: *sdk,
            generated_count,
            configured: u32::from(configured),
        };
        let result = state.ledger.remember(wanted.then_some(&source));
        if result != 0 {
            let reason = if result == AMBIGUOUS {
                c"ambiguous-source-identity"
            } else {
                c"source-outside-sdk-token-range"
            };
            self.failure(record.frame_id, reason, VK_INITIALIZATION_FAILED);
        }
    }

    // Replaces exactly one real SDK WSI Present. The common callback is
    // synchronous and already owns its SDK queue. Never acquire a session,
    // PCL or host-queue lock from this presenter thread.
    fn present(&self, packet: &Packet) -> i32 {
        let ops = self.ops;
        let original = |waits: u32| unsafe { (ops.present)(packet, waits) };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let state = &mut *state;
        let source = state.ledger.source(packet.source_frame).copied();
        // Preserve raw producer facts even when the SDK boundary does not map
        // to a retained source. Otherwise a mapping bug can hide generated
        // presents and make a real-only capture look like disabled generation.
        if let Some(observed) = state.ledger.observed()
            && self.wants(observed.record.frame_id)
        {
            self.trace(packet, observed, source.is_some());
        }
        let Some(source) = source.filter(|source| {
            packet.boundary_valid != 0
                && self.wants(source.record.frame_id)
                && state.ledger.can_record()
        }) else {
            return original(1);
        };
        let frame = packet.source_frame;
        let (image, target) = match ops.present_image(packet) {
            Ok(image) => image,
            Err((reason, result)) => {
                self.failure(frame, reason, result);
                return original(1);
            }
        };
        let Some((commands, fence)) = self.commands(state, packet.queue_family) else {
            self.failure(frame, c"capture-commands", VK_INITIALIZATION_FAILED);
            return original(1);
        };
        let result = unsafe { (ops.begin)(self.native, commands) };
        if result != VK_SUCCESS {
            self.failure(frame, c"begin", result);
            return original(1);
        }
        let before = self.readbacks();
        let generated = packet.generated_index >= 0;
        let generated_role = generated.then(|| {
            CString::new(format!("fg-generated-present-{}", packet.generated_index))
                .expect("a decimal generated index has no NUL")
        });
        let role = generated_role.as_deref().unwrap_or(c"fg-real-present");
        let recorded =
            unsafe { (ops.record)(self.native, commands, &image, &source.record, role.as_ptr()) }
                != 0;
        let result = unsafe { (ops.end)(self.native, commands) };
        if !recorded || result != VK_SUCCESS {
            if self.readbacks() > before {
                state.ledger.unsubmitted();
            }
            self.failure(frame, c"record", result);
            return original(1);
        }
        // A failed submission may already have consumed the binary waits.
        // Return it without a second consumption.
        let result = unsafe { (ops.submit)(packet, commands, fence) };
        if result != VK_SUCCESS {
            if self.readbacks() > before {
                state.ledger.unsubmitted();
            }
            self.failure(frame, c"submit", result);
            return result;
        }
        state.ledger.submitted(packet.queue_family);
        let result = unsafe { (ops.wait_fence)(self.native, fence, WAIT_NS) };
        if result != VK_SUCCESS {
            self.failure(frame, c"wait", result);
            return if result > 0 {
                VK_INITIALIZATION_FAILED
            } else {
                result
            };
        }
        state.ledger.completed(packet.queue_family);
        unsafe { (ops.complete_readback)(self.native, commands) };
        // Every earlier submitted copy must also have a successful own-fence
        // wait, including other queue families after a previous timeout. No
        // global idle or SDK lock is acquired here. Legacy readbacks have no
        // core completion ticket, so a ready-only flush would leave their
        // pixels unwritten.
        let ready = state.ledger.can_flush();
        let flushed = ready && unsafe { (ops.flush)(self.native) } != 0;
        if ready && !flushed {
            self.failure(frame, c"completed-readback-flush", VK_INITIALIZATION_FAILED);
        }
        // The copy fence completed the original binary waits, so the native
        // Present needs no replacement semaphore or image-index assumption.
        let result = original(0);
        let sequence = state.ledger.next_sequence();
        self.append(
            c"dlss-present-capture.jsonl",
            &format!(
                concat!(
                    "{{\"sequence\":{},\"session\":{},\"frame\":{},\"view\":{},",
                    "\"resource_generation\":{},\"sdk_source_frame\":{},\"sdk_present_frame\":{},",
                    "\"sdk_generated_index\":{},\"requested_generated_count\":{},\"generated\":{},",
                    "\"real_image_index\":{},\"real_swapchain\":{},\"present_result\":{},",
                    "\"legacy_flush_attempted\":{},\"legacy_flush_succeeded\":{},",
                    "\"readback_completion_proof\":{},\"role\":\"{}\",\"fraction_established\":false}}\n",
                ),
                sequence,
                source.record.session_id,
                source.record.frame_id,
                source.record.view_id,
                source.record.resource_generation,
                packet.source_frame,
                packet.present_frame,
                packet.generated_index,
                source.generated_count,
                generated,
                target.index,
                target.chain,
                result,
                ready,
                flushed,
                if ready { "\"dedicated-capture-fences\"" } else { "null" },
                role.to_string_lossy(),
            ),
        );
        result
    }

    fn flush(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.ledger.drained();
        if state.ledger.can_flush() {
            unsafe { (self.ops.flush)(self.native) };
        }
    }

    fn detach(&self) -> i32 {
        if self.registered.load(Ordering::Acquire) {
            let user = ptr::from_ref(self).cast_mut().cast();
            if unsafe { (self.ops.register)(self.native, None, user) } != VK_SUCCESS {
                return NOT_READY;
            }
            self.registered.store(false, Ordering::Release);
        }
        OK
    }
}

unsafe extern "C" fn present(user: Handle, packet: *const Packet) -> i32 {
    if user.is_null() || packet.is_null() {
        return VK_INITIALIZATION_FAILED;
    }
    let size = unsafe { ptr::addr_of!((*packet).struct_size).read() };
    if (size as usize) < size_of::<Packet>() {
        return VK_INITIALIZATION_FAILED;
    }
    let packet = unsafe { &*packet };
    if packet.abi_version != PACKET_ABI_VERSION
        || packet.device.is_null()
        || packet.queue.is_null()
        || packet.present_info.is_null()
        || packet.swapchain_info.is_null()
        || packet.native_present == 0
        || packet.native_images == 0
        || packet.native_submit == 0
        || packet.native_wsi_call == 0
    {
        return VK_INITIALIZATION_FAILED;
    }
    unsafe { &*user.cast::<Owner>() }.present(packet)
}

// Publishes no owner while capture is disabled. Registration is the last
// step, so a refused setup never leaves a callback behind.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dlss_capture_open(
    ops: &'static Platform,
    setup: *const Setup,
    out: *mut *mut Owner,
) -> i32 {
    if out.is_null() || setup.is_null() {
        return INIT_FAILED;
    }
    unsafe { out.write(ptr::null_mut()) };
    let mut native = ptr::null_mut();
    let result = unsafe { (ops.open)(setup, &mut native) };
    if result != OK || native.is_null() {
        return result;
    }
    let owner = allocate_handle(Owner {
        ops,
        native,
        registered: AtomicBool::new(false),
        state: Mutex::new(State {
            ledger: Ledger::default(),
            families: BTreeMap::new(),
        }),
    });
    if owner.is_null() {
        return INIT_FAILED;
    }
    if unsafe { (ops.register)(native, Some(present), owner.cast()) } != VK_SUCCESS {
        drop(unsafe { Box::from_raw(owner) });
        return NOT_READY;
    }
    unsafe {
        (*owner).registered.store(true, Ordering::Release);
        out.write(owner);
    }
    OK
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dlss_capture_remember(
    owner: *const Owner,
    record: *const Record,
    generated_count: u32,
    sdk: *const SdkStatus,
    configured: u32,
) {
    let (Some(owner), Some(record), Some(sdk)) =
        (unsafe { (owner.as_ref(), record.as_ref(), sdk.as_ref()) })
    else {
        return;
    };
    owner.remember(record, generated_count, sdk, configured != 0);
}

// Presenter and GPU must already be drained.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dlss_capture_flush(owner: *const Owner) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        owner.flush();
    }
}

// Detach keeps the owner alive until close.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dlss_capture_detach(owner: *const Owner) -> i32 {
    unsafe { owner.as_ref() }.map_or(OK, Owner::detach)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_dlss_capture_close(owner: *mut Owner) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[path = "dlss_vulkan_capture/leaves.rs"]
pub(crate) mod leaves;

#[cfg(test)]
#[path = "dlss_vulkan_capture/owner_tests.rs"]
mod owner_tests;
