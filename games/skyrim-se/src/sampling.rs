// SPDX-License-Identifier: MIT
//! Skyrim's native PS sampler selection and restoration around the cached-state flush.

use crate::platform::{self, Platform};
use crate::render_hook::RenderHook;
use crate::render_inputs::renderer_thread;
use crate::render_native::thread;
use crate::resolution;
use rsf_game_sys::d3d11_host_abi::rsf_d3d11_sampler_bias as Bias;
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

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

#[derive(Clone, Copy)]
struct Binding {
    platform: &'static Platform,
    base: usize,
    bind: Bind,
    host: rsf_game_host_services,
}

// The session keeps the Runtime and its host services alive while the hook or restore is busy.
// Every engine read and graphics call uses the renderer thread.
unsafe impl Send for Binding {}

type Bind = unsafe extern "C" fn(*mut c_void, *mut c_void, &mut Bias) -> i32;

static HOOK: RenderHook<Binding> = RenderHook::new();
static PENDING: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU32 = AtomicU32::new(0);
static REPORTS: AtomicU32 = AtomicU32::new(0);

impl Binding {
    fn scene_extent(&self, width: u32, height: u32) -> Option<(u32, u32)> {
        let [rw, rh] = resolution::extent(width, height)?;
        // Primary colour1/depth0 select the scene raster. Postprocess and UI change colour
        // targets and shadow views change depth. The live ratios must also show reduction.
        (rw < width
            && self.platform.read_u32(self.base + COLOUR_TARGET) == Some(1)
            && self.platform.read_u32(self.base + DEPTH_TARGET) == Some(0)
            && self.platform.read_u32(self.base + SHADOW_VIEW) == Some(0)
            && self
                .platform
                .read_u32(self.base + RASTER_RATIO)
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
        if self.host.user.is_null() || renderer_thread() != thread() {
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
        let scene = if !restore && HOOK.live.load(Ordering::Acquire) {
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
            (self.bind)(
                self.host.user,
                ptr::with_exposed_provenance_mut(context),
                &mut request,
            )
        };
        if result != 0 {
            return None;
        }
        PENDING.store(request.applied_mask != 0, Ordering::Release);
        if self.host.log.is_some()
            && request.applied_mask != 0
            && REPORTS
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    (n < 4).then_some(n + 1)
                })
                .is_ok()
        {
            let (rw, rh) = scene.unwrap_or_default();
            self.platform.log_text(&self.host, &format!(
                "Skyrim scene sampler mip bias={} slots=0x{:x} render={rw}x{rh} output={width}x{height}; native comparison/no-mip samplers retained",
                request.bias, request.applied_mask,
            ));
        }
        Some(())
    }
}

fn apply(restore: bool) {
    let binding = HOOK.binding();
    if let Some(binding) = binding {
        let _ = binding.apply(restore);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_sampling_guard() -> bool {
    let Some(platform) = platform::get() else {
        return false;
    };
    let base = platform.main_module();
    base != 0 && platform.matches(base + FLUSH, &FLUSH_BYTES)
}

/// Lifecycle calls are serialized by the adapter session.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_sampling_start(host: &rsf_game_host_services) -> bool {
    let Some(platform) = platform::get() else {
        return false;
    };
    let base = platform.main_module();
    if base == 0 {
        return false;
    }
    let target = base + FLUSH;
    if !HOOK.live.load(Ordering::Acquire) && !platform.matches(target, &FLUSH_BYTES) {
        return false;
    }
    let runtime = platform.module_handle("ReScaleFrame.Runtime.dll");
    let service = platform.export_address(runtime, c"rsf_d3d11_host_bind_sampler_bias");
    if service == 0 {
        return false;
    }
    let binding = Binding {
        platform,
        base,
        bind: unsafe { std::mem::transmute::<usize, Bind>(service) },
        host: *host,
    };
    HOOK.enable(
        binding,
        || platform.hook_create(target, observe as *const c_void),
        || platform.hook_enable(target),
    )
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_sampling_quiesce() {
    HOOK.live.store(false, Ordering::Release);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_sampling_busy() -> bool {
    PENDING.load(Ordering::Acquire) || CALLS.load(Ordering::Acquire) != 0
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_sampling_stop() -> bool {
    rsf_skyrim_sampling_quiesce();
    if rsf_skyrim_sampling_busy() {
        return false;
    }
    if HOOK.original.load(Ordering::Acquire).is_null() {
        return true;
    }
    let Some(platform) = platform::get() else {
        return false;
    };
    let base = platform.main_module();
    base != 0 && platform.hook_disable(base + FLUSH) && !rsf_skyrim_sampling_busy()
}

unsafe extern "C" fn observe(preserve_input_layout: u8) -> usize {
    CALLS.fetch_add(1, Ordering::AcqRel);
    struct Call;
    impl Drop for Call {
        fn drop(&mut self) {
            CALLS.fetch_sub(1, Ordering::AcqRel);
        }
    }
    let _call = Call;
    let original = HOOK.original.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // The exact-build guard establishes this native flush signature.
    let flush: unsafe extern "C" fn(u8) -> usize = unsafe { std::mem::transmute(original) };
    let result = unsafe { flush(preserve_input_layout) };
    apply(false);
    result
}

pub(crate) fn finish() {
    if PENDING.load(Ordering::Acquire) {
        CALLS.fetch_add(1, Ordering::AcqRel);
        apply(true);
        CALLS.fetch_sub(1, Ordering::AcqRel);
    }
}
