// SPDX-License-Identifier: MIT
use super::{Draw, Extent, Log, PREPARE, Platform, Ptr, REINSERT, ROUND_TRIP, UPSCALE};
use crate::lease::Lease;
use std::ffi::CStr;
use std::ptr::{NonNull, null_mut};

#[repr(C)]
struct DrawViews {
    output: Extent,
    source: Extent,
    targets: [Ptr; 3],
    depth: Ptr,
    inputs: [Ptr; 5],
    first_person_depth: u32,
    first_person_zero: u32,
}

const _: () = assert!(size_of::<DrawViews>() == 96);

impl DrawViews {
    fn sizes(&self) -> [u32; 4] {
        [
            self.output.width,
            self.output.height,
            self.source.width,
            self.source.height,
        ]
    }
}

struct States {
    raster: Lease,
    blend: Lease,
    depth: Lease,
}

impl States {
    fn new(platform: &Platform, device: Ptr, write_depth: bool) -> Option<Self> {
        Some(Self {
            raster: platform.lease(unsafe { (platform.raster_state)(device) })?,
            blend: platform.lease(unsafe { (platform.blend_state)(device) })?,
            depth: platform
                .lease(unsafe { (platform.depth_state)(device, u32::from(write_depth)) })?,
        })
    }
}

struct TemporalPass {
    prepare: Lease,
    reinsert: Lease,
    constants: Lease,
    no_depth: Lease,
}

#[derive(Clone, Copy)]
struct Compiler<'a> {
    platform: &'a Platform,
    device: Ptr,
    source: u32,
    log: Log,
    user: Ptr,
}

impl Compiler<'_> {
    // A normal compile failure disables the optional temporal passes. A native
    // exception has always made the entire pass creation fail.
    fn shader(self, vertex: bool, entry: &CStr) -> Result<Option<Lease>, ()> {
        let mut shader = null_mut();
        let result = unsafe {
            (self.platform.shader)(
                self.device,
                self.source,
                u32::from(vertex),
                entry.as_ptr(),
                self.log,
                self.user,
                &mut shader,
            )
        };
        let lease = self.platform.lease(shader);
        match result {
            1 => Ok(lease),
            0 => Ok(None),
            _ => Err(()),
        }
    }

    fn constants(self) -> Option<Lease> {
        self.platform
            .lease(unsafe { (self.platform.constant_buffer)(self.device) })
    }

    fn temporal(self) -> Result<Option<TemporalPass>, ()> {
        let Some(prepare) = self.shader(false, c"rsf_prepare_ps")? else {
            return Ok(None);
        };
        let Some(reinsert) = self.shader(false, c"rsf_reinsert_ps")? else {
            return Ok(None);
        };
        let Some(constants) = self.constants() else {
            return Ok(None);
        };
        let Some(States { depth, .. }) = States::new(self.platform, self.device, false) else {
            return Ok(None);
        };
        Ok(Some(TemporalPass {
            prepare,
            reinsert,
            constants,
            no_depth: depth,
        }))
    }
}

pub(super) struct Pass {
    platform: &'static Platform,
    pub(super) kind: u32,
    vertex: Lease,
    pixel: Lease,
    constants: Lease,
    states: States,
    temporal: Option<TemporalPass>,
    pub(super) device: Lease,
}

// The draw is synchronous. The COM context is borrowed from the calling render
// thread until the last touched binding and predicate have been restored.
struct SavedState<'a> {
    platform: &'a Platform,
    context: Ptr,
    snapshot: NonNull<std::ffi::c_void>,
}

impl<'a> SavedState<'a> {
    fn new(platform: &'a Platform, context: Ptr) -> Option<Self> {
        let snapshot = unsafe { (platform.save_state)(context) };
        Some(Self {
            platform,
            context,
            snapshot: NonNull::new(snapshot)?,
        })
    }

    fn constants(&self, buffer: &Lease, values: [u32; 4]) {
        unsafe { (self.platform.update_constants)(self.context, buffer.as_ptr(), values.as_ptr()) };
    }

    fn targets(&self, targets: &[Ptr], depth: Ptr) {
        unsafe {
            (self.platform.targets)(self.context, targets.len() as u32, targets.as_ptr(), depth)
        };
    }

    fn resources(&self, slot: u32, inputs: &[Ptr]) {
        unsafe {
            (self.platform.ps_resources)(self.context, slot, inputs.len() as u32, inputs.as_ptr())
        };
    }
}

impl Drop for SavedState<'_> {
    fn drop(&mut self) {
        unsafe { (self.platform.restore_state)(self.context, self.snapshot.as_ptr()) };
    }
}

