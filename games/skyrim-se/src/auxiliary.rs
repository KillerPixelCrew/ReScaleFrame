// SPDX-License-Identifier: MIT
//! Native auxiliary surface selection and the scoped handoff to the Runtime.

use crate::platform::{self, Platform};
use rsf_game_sys::{rsf_game_host_services, rsf_game_render_pass};
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

const TARGETS: usize = 0x333_1f40 + 0xa58;
const TARGET_STRIDE: usize = 0x30;
const DOF_SELECTOR: usize = 0x343_6358;
const FORMATS: [u32; 4] = [44, 28, 34, 61]; // R24G8, RGBA8, RG16F, R8

#[repr(C)]
struct Surface {
    struct_size: u32,
    kind: u32,
    texture: *mut c_void,
    source_rect: [f32; 4],
    output_rect: [i32; 4],
    jitter_pixels: [f32; 2],
    name: *const c_char,
    source_name: *const c_char,
    flags: u32,
}

#[repr(C)]
struct Pass {
    struct_size: u32,
    abi_version: u32,
    pass: *const rsf_game_render_pass,
    surface_count: u32,
    surfaces: *const Surface,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Scope {
    struct_size: u32,
    abi_version: u32,
    session_id: u64,
    native_frame: u64,
    view_key: u64,
}

#[repr(C)]
struct TextureFacts {
    texture: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
}

#[repr(C)]
struct Observation {
    texture: *mut c_void,
    rtv: *mut c_void,
    srv: *mut c_void,
    name: *const c_char,
    capture: usize,
    index: u32,
    selector: u32,
    describe: bool,
}

#[repr(C)]
struct Leaves {
    ready: unsafe extern "C" fn() -> bool,
    enter: unsafe extern "C" fn() -> bool,
    leave: unsafe extern "C" fn() -> bool,
    thread: unsafe extern "C" fn() -> u32,
    hold: unsafe extern "C" fn(*mut c_void, *mut TextureFacts),
    release: unsafe extern "C" fn(*mut c_void),
    upscale: unsafe extern "C" fn(usize, *mut c_void, *mut c_void, &Pass) -> bool,
    restore: unsafe extern "C" fn(usize, *mut c_void, &Scope) -> bool,
    report: unsafe extern "C" fn(&rsf_game_host_services, u64),
    observe: unsafe extern "C" fn(
        *mut c_void,
        &rsf_game_render_pass,
        &rsf_game_host_services,
        &Observation,
    ),
}

const _: () = {
    assert!(size_of::<Surface>() == 80 && std::mem::offset_of!(Surface, flags) == 72);
    assert!(size_of::<Pass>() == 32 && std::mem::offset_of!(Pass, surfaces) == 24);
    assert!(size_of::<Scope>() == 32);
    assert!(size_of::<TextureFacts>() == 24);
    assert!(size_of::<Observation>() == 56 && std::mem::offset_of!(Observation, describe) == 48);
    assert!(size_of::<Leaves>() == 80);
};

struct Texture<'a> {
    facts: TextureFacts,
    leaves: &'a Leaves,
}

impl<'a> Texture<'a> {
    fn hold(leaves: &'a Leaves, texture: *mut c_void) -> Option<Self> {
        if texture.is_null() {
            return None;
        }
        let mut facts = TextureFacts {
            texture: ptr::null_mut(),
            width: 0,
            height: 0,
            format: 0,
        };
        unsafe { (leaves.hold)(texture, &mut facts) };
        (!facts.texture.is_null()).then(|| Self { facts, leaves })
    }
}

impl Drop for Texture<'_> {
    fn drop(&mut self) {
        unsafe { (self.leaves.release)(self.facts.texture) };
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    session: u64,
    user: usize,
    thread: u32,
}

impl Owner {
    fn new(host: &rsf_game_host_services, leaves: &Leaves) -> Self {
        Self {
            session: host.session_id,
            user: host.user.addr(),
            thread: unsafe { (leaves.thread)() },
        }
    }
}

