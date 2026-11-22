// SPDX-License-Identifier: MIT
//! Skyrim's pre-HDR SR and FG producers use the same native texture admission.

use crate::cpu_events::{render_diagnostic, render_eligible, render_identity};

use crate::auxiliary;
use crate::camera::render::{RenderCamera, accept_sr, read_frame};
use crate::layout::{NATIVE_FRAME, target_slot};
use crate::platform::Platform;
use crate::presentation::{begin_inputs, empty_pass, identify, same_source};
use crate::render_native::{
    ComRef, rsf_skyrim_call_render_pass, rsf_skyrim_call_upscale, thread, viewport,
};
use rsf_game_sys::{
    RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_SR,
    rsf_game_host_services, rsf_game_render_pass,
};
use std::ptr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use winapi::Interface;
use winapi::shared::dxgi::IDXGISwapChain;
use winapi::um::d3d11::{
    D3D11_DEVICE_CONTEXT_IMMEDIATE, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};

const PRE_HDR_CALL: u64 = 0x153_ade7;
const IMMEDIATE_CONTEXT: usize = 0x333_1f30;

const MAIN_DEPTH: usize = 0x333_3f58;
const RATIO_LOCK: usize = 0x333_64c8;
const GAMEPLAY: u32 = 8;
const R11G11B10_FLOAT: u32 = 26;
const R16G16B16A16_FLOAT: u32 = 10;
const R24G8_TYPELESS: u32 = 44;
const R16G16_FLOAT: u32 = 34;

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
    fn read(texture: &ComRef<ID3D11Texture2D>) -> Option<Self> {
        let mut device = ptr::null_mut();
        unsafe { (*texture.as_raw()).GetDevice(&mut device) };
        let device = unsafe { ComRef::from_owned(device) }?;
        let desc = texture.desc();
        Some(Self {
            device: device.as_raw().addr(),
            width: desc.Width,
            height: desc.Height,
            samples: desc.SampleDesc.Count,
            slices: desc.ArraySize,
            mips: desc.MipLevels,
            format: desc.Format,
        })
    }

    fn matches(&self, device: usize, output: [u32; 2]) -> bool {
        self.device == device
            && [self.width, self.height] == output
            && self.samples == 1
            && self.slices == 1
            && self.mips == 1
    }
}

struct Inputs {
    context: ComRef<ID3D11DeviceContext>,
    colour: ComRef<ID3D11Texture2D>,
    depth: ComRef<ID3D11Texture2D>,
    motion: ComRef<ID3D11Texture2D>,
    chain: ComRef<IDXGISwapChain>,
    mask: Option<ComRef<ID3D11Texture2D>>,
    _device: ComRef<ID3D11Device>,
}

static SCENE_THREAD: AtomicU32 = AtomicU32::new(0);
static LOGGED_EXTENT: AtomicU64 = AtomicU64::new(0);

fn log_viewport(
    platform: &Platform,
    base: usize,
    context: *mut ID3D11DeviceContext,
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
    let (viewport, count) = unsafe { viewport(context) };
    let [rw, rh] = view.render;
    let [width, height] = view.output;
    let [x, y] = view.ratios;
    platform.log_text(host, &format!(
        "Skyrim pre-HDR native viewport {}x{} ({count} views); active inputs {rw}x{rh} allocation {width}x{height} lock={lock} ratios={x},{y}",
        viewport.Width, viewport.Height,
    ));
}

// Numeric X/Y rejection channels are permitted, including the four-channel equivalents.
fn mask_format(format: u32) -> bool {
    matches!(
        format,
        49 | 51 | 28 | 31 | 34 | 35 | 37 | 10 | 11 | 13 | 16 | 2
    )
}

