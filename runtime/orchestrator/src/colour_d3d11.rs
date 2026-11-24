// SPDX-License-Identifier: MIT
use crate::com_ref;
use crate::d3d11_output::{SavedBindings, same_device, texture_desc};
use crate::d3d11_output_platform::srv_format;
use crate::d3d11_output_platform::{
    rsf_d3d11_output_clear_state, rsf_d3d11_output_copy_region, rsf_d3d11_output_copy_resource,
    rsf_d3d11_output_create_texture, rsf_d3d11_output_is_immediate,
};
use crate::graphics_d3d11::{
    GROUP_EDGE, Handle, SourceViews, bind_compute_parameters, compute_shader, constant_buffer,
    dispatch_compute, prepare_compute, rsf_d3d11_state_restore, rsf_d3d11_state_save, shader_code,
    source_view, target_view,
};
use crate::sys::{
    RSF_D3D11_BIND_SHADER_RESOURCE, RSF_D3D11_FORMAT_R32_FLOAT, RSF_D3D11_FORMAT_RGBA16_FLOAT,
    RSF_D3D11_FORMAT_RGBA32_FLOAT, RSF_D3D11_USAGE_IMMUTABLE,
    rsf_d3d11_texture_desc as TextureDesc,
};
use std::ffi::CStr;
use std::ptr::{self, null_mut};
use std::sync::Mutex;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCOMPILE_ENABLE_STRICTNESS;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_FLOAT32_MAX,
    D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, ID3D11Buffer, ID3D11ComputeShader,
    ID3D11Device, ID3D11DeviceContext, ID3D11SamplerState, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;
use windows::core::IUnknown;
use windows::core::Interface;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

const BIND_UNORDERED_ACCESS: u32 = 0x80;
type Extent = [u32; 2];

const TRANSPORT_SHADER: &[u8] = include_bytes!("../../graphics/shaders/colour_transport.hlsl");
const FIDELITY_SHADER: &[u8] = include_bytes!("../../graphics/shaders/colour_fidelity.hlsl");

fn clamp_sampler(device: &ID3D11Device, mip_linear: bool) -> Option<ID3D11SamplerState> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: if mip_linear {
            D3D11_FILTER_MIN_MAG_MIP_LINEAR
        } else {
            D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT
        },
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MaxLOD: if mip_linear { D3D11_FLOAT32_MAX } else { 0.0 },
        ..Default::default()
    };
    let mut sampler = None;
    unsafe { device.CreateSamplerState(&desc, Some(&mut sampler)) }.ok()?;
    sampler
}

struct Program {
    shaders: [ID3D11ComputeShader; 2],
    constants: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    device: IUnknown,
}

impl Program {
    fn new<T>(
        device: Handle,
        source: &[u8],
        name: &CStr,
        entries: [&CStr; 2],
        mip_linear: bool,
    ) -> Option<Self> {
        let device = unsafe { com_ref::retain(device) };
        let raw = device.as_raw();
        let native = unsafe { ID3D11Device::from_raw_borrowed(&raw) }?;
        let compile = |entry: &CStr| {
            let code =
                shader_code(source, name, entry, c"cs_5_0", D3DCOMPILE_ENABLE_STRICTNESS).ok()?;
            compute_shader(native, &code)
        };
        let shaders = [compile(entries[0])?, compile(entries[1])?];
        let constants = constant_buffer(native, size_of::<T>() as u32, false)?;
        let sampler = clamp_sampler(native, mip_linear)?;
        Some(Self {
            shaders,
            constants,
            sampler,
            device,
        })
    }

    fn same_device(&self, child: Handle) -> bool {
        unsafe { same_device(self.device.as_raw(), child) }
    }

    fn accepts_context(&self, context: Handle) -> bool {
        (unsafe { rsf_d3d11_output_is_immediate(context) != 0 }) && self.same_device(context)
    }

    fn describe(&self, texture: Handle) -> Option<TextureDesc> {
        self.same_device(texture)
            .then(|| unsafe { texture_desc(texture) })
    }

