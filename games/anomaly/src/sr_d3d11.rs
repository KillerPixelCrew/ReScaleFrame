// SPDX-License-Identifier: MIT
use crate::lease::Lease;
use crate::sr_swap_abi::{
    RSF_ANOMALY_SWAP_MAKE_CREATED, RSF_ANOMALY_SWAP_MAKE_REFUSED,
    rsf_anomaly_swap_request as Request, rsf_anomaly_swap_twins as Twins,
};
use passes::Pass;
use std::ffi::{c_char, c_void};
use std::ptr::{from_ref, null, null_mut};
use targets::{TwinBatch, TwinBatchInfo, allocate_twins};

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub(crate) use native::texture_format;
mod passes;
mod targets;
#[cfg(test)]
mod tests;
pub(crate) use passes::DrawViews;
pub(crate) use targets::TemporalTargets;

type Ptr = *mut c_void;
type Log = Option<unsafe extern "C" fn(Ptr, *const c_char)>;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Extent {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Extent {
    fn valid(self) -> bool {
        self.width != 0 && self.height != 0
    }

    fn fits(self, output: Self) -> bool {
        self.valid() && self.width <= output.width && self.height <= output.height
    }
}

// D3D11_TEXTURE2D_DESC, checked field by field in sr_d3d11.cpp.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Texture {
    pub(crate) size: Extent,
    pub(crate) mips: u32,
    pub(crate) layers: u32,
    pub(crate) format: u32,
    pub(crate) samples: u32,
    pub(crate) sample_quality: u32,
    pub(crate) usage: u32,
    pub(crate) bindings: u32,
    pub(crate) cpu_access: u32,
    pub(crate) misc: u32,
}

#[repr(C)]
#[derive(Default)]
struct View {
    texture2d: u32,
    format: u32,
    flags: u32,
    descriptor: [u32; 6],
}

#[repr(C)]
struct Draw {
    vertex: Ptr,
    pixel: Ptr,
    constants: Ptr,
    raster: Ptr,
    blend: Ptr,
    depth: Ptr,
    size: Extent,
}

#[repr(C)]
pub(crate) struct Platform {
    retain: unsafe extern "C" fn(Ptr),
    pub(crate) release: unsafe extern "C" fn(Ptr),
    pub(crate) as_texture: unsafe extern "C" fn(Ptr) -> Ptr,
    view_resource: unsafe extern "C" fn(Ptr) -> Ptr,
    pub(crate) texture_device: unsafe extern "C" fn(Ptr) -> Ptr,
    pub(crate) describe_texture: unsafe extern "C" fn(Ptr, &mut Texture),
    describe_view: unsafe extern "C" fn(Ptr, u32, &mut View),
    format_support: unsafe extern "C" fn(Ptr, u32) -> u32,
    pub(crate) texture: unsafe extern "C" fn(Ptr, &Texture, *const c_void, u32) -> Ptr,
    view: unsafe extern "C" fn(Ptr, Ptr, u32, Option<&View>) -> Ptr,
    shader: unsafe extern "C" fn(Ptr, u32, u32, *const c_char, Log, Ptr, &mut Ptr) -> i32,
    constant_buffer: unsafe extern "C" fn(Ptr) -> Ptr,
    raster_state: unsafe extern "C" fn(Ptr) -> Ptr,
    blend_state: unsafe extern "C" fn(Ptr) -> Ptr,
    depth_state: unsafe extern "C" fn(Ptr, u32) -> Ptr,
    save_state: unsafe extern "C" fn(Ptr) -> Ptr,
    restore_state: unsafe extern "C" fn(Ptr, Ptr),
    update_constants: unsafe extern "C" fn(Ptr, Ptr, *const u32),
    ps_constant: unsafe extern "C" fn(Ptr, u32, Ptr),
    targets: unsafe extern "C" fn(Ptr, u32, *const Ptr, Ptr),
    ps_resources: unsafe extern "C" fn(Ptr, u32, u32, *const Ptr),
    clear_stencil: unsafe extern "C" fn(Ptr, Ptr),
    draw: unsafe extern "C" fn(Ptr, &Draw),
}

const _: () = {
    assert!(size_of::<Extent>() == 8);
    assert!(size_of::<Texture>() == 44);
    assert!(size_of::<View>() == 36);
    assert!(size_of::<Draw>() == 56);
    assert!(size_of::<Platform>() == 184);
};

impl Platform {
    pub(crate) fn lease(&self, reference: Ptr) -> Option<Lease> {
        Lease::new(reference, self.release)
    }
}

fn pointer(lease: &Option<Lease>) -> Ptr {
    lease.as_ref().map_or(std::ptr::null_mut(), Lease::as_ptr)
}

