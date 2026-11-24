// SPDX-License-Identifier: MIT
// Dedicated DLSS-G present capture owner, formerly native_fg_dlss_vulkan_capture.cpp. It decides
// admission, owns the per-queue-family command pools and fences, orders submission behind the
// original Present waits and retires the readbacks. Frame capture and file output are the leaves
// in leaves.rs, which the DLSS-G fixture replaces.
use crate::allocate_handle;
use crate::dlss_vulkan_capture_ledger::{AMBIGUOUS, Ledger, Source};
use crate::frame_capture_vk::rsf_core_capture_vk_complete_group;
use crate::sys::rsf_srvk_image as Image;
use crate::sys::{rsf_dlssgvk_status as SdkStatus, rsf_fgvk_setup as Setup};
use crate::vulkan_capture_commands::Commands;
use ash::vk::{self, Handle as _};
use leaves::{Native, native};
use rsf_game_sys::backend_result::{INIT_FAILED, NOT_READY, OK};
use rsf_game_sys::rsf_frame_record as Record;
use rsf_streamline_vk::sys::{
    RSF_SL_PRESENT_CAPTURE_ABI_VERSION, rsf_sl_present_capture_packet as Packet,
    rsf_sl_present_capture_registration as Registration,
};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

const VK_SUCCESS: i32 = vk::Result::SUCCESS.as_raw();
const VK_INITIALIZATION_FAILED: i32 = vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
const VK_FORMAT_NOT_SUPPORTED: i32 = vk::Result::ERROR_FORMAT_NOT_SUPPORTED.as_raw();
const WAIT_NS: u64 = 15_000_000_000;

type Handle = *mut c_void;
type Callback = unsafe extern "C" fn(Handle, *const Packet) -> i32;
type WsiCallback = unsafe extern "C" fn(*mut c_void) -> i32;
type WsiCall = unsafe extern "C" fn(WsiCallback, *mut c_void) -> i32;

/// The capture's frame-capture and file leaves: `leaves::LEAVES` in production; the DLSS-G
/// fixture (vulkan_dlss_fg/fixture/sdk.rs) replaces them.
#[derive(Clone, Copy)]
pub(crate) struct Platform {
    // Writes a null native handle and succeeds when capture is disabled.
    pub(crate) open: unsafe extern "C" fn(*const Setup, *mut Handle) -> i32,
    pub(crate) wants: unsafe extern "C" fn(Handle, u64) -> u32,
    pub(crate) readbacks: unsafe extern "C" fn(Handle) -> u64,
    pub(crate) record:
        unsafe extern "C" fn(Handle, Handle, *const Image, *const Record, *const c_char) -> u32,
    pub(crate) flush: unsafe extern "C" fn(Handle) -> u32,
    pub(crate) failure: unsafe extern "C" fn(Handle, u64, *const c_char, i32),
    pub(crate) append: unsafe extern "C" fn(Handle, *const c_char, *const u8, usize),
}

// The image query and the native Present bypass the SDK proxy under its LL2 WSI lease.
fn wsi_call(packet: &Packet, call: &mut dyn FnMut() -> i32) -> i32 {
    unsafe extern "C" fn trampoline(user: *mut c_void) -> i32 {
        let call = unsafe { &mut *user.cast::<&mut dyn FnMut() -> i32>() };
        call()
    }
    let lease: WsiCall = unsafe { std::mem::transmute(packet.native_wsi_call) };
    let mut call = call;
    unsafe { lease(trampoline, (&raw mut call).cast()) }
}

fn present_info(packet: &Packet) -> &vk::PresentInfoKHR<'_> {
    unsafe { &*packet.present_info.cast() }
}

/// The original Present, with its waits or, after the copy fence consumed them, without.
fn present_original(packet: &Packet, keep_waits: bool) -> i32 {
    let original = present_info(packet);
    let completed = vk::PresentInfoKHR {
        wait_semaphore_count: 0,
        p_wait_semaphores: ptr::null(),
        ..*original
    };
    let info = if keep_waits { original } else { &completed };
    let next: vk::PFN_vkQueuePresentKHR = unsafe { std::mem::transmute(packet.native_present) };
    let queue = vk::Queue::from_raw(packet.queue as u64);
    wsi_call(packet, &mut || unsafe { next(queue, info) }.as_raw())
}