    fn source(
        &self,
        cache: &mut SourceViews<8>,
        texture: Handle,
        desc: &TextureDesc,
    ) -> Option<IUnknown> {
        let format = srv_format(DXGI_FORMAT(desc.format as i32)).0 as u32;
        // A batch can touch several cache entries at the same clock tick.
        // Retain each selected view so a later lookup cannot evict it mid-batch.
        unsafe {
            cache.get(
                self.device.as_raw(),
                texture,
                format,
                GetTickCount64(),
                source_view,
            )
        }
        .cloned()
    }

    fn save(&self, context: Handle) -> Option<SavedBindings> {
        unsafe { SavedBindings::new(context, rsf_d3d11_state_save, rsf_d3d11_state_restore) }
    }

    fn bind<T>(&self, context: Handle, constants: &T) {
        let context =
            unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.expect("live context");
        unsafe {
            context.UpdateSubresource(
                &self.constants,
                0,
                None,
                ptr::from_ref(constants).cast(),
                0,
                0,
            )
        };
        bind_compute_parameters(context, self.constants.as_raw(), self.sampler.as_raw());
    }

    fn dispatch<const N: usize>(
        &self,
        context: Handle,
        shader: &ID3D11ComputeShader,
        sources: &[Handle; N],
        target: &Target,
        [width, height]: Extent,
        clear_sources: bool,
    ) {
        const { assert!(N <= 4) }
        let context =
            unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.expect("live context");
        dispatch_compute(
            context,
            shader,
            sources,
            &[target.view.as_raw()],
            None,
            [width.div_ceil(GROUP_EDGE), height.div_ceil(GROUP_EDGE)],
            clear_sources,
            None,
        );
    }
}

fn extent(desc: &TextureDesc) -> Extent {
    [desc.width, desc.height]
}

fn covers(desc: &TextureDesc, [width, height]: Extent) -> bool {
    width != 0
        && height != 0
        && desc.width >= width
        && desc.height >= height
        && desc.samples == 1
        && desc.layers == 1
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TargetSpec {
    size: Extent,
    format: u32,
    sample_quality: u32,
}

struct Target {
    view: IUnknown,
    texture: IUnknown,
}

impl Target {
    fn new(program: &Program, spec: TargetSpec, view_format: u32) -> Option<Self> {
        let device = program.device.as_raw();
        let desc = TextureDesc {
            width: spec.size[0],
            height: spec.size[1],
            mips: 1,
            layers: 1,
            format: spec.format,
            samples: 1,
            sample_quality: spec.sample_quality,
            bind_flags: RSF_D3D11_BIND_SHADER_RESOURCE | BIND_UNORDERED_ACCESS,
            ..Default::default()
        };
        let texture = unsafe { com_ref::owned(rsf_d3d11_output_create_texture(device, &desc)) }?;
        let raw = texture.as_raw();
        let native_device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
        let native_texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }?;
        let view = target_view(native_device, native_texture, view_format)?;
        let view = unsafe { com_ref::owned(view.into_raw()) }?;
        Some(Self { view, texture })
    }
}

#[repr(C)]
struct TransportConstants {
    size: Extent,
    has_exposure: u32,
    has_scene: u32,
    scene_view: [f32; 2],
    scene_texture: [f32; 2],
    jitter: [f32; 2],
    padding: [f32; 2],
}

const _: () = assert!(size_of::<TransportConstants>() == 48);

struct Highlights {
    scene: Handle,
    size: Extent,
    jitter: [f32; 2],
}

pub(crate) struct Transport {
    sources: SourceViews<8>,
    encoded: Option<(TargetSpec, Target)>,
    scratch: Option<(TargetSpec, Target)>,
    program: Program,
}

