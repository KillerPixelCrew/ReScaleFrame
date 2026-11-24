// SPDX-License-Identifier: MIT
use super::{Draw, Extent, Log, PREPARE, Ptr, REINSERT, ROUND_TRIP, UPSCALE, d3d, lease};
use crate::lease::Lease;
use std::ffi::CStr;
use std::ptr::{NonNull, null_mut};

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct DrawViews {
    pub(crate) output: Extent,
    pub(crate) source: Extent,
    pub(crate) targets: [Ptr; 3],
    pub(crate) depth: Ptr,
    pub(crate) inputs: [Ptr; 5],
    pub(crate) first_person_depth: u32,
    pub(crate) first_person_zero: u32,
}

const _: () = assert!(size_of::<DrawViews>() == 96);

impl Default for DrawViews {
    fn default() -> Self {
        Self {
            output: Extent::default(),
            source: Extent::default(),
            targets: [null_mut(); 3],
            depth: null_mut(),
            inputs: [null_mut(); 5],
            first_person_depth: 0,
            first_person_zero: 0,
        }
    }
}

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
    fn new(device: Ptr, write_depth: bool) -> Option<Self> {
        Some(Self {
            raster: lease(unsafe { d3d::raster_state(device) })?,
            blend: lease(unsafe { d3d::blend_state(device) })?,
            depth: lease(unsafe { d3d::depth_state(device, u32::from(write_depth)) })?,
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
struct Compiler {
    device: Ptr,
    source: u32,
    log: Log,
    user: Ptr,
}

impl Compiler {
    // A normal compile failure disables the optional temporal passes. A native
    // exception has always made the entire pass creation fail.
    fn shader(self, vertex: bool, entry: &CStr) -> Result<Option<Lease>, ()> {
        let mut shader = null_mut();
        let result = unsafe {
            d3d::shader(
                self.device,
                self.source,
                u32::from(vertex),
                entry.as_ptr(),
                self.log,
                self.user,
                &mut shader,
            )
        };
        let lease = lease(shader);
        match result {
            1 => Ok(lease),
            0 => Ok(None),
            _ => Err(()),
        }
    }

    fn constants(self) -> Option<Lease> {
        lease(unsafe { d3d::constant_buffer(self.device) })
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
        let Some(no_depth) = lease(unsafe { d3d::depth_state(self.device, 0) }) else {
            return Ok(None);
        };
        Ok(Some(TemporalPass {
            prepare,
            reinsert,
            constants,
            no_depth,
        }))
    }
}

pub(crate) struct Pass {
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
struct SavedState {
    context: Ptr,
    snapshot: NonNull<std::ffi::c_void>,
}

impl SavedState {
    fn new(context: Ptr) -> Option<Self> {
        let snapshot = unsafe { d3d::save_state(context) };
        Some(Self {
            context,
            snapshot: NonNull::new(snapshot)?,
        })
    }

    fn constants(&self, buffer: &Lease, values: [u32; 4]) {
        unsafe { d3d::update_constants(self.context, buffer.as_ptr(), values.as_ptr()) };
    }

    fn targets(&self, targets: &[Ptr], depth: Ptr) {
        unsafe { d3d::targets(self.context, targets.len() as u32, targets.as_ptr(), depth) };
    }

    fn resources(&self, slot: u32, inputs: &[Ptr]) {
        unsafe { d3d::ps_resources(self.context, slot, inputs.len() as u32, inputs.as_ptr()) };
    }
}

impl Drop for SavedState {
    fn drop(&mut self) {
        unsafe { d3d::restore_state(self.context, self.snapshot.as_ptr()) };
    }
}

impl Pass {
    #[cfg(all(windows, feature = "sr-fixtures"))]
    pub(super) fn temporal_ready(&self) -> bool {
        self.temporal.is_some()
    }

    pub(super) fn new(device: Ptr, kind: u32, log: Log, user: Ptr) -> Option<Self> {
        if device.is_null() {
            return None;
        }
        let (vs, ps) = match kind {
            ROUND_TRIP => (c"rsf_roundtrip_vs", c"rsf_roundtrip_ps"),
            UPSCALE => (c"rsf_upscale_vs", c"rsf_upscale_ps"),
            _ => return None,
        };
        let compiler = Compiler {
            device,
            source: kind,
            log,
            user,
        };
        let vertex = compiler.shader(true, vs).ok()??;
        let pixel = compiler.shader(false, ps).ok()??;
        let constants = compiler.constants()?;
        let states = States::new(device, kind == UPSCALE)?;
        let temporal = if kind == UPSCALE {
            compiler.temporal().ok()?
        } else {
            None
        };
        unsafe { d3d::retain(device) };
        Some(Self {
            kind,
            vertex,
            pixel,
            constants,
            states,
            temporal,
            device: lease(device)?,
        })
    }

    fn draw(
        &self,
        context: Ptr,
        size: Extent,
        pixel: &Lease,
        depth: &Lease,
        bind: impl FnOnce(&SavedState),
    ) -> bool {
        let Some(state) = SavedState::new(context) else {
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
        unsafe { d3d::draw(context, &draw) };
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
                unsafe { d3d::ps_constant(context, 1, temporal.constants.as_ptr()) };
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
            unsafe { d3d::clear_stencil(context, views.depth) };
            state.targets(&views.targets, views.depth);
            state.resources(0, inputs);
        })
    }

    pub(super) fn run(&self, context: Ptr, kind: u32, views: &DrawViews) -> bool {
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
pub(crate) extern "C" fn rsf_anomaly_sr_temporal_ready(owner: Option<&Pass>) -> u32 {
    u32::from(owner.is_some_and(|pass| pass.temporal.is_some()))
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_run_pass(
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