struct Receipt {
    owner: Owner,
    scope: Scope,
    restore: usize,
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

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_auxiliaries_pending() -> bool {
    !matches!(*state(), State::Idle)
}

fn target(base: usize, index: u32) -> usize {
    base + TARGETS + index as usize * TARGET_STRIDE
}

fn surface(
    camera: &rsf_game_sys::rsf_camera_frame,
    facts: &TextureFacts,
    index: usize,
) -> Option<Surface> {
    let mask = index == 3;
    let divisor = if mask { 2 } else { 1 };
    if facts.width != camera.output_width / divisor
        || facts.height != camera.output_height / divisor
        || facts.format != FORMATS[index]
    {
        return None;
    }
    let names = [
        c"post_sr_depth",
        c"post_sr_refraction_normals",
        c"post_sr_motion",
        c"post_sr_underwater_mask",
    ];
    let dimensions = [facts.width as f32, facts.height as f32];
    let output = [camera.output_width as f32, camera.output_height as f32];
    let render = [camera.render_width as f32, camera.render_height as f32];
    let mut surface = Surface {
        struct_size: size_of::<Surface>() as u32,
        kind: u32::from(index != 0),
        texture: facts.texture,
        source_rect: [0.0, 0.0, render[0], render[1]],
        output_rect: [
            0,
            0,
            i32::try_from(facts.width).ok()?,
            i32::try_from(facts.height).ok()?,
        ],
        jitter_pixels: [0.0; 2],
        name: names[index].as_ptr(),
        source_name: match index {
            1 => c"native_refraction_normals".as_ptr(),
            3 => c"native_underwater_mask".as_ptr(),
            _ => ptr::null(),
        },
        // DOF clears the half-size underwater mask. A successful scope must preserve that clear.
        flags: u32::from(!mask),
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
    leaves: &Leaves,
    owner: Owner,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    host: &rsf_game_host_services,
) -> Option<Receipt> {
    if pass.session_id != owner.session
        || pass.camera.output_width == 0
        || pass.camera.output_height == 0
        || !unsafe { (leaves.ready)() }
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
    let normals = platform.read_usize(target(base, 13))?;
    let mask = platform.read_usize(target(base, 12))?;
    let textures = [
        Texture::hold(leaves, pass.depth)?,
        Texture::hold(leaves, ptr::with_exposed_provenance_mut(normals))?,
        Texture::hold(leaves, pass.motion)?,
        Texture::hold(leaves, ptr::with_exposed_provenance_mut(mask))?,
    ];
    let surfaces = [
        surface(&pass.camera, &textures[0].facts, 0)?,
        surface(&pass.camera, &textures[1].facts, 1)?,
        surface(&pass.camera, &textures[2].facts, 2)?,
        surface(&pass.camera, &textures[3].facts, 3)?,
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
    if !unsafe { (leaves.upscale)(upscale, host.user, context, &request) } {
        return None;
    }
    let entered = unsafe { (leaves.enter)() };
    Some(Receipt {
        owner,
        scope,
        restore,
        entered,
        restored: false,
    })
}

fn retire(leaves: &Leaves, host: &rsf_game_host_services, mut receipt: Receipt) {
    if !receipt.restored {
        if !unsafe { (leaves.restore)(receipt.restore, host.user, &receipt.scope) } {
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
            unsafe { (leaves.report)(host, receipt.scope.native_frame) };
        }
    }
    if receipt.entered && !unsafe { (leaves.leave)() } {
        *state() = State::Pending(receipt);
        return;
    }
    *state() = State::Idle;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_auxiliaries_apply(
    platform: *const Platform,
    leaves: &Leaves,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    host: &rsf_game_host_services,
) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    let owner = Owner::new(host, leaves);
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
    let Some(receipt) = apply(platform, leaves, owner, context, pass, host) else {
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
    retire(leaves, host, receipt);
    false
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_auxiliaries_finish(leaves: &Leaves, host: &rsf_game_host_services) {
    let owner = Owner::new(host, leaves);
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
        retire(leaves, host, receipt)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_auxiliaries_observe(
    platform: *const Platform,
    leaves: &Leaves,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    host: &rsf_game_host_services,
) {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return;
    };
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
        let address = target(base, index);
        let (Some(texture), Some(rtv), Some(srv)) = (
            platform.read_usize(address),
            platform.read_usize(address + 0x10),
            platform.read_usize(address + 0x18),
        ) else {
            continue;
        };
        let observation = Observation {
            texture: ptr::with_exposed_provenance_mut(texture),
            rtv: ptr::with_exposed_provenance_mut(rtv),
            srv: ptr::with_exposed_provenance_mut(srv),
            name: name.as_ptr(),
            index,
            selector,
            describe,
            capture: if i < 3 && (!rsf_skyrim_auxiliaries_pending() || i == 2) {
                capture
            } else {
                0
            },
        };
        unsafe { (leaves.observe)(context, pass, host, &observation) };
    }
    *DESCRIBED.lock().unwrap_or_else(PoisonError::into_inner) = dims;
}