impl Transport {
    fn bindings(
        &mut self,
        source: Handle,
        desc: &TextureDesc,
        exposure: Handle,
        size: Extent,
        highlights: Option<Highlights>,
    ) -> Option<([Option<IUnknown>; 3], TransportConstants)> {
        if !covers(desc, size) {
            return None;
        }
        let source = self.program.source(&mut self.sources, source, desc)?;
        let exposure = if exposure.is_null() {
            None
        } else {
            let desc = self.program.describe(exposure)?;
            if desc.format != RSF_D3D11_FORMAT_R32_FLOAT || !covers(&desc, [1, 1]) {
                return None;
            }
            Some(self.program.source(&mut self.sources, exposure, &desc)?)
        };
        let mut constants = TransportConstants {
            size,
            has_exposure: exposure.is_some().into(),
            has_scene: 0,
            scene_view: [0.0; 2],
            scene_texture: [0.0; 2],
            jitter: [0.0; 2],
            padding: [0.0; 2],
        };
        let scene = if let Some(highlights) = highlights {
            let desc = self.program.describe(highlights.scene)?;
            if !covers(&desc, highlights.size) {
                return None;
            }
            constants.has_scene = 1;
            constants.scene_view = highlights.size.map(|size| size as f32);
            // The active view can occupy only part of the allocation.
            constants.scene_texture = extent(&desc).map(|size| size as f32);
            constants.jitter = highlights.jitter;
            Some(
                self.program
                    .source(&mut self.sources, highlights.scene, &desc)?,
            )
        } else {
            None
        };
        Some(([Some(source), exposure, scene], constants))
    }

    fn dispatch(
        &self,
        context: Handle,
        shader: &ID3D11ComputeShader,
        views: &[Option<IUnknown>; 3],
        target: &Target,
        constants: &TransportConstants,
    ) -> Option<()> {
        let _restore = self.program.save(context)?;
        let native =
            unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.expect("live context");
        prepare_compute(native);
        self.program.bind(context, constants);
        let sources: [Handle; 3] =
            std::array::from_fn(|i| views[i].as_ref().map_or(null_mut(), Interface::as_raw));
        self.program
            .dispatch(context, shader, &sources, target, constants.size, false);
        Some(())
    }

    fn encode(
        &mut self,
        context: Handle,
        source: Handle,
        exposure: Handle,
        size: Extent,
    ) -> Option<Handle> {
        if !self.program.accepts_context(context) {
            return None;
        }
        let desc = self.program.describe(source)?;
        let (views, constants) = self.bindings(source, &desc, exposure, size, None)?;
        let spec = TargetSpec {
            size,
            format: RSF_D3D11_FORMAT_RGBA16_FLOAT,
            sample_quality: 0,
        };
        let replacement = if self.encoded.as_ref().is_some_and(|(last, _)| *last == spec) {
            None
        } else {
            Some((spec, Target::new(&self.program, spec, 0)?))
        };
        let (_, target) = replacement.as_ref().or(self.encoded.as_ref())?;
        let encoded = target.texture.as_raw();
        if source == encoded {
            return None;
        }
        self.dispatch(
            context,
            &self.program.shaders[0],
            &views,
            target,
            &constants,
        )?;
        if replacement.is_some() {
            self.encoded = replacement;
        }
        Some(encoded)
    }

    fn decode(
        &mut self,
        context: Handle,
        target: Handle,
        exposure: Handle,
        size: Extent,
        highlights: Option<Highlights>,
    ) -> Option<()> {
        if !self.program.accepts_context(context) {
            return None;
        }
        let desc = self.program.describe(target)?;
        if desc.bind_flags & RSF_D3D11_BIND_SHADER_RESOURCE == 0
            || desc.usage == RSF_D3D11_USAGE_IMMUTABLE
        {
            return None;
        }
        let (views, constants) = self.bindings(target, &desc, exposure, size, highlights)?;
        let spec = TargetSpec {
            size: extent(&desc),
            format: desc.format,
            sample_quality: desc.sample_quality,
        };
        if self.scratch.as_ref().is_none_or(|(last, _)| *last != spec) {
            let format = srv_format(DXGI_FORMAT(spec.format as i32)).0 as u32;
            self.scratch = Some((spec, Target::new(&self.program, spec, format)?));
        }
        let (_, scratch) = self.scratch.as_ref()?;
        self.dispatch(
            context,
            &self.program.shaders[1],
            &views,
            scratch,
            &constants,
        )?;
        // Dispatch restored the state. Copy only the active rectangle of mip zero.
        unsafe {
            rsf_d3d11_output_copy_region(
                context,
                target,
                scratch.texture.as_raw(),
                0,
                0,
                size[0],
                size[1],
            )
        };
        Some(())
    }
}

