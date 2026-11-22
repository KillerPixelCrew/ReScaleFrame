// SPDX-License-Identifier: MIT
//! Skyrim's pre-HDR SR and FG producers use the same native texture admission.

use crate::camera::render::{Leaves as CameraLeaves, RenderCamera, accept_sr, read_frame};
use crate::platform::{self, Platform};
use crate::presentation::{begin_inputs, empty_pass, identify, same_source};
use rsf_game_sys::{
    RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_SR,
    rsf_game_host_services, rsf_game_render_pass,
};
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const PRE_HDR_CALL: u64 = 0x153_ade7;
const NATIVE_FRAME: usize = 0x333_63f4;
const IMMEDIATE_CONTEXT: usize = 0x333_1f30;
const TARGETS: usize = 0x333_1f40 + 0xa58;
const TARGET_STRIDE: usize = 0x30;
const MAIN_DEPTH: usize = 0x333_3f58;
const RATIO_LOCK: usize = 0x333_64c8;
const GAMEPLAY: u32 = 8;
const R11G11B10_FLOAT: u32 = 26;
const R16G16B16A16_FLOAT: u32 = 10;
const R24G8_TYPELESS: u32 = 44;
const R16G16_FLOAT: u32 = 34;

#[repr(C)]
#[derive(Default)]
struct TextureFacts {
    device: usize,
    width: u32,
    height: u32,
    samples: u32,
    slices: u32,
    mips: u32,
    format: u32,
}

impl TextureFacts {
    fn matches(&self, device: usize, output: [u32; 2]) -> bool {
        self.device == device
            && [self.width, self.height] == output
            && self.samples == 1
            && self.slices == 1
            && self.mips == 1
    }
}

#[repr(C)]
struct Viewport {
    render: [u32; 2],
    output: [u32; 2],
    ratios: [f32; 2],
    lock: u32,
}

type Upscale =
    unsafe extern "C" fn(*mut c_void, &rsf_game_render_pass, &rsf_game_host_services) -> bool;

#[repr(C)]
struct Leaves {
    camera: CameraLeaves,
    reference: unsafe extern "C" fn(*mut c_void, bool),
    context_device: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    chain_device: unsafe extern "C" fn(*mut c_void) -> usize,
    texture: unsafe extern "C" fn(*mut c_void, *mut TextureFacts),
    thread: unsafe extern "C" fn() -> u32,
    identity: unsafe extern "C" fn(&mut rsf_game_render_pass) -> bool,
    eligible: unsafe extern "C" fn() -> bool,
    diagnostic: unsafe extern "C" fn(&rsf_game_render_pass),
    viewport: unsafe extern "C" fn(*mut c_void, &rsf_game_host_services, &Viewport),
    upscale: Upscale,
    auxiliaries: Upscale,
    observe: unsafe extern "C" fn(*mut c_void, &rsf_game_render_pass, &rsf_game_host_services),
    publish: Upscale,
}

const _: () = {
    assert!(size_of::<TextureFacts>() == 32);
    assert!(size_of::<Viewport>() == 28);
    assert!(size_of::<Leaves>() == 120);
};

struct Lease<'a> {
    ptr: NonNull<c_void>,
    leaves: &'a Leaves,
}

impl<'a> Lease<'a> {
    fn hold(leaves: &'a Leaves, address: usize) -> Option<Self> {
        let ptr = NonNull::new(ptr::with_exposed_provenance_mut(address))?;
        unsafe { (leaves.reference)(ptr.as_ptr(), true) };
        Some(Self { ptr, leaves })
    }

    fn texture(&self) -> TextureFacts {
        let mut facts = TextureFacts::default();
        unsafe { (self.leaves.texture)(self.ptr.as_ptr(), &mut facts) };
        facts
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        unsafe { (self.leaves.reference)(self.ptr.as_ptr(), false) };
    }
}

struct Inputs<'a> {
    context: Lease<'a>,
    colour: Lease<'a>,
    depth: Lease<'a>,
    motion: Lease<'a>,
    chain: Lease<'a>,
    mask: Option<Lease<'a>>,
    _device: Lease<'a>,
}

