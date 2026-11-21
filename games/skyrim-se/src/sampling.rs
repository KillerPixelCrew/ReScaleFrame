// SPDX-License-Identifier: MIT
//! Skyrim's native PS sampler selection and restoration around the cached-state flush.

use crate::platform::{self, Platform};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

const FLUSH: usize = 0x101_0de0;
const FLUSH_BYTES: [u8; 18] = [
    0x4c, 0x8b, 0xdc, 0x41, 0x56, 0x48, 0x81, 0xec, 0xa0, 0, 0, 0, 0x8b, 0x15, 0x0e, 0xee, 0x0b,
    0x01,
];
const SURFACE_WIDTH: usize = 0x333_63c4;
const COLOUR_TARGET: usize = 0x20c_fc18;
const DEPTH_TARGET: usize = 0x20c_fc38;
const RASTER_RATIO: usize = 0x333_64b4;
const SHADOW_VIEW: usize = 0x333_64c8;
const ADDRESS_MODES: usize = 0x20c_fcbc;
const SAMPLER_TABLE: usize = 0x333_1990;
const IMMEDIATE_CONTEXT: usize = 0x333_1f30;
const SLOTS: usize = 16;
const ADDRESSES: usize = 6;
const FILTERS: usize = 5;

#[repr(C)]
struct Bias {
    struct_size: u32,
    abi_version: u32,
    session_id: u64,
    samplers: [*mut c_void; SLOTS],
    slot_mask: u32,
    bias: f32,
    applied_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Leaves {
    on_renderer_thread: unsafe extern "C" fn() -> bool,
    extent: unsafe extern "C" fn(u32, u32, *mut u32, *mut u32) -> bool,
    bind: unsafe extern "C" fn(usize, *mut c_void, *mut c_void, *mut Bias) -> i32,
    report: unsafe extern "C" fn(&rsf_game_host_services, f32, u32, u32, u32, u32, u32),
}

const _: () = {
    assert!(size_of::<Bias>() == 160);
    assert!(std::mem::offset_of!(Bias, slot_mask) == 144);
    assert!(std::mem::offset_of!(Bias, applied_mask) == 152);
    assert!(size_of::<Leaves>() == 32);
};

#[derive(Clone, Copy)]
struct Binding {
    platform: &'static Platform,
    base: usize,
    service: usize,
    host: rsf_game_host_services,
    leaves: Leaves,
}

// The session keeps the Runtime and its host services alive while the hook or restore is busy.
// Every engine read and graphics call uses the renderer thread.
unsafe impl Send for Binding {}

static BINDING: Mutex<Option<Binding>> = Mutex::new(None);
static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static LIVE: AtomicBool = AtomicBool::new(false);
static PENDING: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU32 = AtomicU32::new(0);
static REPORTS: AtomicU32 = AtomicU32::new(0);

fn read_word(platform: &Platform, address: usize) -> Option<u32> {
    let mut bytes = [0; 4];
    platform
        .read(address, &mut bytes)
        .then(|| u32::from_le_bytes(bytes))
}

impl Binding {
    fn scene_extent(&self, width: u32, height: u32) -> Option<(u32, u32)> {
        let mut rw = 0;
        let mut rh = 0;
        // Primary colour1/depth0 select the scene raster. Postprocess and UI change colour
        // targets and shadow views change depth. The live ratios must also show reduction.
        (unsafe { (self.leaves.extent)(width, height, &mut rw, &mut rh) }
            && rw < width
            && read_word(self.platform, self.base + COLOUR_TARGET) == Some(1)
            && read_word(self.platform, self.base + DEPTH_TARGET) == Some(0)
            && read_word(self.platform, self.base + SHADOW_VIEW) == Some(0)
            && read_word(self.platform, self.base + RASTER_RATIO)
                .is_some_and(|ratio| f32::from_bits(ratio) < 1.0))
        .then_some((rw, rh))
    }

    fn sampler_request(&self) -> Option<Bias> {
        let mut modes = [[0; 4]; 2 * SLOTS];
        let mut table = [[0; size_of::<usize>()]; ADDRESSES * FILTERS];
        // Cached address and filter arrays are consecutive. The table is address-major.
        if !self
            .platform
            .read(self.base + ADDRESS_MODES, modes.as_flattened_mut())
            || !self
                .platform
                .read(self.base + SAMPLER_TABLE, table.as_flattened_mut())
        {
            return None;
        }
        let mut request = Bias {
            struct_size: size_of::<Bias>() as u32,
            abi_version: self.platform.d3d11_host_abi,
            session_id: self.host.session_id,
            samplers: [ptr::null_mut(); SLOTS],
            slot_mask: 0,
            bias: 0.0,
            applied_mask: 0,
        };
        for (slot, (address, filter)) in modes[..SLOTS].iter().zip(&modes[SLOTS..]).enumerate() {
            let address = u32::from_le_bytes(*address) as usize;
            let filter = u32::from_le_bytes(*filter) as usize;
            if address >= ADDRESSES || filter >= FILTERS {
                return None;
            }
            request.samplers[slot] = ptr::with_exposed_provenance_mut(usize::from_le_bytes(
                table[address * FILTERS + filter],
            ));
            // Filters 2 and 3 are mip-linear and anisotropic. The rest are no-mip or comparison.
            if matches!(filter, 2 | 3) {
                request.slot_mask |= 1 << slot;
            }
        }
        Some(request)
    }