#[repr(C)]
struct FidelityConstants {
    render: Extent,
    output: Extent,
    jitter: [f32; 2],
    depth_inverted: u32,
    padding: u32,
}

const _: () = assert!(size_of::<FidelityConstants>() == 32);

struct FidelityTarget {
    source_view: IUnknown,
    target: Target,
}

impl FidelityTarget {
    fn new(program: &Program, size: Extent, format: u32) -> Option<Self> {
        let target = Target::new(
            program,
            TargetSpec {
                size,
                format,
                sample_quality: 0,
            },
            0,
        )?;
        let source_view = unsafe {
            com_ref::owned(source_view(
                program.device.as_raw(),
                target.texture.as_raw(),
                0,
            ))
        }?;
        Some(Self {
            source_view,
            target,
        })
    }
}

#[derive(PartialEq, Eq)]
struct FidelityLayout {
    render: Extent,
    output: Extent,
    format: u32,
}

struct FidelityTargets {
    layout: FidelityLayout,
    residual: FidelityTarget,
    corrected: [FidelityTarget; 2],
}

impl FidelityTargets {
    fn new(program: &Program, layout: FidelityLayout) -> Option<Self> {
        let residual = FidelityTarget::new(program, layout.render, RSF_D3D11_FORMAT_RGBA32_FLOAT)?;
        let corrected = [
            FidelityTarget::new(program, layout.output, layout.format)?,
            FidelityTarget::new(program, layout.output, layout.format)?,
        ];
        Some(Self {
            layout,
            residual,
            corrected,
        })
    }
}

pub(crate) struct Fidelity {
    sources: SourceViews<8>,
    targets: Option<FidelityTargets>,
    program: Program,
}

impl Fidelity {
    fn run(
        &mut self,
        context: Handle,
        scene: Handle,
        output: Handle,
        depth: Handle,
        depth_inverted: u32,
        jitter: [f32; 2],
    ) -> Option<()> {
        if scene == output
            || !self.program.accepts_context(context)
            || !jitter.into_iter().all(f32::is_finite)
        {
            return None;
        }
        let low = self.program.describe(scene)?;
        let high = self.program.describe(output)?;
        let depth_desc = self.program.describe(depth)?;
        let render = extent(&low);
        let size = extent(&high);
        if !covers(&low, render) || !covers(&high, render) || !covers(&depth_desc, render)
            || extent(&depth_desc) != render
            || high.width > low.width.saturating_mul(8) || high.height > low.height.saturating_mul(8)
            || !matches!(high.format, RSF_D3D11_FORMAT_RGBA16_FLOAT | RSF_D3D11_FORMAT_RGBA32_FLOAT)
            // CopyResource requires the corrected texture's topology and a writable destination.
            || high.mips != 1 || high.sample_quality != 0 || high.usage == RSF_D3D11_USAGE_IMMUTABLE
        {
            return None;
        }
        let scene_view = self.program.source(&mut self.sources, scene, &low)?;
        let output_view = self.program.source(&mut self.sources, output, &high)?;
        let depth_view = self.program.source(&mut self.sources, depth, &depth_desc)?;
        let layout = FidelityLayout {
            render,
            output: size,
            format: high.format,
        };
        if self
            .targets
            .as_ref()
            .is_none_or(|targets| targets.layout != layout)
        {
            self.targets = Some(FidelityTargets::new(&self.program, layout)?);
        }
        let targets = self.targets.as_ref()?;
        let _restore = self.program.save(context)?;
        // Vendor work can leave the output bound at any stage or UAV slot.
        unsafe { rsf_d3d11_output_clear_state(context) };
        self.program.bind(
            context,
            &FidelityConstants {
                render,
                output: size,
                jitter,
                depth_inverted,
                padding: 0,
            },
        );
        let mut inputs = [
            scene_view.as_raw(),
            output_view.as_raw(),
            null_mut(),
            depth_view.as_raw(),
        ];
        for corrected in [
            &targets.corrected[0],
            &targets.corrected[1],
            &targets.corrected[0],
        ] {
            inputs[2] = null_mut();
            self.program.dispatch(
                context,
                &self.program.shaders[0],
                &inputs,
                &targets.residual.target,
                render,
                true,
            );
            inputs[2] = targets.residual.source_view.as_raw();
            self.program.dispatch(
                context,
                &self.program.shaders[1],
                &inputs,
                &corrected.target,
                size,
                true,
            );
            inputs[1] = corrected.source_view.as_raw();
        }
        unsafe {
            rsf_d3d11_output_copy_resource(
                context,
                output,
                targets.corrected[0].target.texture.as_raw(),
            )
        };
        Some(())
    }
}