/// The swapchain image this Present selects, read through the real image query.
fn present_image(packet: &Packet) -> Result<(Image, vk::SwapchainKHR, u32), (&'static CStr, i32)> {
    let info = present_info(packet);
    let chain = unsafe {
        &*packet
            .swapchain_info
            .cast::<vk::SwapchainCreateInfoKHR<'_>>()
    };
    if !chain
        .image_usage
        .contains(vk::ImageUsageFlags::TRANSFER_SRC)
        || info.swapchain_count != 1
        || info.p_swapchains.is_null()
        || info.p_image_indices.is_null()
        || chain.image_array_layers != 1
        || !matches!(
            chain.image_format,
            vk::Format::B8G8R8A8_UNORM | vk::Format::R8G8B8A8_UNORM
        )
    {
        return Err((c"real-chain-copy-contract", VK_FORMAT_NOT_SUPPORTED));
    }
    let (swapchain, index) = unsafe { (*info.p_swapchains, *info.p_image_indices) };
    let next: vk::PFN_vkGetSwapchainImagesKHR =
        unsafe { std::mem::transmute(packet.native_images) };
    let device = vk::Device::from_raw(packet.device as u64);
    let images = |count: &mut u32, out: *mut vk::Image| {
        wsi_call(packet, &mut || {
            unsafe { next(device, swapchain, count, out) }.as_raw()
        })
    };
    let mut count = 0;
    let result = images(&mut count, ptr::null_mut());
    if result != VK_SUCCESS || index >= count {
        return Err((c"real-images-count", result));
    }
    let mut actual = Vec::new();
    actual
        .try_reserve_exact(count as usize)
        .map_err(|_| (c"real-images", VK_INITIALIZATION_FAILED))?;
    actual.resize(count as usize, vk::Image::null());
    let result = images(&mut count, actual.as_mut_ptr());
    if result != VK_SUCCESS {
        return Err((c"real-images", result));
    }
    if count as usize > actual.len() || index >= count || actual[index as usize].is_null() {
        return Err((c"real-images", VK_INITIALIZATION_FAILED));
    }
    let image = Image {
        device: packet.device,
        image: actual[index as usize].as_raw(),
        format: chain.image_format.as_raw() as u32,
        width: chain.image_extent.width,
        height: chain.image_extent.height,
        layout: vk::ImageLayout::PRESENT_SRC_KHR.as_raw() as u32,
        copy_source: 1,
    };
    Ok((image, swapchain, index))
}

// The command buffer is freed with its pool.
#[derive(Default)]
struct Family {
    pool: vk::CommandPool,
    commands: vk::CommandBuffer,
    fence: vk::Fence,
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
        let commands = native(self.native).commands.as_ref();
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        if let Some(commands) = commands {
            for family in state.families.values() {
                if !family.fence.is_null() {
                    commands.destroy_fence(family.fence);
                }
                if !family.pool.is_null() {
                    commands.destroy_pool(family.pool);
                }
            }
        }
        drop(unsafe { Box::from_raw(self.native.cast::<Native>()) });
    }
}

impl Owner {
    fn commands(&self) -> Option<&Commands> {
        native(self.native).commands.as_ref()
    }

    fn register(&self, callback: Option<Callback>) -> i32 {
        let native = native(self.native);
        let registration = Registration {
            struct_size: size_of::<Registration>() as u32,
            abi_version: RSF_SL_PRESENT_CAPTURE_ABI_VERSION,
            device: native.setup.graphics.device,
            callback,
            user: ptr::from_ref(self).cast_mut().cast(),
        };
        unsafe { (native.register)(&registration) }
    }

