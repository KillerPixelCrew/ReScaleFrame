// SPDX-License-Identifier: MIT
//! Native auxiliary surface selection and the scoped handoff to the Runtime.

use crate::layout::target_slot;
use crate::platform::Platform;
use crate::render_native::{ComRef, thread, viewport};
use crate::resolution;
use rsf_game_sys::rsf_d3d11_host_capture as Capture;
use rsf_game_sys::{
    RSF_D3D11_AUXILIARY_COLOUR, RSF_D3D11_AUXILIARY_DEPTH_STENCIL,
    RSF_D3D11_AUXILIARY_RESTORE_CONTENTS, rsf_d3d11_auxiliary_pass as Pass,
    rsf_d3d11_auxiliary_scope as Scope, rsf_d3d11_auxiliary_surface as Surface,
};
use rsf_game_sys::{rsf_game_host_services, rsf_game_render_pass};
use std::ffi::{CStr, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use winapi::um::d3d11::{
    D3D11_RENDER_TARGET_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC, ID3D11RenderTargetView,
    ID3D11ShaderResourceView, ID3D11Texture2D,
};

const DOF_SELECTOR: usize = 0x343_6358;
type Upscale = unsafe extern "C" fn(*mut c_void, *mut c_void, &Pass) -> i32;
type Restore = unsafe extern "C" fn(*mut c_void, &Scope) -> i32;
type CaptureTexture = unsafe extern "C" fn(*mut c_void, *mut c_void, &Capture) -> i32;

const FORMATS: [u32; 4] = [44, 28, 34, 61]; // R24G8, RGBA8, RG16F, R8

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    session: u64,
    user: usize,
    thread: u32,
}

impl Owner {
    fn new(host: &rsf_game_host_services) -> Self {
        Self {
            session: host.session_id,
            user: host.user.addr(),
            thread: thread(),
        }
    }
}

struct Receipt {
    owner: Owner,
    scope: Scope,
    restore: Restore,
    entered: bool,
    restored: bool,
}

enum State {
    Idle,
    Applying { owner: Owner, finish: bool },
    Pending(Receipt),
    Retiring,
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
static RESTORE_REPORTS: AtomicU32 = AtomicU32::new(0);
static CAPTURE: OnceLock<usize> = OnceLock::new();
static DESCRIBED: Mutex<[u32; 3]> = Mutex::new([0, 0, u32::MAX]);

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn pending() -> bool {
    !matches!(*state(), State::Idle)
}

fn surface(
    camera: &rsf_game_sys::rsf_camera_frame,
    texture: &ComRef<ID3D11Texture2D>,
    index: usize,
) -> Option<Surface> {
    let facts = texture.desc();
    let mask = index == 3;
    let divisor = if mask { 2 } else { 1 };
    if facts.Width != camera.output_width / divisor
        || facts.Height != camera.output_height / divisor
        || facts.Format != FORMATS[index]
    {
        return None;
    }
    let names = [
        c"post_sr_depth",
        c"post_sr_refraction_normals",
        c"post_sr_motion",
        c"post_sr_underwater_mask",
    ];
    let dimensions = [facts.Width as f32, facts.Height as f32];
    let output = [camera.output_width as f32, camera.output_height as f32];
    let render = [camera.render_width as f32, camera.render_height as f32];
    let mut surface = Surface {
        struct_size: size_of::<Surface>() as u32,
        kind: if index == 0 {
            RSF_D3D11_AUXILIARY_DEPTH_STENCIL
        } else {
            RSF_D3D11_AUXILIARY_COLOUR
        },
        texture: texture.as_raw().cast(),
        source_rect: [0.0, 0.0, render[0], render[1]],
        output_rect: [
            0,
            0,
            i32::try_from(facts.Width).ok()?,
            i32::try_from(facts.Height).ok()?,
        ],
        jitter_pixels: [0.0; 2],
        name: names[index].as_ptr(),
        source_name: match index {
            1 => c"native_refraction_normals".as_ptr(),
            3 => c"native_underwater_mask".as_ptr(),
            _ => ptr::null(),
        },
        // DOF clears the half-size underwater mask. A successful scope must preserve that clear.
        flags: if mask {
            0
        } else {
            RSF_D3D11_AUXILIARY_RESTORE_CONTENTS
        },
    };
    for i in 0..2 {
        if mask {
            surface.source_rect[2 + i] = dimensions[i] * (render[i] / output[i]);
        } else {
            surface.jitter_pixels[i] = -camera.jitter_pixels[i] * dimensions[i] / output[i];
        }
    }
    Some(surface)
}

fn apply(
    platform: &Platform,
    owner: Owner,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    host: &rsf_game_host_services,
) -> Option<Receipt> {
    if pass.session_id != owner.session
        || pass.camera.output_width == 0
        || pass.camera.output_height == 0
        || !resolution::output_ready(platform, pass)
    {
        return None;
    }
    let runtime = platform.module_handle("ReScaleFrame.Runtime.dll");
    let upscale = platform.export_address(runtime, c"rsf_d3d11_host_upscale_auxiliaries");
    let restore = platform.export_address(runtime, c"rsf_d3d11_host_restore_auxiliaries");
    if upscale == 0 || restore == 0 {
        return None;
    }
    let base = platform.main_module();
    if base == 0 {
        return None;
    }
    let normals = platform.read_usize(target_slot(base, 13))?;
    let mask = platform.read_usize(target_slot(base, 12))?;
    let textures = unsafe {
        [
            ComRef::retain(pass.depth.cast())?,
            ComRef::retain(ptr::with_exposed_provenance_mut(normals))?,
            ComRef::retain(pass.motion.cast())?,
            ComRef::retain(ptr::with_exposed_provenance_mut(mask))?,
        ]
    };
    let surfaces = [
        surface(&pass.camera, &textures[0], 0)?,
        surface(&pass.camera, &textures[1], 1)?,
        surface(&pass.camera, &textures[2], 2)?,
        surface(&pass.camera, &textures[3], 3)?,
    ];
    let scope = Scope {
        struct_size: size_of::<Scope>() as u32,
        abi_version: platform.d3d11_host_abi,
        session_id: pass.session_id,
        native_frame: pass.native_frame,
        view_key: pass.view_key,
    };
    let request = Pass {
        struct_size: size_of::<Pass>() as u32,
        abi_version: scope.abi_version,
        pass,
        surface_count: surfaces.len() as u32,
        surfaces: surfaces.as_ptr(),
    };
    let upscale: Upscale = unsafe { std::mem::transmute(upscale) };
    if unsafe { upscale(host.user, context, &request) } != 0 {
        return None;
    }
    let entered = resolution::enter(platform, pass);
    Some(Receipt {
        owner,
        scope,
        restore: unsafe { std::mem::transmute::<usize, Restore>(restore) },
        entered,
        restored: false,
    })
}

fn retire(platform: &Platform, host: &rsf_game_host_services, mut receipt: Receipt) {
    if !receipt.restored {
        if unsafe { (receipt.restore)(host.user, &receipt.scope) } != 0 {
            *state() = State::Pending(receipt);
            return;
        }
        receipt.restored = true;
        if host.log.is_some()
            && RESTORE_REPORTS
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    (n < 4).then_some(n + 1)
                })
                .is_ok()
        {
            report_restore(platform, host, receipt.scope.native_frame);
        }
    }
    if receipt.entered && !resolution::leave() {
        *state() = State::Pending(receipt);
        return;
    }
    *state() = State::Idle;
}

