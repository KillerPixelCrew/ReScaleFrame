// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::texture_desc;
use crate::d3d11_output_abi::{
    RSF_D3D11_BIND_SHADER_RESOURCE, RSF_D3D11_FORMAT_R32_FLOAT, RSF_D3D11_FORMAT_RGBA16_FLOAT,
    RSF_D3D11_FORMAT_RGBA32_FLOAT, RSF_D3D11_USAGE_IMMUTABLE, rsf_d3d11_output_child_device,
    rsf_d3d11_output_clear_state, rsf_d3d11_output_copy_region, rsf_d3d11_output_copy_resource,
    rsf_d3d11_output_create_texture, rsf_d3d11_output_is_immediate, rsf_d3d11_output_srv_format,
    rsf_d3d11_state, rsf_d3d11_texture_desc as TextureDesc,
};
use crate::motion_d3d11::{GROUP_EDGE, Handle, MakeView, SourceViews};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{self, null_mut};
use std::sync::Mutex;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

const BIND_UNORDERED_ACCESS: u32 = 0x80;
type Extent = [u32; 2];

#[repr(C)]
struct Platform {
    compile: unsafe extern "C" fn(Handle, *const c_char, *const c_char, *const c_char) -> Handle,
    constant_buffer: unsafe extern "C" fn(Handle, u32) -> Handle,
    sampler: unsafe extern "C" fn(Handle, u32) -> Handle,
    target_view: unsafe extern "C" fn(Handle, Handle, u32) -> Handle,
    source_view: MakeView,
    save: unsafe extern "C" fn(Handle, &mut rsf_d3d11_state) -> u32,
    restore: unsafe extern "C" fn(Handle, &mut rsf_d3d11_state),
    prepare_transport: unsafe extern "C" fn(Handle),
    bind_parameters: unsafe extern "C" fn(Handle, Handle, Handle, *const c_void),
    dispatch: unsafe extern "C" fn(Handle, Handle, *const Handle, u32, Handle, u32, u32, u32),
}

const _: () = assert!(size_of::<Platform>() == 10 * size_of::<Handle>());

struct ContextState<'a> {
    context: Handle,
    saved: &'a mut rsf_d3d11_state,
    platform: &'static Platform,
}

impl Drop for ContextState<'_> {
    fn drop(&mut self) {
        unsafe { (self.platform.restore)(self.context, self.saved) };
    }
}

struct Program {
    shaders: [ComRef; 2],
    constants: ComRef,
    sampler: ComRef,
    platform: &'static Platform,
    device: ComRef,
}

impl Program {
    fn new<T>(
        device: Handle,
        source: *const c_char,
        name: &CStr,
        entries: [&CStr; 2],
        mip_linear: bool,
        platform: &'static Platform,
    ) -> Option<Self> {
        let device = unsafe { ComRef::retain(device) };
        let raw = device.as_ptr();
        let compile = |entry: &CStr| unsafe {
            ComRef::from_owned((platform.compile)(
                raw,
                source,
                name.as_ptr(),
                entry.as_ptr(),
            ))
        };
        let shaders = [compile(entries[0])?, compile(entries[1])?];
        let constants =
            unsafe { ComRef::from_owned((platform.constant_buffer)(raw, size_of::<T>() as u32)) }?;
        let sampler = unsafe { ComRef::from_owned((platform.sampler)(raw, mip_linear.into())) }?;
        Some(Self {
            shaders,
            constants,
            sampler,
            platform,
            device,
        })
    }

    fn same_device(&self, child: Handle) -> bool {
        unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(child)) }
            .is_some_and(|owner| owner.as_ptr() == self.device.as_ptr())
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
    ) -> Option<ComRef> {
        let format = unsafe { rsf_d3d11_output_srv_format(desc.format) };
        // A batch can touch several cache entries at the same clock tick.
        // Retain each selected view so a later lookup cannot evict it mid-batch.
        unsafe {
            cache.get(
                self.device.as_ptr(),
                texture,
                format,
                GetTickCount64(),
                self.platform.source_view,
            )
        }
        .cloned()
    }

    fn save<'a>(
        &self,
        context: Handle,
        saved: &'a mut rsf_d3d11_state,
    ) -> Option<ContextState<'a>> {
        let platform = self.platform;
        if unsafe { (platform.save)(context, saved) } == 0 {
            return None;
        }
        Some(ContextState {
            context,
            saved,
            platform,
        })
    }

    fn bind<T>(&self, context: Handle, constants: &T) {
        unsafe {
            (self.platform.bind_parameters)(
                context,
                self.constants.as_ptr(),
                self.sampler.as_ptr(),
                ptr::from_ref(constants).cast(),
            );
        }
    }

    fn dispatch<const N: usize>(
        &self,
        context: Handle,
        shader: &ComRef,
        sources: &[Handle; N],
        target: &Target,
        [width, height]: Extent,
        clear_sources: bool,
    ) {
        const { assert!(N <= 4) }
        unsafe {
            (self.platform.dispatch)(
                context,
                shader.as_ptr(),
                sources.as_ptr(),
                N as u32,
                target.view.as_ptr(),
                width.div_ceil(GROUP_EDGE),
                height.div_ceil(GROUP_EDGE),
                clear_sources.into(),
            );
        }
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
    view: ComRef,
    texture: ComRef,
}