static SCENE_THREAD: AtomicU32 = AtomicU32::new(0);
static LOGGED_EXTENT: AtomicU64 = AtomicU64::new(0);

fn log_viewport(
    platform: &Platform,
    base: usize,
    leaves: &Leaves,
    context: *mut c_void,
    view: &RenderCamera,
    host: &rsf_game_host_services,
) {
    if host.log.is_none() {
        return;
    }
    let key = u64::from(view.render[0]) << 32 | u64::from(view.render[1]);
    if LOGGED_EXTENT.load(Ordering::Relaxed) == key {
        return;
    }
    let Some(lock) = platform.read_u32(base + RATIO_LOCK) else {
        return;
    };
    LOGGED_EXTENT.store(key, Ordering::Relaxed);
    let report = Viewport {
        render: view.render,
        output: view.output,
        ratios: view.ratios,
        lock,
    };
    unsafe { (leaves.viewport)(context, host, &report) };
}

// Numeric X/Y rejection channels are permitted, including the four-channel equivalents.
fn mask_format(format: u32) -> bool {
    matches!(
        format,
        49 | 51 | 28 | 31 | 34 | 35 | 37 | 10 | 11 | 13 | 16 | 2
    )
}

fn read_inputs<'a>(
    platform: &Platform,
    base: usize,
    leaves: &'a Leaves,
    view: &RenderCamera,
    host: &rsf_game_host_services,
    fg: bool,
) -> Option<Inputs<'a>> {
    let context = platform.read_usize(base + IMMEDIATE_CONTEXT)?;
    let colour = platform.read_usize(base + TARGETS + TARGET_STRIDE)?;
    let motion = platform.read_usize(base + TARGETS + 7 * TARGET_STRIDE)?;
    let depth = platform.read_usize(base + MAIN_DEPTH)?;
    let chain = platform.read_usize(view.window.checked_add(0x18)?)?;
    let context = Lease::hold(leaves, context)?;
    let colour = Lease::hold(leaves, colour)?;
    let depth = Lease::hold(leaves, depth)?;
    let motion = Lease::hold(leaves, motion)?;
    let mut mask = if fg {
        None
    } else {
        platform
            .read_usize(base + TARGETS + 85 * TARGET_STRIDE)
            .and_then(|address| Lease::hold(leaves, address))
    };
    let chain = Lease::hold(leaves, chain)?;
    // The typed callback returns one retained device from an immediate context.
    let context_ptr = context.ptr.as_ptr();
    let device = Lease {
        ptr: NonNull::new(unsafe { (leaves.context_device)(context_ptr) })?,
        leaves,
    };
    let device_id = device.ptr.as_ptr().addr();
    if !fg {
        log_viewport(platform, base, leaves, context_ptr, view, host)
    }
    let colours = colour.texture();
    let depths = depth.texture();
    let vectors = motion.texture();
    if [&colours, &depths, &vectors]
        .into_iter()
        .any(|facts| !facts.matches(device_id, view.output))
        || unsafe { (leaves.chain_device)(chain.ptr.as_ptr()) } != device_id
        || (colours.format != R11G11B10_FLOAT && (fg || colours.format != R16G16B16A16_FLOAT))
        || depths.format != R24G8_TYPELESS
        || vectors.format != R16G16_FLOAT
    {
        return None;
    }
    if mask.as_ref().is_some_and(|mask| {
        let facts = mask.texture();
        !facts.matches(device_id, view.output) || !mask_format(facts.format)
    }) {
        mask = None;
    }
    Some(Inputs {
        context,
        colour,
        depth,
        motion,
        chain,
        mask,
        _device: device,
    })
}

fn pass_identity(pass: &mut rsf_game_render_pass, view: &RenderCamera, session: u64, fg: bool) {
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.role = if fg {
        RSF_GAME_RENDER_FG_INPUTS
    } else {
        RSF_GAME_RENDER_SR
    };
    pass.session_id = session;
    let key = view.camera as u64;
    pass.family_key = key;
    pass.view_key = key;
    pass.history_key = key;
    pass.pass_key = PRE_HDR_CALL;
    pass.native_frame = u64::from(view.frame);
    pass.flags |= RSF_GAME_RENDER_PRIMARY;
}

