// SPDX-License-Identifier: MIT
use crate::auxiliary_sr_abi::*;
use crate::com_ref::ComRef;
use crate::d3d11_output::SavedBindings;
use rsf_game_sys::d3d11_host_abi::rsf_d3d11_auxiliary_surface;
use rsf_game_sys::{rsf_game_log_fn, rsf_game_render_pass};
use std::ffi::c_void;
use std::ptr;
use std::slice;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

const MAX_AUXILIARIES: usize = 8;
const MAX_SURFACES: usize = MAX_AUXILIARIES + 2;
type Texture = *mut c_void;

struct Owner {
    context: ComRef,
    device: ComRef,
    thread: u32,
}

fn current_thread() -> u32 {
    unsafe { GetCurrentThreadId() }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TextureKey {
    width: u32,
    height: u32,
    format: u32,
    sample_quality: u32,
}

impl From<rsf_aux_texture_info> for TextureKey {
    fn from(desc: rsf_aux_texture_info) -> Self {
        Self {
            width: desc.width,
            height: desc.height,
            format: desc.format,
            sample_quality: desc.sample_quality,
        }
    }
}

struct Snapshot {
    key: TextureKey,
    texture: ComRef,
}

struct ColourProducts {
    target: ComRef,
    view: ComRef,
    output: ComRef,
    source: Snapshot,
    destination: TextureKey,
}

struct Pipeline {
    vertex: ComRef,
    pixel: ComRef,
    raster: ComRef,
    constants: ComRef,
}

impl Pipeline {
    fn borrow(&self) -> rsf_aux_colour_pipeline {
        rsf_aux_colour_pipeline {
            vertex: self.vertex.0.as_ptr(),
            pixel: self.pixel.0.as_ptr(),
            raster: self.raster.0.as_ptr(),
            constants: self.constants.0.as_ptr(),
        }
    }
}

#[derive(Clone, Copy)]
struct Mapping {
    source: [f32; 4],
    output: [i32; 4],
    jitter: [f32; 2],
}

impl From<&rsf_d3d11_auxiliary_surface> for Mapping {
    fn from(surface: &rsf_d3d11_auxiliary_surface) -> Self {
        Self {
            source: surface.source_rect,
            output: surface.output_rect,
            jitter: surface.jitter_pixels,
        }
    }
}

impl Mapping {
    fn source_fits(&self, key: TextureKey) -> bool {
        let [left, top, right, bottom] = self.source;
        self.source.into_iter().all(f32::is_finite)
            && left >= 0.0
            && top >= 0.0
            && right <= key.width as f32
            && bottom <= key.height as f32
            && right - left >= 1.0
            && bottom - top >= 1.0
    }

    fn output_fits(&self, key: TextureKey) -> bool {
        let [left, top, right, bottom] = self.output;
        left >= 0
            && top >= 0
            && right > left
            && bottom > top
            && right as u32 <= key.width
            && bottom as u32 <= key.height
    }

    fn integral(&self) -> bool {
        self.source.into_iter().all(|bound| bound.floor() == bound)
    }

    fn jitter_valid(&self) -> bool {
        self.jitter
            .into_iter()
            .all(|value| (-16384.0..=16384.0).contains(&value))
    }

    fn colour_constants(&self) -> rsf_aux_draw_constants {
        let [left, top, right, bottom] = self.source.map(|edge| (edge - 0.5).ceil());
        rsf_aux_draw_constants {
            source: self.source,
            output: self.output.map(|edge| edge as f32),
            valid: [left, top, left.max(right - 1.0), top.max(bottom - 1.0)],
            jitter: self.jitter,
            padding: [0.0; 2],
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct SurfaceKey {
    texture: Texture,
    kind: u32,
    flags: u32,
    output: [i32; 4],
}

impl From<&rsf_d3d11_auxiliary_surface> for SurfaceKey {
    fn from(input: &rsf_d3d11_auxiliary_surface) -> Self {
        Self {
            texture: input.texture,
            kind: input.kind,
            flags: input.flags,
            output: input.output_rect,
        }
    }
}

impl SurfaceKey {
    pub(crate) fn matches(self, input: &rsf_d3d11_auxiliary_surface) -> bool {
        surface_size_valid(input) && self == Self::from(input)
    }
}

#[derive(Clone, Copy)]
struct SurfacePlan {
    original: Texture,
    source: Texture,
    original_desc: rsf_aux_texture_info,
    source_desc: rsf_aux_texture_info,
    kind: u32,
    flags: u32,
    mapping: Mapping,
}

impl SurfacePlan {
    fn depth(&self) -> bool {
        self.kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL
    }

    fn restore_contents(&self) -> bool {
        self.flags & RSF_D3D11_AUXILIARY_RESTORE_CONTENTS != 0
    }

    fn accepts(&self, request: &rsf_d3d11_auxiliary_surface, spatial: bool) -> bool {
        let mapping = Mapping::from(request);
        SurfaceKey {
            texture: self.original,
            kind: self.kind,
            flags: self.flags,
            output: self.mapping.output,
        }
        .matches(request)
            && mapping.source_fits(self.original_desc.into())
            && mapping.source_fits(self.source_desc.into())
            && (!(spatial || self.depth()) || mapping.integral())
            && if spatial {
                mapping.jitter == [0.0; 2]
            } else {
                mapping.jitter_valid()
            }
    }
}

struct Surface {
    plan: SurfacePlan,
    _original: ComRef,
    _distinct_source: Option<ComRef>,
}

impl Surface {
    fn retain(plan: SurfacePlan) -> Self {
        // Reservation holds all the borrowed resources through allocation.
        Self {
            _original: unsafe { ComRef::retain(plan.original) },
            _distinct_source: (plan.source != plan.original)
                .then(|| unsafe { ComRef::retain(plan.source) }),
            plan,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Reserved,
    Capturing,
    Sealed,
    Prepared(usize),
    Publishing,
    Accepted,
}

struct Frame {
    scope: [u64; 3],
    surfaces: [Option<Surface>; MAX_SURFACES],
    count: usize,
    main_begin: usize,
    originals: [Option<ComRef>; 2],
    backups_captured: bool,
    depth_restore: Option<bool>,
    published_colours: usize,
    phase: Phase,
}

struct Bindings<'a> {
    platform: &'a rsf_aux_platform,
    context: *mut c_void,
    _saved: SavedBindings,
}

impl<'a> Bindings<'a> {
    fn new(platform: &'a rsf_aux_platform, context: *mut c_void) -> Option<Self> {
        let saved = unsafe { SavedBindings::new(context, platform.save?, platform.restore?) }?;
        let bindings = Self {
            platform,
            context,
            _saved: saved,
        };
        bindings.clear();
        Some(bindings)
    }

    fn clear(&self) {
        unsafe { self.platform.clear.unwrap()(self.context) };
    }

    fn copy(&self, destination: Texture, source: Texture) {
        unsafe { self.platform.copy.unwrap()(self.context, destination, source) };
    }
}

fn device_ready(platform: &rsf_aux_platform, device: *mut c_void) -> bool {
    unsafe { platform.device_ready.unwrap()(device) != 0 }
}

pub(crate) struct AuxiliarySr {
    // Drop native leases and private products while the context and device are still retained.
    frame: Option<Frame>,
    backups: [Option<Snapshot>; 2],
    colours: [Option<ColourProducts>; MAX_SURFACES],
    pipeline: Option<Pipeline>,
    owner: Option<Owner>,
    depth: *mut rsf_depth_scale,
    platform: rsf_aux_platform,
}

impl AuxiliarySr {
    pub(crate) fn new(platform: rsf_aux_platform) -> Self {
        Self {
            platform,
            owner: None,
            pipeline: None,
            depth: ptr::null_mut(),
            colours: [const { None }; MAX_SURFACES],
            backups: [None, None],
            frame: None,
        }
    }

    fn on_owner_thread(&self) -> bool {
        self.owner
            .as_ref()
            .is_some_and(|owner| owner.thread == current_thread())
    }

    fn bind_owner(&mut self, context: *mut c_void) -> Option<*mut c_void> {
        if context.is_null() {
            return None;
        }
        let thread = current_thread();
        if let Some(owner) = &self.owner {
            return (owner.context.0.as_ptr() == context && owner.thread == thread)
                .then_some(owner.device.0.as_ptr());
        }
        let device =
            unsafe { ComRef::from_owned(self.platform.immediate_device.unwrap()(context)) }?;
        let device_ptr = device.0.as_ptr();
        self.owner = Some(Owner {
            context: unsafe { ComRef::retain(context) },
            device,
            thread,
        });
        Some(device_ptr)
    }

    fn texture_info(&self, device: *mut c_void, texture: Texture) -> Option<rsf_aux_texture_info> {
        let mut facts = rsf_aux_texture_facts::default();
        if texture.is_null()
            || unsafe { self.platform.describe_texture.unwrap()(texture, device, &mut facts) } == 0
            || facts.same_device == 0
            || facts.array_size != 1
            || facts.mip_levels != 1
            || facts.sample_count != 1
            || facts.usage != RSF_AUX_USAGE_DEFAULT
        {
            return None;
        }
        Some(facts.info)
    }

    fn colour_supported(&self, device: *mut c_void, format: u32) -> bool {
        match format {
            RSF_AUX_FORMAT_R11G11B10_FLOAT => {
                // Numeric nearest loads through an SRV and draws into an RTV.
                let mut flags = 0;
                (unsafe { self.platform.packed_colour_support.unwrap()(device, &mut flags) } != 0)
                    && flags & RSF_AUX_PACKED_SUPPORT_MASK == RSF_AUX_PACKED_SUPPORT_MASK
            }
            RSF_AUX_FORMAT_RGBA32_FLOAT
            | RSF_AUX_FORMAT_RGBA16_FLOAT
            | RSF_AUX_FORMAT_RGBA8_UNORM
            | RSF_AUX_FORMAT_RG16_FLOAT
            | RSF_AUX_FORMAT_R32_FLOAT
            | RSF_AUX_FORMAT_R16_FLOAT
            | RSF_AUX_FORMAT_R8_UNORM => true,
            _ => false,
        }
    }

    fn plan_surfaces(
        &self,
        device: *mut c_void,
        pass: &rsf_game_render_pass,
        inputs: &[rsf_d3d11_auxiliary_surface],
        recovery: bool,
    ) -> Option<([Option<SurfacePlan>; MAX_SURFACES], usize)> {
        let mut plans = [None; MAX_SURFACES];
        let mut main_begin = inputs.len();
        let mut depth_seen = false;
        for (index, input) in inputs.iter().enumerate() {
            let main =
                input.texture == pass.color_output || input.texture == pass.color_output_readable;
            if !surface_size_valid(input)
                || input.kind > RSF_D3D11_AUXILIARY_COLOUR
                || (!recovery && main)
                || input.flags & !RSF_D3D11_AUXILIARY_RESTORE_CONTENTS != 0
                || inputs[..index]
                    .iter()
                    .any(|seen| seen.texture == input.texture)
            {
                return None;
            }

            let original_desc = self.texture_info(device, input.texture)?;
            let mapping = Mapping::from(input);
            if !mapping.source_fits(original_desc.into())
                || !mapping.output_fits(original_desc.into())
                || !mapping.jitter_valid()
                || (recovery && (!mapping.integral() || mapping.jitter != [0.0; 2]))
            {
                return None;
            }
            let (source, source_desc) = match input.kind {
                RSF_D3D11_AUXILIARY_DEPTH_STENCIL => {
                    if depth_seen
                        || original_desc.format != RSF_AUX_FORMAT_R24G8_TYPELESS
                        || original_desc.depth_stencil == 0
                        || !mapping.integral()
                    {
                        return None;
                    }
                    depth_seen = true;
                    (input.texture, original_desc)
                }
                RSF_D3D11_AUXILIARY_COLOUR => {
                    if !self.colour_supported(device, original_desc.format) {
                        return None;
                    }
                    if main {
                        let source_desc = self.texture_info(device, pass.color_input)?;
                        if !self.colour_supported(device, source_desc.format)
                            || !mapping.source_fits(source_desc.into())
                        {
                            return None;
                        }
                        (pass.color_input, source_desc)
                    } else {
                        (input.texture, original_desc)
                    }
                }
                _ => return None,
            };
            if recovery && input.texture == pass.color_output {
                main_begin = index;
            }
            plans[index] = Some(SurfacePlan {
                original: input.texture,
                source,
                original_desc,
                source_desc,
                kind: input.kind,
                flags: input.flags,
                mapping,
            });
        }
        Some((plans, main_begin))
    }

    fn create_texture(
        &self,
        device: *mut c_void,
        desc: &rsf_aux_texture_info,
        usage: u32,
    ) -> Option<ComRef> {
        unsafe { ComRef::from_owned(self.platform.create_texture.unwrap()(device, desc, usage)) }
    }

    fn reserve_backup(
        &mut self,
        device: *mut c_void,
        index: usize,
        original: Texture,
    ) -> Option<()> {
        let desc = self.texture_info(device, original)?;
        let key = desc.into();
        if self.backups[index]
            .as_ref()
            .is_none_or(|backup| backup.key != key)
        {
            self.backups[index] = Some(Snapshot {
                key,
                texture: self.create_texture(device, &desc, RSF_AUX_TEXTURE_BACKUP)?,
            });
        }
        Some(())
    }

    fn reserve_colour(
        &mut self,
        device: *mut c_void,
        index: usize,
        plan: &SurfacePlan,
    ) -> Option<()> {
        let source_key = plan.source_desc.into();
        let destination = plan.original_desc.into();
        if self.colours[index].as_ref().is_some_and(|cached| {
            cached.source.key == source_key && cached.destination == destination
        }) {
            return Some(());
        }
        let snapshot = self.create_texture(device, &plan.source_desc, RSF_AUX_TEXTURE_SOURCE)?;
        let output = self.create_texture(device, &plan.original_desc, RSF_AUX_TEXTURE_OUTPUT)?;
        let view = unsafe {
            ComRef::from_owned(self.platform.create_source_view.unwrap()(
                device,
                snapshot.as_ptr(),
            ))
        }?;
        let target = unsafe {
            ComRef::from_owned(self.platform.create_output_view.unwrap()(
                device,
                output.as_ptr(),
            ))
        }?;
        self.colours[index] = Some(ColourProducts {
            source: Snapshot {
                key: source_key,
                texture: snapshot,
            },
            destination,
            output,
            view,
            target,
        });
        Some(())
    }

    fn reserve(
        &mut self,
        context: *mut c_void,
        pass: &rsf_game_render_pass,
        inputs: &[rsf_d3d11_auxiliary_surface],
        log: rsf_game_log_fn,
        user: *mut c_void,
        recovery: bool,
    ) -> Option<()> {
        if self.frame.is_some() || inputs.is_empty() {
            return None;
        }
        let device = self.bind_owner(context)?;
        let (plans, main_begin) = self.plan_surfaces(device, pass, inputs, recovery)?;
        if !device_ready(&self.platform, device) {
            return None;
        }
        if self.pipeline.is_none() {
            let mut pipeline = rsf_aux_colour_pipeline::default();
            if unsafe { self.platform.create_pipeline.unwrap()(device, log, user, &mut pipeline) }
                == 0
            {
                return None;
            }
            let (Some(vertex), Some(pixel), Some(raster), Some(constants)) = (unsafe {
                (
                    ComRef::from_owned(pipeline.vertex),
                    ComRef::from_owned(pipeline.pixel),
                    ComRef::from_owned(pipeline.raster),
                    ComRef::from_owned(pipeline.constants),
                )
            }) else {
                return None;
            };
            self.pipeline = Some(Pipeline {
                vertex,
                pixel,
                raster,
                constants,
            });
        }

        // Cache private allocations independently of any in-flight native leases.
        let readable = (!pass.color_output_readable.is_null()
            && pass.color_output_readable != pass.color_output)
            .then_some(pass.color_output_readable);
        self.reserve_backup(device, 0, pass.color_output)?;
        if let Some(texture) = readable {
            self.reserve_backup(device, 1, texture)?;
        }
        for (index, plan) in plans[..inputs.len()].iter().flatten().enumerate() {
            if plan.depth() {
                let setup = rsf_depth_scale_setup {
                    struct_size: size_of::<rsf_depth_scale_setup>() as u32,
                    abi_version: RSF_DEPTH_SCALE_ABI_VERSION,
                    log,
                    log_user: user,
                };
                if self.depth.is_null()
                    && unsafe {
                        self.platform.depth_create.unwrap()(context, &setup, &mut self.depth)
                    } != RSF_DEPTH_SCALE_OK as i32
                {
                    return None;
                }
                if unsafe { self.platform.depth_reserve.unwrap()(self.depth, plan.original) } == 0 {
                    return None;
                }
            } else {
                self.reserve_colour(device, index, plan)?;
            }
        }

        // The borrowed originals survive this call only after every allocation has succeeded.
        self.frame = Some(Frame {
            scope: [pass.session_id, pass.native_frame, pass.view_key],
            surfaces: plans.map(|plan| plan.map(Surface::retain)),
            count: inputs.len(),
            main_begin,
            originals: [
                Some(unsafe { ComRef::retain(pass.color_output) }),
                readable.map(|texture| unsafe { ComRef::retain(texture) }),
            ],
            backups_captured: false,
            depth_restore: None,
            published_colours: 0,
            phase: Phase::Reserved,
        });
        Some(())
    }

    fn refine(&mut self, inputs: &[rsf_d3d11_auxiliary_surface]) -> bool {
        if !self.on_owner_thread() {
            return false;
        }
        let Some(frame) = &mut self.frame else {
            return false;
        };
        if frame.phase != Phase::Reserved
            || inputs.len() != frame.count
            || !frame
                .surfaces
                .iter()
                .flatten()
                .zip(inputs)
                .all(|(surface, input)| surface.plan.accepts(input, true))
        {
            return false;
        }
        for (surface, input) in frame.surfaces.iter_mut().flatten().zip(inputs) {
            surface.plan.mapping = Mapping::from(input);
        }
        true
    }

    fn original_colour(&self) -> Texture {
        let Some(frame) = &self.frame else {
            return ptr::null_mut();
        };
        if !self.on_owner_thread()
            || matches!(frame.phase, Phase::Reserved | Phase::Capturing)
            || frame.main_begin == frame.count
        {
            return ptr::null_mut();
        }
        self.colours[frame.main_begin]
            .as_ref()
            .map_or(ptr::null_mut(), |colour| colour.source.texture.0.as_ptr())
    }

    fn render(&mut self, capture: bool, temporal: Option<&[rsf_d3d11_auxiliary_surface]>) -> bool {
        if !self.on_owner_thread() {
            return false;
        }
        let owner = self.owner.as_ref().unwrap();
        let device = owner.device.0.as_ptr();
        let Some(frame) = &mut self.frame else {
            return false;
        };
        if (capture && frame.phase != Phase::Reserved)
            || (!capture && !matches!(frame.phase, Phase::Prepared(_) | Phase::Sealed))
            || temporal.is_some_and(|inputs| {
                inputs.len() != frame.main_begin
                    || !frame
                        .surfaces
                        .iter()
                        .flatten()
                        .zip(inputs)
                        .all(|(surface, input)| surface.plan.accepts(input, false))
            })
            || !device_ready(&self.platform, device)
        {
            return false;
        }

        // An incomplete redraw invalidates the products, but the sealed originals still
        // support a spatial retry. An incomplete capture is only safe to roll back.
        if !capture {
            frame.phase = Phase::Sealed;
        }
        let Some(bindings) = Bindings::new(&self.platform, owner.context.0.as_ptr()) else {
            return false;
        };
        if capture {
            frame.phase = Phase::Capturing;
            for (index, original) in frame.originals.iter().enumerate() {
                if let Some(original) = original {
                    bindings.copy(
                        self.backups[index].as_ref().unwrap().texture.0.as_ptr(),
                        original.0.as_ptr(),
                    );
                }
            }
            frame.backups_captured = true;
        }

        let pipeline = self.pipeline.as_ref().unwrap().borrow();
        let count = temporal.map_or(frame.count, <[_]>::len);
        for (index, surface) in frame.surfaces[..count].iter().flatten().enumerate() {
            let plan = &surface.plan;
            let mapping = temporal.map_or(plan.mapping, |inputs| Mapping::from(&inputs[index]));
            if plan.depth() {
                let depth_frame = rsf_depth_scale_frame {
                    struct_size: size_of::<rsf_depth_scale_frame>() as u32,
                    abi_version: RSF_DEPTH_SCALE_ABI_VERSION,
                    texture: plan.original,
                    source_rect: mapping.source.map(|edge| edge as i32),
                    output_rect: mapping.output,
                    jitter_pixels: mapping.jitter,
                };
                let render_depth = if capture {
                    self.platform.depth_prepare
                } else {
                    self.platform.depth_redraw
                };
                if unsafe { render_depth.unwrap()(self.depth, &depth_frame) }
                    != RSF_DEPTH_SCALE_OK as i32
                {
                    return false;
                }
                frame.depth_restore = Some(plan.restore_contents());
            } else {
                let colour = self.colours[index].as_ref().unwrap();
                let source = colour.source.texture.0.as_ptr();
                if capture {
                    bindings.copy(source, plan.source);
                }
                let mut baseline = source;
                if index >= frame.main_begin {
                    // Main may read a different allocation. Keep the destination's margins.
                    if let Some(backup) = frame.originals.iter().position(|original| {
                        original
                            .as_ref()
                            .is_some_and(|original| original.0.as_ptr() == plan.original)
                    }) {
                        baseline = self.backups[backup].as_ref().unwrap().texture.0.as_ptr();
                    }
                }
                bindings.copy(colour.output.0.as_ptr(), baseline);
                unsafe {
                    self.platform.draw.unwrap()(
                        bindings.context,
                        &pipeline,
                        colour.view.0.as_ptr(),
                        colour.target.0.as_ptr(),
                        &mapping.colour_constants(),
                    )
                };
            }
            bindings.clear();
        }
        if !device_ready(&self.platform, device) {
            return false;
        }
        frame.phase = Phase::Prepared(count);
        true
    }

    fn cancel(&mut self) -> bool {
        if !self.on_owner_thread()
            || self
                .frame
                .as_ref()
                .is_none_or(|frame| frame.phase != Phase::Reserved)
        {
            return false;
        }
        self.frame = None;
        true
    }

    fn commit(&mut self, temporal: bool) -> bool {
        if !self.on_owner_thread() {
            return false;
        }
        let owner = self.owner.as_ref().unwrap();
        let Some(frame) = &mut self.frame else {
            return false;
        };
        let Phase::Prepared(prepared) = frame.phase else {
            return false;
        };
        let count = if temporal {
            frame.main_begin
        } else {
            frame.count
        };
        if count > prepared {
            return false;
        }
        let Some(bindings) = Bindings::new(&self.platform, owner.context.0.as_ptr()) else {
            return false;
        };
        frame.phase = Phase::Publishing;
        if frame.depth_restore.is_some()
            && unsafe { self.platform.depth_commit.unwrap()(self.depth) }
                != RSF_DEPTH_SCALE_OK as i32
        {
            return false;
        }
        for (index, surface) in frame.surfaces[..count].iter().flatten().enumerate() {
            if !surface.plan.depth() {
                bindings.copy(
                    surface.plan.original,
                    self.colours[index].as_ref().unwrap().output.0.as_ptr(),
                );
            }
        }
        frame.published_colours = count;
        if !device_ready(&self.platform, owner.device.0.as_ptr()) {
            return false;
        }
        frame.phase = Phase::Accepted;
        frame.originals = [None, None];
        true
    }

    fn restore(&mut self) -> bool {
        let Some(frame) = &mut self.frame else {
            return true;
        };
        if frame.phase == Phase::Reserved {
            return self.cancel();
        }
        let owner = self.owner.as_ref().unwrap();
        if owner.thread != current_thread() {
            return false;
        }
        let Some(bindings) = Bindings::new(&self.platform, owner.context.0.as_ptr()) else {
            return false;
        };
        let accepted = frame.phase == Phase::Accepted;
        if let Some(restore) = frame.depth_restore {
            let depth_release = if accepted && !restore {
                self.platform.depth_discard
            } else {
                self.platform.depth_restore
            };
            if unsafe { depth_release.unwrap()(self.depth) } != RSF_DEPTH_SCALE_OK as i32 {
                return false;
            }
            frame.depth_restore = None;
        }
        for (index, surface) in frame.surfaces[..frame.published_colours.min(frame.main_begin)]
            .iter()
            .flatten()
            .enumerate()
        {
            if !surface.plan.depth() && (!accepted || surface.plan.restore_contents()) {
                bindings.copy(
                    surface.plan.original,
                    self.colours[index]
                        .as_ref()
                        .unwrap()
                        .source
                        .texture
                        .0
                        .as_ptr(),
                );
            }
        }
        if frame.backups_captured {
            for (index, original) in frame.originals.iter().enumerate() {
                if let Some(original) = original {
                    bindings.copy(
                        original.0.as_ptr(),
                        self.backups[index].as_ref().unwrap().texture.0.as_ptr(),
                    );
                }
            }
        }
        if !device_ready(&self.platform, owner.device.0.as_ptr()) {
            return false;
        }
        // Release frame references after the game's bindings are back.
        // Leaving this scope also restores them on every failure.
        drop(bindings);
        self.frame = None;
        true
    }
}

impl crate::d3d11_host_reduced::Products for AuxiliarySr {
    fn pending(&self) -> bool {
        self.frame.is_some()
    }

    fn matches(&self, scope: [u64; 3]) -> bool {
        self.on_owner_thread()
            && self
                .frame
                .as_ref()
                .is_some_and(|frame| frame.scope == scope)
    }

    fn reserve(
        &mut self,
        context: *mut c_void,
        pass: &rsf_game_render_pass,
        inputs: &[rsf_d3d11_auxiliary_surface],
        log: rsf_game_log_fn,
        user: *mut c_void,
        recovery: bool,
    ) -> bool {
        let limit = if recovery {
            MAX_SURFACES
        } else {
            MAX_AUXILIARIES
        };
        inputs.len() <= limit
            && Self::reserve(self, context, pass, inputs, log, user, recovery).is_some()
    }

    fn refine(&mut self, inputs: &[rsf_d3d11_auxiliary_surface]) -> bool {
        inputs.len() <= MAX_SURFACES && Self::refine(self, inputs)
    }

    fn seal(&mut self) -> bool {
        self.render(true, None)
    }

    fn cancel(&mut self) -> bool {
        Self::cancel(self)
    }

    fn original_colour(&self) -> Texture {
        Self::original_colour(self)
    }

    fn temporal_prepare(&mut self, inputs: &[rsf_d3d11_auxiliary_surface]) -> bool {
        inputs.len() <= MAX_SURFACES && self.render(false, Some(inputs))
    }

    fn spatial_prepare(&mut self) -> bool {
        self.render(false, None)
    }

    fn commit(&mut self, temporal: bool) -> bool {
        Self::commit(self, temporal)
    }

    fn restore(&mut self) -> bool {
        Self::restore(self)
    }
}

impl Drop for AuxiliarySr {
    fn drop(&mut self) {
        let _ = self.restore();
        if !self.depth.is_null() {
            unsafe { self.platform.depth_destroy.unwrap()(self.depth) };
        }
    }
}

fn surface_size_valid(input: &rsf_d3d11_auxiliary_surface) -> bool {
    input.struct_size >= size_of::<rsf_d3d11_auxiliary_surface>() as u32
}

unsafe fn borrowed_surfaces<'a>(
    count: u32,
    inputs: *const rsf_d3d11_auxiliary_surface,
    limit: usize,
) -> Option<&'a [rsf_d3d11_auxiliary_surface]> {
    let count = count as usize;
    if inputs.is_null() || count > limit {
        return None;
    }
    Some(unsafe { slice::from_raw_parts(inputs, count) })
}

// Private C++ facade calls serialize access, pass live borrowed interfaces and
// supply the platform callbacks. The owner checks thread and frame state.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_create(
    platform: *const rsf_aux_platform,
) -> *mut AuxiliarySr {
    let Some(platform) = (unsafe { platform.as_ref() }) else {
        return ptr::null_mut();
    };
    crate::allocate_handle(AuxiliarySr::new(*platform))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_destroy(handle: *mut AuxiliarySr) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_pending(handle: *const AuxiliarySr) -> i32 {
    unsafe { handle.as_ref() }.is_some_and(|state| state.frame.is_some()) as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_matches(
    handle: *const AuxiliarySr,
    session: u64,
    frame: u64,
    view: u64,
) -> i32 {
    unsafe { handle.as_ref() }.is_some_and(|state| {
        state.on_owner_thread()
            && state
                .frame
                .as_ref()
                .is_some_and(|f| f.scope == [session, frame, view])
    }) as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_reserve(
    handle: *mut AuxiliarySr,
    context: *mut c_void,
    pass: *const rsf_game_render_pass,
    count: u32,
    inputs: *const rsf_d3d11_auxiliary_surface,
    log: rsf_game_log_fn,
    user: *mut c_void,
    recovery: u32,
) -> i32 {
    let (Some(state), Some(pass)) = (unsafe { handle.as_mut() }, unsafe { pass.as_ref() }) else {
        return 0;
    };
    let limit = if recovery != 0 {
        MAX_SURFACES
    } else {
        MAX_AUXILIARIES
    };
    let Some(inputs) = (unsafe { borrowed_surfaces(count, inputs, limit) }) else {
        return 0;
    };
    state
        .reserve(context, pass, inputs, log, user, recovery != 0)
        .is_some() as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_refine(
    handle: *mut AuxiliarySr,
    count: u32,
    inputs: *const rsf_d3d11_auxiliary_surface,
) -> i32 {
    unsafe { handle.as_mut() }.is_some_and(|state| {
        unsafe { borrowed_surfaces(count, inputs, MAX_SURFACES) }
            .is_some_and(|inputs| state.refine(inputs))
    }) as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_seal(handle: *mut AuxiliarySr) -> i32 {
    unsafe { handle.as_mut() }.is_some_and(|state| state.render(true, None)) as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_cancel(handle: *mut AuxiliarySr) -> i32 {
    unsafe { handle.as_mut() }.is_some_and(AuxiliarySr::cancel) as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_original_colour(handle: *const AuxiliarySr) -> Texture {
    unsafe { handle.as_ref() }.map_or(ptr::null_mut(), AuxiliarySr::original_colour)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_temporal_prepare(
    handle: *mut AuxiliarySr,
    count: u32,
    inputs: *const rsf_d3d11_auxiliary_surface,
) -> i32 {
    unsafe { handle.as_mut() }.is_some_and(|state| {
        unsafe { borrowed_surfaces(count, inputs, MAX_SURFACES) }
            .is_some_and(|inputs| state.render(false, Some(inputs)))
    }) as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_spatial_prepare(handle: *mut AuxiliarySr) -> i32 {
    unsafe { handle.as_mut() }.is_some_and(|state| state.render(false, None)) as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_commit(handle: *mut AuxiliarySr, temporal: u32) -> i32 {
    unsafe { handle.as_mut() }.is_some_and(|state| state.commit(temporal != 0)) as i32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_auxiliary_sr_restore(handle: *mut AuxiliarySr) -> i32 {
    unsafe { handle.as_mut() }.is_none_or(AuxiliarySr::restore) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{Fake, add_ref};

    #[repr(C)]
    struct FakeCom {
        com: Fake,
        width: u32,
    }

    impl FakeCom {
        fn new(width: u32) -> Self {
            Self {
                com: Fake::new(),
                width,
            }
        }

        fn raw(&self) -> *mut c_void {
            ptr::from_ref(self).cast_mut().cast()
        }
    }

    unsafe extern "C" fn retain(object: *mut c_void) -> *mut c_void {
        unsafe { add_ref(object.cast()) };
        object
    }

    unsafe extern "C" fn ready(_: *mut c_void) -> i32 {
        1
    }

    unsafe extern "C" fn describe(
        raw: Texture,
        _: *mut c_void,
        out: *mut rsf_aux_texture_facts,
    ) -> i32 {
        let texture = unsafe { &*raw.cast::<FakeCom>() };
        if texture.width == 0 {
            return 0;
        }
        unsafe {
            out.write(rsf_aux_texture_facts {
                info: rsf_aux_texture_info {
                    width: texture.width,
                    height: texture.width,
                    format: RSF_AUX_FORMAT_RGBA8_UNORM,
                    ..Default::default()
                },
                same_device: 1,
                array_size: 1,
                mip_levels: 1,
                sample_count: 1,
                usage: RSF_AUX_USAGE_DEFAULT,
            })
        };
        1
    }

    unsafe extern "C" fn create_texture(
        device: *mut c_void,
        _: *const rsf_aux_texture_info,
        _: u32,
    ) -> Texture {
        unsafe { retain(device) }
    }

    unsafe extern "C" fn create_view(device: *mut c_void, _: Texture) -> *mut c_void {
        unsafe { retain(device) }
    }

    #[test]
    fn failed_readable_backup_leaves_no_native_surface_leases() {
        // Reuse the fake context interface as the device and every private resource.
        // Only COM reference counts and the reservation logic run in this test.
        let context = FakeCom::new(0);
        let main = FakeCom::new(8);
        let unreadable = FakeCom::new(0);
        let auxiliary = FakeCom::new(8);
        let platform = rsf_aux_platform {
            immediate_device: Some(retain),
            device_ready: Some(ready),
            describe_texture: Some(describe),
            create_texture: Some(create_texture),
            create_source_view: Some(create_view),
            create_output_view: Some(create_view),
            ..Default::default()
        };
        let mut owner = AuxiliarySr::new(platform);
        owner.pipeline = Some(Pipeline {
            vertex: unsafe { ComRef::retain(context.raw()) },
            pixel: unsafe { ComRef::retain(context.raw()) },
            raster: unsafe { ComRef::retain(context.raw()) },
            constants: unsafe { ComRef::retain(context.raw()) },
        });
        // All fields in the generated pass are numeric values or raw pointers.
        let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        pass.color_output = main.raw();
        pass.color_output_readable = unreadable.raw();
        let inputs = [rsf_d3d11_auxiliary_surface {
            struct_size: size_of::<rsf_d3d11_auxiliary_surface>() as u32,
            kind: RSF_D3D11_AUXILIARY_COLOUR,
            texture: auxiliary.raw(),
            source_rect: [0.0, 0.0, 4.0, 4.0],
            output_rect: [0, 0, 8, 8],
            ..Default::default()
        }];

        assert!(
            owner
                .reserve(context.raw(), &pass, &inputs, None, ptr::null_mut(), false)
                .is_none()
        );
        assert!(owner.backups[0].is_some());
        assert!(owner.frame.is_none());
        assert_eq!(main.com.refs.get(), 1);
        assert_eq!(unreadable.com.refs.get(), 1);
        assert_eq!(auxiliary.com.refs.get(), 1);

        pass.color_output_readable = ptr::null_mut();
        assert!(
            owner
                .reserve(context.raw(), &pass, &inputs, None, ptr::null_mut(), false)
                .is_some()
        );
        assert_eq!(main.com.refs.get(), 2);
        assert_eq!(auxiliary.com.refs.get(), 2);
        assert!(owner.cancel());
        assert_eq!(main.com.refs.get(), 1);
        assert_eq!(auxiliary.com.refs.get(), 1);
        drop(owner);
        assert_eq!(context.com.refs.get(), 1);
    }
}
