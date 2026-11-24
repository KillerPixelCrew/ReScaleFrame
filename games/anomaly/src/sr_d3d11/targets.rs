// SPDX-License-Identifier: MIT
use super::{DEPTH_SRV, DSV, Extent, Ptr, RTV, SRV, Texture, UAV, View, d3d, lease, pointer};
use crate::lease::Lease;
use crate::sys::{
    RSF_ANOMALY_SWAP_MAKE_DEPTH, RSF_ANOMALY_SWAP_MAKE_FAILED, RSF_ANOMALY_SWAP_MAKE_REFUSED,
    RSF_ANOMALY_SWAP_SLOT_COUNT, rsf_anomaly_swap_fields as Fields,
    rsf_anomaly_swap_request as Request, rsf_anomaly_swap_twins as Twins,
};
use std::ffi::c_void;
use std::ptr::null;

const UNKNOWN: u32 = 0;
const RGBA16F: u32 = 10;
const R32_FLOAT: u32 = 41;
const R16_FLOAT: u32 = 54;
const R24G8_TYPELESS: u32 = 44;
const D24_UNORM_S8_UINT: u32 = 45;
const UNORM8: [u32; 4] = [27, 28, 87, 90];
const D3DFMT_A16B16G16R16F: u32 = 113;
const DEFAULT: u32 = 0;
const IMMUTABLE: u32 = 1;
const BIND_SRV: u32 = 0x08;
const BIND_RTV: u32 = 0x20;
const BIND_DSV: u32 = 0x40;
const BIND_UAV: u32 = 0x80;
const COLOUR_BINDINGS: u32 = BIND_RTV | BIND_SRV;
const PROMOTED_SUPPORT: u32 = 0x20 | 0x100 | 0x200 | 0x4000 | 0x8000;
const SUPPORT_UAV: u32 = 0x0200_0000;

#[repr(u8)]
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum Error {
    #[default]
    None,
    NotTexture,
    Shape,
    Format,
    Support,
    Create,
    Depth,
}

// The names the stage log uses for TwinBatchInfo::cause.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_twin_error_name(cause: u8) -> *const std::ffi::c_char {
    const NAMES: [&std::ffi::CStr; 7] = [
        c"none",
        c"not_texture",
        c"shape",
        c"format",
        c"support",
        c"create",
        c"depth",
    ];
    NAMES
        .get(usize::from(cause))
        .copied()
        .unwrap_or(c"unknown")
        .as_ptr()
}

#[repr(C)]
pub(super) struct TwinInput {
    pub(super) texture: Ptr,
    pub(super) rtv: Ptr,
    pub(super) srv: Ptr,
    pub(super) uav: Ptr,
    pub(super) size: Extent,
    pub(super) format: u32,
    pub(super) promote: u32,
    pub(super) with_uav: u32,
    pub(super) world: u32,
}

pub(super) struct Target {
    rtv: Option<Lease>,
    srv: Option<Lease>,
    uav: Option<Lease>,
    dsv: Option<Lease>,
    texture: Lease,
}

#[repr(C)]
pub(super) struct TargetView {
    pub(super) texture: Ptr,
    pub(super) rtv: Ptr,
    pub(super) srv: Ptr,
    pub(super) uav: Ptr,
    pub(super) dsv: Ptr,
    pub(super) size: Extent,
    pub(super) format: u32,
    pub(super) descriptor: Texture,
}

impl TargetView {
    pub(super) fn fields(&self) -> Fields {
        Fields {
            surface: self.texture,
            rtv: self.rtv,
            uav: self.uav,
            width: self.size.width,
            height: self.size.height,
            format: self.format,
            texture_surface: self.texture,
            desc_cache: self.texture,
            // Texture mirrors the initialized 44-byte D3D11_TEXTURE2D_DESC.
            desc: unsafe { std::mem::transmute::<Texture, [u8; 44]>(self.descriptor) },
            srv: self.srv,
            ..Fields::default()
        }
    }
}