pub(crate) fn reconstruct(
    platform: &Platform,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    host: &rsf_game_host_services,
) -> bool {
    let owner = Owner::new(host);
    {
        let mut state = state();
        if !matches!(*state, State::Idle) {
            return false;
        }
        *state = State::Applying {
            owner,
            finish: false,
        };
    }
    let Some(receipt) = apply(platform, owner, context, pass, host) else {
        *state() = State::Idle;
        return false;
    };
    {
        let mut state = state();
        if receipt.entered && !matches!(*state, State::Applying { finish: true, .. }) {
            *state = State::Pending(receipt);
            return true;
        }
        *state = State::Retiring;
    }
    retire(platform, host, receipt);
    false
}

pub(crate) fn finish(platform: &Platform, host: &rsf_game_host_services) {
    let owner = Owner::new(host);
    let receipt = {
        let mut state = state();
        match &mut *state {
            State::Applying {
                owner: active,
                finish,
            } if *active == owner => {
                *finish = true;
                return;
            }
            State::Pending(receipt) if receipt.owner == owner => {
                std::mem::replace(&mut *state, State::Retiring)
            }
            _ => return,
        }
    };
    if let State::Pending(receipt) = receipt {
        retire(platform, host, receipt)
    }
}

pub(crate) fn observe(
    platform: &Platform,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    host: &rsf_game_host_services,
) {
    let base = platform.main_module();
    if base == 0 {
        return;
    }
    let mut selector = [0; 4];
    if !platform.read(base + DOF_SELECTOR, &mut selector) {
        return;
    }
    let selector = u32::from_le_bytes(selector);
    let capture = *CAPTURE.get_or_init(|| {
        let runtime = platform.module_handle("ReScaleFrame.Runtime.dll");
        platform.export_address(runtime, c"rsf_d3d11_host_capture_texture")
    });
    let dims = [
        pass.camera.render_width,
        pass.camera.render_height,
        selector,
    ];
    let describe =
        *DESCRIBED.lock().unwrap_or_else(PoisonError::into_inner) != dims && host.log.is_some();
    let observations = [
        (12, c"native_underwater_mask"),
        (13, c"native_refraction_normals"),
        (49, c"native_sao_camera_z"),
        (49_u32.wrapping_add(selector), c"native_sao_dof_view"),
    ];
    for (i, (index, name)) in observations.into_iter().enumerate() {
        if i == 3 && selector > 12 {
            continue;
        }
        let address = target_slot(base, index as usize);
        let (Some(texture), Some(rtv), Some(srv)) = (
            platform.read_usize(address),
            platform.read_usize(address + 0x10),
            platform.read_usize(address + 0x18),
        ) else {
            continue;
        };
        let capture = if i < 3 && (!pending() || i == 2) {
            capture
        } else {
            0
        };
        if describe || capture != 0 {
            describe_allocation(
                platform,
                context,
                pass,
                host,
                Observation {
                    texture,
                    rtv,
                    srv,
                    name,
                    index,
                    selector,
                    describe,
                    capture,
                },
            );
        }
    }
    *DESCRIBED.lock().unwrap_or_else(PoisonError::into_inner) = dims;
}