    fn apply(self, restore: bool) -> Option<()> {
        if self.host.user.is_null() || !unsafe { (self.leaves.on_renderer_thread)() } {
            return None;
        }
        let mut dimensions = [0; 8];
        if !self
            .platform
            .read(self.base + SURFACE_WIDTH, &mut dimensions)
        {
            return None;
        }
        let width = u32::from_le_bytes(dimensions[..4].try_into().ok()?);
        let height = u32::from_le_bytes(dimensions[4..].try_into().ok()?);
        let scene = if !restore && LIVE.load(Ordering::Acquire) {
            self.scene_extent(width, height)
        } else {
            None
        };
        let pending = PENDING.load(Ordering::Acquire);
        if scene.is_none() && !pending {
            return None;
        }
        let mut request = self.sampler_request()?;
        if let Some((rw, _)) = scene {
            request.bias = (rw as f32 / width as f32).log2();
        } else {
            request.slot_mask = 0;
        }
        if request.slot_mask == 0 && !pending {
            return None;
        }
        let context = self.platform.read_usize(self.base + IMMEDIATE_CONTEXT)?;
        let result = unsafe {
            (self.leaves.bind)(
                self.service,
                self.host.user,
                ptr::with_exposed_provenance_mut(context),
                &mut request,
            )
        };
        if result != 0 {
            return None;
        }
        PENDING.store(request.applied_mask != 0, Ordering::Release);
        if request.applied_mask != 0
            && REPORTS
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    (n < 4).then_some(n + 1)
                })
                .is_ok()
        {
            let (rw, rh) = scene.unwrap_or_default();
            unsafe {
                (self.leaves.report)(
                    &self.host,
                    request.bias,
                    request.applied_mask,
                    rw,
                    rh,
                    width,
                    height,
                )
            };
        }
        Some(())
    }
}

fn apply(restore: bool) {
    let binding = *BINDING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(binding) = binding {
        let _ = binding.apply(restore);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sampling_guard(platform: *const Platform) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    let base = platform.main_module();
    base != 0 && platform.matches(base + FLUSH, &FLUSH_BYTES)
}

/// Lifecycle calls are serialized by the adapter session.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sampling_start(
    host: &rsf_game_host_services,
    platform: *const Platform,
    leaves: &Leaves,
    detour: *const c_void,
) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    let base = platform.main_module();
    if base == 0 {
        return false;
    }
    let target = base + FLUSH;
    if !LIVE.load(Ordering::Acquire) && !platform.matches(target, &FLUSH_BYTES) {
        return false;
    }
    let runtime = platform.module_handle("ReScaleFrame.Runtime.dll");
    let service = platform.export_address(runtime, c"rsf_d3d11_host_bind_sampler_bias");
    if service == 0 {
        return false;
    }
    if ORIGINAL.load(Ordering::Acquire).is_null() {
        let Some(original) = platform.hook_create(target, detour) else {
            return false;
        };
        ORIGINAL.store(original, Ordering::Release);
    }
    let binding = Binding {
        platform,
        base,
        service,
        host: *host,
        leaves: *leaves,
    };
    let previous = BINDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(binding);
    if !platform.hook_enable(target) {
        *BINDING.lock().unwrap_or_else(PoisonError::into_inner) = previous;
        return false;
    }
    LIVE.store(true, Ordering::Release);
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_sampling_quiesce() {
    LIVE.store(false, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_sampling_busy() -> bool {
    PENDING.load(Ordering::Acquire) || CALLS.load(Ordering::Acquire) != 0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_sampling_stop(platform: *const Platform) -> bool {
    rsf_skyrim_sampling_quiesce();
    if rsf_skyrim_sampling_busy() {
        return false;
    }
    if ORIGINAL.load(Ordering::Acquire).is_null() {
        return true;
    }
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    let base = platform.main_module();
    base != 0 && platform.hook_disable(base + FLUSH) && !rsf_skyrim_sampling_busy()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_sampling_enter() -> *mut c_void {
    CALLS.fetch_add(1, Ordering::AcqRel);
    ORIGINAL.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_sampling_leave(returned: bool) {
    if returned {
        apply(false);
    }
    CALLS.fetch_sub(1, Ordering::AcqRel);
}

pub(crate) fn finish() {
    if PENDING.load(Ordering::Acquire) {
        CALLS.fetch_add(1, Ordering::AcqRel);
        apply(true);
        CALLS.fetch_sub(1, Ordering::AcqRel);
    }
}