impl Target {
    fn new(program: &Program, spec: TargetSpec, view_format: u32) -> Option<Self> {
        let device = program.device.as_ptr();
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
        let texture =
            unsafe { ComRef::from_owned(rsf_d3d11_output_create_texture(device, &desc)) }?;
        let view = unsafe {
            ComRef::from_owned((program.platform.target_view)(
                device,
                texture.as_ptr(),
                view_format,
            ))
        }?;
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

struct Transport {
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
    ) -> Option<([Option<ComRef>; 3], TransportConstants)> {
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
        shader: &ComRef,
        views: &[Option<ComRef>; 3],
        target: &Target,
        constants: &TransportConstants,
    ) -> Option<()> {
        let mut saved = rsf_d3d11_state::default();
        let _restore = self.program.save(context, &mut saved)?;
        unsafe { (self.program.platform.prepare_transport)(context) };
        self.program.bind(context, constants);
        let sources: [Handle; 3] =
            std::array::from_fn(|i| views[i].as_ref().map_or(null_mut(), ComRef::as_ptr));
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
        let encoded = target.texture.as_ptr();
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
            let format = unsafe { rsf_d3d11_output_srv_format(spec.format) };
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
                scratch.texture.as_ptr(),
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
    source_view: ComRef,
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
            ComRef::from_owned((program.platform.source_view)(
                program.device.as_ptr(),
                target.texture.as_ptr(),
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

struct Fidelity {
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
        let mut saved = rsf_d3d11_state::default();
        let _restore = self.program.save(context, &mut saved)?;
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
            scene_view.as_ptr(),
            output_view.as_ptr(),
            null_mut(),
            depth_view.as_ptr(),
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
            inputs[2] = targets.residual.source_view.as_ptr();
            self.program.dispatch(
                context,
                &self.program.shaders[1],
                &inputs,
                &corrected.target,
                size,
                true,
            );
            inputs[1] = corrected.source_view.as_ptr();
        }
        unsafe {
            rsf_d3d11_output_copy_resource(
                context,
                output,
                targets.corrected[0].target.texture.as_ptr(),
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
unsafe extern "C" fn rsf_core_colour_transport_create(
    device: Handle,
    out: *mut *mut Mutex<Transport>,
    source: *const c_char,
    platform: &'static Platform,
) -> i32 {
    unsafe {
        create(device, out, || {
            Some(Transport {
                sources: SourceViews::new(),
                encoded: None,
                scratch: None,
                program: Program::new::<TransportConstants>(
                    device,
                    source,
                    c"ColourTransport",
                    [c"encode", c"decode"],
                    true,
                    platform,
                )?,
            })
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_colour_transport_encode(
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
unsafe extern "C" fn rsf_core_colour_transport_decode(
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
unsafe extern "C" fn rsf_core_colour_transport_destroy(pass: *mut Mutex<Transport>) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_colour_fidelity_create(
    device: Handle,
    out: *mut *mut Mutex<Fidelity>,
    source: *const c_char,
    platform: &'static Platform,
) -> i32 {
    unsafe {
        create(device, out, || {
            Some(Fidelity {
                sources: SourceViews::new(),
                targets: None,
                program: Program::new::<FidelityConstants>(
                    device,
                    source,
                    c"ColourFidelity",
                    [c"project", c"apply"],
                    false,
                    platform,
                )?,
            })
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_colour_fidelity_run(
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
unsafe extern "C" fn rsf_core_colour_fidelity_destroy(pass: *mut Mutex<Fidelity>) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}