impl Target {
    fn new(texture: Lease) -> Self {
        Self {
            texture,
            rtv: None,
            srv: None,
            uav: None,
            dsv: None,
        }
    }

    pub(super) fn view(&self, descriptor: Texture, format: u32) -> TargetView {
        TargetView {
            texture: self.texture.as_ptr(),
            rtv: pointer(&self.rtv),
            srv: pointer(&self.srv),
            uav: pointer(&self.uav),
            dsv: pointer(&self.dsv),
            size: descriptor.size,
            format,
            descriptor,
        }
    }
}

impl Texture {
    fn target(size: Extent, format: u32, bindings: u32) -> Self {
        Self {
            size,
            format,
            bindings,
            mips: 1,
            layers: 1,
            samples: 1,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Device {
    pub(super) handle: Ptr,
}

impl Device {
    fn texture(self, desc: &Texture, data: *const c_void, pitch: u32) -> Result<Lease, Error> {
        let reference = unsafe { d3d::texture(self.handle, desc, data, pitch) };
        lease(reference).ok_or(Error::Create)
    }

    fn view(self, texture: &Lease, kind: u32, template: Option<&View>) -> Result<Lease, Error> {
        let reference =
            unsafe { d3d::view(self.handle, texture.as_ptr(), kind, template) };
        lease(reference).ok_or(Error::Create)
    }

    fn describe(self, texture: &Lease) -> Texture {
        let mut desc = Texture::default();
        unsafe { d3d::describe_texture(texture.as_ptr(), &mut desc) };
        desc
    }

    fn view_info(self, view: Ptr, kind: u32) -> View {
        let mut desc = View::default();
        unsafe { d3d::describe_view(view, kind, &mut desc) };
        desc
    }

    fn query_texture(self, resource: Ptr) -> Option<Lease> {
        if resource.is_null() {
            return None;
        }
        lease(unsafe { d3d::as_texture(resource) })
    }

    pub(super) fn twin(self, input: &TwinInput) -> Result<(Target, Texture, u32), Error> {
        if input.world != 0 && !input.size.valid() {
            return Err(Error::Shape);
        }
        if self.handle.is_null() {
            return Err(Error::NotTexture);
        }
        let source = self.query_texture(input.texture).ok_or(Error::NotTexture)?;
        let mut desc = self.describe(&source);
        if !desc.size.valid()
            || desc.mips != 1
            || desc.layers != 1
            || desc.samples != 1
            || desc.usage != DEFAULT
            || desc.bindings & COLOUR_BINDINGS != COLOUR_BINDINGS
        {
            return Err(Error::Shape);
        }
        let promote = input.promote != 0;
        let with_uav = input.with_uav != 0;
        if promote {
            if !UNORM8.contains(&desc.format) {
                return Err(Error::Format);
            }
            let support = unsafe { d3d::format_support(self.handle, RGBA16F) };
            let needed = PROMOTED_SUPPORT | if with_uav { SUPPORT_UAV } else { 0 };
            if support & needed != needed {
                return Err(Error::Support);
            }
            desc.format = RGBA16F;
        }

        let template =
            |view: Ptr, kind| (!promote && !view.is_null()).then(|| self.view_info(view, kind));
        let rtv = template(input.rtv, RTV);
        let srv = template(input.srv, SRV);
        let uav = template(input.uav, UAV);
        if [&rtv, &srv, &uav]
            .into_iter()
            .flatten()
            .any(|view| view.texture2d == 0)
        {
            return Err(Error::Shape);
        }

        if input.world != 0 {
            desc.size = input.size;
        }
        desc.bindings = COLOUR_BINDINGS | if with_uav { BIND_UAV } else { 0 };
        desc.cpu_access = 0;
        desc.misc = 0;
        let mut twin = Target::new(self.texture(&desc, null(), 0)?);
        twin.rtv = Some(self.view(&twin.texture, RTV, rtv.as_ref())?);
        twin.srv = Some(self.view(&twin.texture, SRV, srv.as_ref())?);
        if with_uav {
            twin.uav = Some(self.view(&twin.texture, UAV, uav.as_ref())?);
        }
        let created = self.describe(&twin.texture);
        let d3d_format = if promote {
            D3DFMT_A16B16G16R16F
        } else {
            input.format
        };
        Ok((twin, created, d3d_format))
    }

    pub(super) fn depth(
        self,
        engine: Ptr,
        output: Extent,
        render: Extent,
    ) -> Result<(Target, Texture), Error> {
        if self.handle.is_null() || engine.is_null() || !render.valid() {
            return Err(Error::NotTexture);
        }
        let view = self.view_info(engine, DSV);
        if view.format != D24_UNORM_S8_UINT || view.texture2d == 0 || view.flags != 0 {
            return Err(Error::Depth);
        }
        let resource = lease(unsafe { d3d::view_resource(engine) })
            .ok_or(Error::Depth)?;
        let texture = self.query_texture(resource.as_ptr()).ok_or(Error::Depth)?;
        let desc = self.describe(&texture);
        if desc.size != output || desc.samples != 1 || desc.layers != 1 {
            return Err(Error::Depth);
        }
        let desc = Texture::target(render, R24G8_TYPELESS, BIND_DSV | BIND_SRV);
        let mut twin = Target::new(self.texture(&desc, null(), 0)?);
        twin.dsv = Some(self.view(&twin.texture, DSV, None)?);
        twin.srv = Some(self.view(&twin.texture, DEPTH_SRV, None)?);
        Ok((twin, desc))
    }

    pub(super) fn scratch(self, engine: Ptr, render: Extent) -> Result<(Target, Texture), Error> {
        if self.handle.is_null() || engine.is_null() || !render.valid() {
            return Err(Error::NotTexture);
        }
        let view = self.view_info(engine, RTV);
        if view.texture2d == 0 || view.format == UNKNOWN {
            return Err(Error::Shape);
        }
        let desc = Texture::target(render, view.format, BIND_RTV);
        let mut scratch = Target::new(self.texture(&desc, null(), 0)?);
        scratch.rtv = Some(self.view(&scratch.texture, RTV, None)?);
        Ok((scratch, desc))
    }
}

const TWIN_SLOTS: usize = RSF_ANOMALY_SWAP_SLOT_COUNT as usize;

pub(super) struct TwinBatch {
    slots: [Option<Target>; TWIN_SLOTS],
    depth: Option<Target>,
    _scratch: Option<Target>,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct TwinBatchInfo {
    pub(crate) bytes: u64,
    pub(crate) made: u32,
    pub(crate) failed_index: u32,
    pub(crate) cause: Error,
    pub(crate) pass_refused: bool,
}

const _: () = {
    assert!(size_of::<TwinBatchInfo>() == 24);
    assert!(std::mem::offset_of!(TwinBatchInfo, cause) == 16);
};

impl TwinBatchInfo {
    fn refuse(&mut self, index: usize, error: Error) -> u32 {
        self.failed_index = index as u32;
        let result = match error {
            Error::Create => RSF_ANOMALY_SWAP_MAKE_FAILED,
            Error::Depth => RSF_ANOMALY_SWAP_MAKE_DEPTH,
            _ => RSF_ANOMALY_SWAP_MAKE_REFUSED,
        };
        self.cause = error;
        result
    }
}

fn bytes_of(description: Texture) -> u64 {
    let texel: u64 = match description.format {
        RGBA16F => 8,
        R16_FLOAT => 2,
        _ => 4,
    };
    (u64::from(description.size.width) * u64::from(description.size.height)).saturating_mul(texel)
}

pub(super) fn allocate_twins(
    handle: Ptr,
    request: &Request,
    out: &mut Twins,
    info: &mut TwinBatchInfo,
) -> Result<TwinBatch, u32> {
    let device = Device { handle };
    let mut batch = TwinBatch {
        slots: [const { None }; TWIN_SLOTS],
        depth: None,
        _scratch: None,
    };
    let mut views = Twins::default();
    let mut bytes = 0u64;
    let mut made = 0;
    let world = request.stage == 2;
    for (index, spec) in request.spec.iter().enumerate() {
        if !spec.present {
            continue;
        }
        let engine = request.engine[index].fields;
        let input = TwinInput {
            texture: engine.surface,
            rtv: engine.rtv,
            srv: engine.srv,
            uav: engine.uav,
            size: Extent {
                width: spec.width,
                height: spec.height,
            },
            format: engine.format,
            promote: u32::from(!world || spec.promote),
            with_uav: u32::from(!engine.uav.is_null()),
            world: u32::from(world),
        };
        let (twin, description, format) = match device.twin(&input) {
            Ok(twin) => twin,
            Err(error) => return Err(info.refuse(index, error)),
        };
        views.targets[index] = twin.view(description, format).fields();
        batch.slots[index] = Some(twin);
        bytes = bytes.saturating_add(bytes_of(description));
        made += 1;
    }
    if world {
        let render = Extent {
            width: request.render_width,
            height: request.render_height,
        };
        let output = Extent {
            width: request.output_width,
            height: request.output_height,
        };
        let (depth, depth_desc) = match device.depth(request.engine_dsv, output, render) {
            Ok(depth) => depth,
            Err(error) => return Err(info.refuse(TWIN_SLOTS, error)),
        };
        let (scratch, scratch_desc) = match device.scratch(request.engine_rtv, render) {
            Ok(scratch) => scratch,
            Err(error) => return Err(info.refuse(TWIN_SLOTS + 1, error)),
        };
        views.depth_dsv = pointer(&depth.dsv);
        views.depth_srv = pointer(&depth.srv);
        views.depth_width = depth_desc.size.width;
        views.depth_height = depth_desc.size.height;
        views.base_rt = pointer(&scratch.rtv);
        batch.depth = Some(depth);
        batch._scratch = Some(scratch);
        bytes = bytes
            .saturating_add(bytes_of(depth_desc))
            .saturating_add(bytes_of(scratch_desc));
    }
    *out = views;
    info.bytes = bytes;
    info.made = made;
    Ok(batch)
}

impl TwinBatch {
    pub(super) fn depth_texture(&self) -> Ptr {
        self.depth
            .as_ref()
            .map_or(std::ptr::null_mut(), |depth| depth.texture.as_ptr())
    }

    pub(super) fn retire(mut self, refs: unsafe extern "C" fn(Ptr) -> u32) -> u64 {
        self.slots
            .iter_mut()
            .flatten()
            .map(|twin| {
                twin.rtv = None;
                twin.srv = None;
                twin.uav = None;
                u64::from(unsafe { refs(twin.texture.as_ptr()) })
            })
            .sum()
    }
}

#[cfg(test)]
extern "C" fn rsf_anomaly_sr_drop_target(owner: *mut Target) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[cfg(test)]
fn rsf_anomaly_sr_release_twin_view(owner: &mut Target, kind: u32) {
    match kind {
        RTV => owner.rtv = None,
        SRV => owner.srv = None,
        UAV => owner.uav = None,
        _ => {}
    }
}

#[repr(C)]
pub(crate) struct TemporalView {
    pub(crate) colour: Ptr,
    pub(crate) motion: Ptr,
    pub(crate) output: Ptr,
    pub(crate) exposure: Ptr,
    empty_motion: Ptr,
    pub(crate) colour_rtv: Ptr,
    pub(crate) motion_rtv: Ptr,
    colour_srv: Ptr,
    motion_srv: Ptr,
    pub(crate) output_srv: Ptr,
    pub(crate) empty_motion_srv: Ptr,
}

const _: () = {
    assert!(size_of::<TwinInput>() == 56);
    assert!(size_of::<TargetView>() == 96);
    assert!(size_of::<TemporalView>() == 88);
};

pub(crate) struct TemporalTargets {
    // The owned textures keep the creating device alive.
    device: Ptr,
    render: Extent,
    output_size: Extent,
    output_format: u32,
    colour_rtv: Lease,
    motion_rtv: Lease,
    colour_srv: Lease,
    motion_srv: Lease,
    output_srv: Lease,
    empty_motion_srv: Lease,
    colour: Lease,
    motion: Lease,
    output: Lease,
    exposure: Lease,
    empty_motion: Lease,
}

// These free-threaded D3D11 references move only under the stage lock.
unsafe impl Send for TemporalTargets {}

fn zero_layout(render: Extent) -> Result<(u32, usize), Error> {
    let pitch = render.width.checked_mul(8).ok_or(Error::Create)?;
    let words = (pitch as usize / size_of::<u16>())
        .checked_mul(render.height as usize)
        .ok_or(Error::Create)?;
    Ok((pitch, words))
}

impl TemporalTargets {
    pub(crate) fn new(
        handle: Ptr,
        render: Extent,
        output_size: Extent,
        output_format: u32,
    ) -> Result<Self, Error> {
        if handle.is_null() || !render.fits(output_size) || output_format == UNKNOWN {
            return Err(Error::Shape);
        }
        let (pitch, words) = zero_layout(render)?;
        let device = Device { handle };
        let mut desc = Texture::target(render, RGBA16F, COLOUR_BINDINGS);
        let colour = device.texture(&desc, null(), 0)?;
        let motion = device.texture(&desc, null(), 0)?;
        let colour_rtv = device.view(&colour, RTV, None)?;
        let motion_rtv = device.view(&motion, RTV, None)?;
        let colour_srv = device.view(&colour, SRV, None)?;
        let motion_srv = device.view(&motion, SRV, None)?;

        // An unset validity channel asks the Runtime to reconstruct camera motion.
        let mut zeros = Vec::<u16>::new();
        zeros.try_reserve_exact(words).map_err(|_| Error::Create)?;
        zeros.resize(words, 0);
        desc.usage = IMMUTABLE;
        desc.bindings = BIND_SRV;
        let empty_motion = device.texture(&desc, zeros.as_ptr().cast(), pitch)?;
        let empty_motion_srv = device.view(&empty_motion, SRV, None)?;

        let desc = Texture::target(output_size, output_format, BIND_SRV);
        let output = device.texture(&desc, null(), 0)?;
        let output_srv = device.view(&output, SRV, None)?;
        let desc = Texture::target(
            Extent {
                width: 1,
                height: 1,
            },
            R32_FLOAT,
            BIND_SRV,
        );
        let exposure = device.texture(
            &desc,
            (&1.0f32 as *const f32).cast(),
            size_of::<f32>() as u32,
        )?;
        Ok(Self {
            device: handle,
            render,
            output_size,
            output_format,
            colour_rtv,
            motion_rtv,
            colour_srv,
            motion_srv,
            output_srv,
            empty_motion_srv,
            colour,
            motion,
            output,
            exposure,
            empty_motion,
        })
    }

    pub(crate) fn view(&self) -> TemporalView {
        TemporalView {
            colour: self.colour.as_ptr(),
            motion: self.motion.as_ptr(),
            output: self.output.as_ptr(),
            exposure: self.exposure.as_ptr(),
            empty_motion: self.empty_motion.as_ptr(),
            colour_rtv: self.colour_rtv.as_ptr(),
            motion_rtv: self.motion_rtv.as_ptr(),
            colour_srv: self.colour_srv.as_ptr(),
            motion_srv: self.motion_srv.as_ptr(),
            output_srv: self.output_srv.as_ptr(),
            empty_motion_srv: self.empty_motion_srv.as_ptr(),
        }
    }

    pub(crate) fn matches(&self, device: Ptr, render: Extent, output: Extent, format: u32) -> bool {
        self.device == device
            && self.render == render
            && self.output_size == output
            && self.output_format == format
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
