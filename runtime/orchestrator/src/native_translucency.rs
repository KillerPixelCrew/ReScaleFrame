// SPDX-License-Identifier: MIT
// Shared D3D11 translucency hints and cloud-depth handover.
use crate::com_ref::{self, vtable};
use crate::d3d11_device_identity::identity as device_identity;
use crate::d3d11_output::{SavedBindings, texture_desc};
use crate::d3d11_output_platform::{
    rsf_d3d11_output_child_device, rsf_d3d11_output_copy_resource, rsf_d3d11_output_create_srv,
    rsf_d3d11_output_is_immediate, rsf_d3d11_output_srv_format, rsf_d3d11_output_unbind_targets,
};
use crate::graphics_d3d11::{rsf_d3d11_state_restore, rsf_d3d11_state_save};
use crate::native_regions::{
    MaskTarget, Region, SourceView, compile_compute, create_constants, crop, output_texture,
    update_constants,
};
use crate::sr_session::has_capacity;
use crate::sync::lock;
use crate::sys::rsf_native_translucency_masks as Masks;
use crate::sys::{RSF_D3D11_FORMAT_R32_FLOAT, rsf_d3d11_texture_desc};
use rsf_game_sys::{
    RSF_GAME_RENDER_CLOUD_DEPTH, RSF_GAME_RENDER_MATERIALS, RSF_GAME_RENDER_PRIMARY,
    RSF_GAME_RENDER_TRANSLUCENCY, RSF_GAME_RENDER_TRANSLUCENCY_LAYER, rsf_game_render_pass as Pass,
};
use rsf_graphics::frame_tap::rsf_frame_tap_set_sampler_bias;
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::BOOL;
use windows::core::{IUnknown, Interface};

type Object = *mut c_void;
const NONE: Object = ptr::null_mut();
// A tonemapped change of a tenth already reads as fully covered.
const COVERAGE_SCALE: f32 = 10.0;
const MODE_SCENE: u32 = 0;
const MODE_LAYER: u32 = 1;

struct Dispatch {
    shader: Object,
    constants: Object,
    sampler: Object,
    views: [Object; 3],
    targets: [Object; 3],
    groups: [u32; 2],
}

// Matches cbuffer Params in the HLSL.
#[repr(C)]
struct Constants {
    origin: [i32; 2],
    size: [u32; 2],
    scene_inv_size: [f32; 2],
    mode: u32,
    coverage_scale: f32,
}

const _: () = {
    assert!(size_of::<Constants>() == 32);
    assert!(std::mem::offset_of!(Constants, mode) == 24);
};

unsafe fn create_sampler(device: Object) -> Option<IUnknown> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MaxLOD: D3D11_FLOAT32_MAX,
        ..Default::default()
    };
    let mut sampler = NONE;
    let status = unsafe {
        (vtable::<ID3D11Device>(&device).CreateSamplerState)(device, &desc, &mut sampler)
    };
    unsafe { com_ref::from_result(status, sampler) }
}

unsafe fn dispatch(context: Object, pass: &Dispatch) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    unsafe {
        (api.CSSetShader)(context, pass.shader, ptr::null(), 0);
        (api.CSSetShaderResources)(context, 0, 3, pass.views.as_ptr());
        (api.CSSetUnorderedAccessViews)(context, 0, 3, pass.targets.as_ptr(), ptr::null());
        (api.CSSetConstantBuffers)(context, 0, 1, &pass.constants);
        (api.CSSetSamplers)(context, 0, 1, &pass.sampler);
        (api.Dispatch)(context, pass.groups[0], pass.groups[1], 1);
        (api.CSSetUnorderedAccessViews)(context, 0, 3, [NONE; 3].as_ptr(), ptr::null());
    }
}

// Identity shared by a translucency scope and the view's later SR pass.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Key {
    session: u64,
    family: u64,
    view: u64,
    frame: u64,
    rect: [i32; 4],
}

impl Key {
    fn of(pass: &Pass) -> Self {
        Self {
            session: pass.session_id,
            family: pass.family_key,
            view: pass.view_key,
            frame: pass.native_frame,
            rect: pass.render_rect,
        }
    }