impl Pass {
    pub(super) fn new(
        platform: &'static Platform,
        device: Ptr,
        kind: u32,
        log: Log,
        user: Ptr,
    ) -> Option<Self> {
        if device.is_null() {
            return None;
        }
        let (vs, ps) = match kind {
            ROUND_TRIP => (c"rsf_roundtrip_vs", c"rsf_roundtrip_ps"),
            UPSCALE => (c"rsf_upscale_vs", c"rsf_upscale_ps"),
            _ => return None,
        };
        let compiler = Compiler {
            platform,
            device,
            source: kind,
            log,
            user,
        };
        let vertex = compiler.shader(true, vs).ok()??;
        let pixel = compiler.shader(false, ps).ok()??;
        let constants = compiler.constants()?;
        let states = States::new(platform, device, kind == UPSCALE)?;
        let temporal = if kind == UPSCALE {
            compiler.temporal().ok()?
        } else {
            None
        };
        unsafe { (platform.retain)(device) };
        Some(Self {
            platform,
            kind,
            vertex,
            pixel,
            constants,
            states,
            temporal,
            device: platform.lease(device)?,
        })
    }

    fn draw(
        &self,
        context: Ptr,
        size: Extent,
        pixel: &Lease,
        depth: &Lease,
        bind: impl FnOnce(&SavedState<'_>),
    ) -> bool {
        let Some(state) = SavedState::new(self.platform, context) else {
            return false;
        };
        state.resources(0, &[null_mut(); 4]);
        bind(&state);
        let draw = Draw {
            vertex: self.vertex.as_ptr(),
            pixel: pixel.as_ptr(),
            constants: self.constants.as_ptr(),
            raster: self.states.raster.as_ptr(),
            blend: self.states.blend.as_ptr(),
            depth: depth.as_ptr(),
            size,
        };
        unsafe { (self.platform.draw)(context, &draw) };
        true
    }

    fn round_trip(&self, context: Ptr, views: &DrawViews) -> bool {
        if !views.source.valid() || views.targets[0].is_null() || views.inputs[0].is_null() {
            return false;
        }
        self.draw(
            context,
            views.output,
            &self.pixel,
            &self.states.depth,
            |state| {
                state.constants(&self.constants, views.sizes());
                state.targets(&views.targets[..1], null_mut());
                state.resources(0, &views.inputs[..1]);
            },
        )
    }

    fn prepare(&self, context: Ptr, views: &DrawViews) -> bool {
        let Some(temporal) = &self.temporal else {
            return false;
        };
        if views.targets[..2]
            .iter()
            .chain(&views.inputs[..3])
            .any(|pointer| pointer.is_null())
        {
            return false;
        }
        let values = [
            views.output.width,
            views.output.height,
            views.first_person_depth,
            views.first_person_zero,
        ];
        self.draw(
            context,
            views.output,
            &temporal.prepare,
            &temporal.no_depth,
            |state| {
                state.constants(&temporal.constants, values);
                unsafe { (self.platform.ps_constant)(context, 1, temporal.constants.as_ptr()) };
                state.targets(&views.targets[..2], null_mut());
                state.resources(0, &[null_mut(); 6]);
                state.resources(0, &views.inputs[0..1]);
                state.resources(3, &views.inputs[1..2]);
                state.resources(5, &views.inputs[2..3]);
            },
        )
    }

    fn upscale(&self, context: Ptr, views: &DrawViews, reinsert: bool) -> bool {
        let (pixel, input_count) = if reinsert {
            let Some(temporal) = &self.temporal else {
                return false;
            };
            (&temporal.reinsert, 5)
        } else {
            (&self.pixel, 4)
        };
        let inputs = &views.inputs[..input_count];
        if !views.source.fits(views.output)
            || views.depth.is_null()
            || views
                .targets
                .iter()
                .chain(inputs)
                .any(|pointer| pointer.is_null())
        {
            return false;
        }
        self.draw(context, views.output, pixel, &self.states.depth, |state| {
            state.constants(&self.constants, views.sizes());
            // The world stencil is internal to the reduced render window.
            unsafe { (self.platform.clear_stencil)(context, views.depth) };
            state.targets(&views.targets, views.depth);
            state.resources(0, inputs);
        })
    }

    fn run(&self, context: Ptr, kind: u32, views: &DrawViews) -> bool {
        if context.is_null() || !views.output.valid() {
            return false;
        }
        match (self.kind, kind) {
            (ROUND_TRIP, ROUND_TRIP) => self.round_trip(context, views),
            (UPSCALE, UPSCALE | REINSERT) => self.upscale(context, views, kind == REINSERT),
            (UPSCALE, PREPARE) => self.prepare(context, views),
            _ => false,
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_create_pass(
    platform: &'static Platform,
    device: Ptr,
    kind: u32,
    log: Log,
    user: Ptr,
) -> *mut Pass {
    Pass::new(platform, device, kind, log, user)
        .map_or(null_mut(), |pass| Box::into_raw(Box::new(pass)))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_drop_pass(owner: *mut Pass) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_pass_device(owner: Option<&Pass>) -> Ptr {
    owner.map_or(null_mut(), |pass| pass.device.as_ptr())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_temporal_ready(owner: Option<&Pass>) -> u32 {
    u32::from(owner.is_some_and(|pass| pass.temporal.is_some()))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_run_pass(
    owner: Option<&Pass>,
    context: Ptr,
    kind: u32,
    views: &DrawViews,
) -> u32 {
    u32::from(owner.is_some_and(|pass| pass.run(context, kind, views)))
}

#[cfg(test)]
#[path = "pass_tests.rs"]
mod tests;