fn report_restore(platform: &Platform, host: &rsf_game_host_services, frame: u64) {
    let base = platform.main_module();
    if base == 0 {
        return;
    }
    let Some(context) = platform
        .read_usize(base + 0x333_1f30)
        .filter(|&context| context != 0)
    else {
        return;
    };
    let mut ratios = [[0; 4]; 4];
    if !platform.read(base + 0x333_64b4, ratios.as_flattened_mut()) {
        return;
    }
    let [x, y, previous_x, previous_y] = ratios.map(f32::from_le_bytes);
    let (viewport, _) = unsafe { viewport(ptr::with_exposed_provenance_mut(context)) };
    platform.log_text(host, &format!(
        "Skyrim auxiliary scope restored after native frame {frame}; final viewport={}x{} ratios={x},{y},{previous_x},{previous_y} before producer-state restoration",
        viewport.Width, viewport.Height,
    ));
}

struct Observation {
    texture: usize,
    rtv: usize,
    srv: usize,
    name: &'static CStr,
    index: u32,
    selector: u32,
    describe: bool,
    capture: usize,
}

fn describe_allocation(
    platform: &Platform,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    host: &rsf_game_host_services,
    facts: Observation,
) {
    let (texture, target, view) = unsafe {
        (
            ComRef::<ID3D11Texture2D>::retain(ptr::with_exposed_provenance_mut(facts.texture)),
            ComRef::<ID3D11RenderTargetView>::retain(ptr::with_exposed_provenance_mut(facts.rtv)),
            ComRef::<ID3D11ShaderResourceView>::retain(ptr::with_exposed_provenance_mut(facts.srv)),
        )
    };
    // Mip aliases can lack a texture in the native slot. GetResource names the
    // allocation and does not establish the active mip.
    let texture = texture.or_else(|| {
        let mut resource = ptr::null_mut();
        unsafe {
            if let Some(view) = &view {
                (*view.as_raw()).GetResource(&mut resource);
            } else if let Some(target) = &target {
                (*target.as_raw()).GetResource(&mut resource);
            }
            ComRef::from_owned(resource)
        }?
        .query::<ID3D11Texture2D>()
    });
    let Some(texture) = texture else {
        return;
    };
    if facts.describe {
        let d = texture.desc();
        let (mut r, mut s): (
            D3D11_RENDER_TARGET_VIEW_DESC,
            D3D11_SHADER_RESOURCE_VIEW_DESC,
        ) = unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
        let (rtv_mip, srv_mip, srv_levels) = unsafe {
            if let Some(target) = &target {
                (*target.as_raw()).GetDesc(&mut r);
            }
            if let Some(view) = &view {
                (*view.as_raw()).GetDesc(&mut s);
            }
            (
                r.u.Texture2D().MipSlice,
                s.u.Texture2D().MostDetailedMip,
                s.u.Texture2D().MipLevels,
            )
        };
        platform.log_text(host, &format!(
            "Skyrim auxiliary {} target={} selector={} native_texture=0x{:x} allocation={:p} size={}x{} format={} mips={} slices={} samples={} bind=0x{:x} RTV={}:{}:mip{} SRV={}:{}:mip{}+{} native_frame={}; descriptor only, active coverage unverified",
            facts.name.to_string_lossy(), facts.index, facts.selector, facts.texture, texture.as_raw(),
            d.Width, d.Height, d.Format, d.MipLevels, d.ArraySize, d.SampleDesc.Count, d.BindFlags,
            r.Format, r.ViewDimension, rtv_mip, s.Format, s.ViewDimension, srv_mip, srv_levels,
            pass.native_frame,
        ));
    }
    if facts.capture != 0 {
        let request = Capture {
            struct_size: size_of::<Capture>() as u32,
            abi_version: platform.d3d11_host_abi,
            session_id: pass.session_id,
            native_frame: pass.native_frame,
            view_key: pass.view_key,
            name: facts.name.as_ptr(),
            description: c"Native pre-HDR auxiliary allocation".as_ptr(),
            texture: texture.as_raw().cast(),
            view: 0,
            scale: 1.0,
        };
        let capture: CaptureTexture = unsafe { std::mem::transmute(facts.capture) };
        unsafe { capture(host.user, context, &request) };
    }
}