    fn same_view(&self, other: &Self) -> bool {
        self.session == other.session && self.family == other.family && self.view == other.view
    }
}

// What the adapter reports with a cloud composite: its session and the
// viewport it covers. It carries no view or frame identity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CloudTag {
    session: u64,
    rect: [i32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Frame {
    key: Key,
    snapshot: bool,
    masks: bool,
    layer: bool,
    clouds: bool,
    // The view's SR pass asked for this frame. Later passes cannot reach it.
    closed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Opening {
    Repeated,
    Opened { adopt_clouds: bool },
}

// Pass order and producer identity, free of COM so the contracts are testable.
#[derive(Default)]
struct Ledger {
    frame: Option<Frame>,
    pending: Option<CloudTag>,
}

impl Ledger {
    // Clouds composite in the post-opaque extension, before the first
    // translucency pass of their frame. One arriving after that pass began and
    // before the view's SR pass belongs to the open frame and is late for it.
    fn cloud_admissible(&self, session: u64) -> bool {
        self.frame
            .is_none_or(|frame| frame.closed || frame.key.session != session)
    }

    fn cloud_refused(&mut self) {
        self.pending = None;
    }

    fn cloud_stored(&mut self, tag: CloudTag) {
        self.pending = Some(tag);
    }

    // The first scene-colour pass of a frame: everything drawn so far is
    // opaque, sky or cloud. A repeated begin of the open frame would snapshot
    // translucency already drawn, so it keeps the first snapshot.
    fn open(&mut self, key: Key) -> Opening {
        if self
            .frame
            .is_some_and(|frame| frame.key == key && !frame.closed)
        {
            return Opening::Repeated;
        }
        let adopt_clouds = self
            .pending
            .take()
            .is_some_and(|tag| tag.session == key.session && tag.rect == key.rect);
        self.frame = Some(Frame {
            key,
            snapshot: false,
            masks: false,
            layer: false,
            clouds: adopt_clouds,
            closed: false,
        });
        Opening::Opened { adopt_clouds }
    }

    fn open_frame(&mut self, key: Key) -> Option<&mut Frame> {
        self.frame
            .as_mut()
            .filter(|frame| frame.key == key && !frame.closed)
    }

    fn snapshot_taken(&mut self, key: Key) {
        if let Some(frame) = self.open_frame(key) {
            frame.snapshot = true;
        }
    }

    // A second scene-colour end would compare against a stale base.
    fn admit_masks(&mut self, key: Key) -> bool {
        self.open_frame(key)
            .is_some_and(|frame| frame.snapshot && !frame.masks)
    }

    fn masks_written(&mut self, key: Key, written: bool) {
        if let Some(frame) = self.open_frame(key) {
            frame.masks = written;
        }
    }

    fn admit_layer(&mut self, key: Key) -> bool {
        self.open_frame(key).is_some_and(|frame| frame.masks)
    }

    // Masks without the layer's contribution would under-report coverage.
    fn layer_merge_failed(&mut self, key: Key) {
        if let Some(frame) = self.open_frame(key) {
            frame.masks = false;
        }
    }

    fn layer_copied(&mut self, key: Key) {
        if let Some(frame) = self.open_frame(key) {
            frame.layer = true;
        }
    }

    // Any SR request for the frame's view closes it, whether or not the masks
    // match, so a refused frame cannot hold later cloud composites back.
    fn take(&mut self, key: Key) -> Option<Frame> {
        let frame = self.frame.as_mut()?;
        if frame.key.same_view(&key) {
            frame.closed = true;
        }
        (frame.key == key && frame.masks).then_some(*frame)
    }
}

// Rejects reentry and concurrent use without holding a lock across driver
// calls. A release during use is deferred to the end of that use, so it is
// never lost.
struct Exclusive<T> {
    slot: Mutex<Slot<T>>,
}

struct Slot<T> {
    state: Option<T>,
    busy: bool,
    release: bool,
}

impl<T> Exclusive<T> {
    const fn new() -> Self {
        Self {
            slot: Mutex::new(Slot {
                state: None,
                busy: false,
                release: false,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Slot<T>> {
        lock(&self.slot)
    }

    fn with<R>(&self, run: impl FnOnce(&mut Option<T>) -> R) -> Option<R> {
        let mut state = {
            let mut slot = self.lock();
            if slot.busy {
                return None;
            }
            slot.busy = true;
            slot.state.take()
        };
        let result = run(&mut state);
        let retired = {
            let mut slot = self.lock();
            slot.busy = false;
            if std::mem::take(&mut slot.release) {
                state
            } else {
                slot.state = state;
                None
            }
        };
        drop(retired);
        Some(result)
    }

    fn release(&self) {
        let retired = {
            let mut slot = self.lock();
            if slot.busy {
                slot.release = true;
                None
            } else {
                slot.state.take()
            }
        };
        drop(retired);
    }
}

struct Kernel {
    shader: IUnknown,
    constants: IUnknown,
    sampler: IUnknown,
}

struct Targets {
    extent: [u32; 2],
    format: u32,
    opaque: IUnknown,
    opaque_view: IUnknown,
    // Reactive, coverage and bias.
    masks: [MaskTarget; 3],
}

struct Layer {
    format: u32,
    texture: IUnknown,
}

struct Cloud {
    extent: [u32; 2],
    texture: IUnknown,
}

struct Owner {
    device: IUnknown,
    identity: IUnknown,
    kernel: Option<Kernel>,
    kernel_failed: bool,
    targets: Option<Targets>,
    layer: Option<Layer>,
    // A pending composite is copied into the slot that is not published, so a
    // frame's borrowed cloud depth stays intact until the next scope.
    clouds: [Option<Cloud>; 2],
    published: usize,
    scene_view: Option<SourceView>,
    layer_view: Option<SourceView>,
    ledger: Ledger,
}

// SAFETY: only D3D11 resources are retained. Each use runs exclusively on the
// caller's immediate context, and borrowed outputs live until the next scope
// or release as the header states.
unsafe impl Send for Owner {}

static OWNER: Exclusive<Owner> = Exclusive::new();

struct Device {
    device: IUnknown,
    identity: IUnknown,
}

impl Device {
    // The device behind the caller's immediate context, or None.
    unsafe fn of_context(context: Object) -> Option<Self> {
        if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
            return None;
        }
        let device = unsafe { com_ref::owned(rsf_d3d11_output_child_device(context)) }?;
        let identity = unsafe { device_identity(device.as_raw()) }?;
        Some(Self { device, identity })
    }

    unsafe fn owns(&self, child: Object) -> bool {
        unsafe { owned_by(&self.identity, child) }
    }
}

// Resources of another device are refused, never copied across.
unsafe fn owned_by(identity: &IUnknown, child: Object) -> bool {
    let Some(owner) = (unsafe { com_ref::owned(rsf_d3d11_output_child_device(child)) }) else {
        return false;
    };
    unsafe { device_identity(owner.as_raw()) }
        .is_some_and(|owner| owner.as_raw() == identity.as_raw())
}

fn for_device(owner: &mut Option<Owner>, device: Device) -> &mut Owner {
    if owner
        .as_ref()
        .is_some_and(|owner| owner.identity.as_raw() != device.identity.as_raw())
    {
        *owner = None;
    }
    owner.get_or_insert_with(|| Owner {
        device: device.device,
        identity: device.identity,
        kernel: None,
        kernel_failed: false,
        targets: None,
        layer: None,
        clouds: [None, None],
        published: 0,
        scene_view: None,
        layer_view: None,
        ledger: Ledger::default(),
    })
}

impl Kernel {
    unsafe fn new(device: Object) -> Option<Self> {
        Some(Self {
            shader: unsafe {
                compile_compute(
                    device,
                    include_bytes!("composition_shaders/translucency_masks.hlsl"),
                    c"translucency_masks",
                    0,
                )
            }?,
            constants: unsafe { create_constants(device, size_of::<Constants>()) }?,
            sampler: unsafe { create_sampler(device) }?,
        })
    }
}

impl Targets {
    unsafe fn new(device: Object, extent: [u32; 2], format: u32) -> Option<Self> {
        let opaque = unsafe { output_texture(device, extent, format, false) }?;
        let opaque_view = unsafe {
            com_ref::owned(rsf_d3d11_output_create_srv(device, opaque.as_raw(), format))
        }?;
        Some(Self {
            extent,
            format,
            opaque,
            opaque_view,
            masks: [
                unsafe { MaskTarget::new(device, extent) }?,
                unsafe { MaskTarget::new(device, extent) }?,
                unsafe { MaskTarget::new(device, extent) }?,
            ],
        })
    }
}

impl Owner {
    // A failed compile or allocation is not retried on this device.
    unsafe fn prepare_kernel(&mut self) -> bool {
        if self.kernel.is_some() {
            return true;
        }
        if self.kernel_failed {
            return false;
        }
        self.kernel = unsafe { Kernel::new(self.device.as_raw()) };
        self.kernel_failed = self.kernel.is_none();
        !self.kernel_failed
    }

    unsafe fn prepare_targets(&mut self, extent: [u32; 2], format: u32) -> bool {
        if self
            .targets
            .as_ref()
            .is_some_and(|targets| targets.extent == extent && targets.format == format)
        {
            return true;
        }
        self.targets = None;
        self.layer = None;
        self.targets = unsafe { Targets::new(self.device.as_raw(), extent, format) };
        self.targets.is_some()
    }

    unsafe fn open(
        &mut self,
        context: Object,
        scene: Object,
        scene_desc: &rsf_d3d11_texture_desc,
        region: Region,
        key: Key,
    ) {
        let Opening::Opened { adopt_clouds } = self.ledger.open(key) else {
            return;
        };
        if adopt_clouds {
            self.published ^= 1;
        }
        let format = rsf_d3d11_output_srv_format(scene_desc.format);
        if !unsafe { self.prepare_targets(region.extent, format) } {
            return;
        }
        let Some(targets) = &self.targets else {
            return;
        };
        unsafe { crop(context, targets.opaque.as_raw(), scene, &region.bounds) };
        self.ledger.snapshot_taken(key);
    }

    unsafe fn dispatch(
        &mut self,
        context: Object,
        scene: (Object, &rsf_d3d11_texture_desc),
        layer: Option<(Object, &rsf_d3d11_texture_desc)>,
        region: Region,
    ) -> bool {
        let device = self.device.as_raw();
        let (Some(kernel), Some(targets)) = (&self.kernel, &self.targets) else {
            return false;
        };
        let (scene, scene_desc) = scene;
        let scene_format = rsf_d3d11_output_srv_format(scene_desc.format);
        let Some(scene_view) =
            (unsafe { SourceView::get(&mut self.scene_view, device, scene, scene_format) })
        else {
            return false;
        };
        let layer_view = match layer {
            None => NONE,
            Some((_, desc)) if desc.samples != 1 || desc.layers != 1 => return false,
            Some((layer, desc)) => {
                let format = rsf_d3d11_output_srv_format(desc.format);
                match unsafe { SourceView::get(&mut self.layer_view, device, layer, format) } {
                    Some(view) => view,
                    None => return false,
                }
            }
        };
        let constants = Constants {
            origin: [region.bounds[0] as i32, region.bounds[1] as i32],
            size: targets.extent,
            scene_inv_size: [
                1.0 / scene_desc.width as f32,
                1.0 / scene_desc.height as f32,
            ],
            mode: if layer.is_some() {
                MODE_LAYER
            } else {
                MODE_SCENE
            },
            coverage_scale: COVERAGE_SCALE,
        };
        let pass = Dispatch {
            shader: kernel.shader.as_raw(),
            constants: kernel.constants.as_raw(),
            sampler: kernel.sampler.as_raw(),
            views: [scene_view, targets.opaque_view.as_raw(), layer_view],
            targets: targets.masks.each_ref().map(|mask| mask.view.as_raw()),
            groups: region.groups(),
        };
        let Some(_bindings) =
            (unsafe { SavedBindings::new(context, rsf_d3d11_state_save, rsf_d3d11_state_restore) })
        else {
            return false;
        };
        unsafe {
            (vtable::<ID3D11DeviceContext>(&context).SetPredication)(context, NONE, BOOL(0));
            rsf_d3d11_output_unbind_targets(context);
            update_constants(context, kernel.constants.as_raw(), &constants);
            dispatch(context, &pass);
        }
        true
    }

    unsafe fn merge_layer(
        &mut self,
        context: Object,
        layer: Object,
        scene: (Object, &rsf_d3d11_texture_desc),
        region: Region,
        key: Key,
    ) {
        if layer.is_null() || !self.ledger.admit_layer(key) {
            return;
        }
        let layer_desc = unsafe { texture_desc(layer) };
        if !unsafe { owned_by(&self.identity, layer) }
            || !unsafe { self.dispatch(context, scene, Some((layer, &layer_desc)), region) }
        {
            self.ledger.layer_merge_failed(key);
            return;
        }
        // Only a layer at scene size shares the view rectangle; a scaled one
        // contributes to the masks only.
        let scene_desc = scene.1;
        if (layer_desc.width, layer_desc.height) != (scene_desc.width, scene_desc.height)
            || layer_desc.samples != 1
            || layer_desc.layers != 1
        {
            return;
        }
        let format = rsf_d3d11_output_srv_format(layer_desc.format);
        let Some(extent) = self.targets.as_ref().map(|targets| targets.extent) else {
            return;
        };
        if self
            .layer
            .as_ref()
            .is_none_or(|layer| layer.format != format)
        {
            self.layer = None;
            let Some(texture) =
                (unsafe { output_texture(self.device.as_raw(), extent, format, false) })
            else {
                return;
            };
            self.layer = Some(Layer { format, texture });
        }
        let Some(copy) = &self.layer else {
            return;
        };
        unsafe { crop(context, copy.texture.as_raw(), layer, &region.bounds) };
        self.ledger.layer_copied(key);
    }

    unsafe fn store_cloud(
        &mut self,
        context: Object,
        cloud: Object,
        extent: [u32; 2],
        tag: CloudTag,
    ) {
        let slot = &mut self.clouds[self.published ^ 1];
        if slot.as_ref().is_none_or(|stored| stored.extent != extent) {
            *slot = None;
            let Some(texture) = (unsafe {
                output_texture(
                    self.device.as_raw(),
                    extent,
                    RSF_D3D11_FORMAT_R32_FLOAT,
                    false,
                )
            }) else {
                return;
            };
            *slot = Some(Cloud { extent, texture });
        }
        let Some(stored) = slot.as_ref() else {
            return;
        };
        unsafe { rsf_d3d11_output_copy_resource(context, stored.texture.as_raw(), cloud) };
        self.ledger.cloud_stored(tag);
    }
}

fn material_bias(pass: &Pass, begin: bool) -> f32 {
    let bias = pass.material_texture_mip_bias;
    if begin && bias.is_finite() { bias } else { 0.0 }
}

unsafe fn cloud_depth(context: Object, pass: &Pass) {
    let device = unsafe { Device::of_context(context) };
    let cloud = pass.depth;
    let desc = unsafe { texture_desc(cloud) };
    let extent = Region::new(pass.render_rect).map(|region| region.extent);
    let compatible = device
        .as_ref()
        .is_some_and(|device| unsafe { device.owns(cloud) })
        && desc.format == RSF_D3D11_FORMAT_R32_FLOAT
        && (desc.mips, desc.layers, desc.samples) == (1, 1, 1)
        && extent == Some([desc.width, desc.height]);
    let tag = CloudTag {
        session: pass.session_id,
        rect: pass.render_rect,
    };
    OWNER.with(|owner| {
        // Any composite replaces the previous one, including a refused one.
        if let Some(owner) = owner.as_mut() {
            owner.ledger.cloud_refused();
        }
        let (Some(device), Some(extent), true) = (device, extent, compatible) else {
            return;
        };
        let owner = for_device(owner, device);
        if owner.ledger.cloud_admissible(tag.session) {
            unsafe { owner.store_cloud(context, cloud, extent, tag) };
        }
    });
}

unsafe fn translucency(context: Object, pass: &Pass, begin: bool) {
    let Some(device) = (unsafe { Device::of_context(context) }) else {
        return;
    };
    let scene = pass.color_input;
    if !unsafe { device.owns(scene) } {
        return;
    }
    let key = Key::of(pass);
    let layer_pass = pass.flags & RSF_GAME_RENDER_TRANSLUCENCY_LAYER != 0;
    OWNER.with(|owner| {
        let owner = for_device(owner, device);
        if !unsafe { owner.prepare_kernel() } {
            return;
        }
        let scene_desc = unsafe { texture_desc(scene) };
        let Some(region) = Region::new(key.rect).filter(|region| region.fits(&scene_desc)) else {
            return;
        };
        if begin {
            if !layer_pass {
                unsafe { owner.open(context, scene, &scene_desc, region, key) };
            }
        } else if !layer_pass {
            if owner.ledger.admit_masks(key) {
                let written =
                    unsafe { owner.dispatch(context, (scene, &scene_desc), None, region) };
                owner.ledger.masks_written(key, written);
            }
        } else {
            unsafe {
                owner.merge_layer(
                    context,
                    pass.color_output,
                    (scene, &scene_desc),
                    region,
                    key,
                )
            };
        }
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_translucency_pass(context: Object, pass: *const Pass, begin: u32) {
    if context.is_null() || !unsafe { has_capacity(pass) } {
        return;
    }
    let pass = unsafe { &*pass };
    let begin = begin != 0;
    match pass.role {
        RSF_GAME_RENDER_CLOUD_DEPTH if begin && !pass.depth.is_null() => unsafe {
            cloud_depth(context, pass)
        },
        RSF_GAME_RENDER_MATERIALS => {
            rsf_frame_tap_set_sampler_bias(material_bias(pass, begin));
        }
        RSF_GAME_RENDER_TRANSLUCENCY
            if pass.flags & RSF_GAME_RENDER_PRIMARY != 0 && !pass.color_input.is_null() =>
        {
            // Translucent materials take the same bias: off before the
            // end-of-pass masks, on after the opening snapshot. Both are compute
            // work the bias does not touch.
            if !begin {
                rsf_frame_tap_set_sampler_bias(0.0);
            }
            unsafe { translucency(context, pass, begin) };
            if begin {
                rsf_frame_tap_set_sampler_bias(material_bias(pass, true));
            }
        }
        _ => {}
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_translucency_take(
    sr_pass: *const Pass,
    out: *mut Masks,
) -> i32 {
    if !unsafe { has_capacity(sr_pass) && has_capacity(out.cast_const()) } {
        return 0;
    }
    let key = Key::of(unsafe { &*sr_pass });
    let published = OWNER.with(|owner| {
        let owner = owner.as_mut()?;
        let frame = owner.ledger.take(key)?;
        let targets = owner.targets.as_ref()?;
        let layer = owner.layer.as_ref().filter(|_| frame.layer);
        let clouds = owner.clouds[owner.published]
            .as_ref()
            .filter(|_| frame.clouds);
        let [reactive, coverage, bias] = targets.masks.each_ref().map(|mask| mask.texture.as_raw());
        Some(Masks {
            struct_size: 0,
            color_before_transparency: targets.opaque.as_raw(),
            transparency_layer: layer.map_or(NONE, |layer| layer.texture.as_raw()),
            reactive,
            coverage,
            bias,
            motion_depth: clouds.map_or(NONE, |cloud| cloud.texture.as_raw()),
        })
    });
    let Some(masks) = published.flatten() else {
        return 0;
    };
    // Borrowed: valid until the next translucency scope or release.
    let out = unsafe { &mut *out };
    *out = Masks {
        struct_size: out.struct_size,
        ..masks
    };
    1
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_native_translucency_release() {
    OWNER.release();
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: [i32; 4] = [4, 2, 12, 6];

    fn key(frame: u64) -> Key {
        Key {
            session: 0,
            family: 1,
            view: 2,
            frame,
            rect: RECT,
        }
    }

    fn cloud(session: u64, rect: [i32; 4]) -> CloudTag {
        CloudTag { session, rect }
    }

    fn masks_ready(ledger: &mut Ledger, key: Key) -> Opening {
        let opening = ledger.open(key);
        ledger.snapshot_taken(key);
        assert!(ledger.admit_masks(key));
        ledger.masks_written(key, true);
        opening
    }

    #[test]
    fn masks_follow_snapshot_and_reach_only_the_same_identity() {
        let mut ledger = Ledger::default();
        assert_eq!(
            ledger.open(key(3)),
            Opening::Opened {
                adopt_clouds: false
            }
        );
        // No end-of-pass masks without the opening snapshot.
        assert!(!ledger.admit_masks(key(3)));
        ledger.snapshot_taken(key(3));
        assert!(!ledger.admit_masks(key(4)));
        assert!(ledger.admit_masks(key(3)));
        ledger.masks_written(key(3), true);
        // A second scene-colour end would compare against a stale base.
        assert!(!ledger.admit_masks(key(3)));
        assert!(ledger.admit_layer(key(3)));
        ledger.layer_copied(key(3));

        let mut other_rect = key(3);
        other_rect.rect[2] += 1;
        let mut other_view = key(3);
        other_view.view = 9;
        assert_eq!(ledger.take(other_view), None);
        assert!(!ledger.frame.unwrap().closed);
        assert_eq!(ledger.take(other_rect), None);
        let frame = ledger.take(key(3)).unwrap();
        assert!(frame.layer && !frame.clouds && frame.closed);
        // Repeated requests keep receiving the same borrowed masks.
        assert!(ledger.take(key(3)).is_some());
        assert_eq!(ledger.take(key(4)), None);
    }

    #[test]
    fn repeated_open_keeps_the_first_snapshot_until_sr_closes_the_frame() {
        let mut ledger = Ledger::default();
        masks_ready(&mut ledger, key(3));
        assert_eq!(ledger.open(key(3)), Opening::Repeated);
        assert!(ledger.take(key(3)).is_some());
        // After SR, the same identity is a new frame of an adapter that does
        // not advance its frame number.
        assert!(matches!(ledger.open(key(3)), Opening::Opened { .. }));
        assert_eq!(ledger.take(key(3)), None);
    }

    #[test]
    fn passes_after_sr_and_failed_layers_are_refused() {
        let mut ledger = Ledger::default();
        ledger.open(key(3));
        ledger.snapshot_taken(key(3));
        // A layer before the scene masks has nothing to merge into.
        assert!(!ledger.admit_layer(key(3)));
        ledger.masks_written(key(3), true);
        assert!(ledger.admit_layer(key(3)));
        ledger.layer_merge_failed(key(3));
        assert_eq!(ledger.take(key(3)), None);

        masks_ready(&mut ledger, key(4));
        assert!(ledger.take(key(4)).is_some());
        assert!(!ledger.admit_layer(key(4)));
        ledger.layer_copied(key(4));
        assert!(!ledger.take(key(4)).unwrap().layer);
    }

    #[test]
    fn a_second_session_cannot_read_or_close_the_first_sessions_masks() {
        let mut ledger = Ledger::default();
        let first = key(3);
        let second = Key {
            session: 1,
            ..first
        };
        masks_ready(&mut ledger, first);

        assert_eq!(ledger.take(second), None);
        assert!(!ledger.frame.unwrap().closed);
        assert!(!ledger.admit_masks(second));
        assert!(!ledger.admit_layer(second));

        // The second session can open its own scope even if the first never reached SR.
        assert!(ledger.cloud_admissible(second.session));
        ledger.cloud_stored(cloud(second.session, RECT));
        assert_eq!(ledger.open(second), Opening::Opened { adopt_clouds: true });
        assert_eq!(ledger.take(first), None);
        assert_eq!(ledger.take(second), None);
    }

    #[test]
    fn clouds_need_the_scope_session_and_rectangle() {
        let mut ledger = Ledger::default();
        let scope = |frame| Key {
            session: 5,
            ..key(frame)
        };
        assert!(ledger.cloud_admissible(5));
        ledger.cloud_stored(cloud(5, RECT));
        assert_eq!(
            masks_ready(&mut ledger, scope(3)),
            Opening::Opened { adopt_clouds: true }
        );
        assert!(ledger.take(scope(3)).unwrap().clouds);

        ledger.cloud_stored(cloud(6, RECT));
        assert_eq!(
            masks_ready(&mut ledger, scope(4)),
            Opening::Opened {
                adopt_clouds: false
            }
        );
        ledger.take(scope(4));

        ledger.cloud_stored(cloud(5, [0, 0, 8, 4]));
        masks_ready(&mut ledger, scope(5));
        assert!(!ledger.take(scope(5)).unwrap().clouds);

        // A pending composite expires at the next opening whether or not it matched.
        masks_ready(&mut ledger, scope(6));
        assert!(!ledger.take(scope(6)).unwrap().clouds);

        // A refused composite withdraws the previous one.
        ledger.cloud_stored(cloud(5, RECT));
        ledger.cloud_refused();
        masks_ready(&mut ledger, scope(7));
        assert!(!ledger.take(scope(7)).unwrap().clouds);
    }

    #[test]
    fn a_cloud_after_the_frame_opened_is_late_until_its_sr_pass() {
        let mut ledger = Ledger::default();
        masks_ready(&mut ledger, key(3));
        assert!(!ledger.cloud_admissible(0));
        // SR for the view closes the frame even when its masks are refused.
        let mut refused = key(3);
        refused.frame = 2;
        assert_eq!(ledger.take(refused), None);
        assert!(ledger.cloud_admissible(0));

        ledger.open(key(4));
        assert!(!ledger.cloud_admissible(0));
        // Another view's SR pass does not close this frame.
        let mut other = key(4);
        other.family = 3;
        ledger.take(other);
        assert!(!ledger.cloud_admissible(0));
        ledger.take(key(4));
        assert!(ledger.cloud_admissible(0));
    }

    #[test]
    fn material_bias_is_on_only_inside_with_a_finite_value() {
        let mut pass: Pass = unsafe { std::mem::zeroed() };
        pass.material_texture_mip_bias = -1.25;
        assert_eq!(material_bias(&pass, true), -1.25);
        assert_eq!(material_bias(&pass, false), 0.0);
        pass.material_texture_mip_bias = f32::NAN;
        assert_eq!(material_bias(&pass, true), 0.0);
        pass.material_texture_mip_bias = f32::NEG_INFINITY;
        assert_eq!(material_bias(&pass, true), 0.0);
    }

    #[test]
    fn exclusive_owner_refuses_reentry_and_defers_release() {
        static OWNER: Exclusive<u32> = Exclusive::new();
        assert_eq!(OWNER.with(|state| *state.get_or_insert(1)), Some(1));
        let nested = OWNER.with(|state| {
            assert_eq!(*state, Some(1));
            let reentered = OWNER.with(|_| ());
            OWNER.release();
            reentered
        });
        assert_eq!(nested, Some(None));
        // The release requested during use retired the state at its end.
        assert_eq!(OWNER.with(|state| state.is_none()), Some(true));
        OWNER.with(|state| *state = Some(2));
        OWNER.release();
        assert_eq!(OWNER.with(|state| state.is_none()), Some(true));
    }

    #[test]
    fn constants_match_the_hlsl_cbuffer() {
        let constants = Constants {
            origin: [4, 2],
            size: [8, 4],
            scene_inv_size: [1.0 / 16.0, 1.0 / 8.0],
            mode: MODE_LAYER,
            coverage_scale: COVERAGE_SCALE,
        };
        let words: [u32; 8] = unsafe { std::mem::transmute(constants) };
        assert_eq!(
            words,
            [
                4,
                2,
                8,
                4,
                (1.0f32 / 16.0).to_bits(),
                (1.0f32 / 8.0).to_bits(),
                1,
                10.0f32.to_bits()
            ]
        );
    }
}