// Serialized by the stage lock. The native SwapHost holds one opaque
// handle, and a frame may borrow the resulting views until it retires them.
#[derive(Default)]
struct StageResources {
    pass: Option<Pass>,
    twins: Option<TwinBatch>,
    twin_bytes: u64,
    drop_refs: u64,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_make_twins(
    handle: &mut *mut StageResources,
    platform: &'static Platform,
    device: Ptr,
    request: &Request,
    out: &mut Twins,
    info: &mut TwinBatchInfo,
    log: Log,
    user: Ptr,
) -> u32 {
    *info = TwinBatchInfo::default();
    *out = Twins::default();
    let kind = match request.stage {
        1 => ROUND_TRIP,
        2 => UPSCALE,
        _ => {
            info.pass_refused = true;
            return RSF_ANOMALY_SWAP_MAKE_REFUSED;
        }
    };
    if handle.is_null() {
        *handle = Box::into_raw(Box::new(StageResources::default()));
    }
    // The native host owns the handle and calls this only under the stage lock.
    let resources = unsafe { &mut **handle };
    if !resources
        .pass
        .as_ref()
        .is_some_and(|pass| pass.device.as_ptr() == device && pass.kind == kind)
    {
        let Some(pass) = Pass::new(platform, device, kind, log, user) else {
            info.pass_refused = true;
            return RSF_ANOMALY_SWAP_MAKE_REFUSED;
        };
        resources.pass = Some(pass);
    }
    let result = allocate_twins(platform, device, request, out, info);
    resources.twin_bytes = info.bytes;
    match result {
        Ok(batch) => {
            resources.twins = Some(batch);
            RSF_ANOMALY_SWAP_MAKE_CREATED
        }
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_retire_twins(
    resources: Option<&mut StageResources>,
    refs: unsafe extern "C" fn(Ptr) -> u32,
) {
    if let Some(resources) = resources {
        resources.drop_refs = resources.twins.take().map_or(0, |batch| batch.retire(refs));
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_reset_pass(resources: Option<&mut StageResources>) {
    if let Some(resources) = resources {
        resources.pass = None;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_stage_drop_resources(owner: *mut StageResources) {
    // The controller is the only caller authorized to retire twins. Static
    // teardown may run with failed cleanup and borrowed engine fields still live.
    if unsafe { owner.as_ref() }.is_some_and(|owner| owner.twins.is_none()) {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_pass(resources: Option<&StageResources>) -> *const Pass {
    resources
        .and_then(|resources| resources.pass.as_ref())
        .map_or(null(), from_ref)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_depth(resources: Option<&StageResources>) -> Ptr {
    resources
        .and_then(|resources| resources.twins.as_ref())
        .map_or(null_mut(), TwinBatch::depth_texture)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_resource_stats(
    resources: Option<&StageResources>,
    bytes: &mut u64,
    refs: &mut u64,
) {
    (*bytes, *refs) = resources.map_or((0, 0), |resources| {
        (resources.twin_bytes, resources.drop_refs)
    });
}

// These values are checked against the SDK in the native leaves.
const RTV: u32 = 0;
const SRV: u32 = 1;
const UAV: u32 = 2;
const DSV: u32 = 3;
const DEPTH_SRV: u32 = 4;
pub(crate) const ROUND_TRIP: u32 = 0;
pub(crate) const UPSCALE: u32 = 1;
pub(crate) const PREPARE: u32 = 2;
pub(crate) const REINSERT: u32 = 3;

#[cfg(test)]
pub(crate) fn fixture_device() -> Ptr {
    tests::ptr(tests::DEVICE)
}

#[cfg(test)]
pub(crate) fn fixture_platform() -> *const c_void {
    std::ptr::from_ref(&tests::PLATFORM).cast()
}

// Registers a texture the fixture's as_texture and describe_texture answer for.
#[cfg(test)]
pub(crate) fn fixture_source(name: usize, description: Texture) -> Ptr {
    tests::fake(|state| {
        state.textures.insert(name, description);
    });
    tests::ptr(name)
}

// The descriptors passed to the fixture's CreateTexture2D since the last call.
#[cfg(test)]
pub(crate) fn fixture_created() -> Vec<Texture> {
    tests::calls()
        .into_iter()
        .filter_map(|call| match call {
            tests::Call::Texture(description, _, _) => Some(description),
            _ => None,
        })
        .collect()
}

// COM references the fixture handed out and has not had released.
#[cfg(test)]
pub(crate) fn fixture_references() -> usize {
    tests::fake(|state| state.owned.values().sum())
}

#[cfg(test)]
pub(crate) fn refuse_fixture_target_view() {
    tests::fake(|state| state.fail_view = Some(RTV));
}