unsafe fn create<T>(
    device: Handle,
    out: *mut *mut Mutex<T>,
    pass: impl FnOnce() -> Option<T>,
) -> i32 {
    if device.is_null() || out.is_null() {
        return 0;
    }
    unsafe { out.write(null_mut()) };
    let Some(pass) = pass() else {
        return 0;
    };
    let handle = crate::allocate_handle(Mutex::new(pass));
    unsafe { out.write(handle) };
    i32::from(!handle.is_null())
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_colour_transport_create(
    device: Handle,
    out: *mut *mut Mutex<Transport>,
) -> i32 {
    unsafe {
        create(device, out, || {
            Some(Transport {
                sources: SourceViews::new(),
                encoded: None,
                scratch: None,
                program: Program::new::<TransportConstants>(
                    device,
                    TRANSPORT_SHADER,
                    c"ColourTransport",
                    [c"encode", c"decode"],
                    true,
                )?,
            })
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_colour_transport_encode(
    pass: *const Mutex<Transport>,
    context: Handle,
    source: Handle,
    exposure: Handle,
    width: u32,
    height: u32,
    encoded: *mut Handle,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return 0;
    };
    if context.is_null() || source.is_null() || encoded.is_null() || width == 0 || height == 0 {
        return 0;
    }
    let Ok(mut pass) = pass.try_lock() else {
        return 0;
    };
    let Some(output) = pass.encode(context, source, exposure, [width, height]) else {
        return 0;
    };
    unsafe { encoded.write(output) };
    1
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_colour_transport_decode(
    pass: *const Mutex<Transport>,
    context: Handle,
    target: Handle,
    exposure: Handle,
    width: u32,
    height: u32,
    scene: Handle,
    scene_width: u32,
    scene_height: u32,
    jitter: *const f32,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return 0;
    };
    if context.is_null() || target.is_null() || width == 0 || height == 0 {
        return 0;
    }
    let Ok(mut pass) = pass.try_lock() else {
        return 0;
    };
    let highlights = (!scene.is_null()).then(|| Highlights {
        scene,
        size: [scene_width, scene_height],
        jitter: unsafe { jitter.cast::<[f32; 2]>().as_ref() }
            .copied()
            .unwrap_or_default(),
    });
    pass.decode(context, target, exposure, [width, height], highlights)
        .is_some()
        .into()
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_colour_transport_destroy(pass: *mut Mutex<Transport>) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_colour_fidelity_create(
    device: Handle,
    out: *mut *mut Mutex<Fidelity>,
) -> i32 {
    unsafe {
        create(device, out, || {
            Some(Fidelity {
                sources: SourceViews::new(),
                targets: None,
                program: Program::new::<FidelityConstants>(
                    device,
                    FIDELITY_SHADER,
                    c"ColourFidelity",
                    [c"project", c"apply"],
                    false,
                )?,
            })
        })
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_colour_fidelity_run(
    pass: *const Mutex<Fidelity>,
    context: Handle,
    scene: Handle,
    output: Handle,
    depth: Handle,
    depth_inverted: u32,
    jitter: *const f32,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return 0;
    };
    if context.is_null() || scene.is_null() || output.is_null() || depth.is_null() {
        return 0;
    }
    let Some(jitter) = (unsafe { jitter.cast::<[f32; 2]>().as_ref() }) else {
        return 0;
    };
    let Ok(mut pass) = pass.try_lock() else {
        return 0;
    };
    pass.run(context, scene, output, depth, depth_inverted, *jitter)
        .is_some()
        .into()
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_colour_fidelity_destroy(pass: *mut Mutex<Fidelity>) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}
