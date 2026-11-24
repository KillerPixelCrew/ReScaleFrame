// SPDX-License-Identifier: MIT
use crate::lease::Lease;
use crate::sys::{
    RSF_ANOMALY_SWAP_MAKE_CREATED, RSF_ANOMALY_SWAP_MAKE_REFUSED,
    rsf_anomaly_swap_request as Request, rsf_anomaly_swap_twins as Twins,
};
use passes::Pass;
use std::ffi::{c_char, c_void};
use std::ptr::{from_ref, null, null_mut};
use targets::{TwinBatch, TwinBatchInfo, allocate_twins};

#[cfg(windows)]
pub(crate) mod native;
#[cfg(all(windows, feature = "sr-fixtures"))]
mod roundtrip_device;
#[cfg(all(windows, feature = "sr-fixtures"))]
mod temporal_device;
#[cfg(windows)]
pub(crate) use native::texture_format;
pub(crate) mod passes;
pub(crate) mod targets;
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

// D3D11_TEXTURE2D_DESC, checked field by field in sr_d3d11_platform.c.
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
pub(crate) struct View {
    texture2d: u32,
    format: u32,
    flags: u32,
    descriptor: [u32; 6],
}

#[repr(C)]
pub(crate) struct Draw {
    vertex: Ptr,
    pixel: Ptr,
    constants: Ptr,
    raster: Ptr,
    blend: Ptr,
    depth: Ptr,
    size: Extent,
}

const _: () = {
    assert!(size_of::<Extent>() == 8);
    assert!(size_of::<Texture>() == 44);
    assert!(size_of::<View>() == 36);
    assert!(size_of::<Draw>() == 56);
};

// The typed D3D11 calls: native.rs and the C pipeline-state leaves, the recording fakes under test.
#[cfg(not(test))]
pub(crate) use native::calls as d3d;
#[cfg(test)]
pub(crate) use tests::calls as d3d;

/// One transferred COM reference, released on drop.
pub(crate) fn lease(reference: Ptr) -> Option<Lease> {
    Lease::new(reference, d3d::release)
}

fn pointer(lease: &Option<Lease>) -> Ptr {
    lease.as_ref().map_or(std::ptr::null_mut(), Lease::as_ptr)
}

// Serialized by the stage lock. The native SwapHost holds one opaque
// handle, and a frame may borrow the resulting views until it retires them.
#[derive(Default)]
pub(crate) struct StageResources {
    pass: Option<Pass>,
    twins: Option<TwinBatch>,
    twin_bytes: u64,
    drop_refs: u64,
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_stage_make_twins(
    handle: &mut *mut StageResources,
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
        let Some(pass) = Pass::new(device, kind, log, user) else {
            info.pass_refused = true;
            return RSF_ANOMALY_SWAP_MAKE_REFUSED;
        };
        resources.pass = Some(pass);
    }
    let result = allocate_twins(device, request, out, info);
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
pub(crate) extern "C" fn rsf_anomaly_sr_stage_retire_twins(
    resources: Option<&mut StageResources>,
    refs: unsafe extern "C" fn(Ptr) -> u32,
) {
    if let Some(resources) = resources {
        resources.drop_refs = resources.twins.take().map_or(0, |batch| batch.retire(refs));
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_stage_reset_pass(resources: Option<&mut StageResources>) {
    if let Some(resources) = resources {
        resources.pass = None;
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_stage_pass(
    resources: Option<&StageResources>,
) -> *const Pass {
    resources
        .and_then(|resources| resources.pass.as_ref())
        .map_or(null(), from_ref)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_stage_depth(resources: Option<&StageResources>) -> Ptr {
    resources
        .and_then(|resources| resources.twins.as_ref())
        .map_or(null_mut(), TwinBatch::depth_texture)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_stage_resource_stats(
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