fn read_inputs(
    platform: &Platform,
    base: usize,
    view: &RenderCamera,
    host: &rsf_game_host_services,
    fg: bool,
) -> Option<Inputs> {
    let context = platform.read_usize(base + IMMEDIATE_CONTEXT)?;
    let colour = platform.read_usize(target_slot(base, 1))?;
    let motion = platform.read_usize(target_slot(base, 7))?;
    let depth = platform.read_usize(base + MAIN_DEPTH)?;
    let chain = platform.read_usize(view.window.checked_add(0x18)?)?;
    let (context, colour, depth, motion, chain) = unsafe {
        (
            ComRef::<ID3D11DeviceContext>::retain(ptr::with_exposed_provenance_mut(context))?,
            ComRef::retain(ptr::with_exposed_provenance_mut(colour))?,
            ComRef::retain(ptr::with_exposed_provenance_mut(depth))?,
            ComRef::retain(ptr::with_exposed_provenance_mut(motion))?,
            ComRef::<IDXGISwapChain>::retain(ptr::with_exposed_provenance_mut(chain))?,
        )
    };
    let mut mask = if fg {
        None
    } else {
        platform
            .read_usize(target_slot(base, 85))
            .and_then(|address| unsafe {
                ComRef::retain(ptr::with_exposed_provenance_mut(address))
            })
    };
    let context_ptr = context.as_raw();
    if unsafe { (*context_ptr).GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return None;
    }
    let mut device = ptr::null_mut();
    unsafe { (*context_ptr).GetDevice(&mut device) };
    let device = unsafe { ComRef::from_owned(device) }?;
    let device_id = device.as_raw().addr();
    let mut chain_device = ptr::null_mut();
    let status = unsafe { (*chain.as_raw()).GetDevice(&ID3D11Device::uuidof(), &mut chain_device) };
    if status < 0 {
        return None;
    }
    let chain_device = unsafe { ComRef::<ID3D11Device>::from_owned(chain_device.cast()) }?;
    if !fg {
        log_viewport(platform, base, context_ptr, view, host)
    }
    let colours = TextureFacts::read(&colour)?;
    let depths = TextureFacts::read(&depth)?;
    let vectors = TextureFacts::read(&motion)?;
    if [&colours, &depths, &vectors]
        .into_iter()
        .any(|facts| !facts.matches(device_id, view.output))
        || chain_device.as_raw().addr() != device_id
        || (colours.format != R11G11B10_FLOAT && (fg || colours.format != R16G16B16A16_FLOAT))
        || depths.format != R24G8_TYPELESS
        || vectors.format != R16G16_FLOAT
    {
        return None;
    }
    if mask.as_ref().is_some_and(|mask| {
        TextureFacts::read(mask).is_none_or(|facts| {
            !facts.matches(device_id, view.output) || !mask_format(facts.format)
        })
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

fn bind_inputs(pass: &mut rsf_game_render_pass, view: &RenderCamera, inputs: &Inputs) {
    let window = view.window as u64;
    pass.viewport_key = window;
    pass.window_key = window;
    pass.rhi_viewport_key = window;
    pass.render_rect = [0, 0, view.render[0] as i32, view.render[1] as i32];
    pass.output_rect = [0, 0, view.output[0] as i32, view.output[1] as i32];
    pass.color_input = inputs.colour.as_raw().cast();
    pass.color_output = pass.color_input;
    pass.depth = inputs.depth.as_raw().cast();
    pass.motion = inputs.motion.as_raw().cast();
    pass.swapchain = inputs.chain.as_raw().cast();
    pass.camera_valid = 1;
    pass.motion_to_uv = [-1.0; 2];
    pass.motion_camera_included = 1;
    if let Some(mask) = &inputs.mask {
        pass.temporal_mask = mask.as_raw().cast();
        pass.temporal_mask_weights[..2].copy_from_slice(&[0.1, 1.0]);
    }
}

pub(crate) fn produce(
    platform: &Platform,
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
    let view = read_frame(platform, manager, fg, host.session_id, &mut pass)?;
    let base = platform.main_module();
    if base == 0 {
        return None;
    }
    let inputs = read_inputs(platform, base, &view, host, fg)?;
    let context = inputs.context.as_raw().cast();
    pass_identity(&mut pass, &view, host.session_id, fg);
    if fg {
        if !render_identity(&mut pass)
            || pass.source_frame_id == 0
            || pass.submission_id == 0
            || pass.scope_id == 0
        {
            return None;
        }
        if !render_eligible() || pass.screen != GAMEPLAY {
            pass.flags |= RSF_GAME_RENDER_NO_FG;
        }
        bind_inputs(&mut pass, &view, &inputs);
        let mut current = empty_pass();
        if platform.read_u32(base + NATIVE_FRAME) != Some(view.frame)
            || !render_identity(&mut current)
            || !same_source(&current, &pass)
        {
            return None;
        }
        // These are the pre-mutation inputs. The Runtime consumes them synchronously.
        render_diagnostic(c"FG_INPUTS", &pass);
        if !unsafe {
            rsf_skyrim_call_render_pass(host, context, &pass, 1)
                && rsf_skyrim_call_render_pass(host, context, &pass, 0)
        } {
            return None;
        }
        identify(&pass);
        return Some(true);
    }

    SCENE_THREAD.store(thread(), Ordering::Release);
    pass.flags |= RSF_GAME_RENDER_NO_FG;
    if host.render_pass.is_some() {
        render_identity(&mut pass);
    }
    pass.flags |= RSF_GAME_RENDER_NO_FG;
    bind_inputs(&mut pass, &view, &inputs);
    let reconstructed = if view.render == view.output {
        unsafe { rsf_skyrim_call_upscale(host, context, &pass) }
    } else {
        auxiliary::reconstruct(platform, context, &pass, host)
    };
    if reconstructed {
        accept_sr(host.session_id, view.frame);
    }
    auxiliary::observe(platform, context, &pass, host);
    identify(&pass);
    Some(reconstructed)
}

pub(crate) fn renderer_thread() -> u32 {
    SCENE_THREAD.load(Ordering::Acquire)
}