fn bind_inputs(pass: &mut rsf_game_render_pass, view: &RenderCamera, inputs: &Inputs<'_>) {
    let window = view.window as u64;
    pass.viewport_key = window;
    pass.window_key = window;
    pass.rhi_viewport_key = window;
    pass.render_rect = [0, 0, view.render[0] as i32, view.render[1] as i32];
    pass.output_rect = [0, 0, view.output[0] as i32, view.output[1] as i32];
    pass.color_input = inputs.colour.ptr.as_ptr();
    pass.color_output = pass.color_input;
    pass.depth = inputs.depth.ptr.as_ptr();
    pass.motion = inputs.motion.ptr.as_ptr();
    pass.swapchain = inputs.chain.ptr.as_ptr();
    pass.camera_valid = 1;
    pass.motion_to_uv = [-1.0; 2];
    pass.motion_camera_included = 1;
    if let Some(mask) = &inputs.mask {
        pass.temporal_mask = mask.ptr.as_ptr();
        pass.temporal_mask_weights[..2].copy_from_slice(&[0.1, 1.0]);
    }
}

fn produce(
    platform: &Platform,
    leaves: &Leaves,
    manager: usize,
    host: &rsf_game_host_services,
    fg: bool,
) -> Option<bool> {
    let admitted = if fg {
        begin_inputs(host)
    } else {
        host.process_upscale.is_some()
    };
    if !admitted {
        return None;
    }
    let mut pass = empty_pass();
    let view = read_frame(
        platform,
        &leaves.camera,
        manager,
        fg,
        host.session_id,
        &mut pass,
    )?;
    let base = platform.main_module();
    if base == 0 {
        return None;
    }
    let inputs = read_inputs(platform, base, leaves, &view, host, fg)?;
    let context = inputs.context.ptr.as_ptr();
    pass_identity(&mut pass, &view, host.session_id, fg);
    if fg {
        if !unsafe { (leaves.identity)(&mut pass) }
            || pass.source_frame_id == 0
            || pass.submission_id == 0
            || pass.scope_id == 0
        {
            return None;
        }
        if !unsafe { (leaves.eligible)() } || pass.screen != GAMEPLAY {
            pass.flags |= RSF_GAME_RENDER_NO_FG;
        }
        bind_inputs(&mut pass, &view, &inputs);
        let mut current = empty_pass();
        if platform.read_u32(base + NATIVE_FRAME) != Some(view.frame)
            || !unsafe { (leaves.identity)(&mut current) }
            || !same_source(&current, &pass)
        {
            return None;
        }
        // These are the pre-mutation inputs. The Runtime consumes them synchronously.
        unsafe { (leaves.diagnostic)(&pass) };
        if !unsafe { (leaves.publish)(context, &pass, host) } {
            return None;
        }
        identify(&pass);
        return Some(true);
    }

    SCENE_THREAD.store(unsafe { (leaves.thread)() }, Ordering::Release);
    pass.flags |= RSF_GAME_RENDER_NO_FG;
    if host.render_pass.is_some() {
        unsafe { (leaves.identity)(&mut pass) };
    }
    pass.flags |= RSF_GAME_RENDER_NO_FG;
    bind_inputs(&mut pass, &view, &inputs);
    let upscale = if view.render == view.output {
        leaves.upscale
    } else {
        leaves.auxiliaries
    };
    let reconstructed = unsafe { upscale(context, &pass, host) };
    if reconstructed {
        accept_sr(host.session_id, view.frame);
    }
    unsafe { (leaves.observe)(context, &pass, host) };
    identify(&pass);
    Some(reconstructed)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_render_inputs(
    platform: *const Platform,
    leaves: &Leaves,
    manager: usize,
    host: &rsf_game_host_services,
    fg: bool,
) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    produce(platform, leaves, manager, host, fg).unwrap_or(false)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_renderer_thread() -> u32 {
    SCENE_THREAD.load(Ordering::Acquire)
}