    fn complete_readback(&self, commands: vk::CommandBuffer) {
        let capture = native(self.native).capture;
        unsafe { rsf_core_capture_vk_complete_group(capture, commands.as_raw()) };
    }

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
    fn family(&self, state: &mut State, family: u32) -> Option<(vk::CommandBuffer, vk::Fence)> {
        let commands = self.commands()?;
        let work = state.families.entry(family).or_default();
        if state.ledger.pending(family) {
            if commands.wait_fence(work.fence, WAIT_NS) != vk::Result::SUCCESS {
                return None;
            }
            state.ledger.completed(family);
            self.complete_readback(work.commands);
        }
        if work.pool.is_null() {
            work.pool = commands
                .create_pool(family, vk::CommandPoolCreateFlags::TRANSIENT)
                .ok()?;
        }
        if work.commands.is_null() {
            work.commands = commands.allocate(work.pool).ok()?;
        }
        if work.fence.is_null() {
            work.fence = commands.create_fence().ok()?;
        }
        if commands.reset_pool(work.pool) != vk::Result::SUCCESS
            || commands.reset_fence(work.fence) != vk::Result::SUCCESS
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
            return present_original(packet, true);
        };
        let frame = packet.source_frame;
        let (image, swapchain, index) = match present_image(packet) {
            Ok(image) => image,
            Err((reason, result)) => {
                self.failure(frame, reason, result);
                return present_original(packet, true);
            }
        };
        let Some((buffer, fence)) = self.family(state, packet.queue_family) else {
            self.failure(frame, c"capture-commands", VK_INITIALIZATION_FAILED);
            return present_original(packet, true);
        };
        let commands = self.commands().expect("a family has loaded commands");
        let result = commands.begin(buffer).as_raw();
        if result != VK_SUCCESS {
            self.failure(frame, c"begin", result);
            return present_original(packet, true);
        }
        let before = self.readbacks();
        let generated = packet.generated_index >= 0;
        let generated_role = generated.then(|| {
            CString::new(format!("fg-generated-present-{}", packet.generated_index))
                .expect("a decimal generated index has no NUL")
        });
        let role = generated_role.as_deref().unwrap_or(c"fg-real-present");
        let raw_buffer = buffer.as_raw() as Handle;
        let recorded = unsafe {
            (self.ops.record)(
                self.native,
                raw_buffer,
                &image,
                &source.record,
                role.as_ptr(),
            )
        } != 0;
        let result = commands.end(buffer).as_raw();
        if !recorded || result != VK_SUCCESS {
            if self.readbacks() > before {
                state.ledger.unsubmitted();
            }
            self.failure(frame, c"record", result);
            return present_original(packet, true);
        }
        // Submit the copy behind the original Present wait semaphores. A failed submission
        // may already have consumed the binary waits; return it without a second consumption.
        let info = present_info(packet);
        let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; info.wait_semaphore_count as usize];
        let submission = vk::SubmitInfo {
            wait_semaphore_count: info.wait_semaphore_count,
            p_wait_semaphores: info.p_wait_semaphores,
            p_wait_dst_stage_mask: stages.as_ptr(),
            command_buffer_count: 1,
            p_command_buffers: &buffer,
            ..Default::default()
        };
        let submit: vk::PFN_vkQueueSubmit = unsafe { std::mem::transmute(packet.native_submit) };
        let queue = vk::Queue::from_raw(packet.queue as u64);
        let result = unsafe { submit(queue, 1, &submission, fence) }.as_raw();
        if result != VK_SUCCESS {
            if self.readbacks() > before {
                state.ledger.unsubmitted();
            }
            self.failure(frame, c"submit", result);
            return result;
        }
        state.ledger.submitted(packet.queue_family);
        let result = commands.wait_fence(fence, WAIT_NS).as_raw();
        if result != VK_SUCCESS {
            self.failure(frame, c"wait", result);
            return if result > 0 {
                VK_INITIALIZATION_FAILED
            } else {
                result
            };
        }
        state.ledger.completed(packet.queue_family);
        self.complete_readback(buffer);
        // Every earlier submitted copy must also have a successful own-fence
        // wait, including other queue families after a previous timeout. No
        // global idle or SDK lock is acquired here. Legacy readbacks have no
        // core completion ticket, so a ready-only flush would leave their
        // pixels unwritten.
        let ready = state.ledger.can_flush();
        let flushed = ready && unsafe { (self.ops.flush)(self.native) } != 0;
        if ready && !flushed {
            self.failure(frame, c"completed-readback-flush", VK_INITIALIZATION_FAILED);
        }
        // The copy fence completed the original binary waits, so the native
        // Present needs no replacement semaphore or image-index assumption.
        let result = present_original(packet, false);
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
                index,
                swapchain.as_raw(),
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
            if self.register(None) != VK_SUCCESS {
                return NOT_READY;
            }
            self.registered.store(false, Ordering::Release);
        }
        OK
    }
}

unsafe extern "C" fn present(user: Handle, packet: *const Packet) -> i32 {
    if user.is_null() || !unsafe { rsf_game_sys::ffi_util::has_capacity(packet) } {
        return VK_INITIALIZATION_FAILED;
    }
    let packet = unsafe { &*packet };
    if packet.abi_version != RSF_SL_PRESENT_CAPTURE_ABI_VERSION
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
pub(crate) unsafe fn rsf_core_dlss_capture_open(
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
    if unsafe { &*owner }.register(Some(present)) != VK_SUCCESS {
        drop(unsafe { Box::from_raw(owner) });
        return NOT_READY;
    }
    unsafe {
        (*owner).registered.store(true, Ordering::Release);
        out.write(owner);
    }
    OK
}

pub(crate) unsafe fn rsf_core_dlss_capture_remember(
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
pub(crate) unsafe fn rsf_core_dlss_capture_flush(owner: *const Owner) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        owner.flush();
    }
}

// Detach keeps the owner alive until close.
pub(crate) unsafe fn rsf_core_dlss_capture_detach(owner: *const Owner) -> i32 {
    unsafe { owner.as_ref() }.map_or(OK, Owner::detach)
}

pub(crate) unsafe fn rsf_core_dlss_capture_close(owner: *mut Owner) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[path = "dlss_vulkan_capture/leaves.rs"]
pub(crate) mod leaves;
