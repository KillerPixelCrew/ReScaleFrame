// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::constant_buffer_read::slot;
use crate::d3d11_output::{FullscreenBlit, same_device, texture_desc};
use crate::d3d11_output_abi::{
    RSF_D3D11_BIND_RENDER_TARGET, RSF_D3D11_BIND_SHADER_RESOURCE, RSF_D3D11_USAGE_DEFAULT,
    rsf_d3d11_output_copy_resource, rsf_d3d11_output_create_srv, rsf_d3d11_output_create_texture,
    rsf_d3d11_output_is_immediate, rsf_d3d11_output_srv_format, rsf_d3d11_texture_desc,
    rsf_fullscreen_pass,
};
use crate::frame_tap_abi::{
    RSF_FRAME_TAP_MAX_SUBSTITUTIONS, rsf_frame_tap_plan as Plan, rsf_frame_tap_substitution as Item,
};
use crate::scene_promote_abi::{
    RSF_PROMOTE_ABI_VERSION, RSF_PROMOTE_MAX_CHAIN_TARGETS, RSF_PROMOTE_MAX_UI_TARGETS,
    rsf_promote_frame_tail as Tail, rsf_promote_setup as Setup, rsf_promote_status as Status,
};
use crate::sr_session::has_capacity;
use std::ffi::{CStr, CString, c_void};
use std::fmt::Display;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, MutexGuard, TryLockError};

type Handle = *mut c_void;

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const RESOURCE_FAILED: i32 = -3;
const NOT_SCALED: i32 = -4;

const UI_TARGETS: usize = RSF_PROMOTE_MAX_UI_TARGETS as usize;
const CHAIN_TARGETS: usize = RSF_PROMOTE_MAX_CHAIN_TARGETS as usize;
const _: () = assert!(5 + UI_TARGETS + CHAIN_TARGETS <= RSF_FRAME_TAP_MAX_SUBSTITUTIONS as usize);

const BGRX8_UNORM: u32 = 88;
const BGRX8_TYPELESS: u32 = 92;

fn typeless_family(format: u32) -> u32 {
    match format {
        9..=14 => 9,        // R16G16B16A16
        23..=25 => 23,      // R10G10B10A2
        27..=32 => 27,      // R8G8B8A8
        87 | 90 | 91 => 90, // B8G8R8A8
        88 | 92 | 93 => BGRX8_TYPELESS,
        _ => format,
    }
}

struct TextureFacts {
    width: u32,
    height: u32,
    format: u32,
    default_view_format: u32,
}

impl TextureFacts {
    fn view_format(&self, hint: u32) -> u32 {
        // The chain's hint can come from a different surface family.
        if hint != 0 && typeless_family(hint) == typeless_family(self.format) {
            hint
        } else if self.format == BGRX8_TYPELESS {
            BGRX8_UNORM
        } else {
            self.default_view_format
        }
    }
}

// The fullscreen pass remains behind its public C ABI.
#[repr(C)]
struct FullscreenSetup {
    struct_size: u32,
    abi_version: u32,
    log: Option<unsafe extern "C" fn(Handle, *const std::ffi::c_char)>,
    log_user: Handle,
}

#[repr(C)]
struct FullscreenDraw {
    struct_size: u32,
    mode: u32,
    exposure: f32,
    width: u32,
    height: u32,
}

unsafe extern "C" {
    fn rsf_fullscreen_pass_create(
        device: Handle,
        setup: *const FullscreenSetup,
        pass: *mut *mut rsf_fullscreen_pass,
    ) -> i32;
    fn rsf_fullscreen_pass_draw(
        pass: *mut rsf_fullscreen_pass,
        context: Handle,
        target: Handle,
        source: Handle,
        draw: *const FullscreenDraw,
    ) -> i32;
    fn rsf_fullscreen_pass_destroy(pass: *mut rsf_fullscreen_pass);
}

unsafe extern "C" fn create_blit(device: Handle) -> *mut rsf_fullscreen_pass {
    let setup = FullscreenSetup {
        struct_size: size_of::<FullscreenSetup>() as u32,
        abi_version: 1,
        log: None,
        log_user: null_mut(),
    };
    let mut pass = null_mut();
    if unsafe { rsf_fullscreen_pass_create(device, &setup, &mut pass) } == OK {
        pass
    } else {
        null_mut()
    }
}

unsafe extern "C" fn draw_blit(
    pass: *mut rsf_fullscreen_pass,
    context: Handle,
    target: Handle,
    source: Handle,
    width: u32,
    height: u32,
) -> u32 {
    let draw = FullscreenDraw {
        struct_size: size_of::<FullscreenDraw>() as u32,
        mode: 0, // RSF_FULLSCREEN_COPY
        exposure: 0.0,
        width,
        height,
    };
    u32::from(unsafe { rsf_fullscreen_pass_draw(pass, context, target, source, &draw) } == OK)
}

// ID3D11Device::CreateRenderTargetView takes a D3D11_RENDER_TARGET_VIEW_DESC.
// Its largest union member occupies three UINTs and Texture2D uses only the first.
#[repr(C)]
struct TargetDesc {
    format: u32,
    dimension: u32,
    union_fields: [u32; 3],
}

unsafe fn make_target(device: Handle, texture: Handle, format: u32) -> Option<ComRef> {
    type Create = unsafe extern "system" fn(Handle, Handle, *const TargetDesc, *mut Handle) -> i32;
    let create: Create = unsafe { std::mem::transmute(slot(device, 9)) };
    let desc = TargetDesc {
        format,
        dimension: 4, // D3D11_RTV_DIMENSION_TEXTURE2D
        union_fields: [0; 3],
    };
    let mut view = null_mut();
    if unsafe { create(device, texture, &desc, &mut view) } < 0 {
        return None;
    }
    unsafe { ComRef::from_owned(view) }
}

unsafe fn make_default_view(device: Handle, texture: Handle) -> Handle {
    type Create = unsafe extern "system" fn(Handle, Handle, *const c_void, *mut Handle) -> i32;
    let create: Create = unsafe { std::mem::transmute(slot(device, 7)) };
    let mut view = null_mut();
    if unsafe { create(device, texture, null(), &mut view) } < 0 {
        return null_mut();
    }
    view
}

const _: () = {
    assert!(size_of::<TargetDesc>() == 20);
    assert!(size_of::<FullscreenDraw>() == 20);
    assert!(size_of::<FullscreenSetup>() == 8 + 2 * size_of::<Handle>());
};

struct Replacement {
    // The original identifies a pooled texture. The caller keeps it alive.
    original: Handle,
    texture: ComRef,
    target: ComRef,
    view: ComRef,
    format: u32,
}

impl Replacement {
    fn substitution(&self, gate: Handle) -> Item {
        Item {
            texture: self.original,
            shader_view: self.view.as_ptr(),
            render_view: self.target.as_ptr(),
            after_target: gate,
        }
    }
}

struct Recombine {
    composed: Replacement,
    scene: Replacement,
    scratch: Replacement,
    source_view: ComRef,
    layer: Option<Item>,
}

struct Frame {
    composite: Replacement,
    ui: [Option<Replacement>; UI_TARGETS],
    chain: [Option<Replacement>; CHAIN_TARGETS],
    scene_color: Handle,
    reconstruction: ComRef,
    recombine: Option<Recombine>,
}

#[derive(Default)]
struct State {
    frame: Option<Frame>,
    seed_pass: Option<FullscreenBlit>,
    render: [u32; 2],
}

struct Promote {
    state: Mutex<State>,
    device: ComRef,
    setup: Setup,
}

enum Draw {
    Seed { reconstructed: bool },
    Finish,
}

impl Promote {
    fn state(&self) -> Result<MutexGuard<'_, State>, i32> {
        match self.state.try_lock() {
            Ok(state) => Ok(state),
            Err(TryLockError::Poisoned(error)) => Ok(error.into_inner()),
            Err(TryLockError::WouldBlock) => Err(RESOURCE_FAILED),
        }
    }

    fn log(&self, message: impl Display) {
        if let Some(log) = self.setup.log
            && let Ok(message) = CString::new(message.to_string())
        {
            unsafe { log(self.setup.log_user, message.as_ptr()) };
        }
    }

    fn describe(&self, texture: Handle) -> TextureFacts {
        let desc = unsafe { texture_desc(texture) };
        TextureFacts {
            width: desc.width,
            height: desc.height,
            format: desc.format,
            default_view_format: unsafe { rsf_d3d11_output_srv_format(desc.format) },
        }
    }

    fn view(&self, texture: Handle, format: Option<u32>) -> Option<ComRef> {
        let device = self.device.as_ptr();
        unsafe {
            ComRef::from_owned(match format {
                Some(format) => rsf_d3d11_output_create_srv(device, texture, format),
                None => make_default_view(device, texture),
            })
        }
    }

    fn source_view(&self, texture: Handle, typed: bool) -> Option<ComRef> {
        if !unsafe { same_device(self.device.as_ptr(), texture) } {
            return None;
        }
        let format = if typed {
            Some(self.describe(texture).view_format(0))
        } else {
            None
        };
        self.view(texture, format)
    }

    fn make_replacement(&self, original: Handle, hint: u32, role: &CStr) -> Option<Replacement> {
        let device = self.device.as_ptr();
        let role_name = role.to_string_lossy();
        if !unsafe { same_device(device, original) } {
            self.log(format_args!(
                "promote: the {role_name} belongs to another device"
            ));
            return None;
        }
        let facts = self.describe(original);
        let (width, height) = (self.setup.output_width, self.setup.output_height);
        self.log(format_args!(
            "promote: the {role_name} {original:p}, {}x{} format {}, becomes {width}x{height}",
            facts.width, facts.height, facts.format
        ));
        let desc = rsf_d3d11_texture_desc {
            width,
            height,
            mips: 1,
            layers: 1,
            format: facts.format,
            samples: 1,
            sample_quality: 0,
            usage: RSF_D3D11_USAGE_DEFAULT,
            bind_flags: RSF_D3D11_BIND_RENDER_TARGET | RSF_D3D11_BIND_SHADER_RESOURCE,
            cpu_access_flags: 0,
            misc_flags: 0,
        };
        let texture = unsafe { ComRef::from_owned(rsf_d3d11_output_create_texture(device, &desc)) };
        let Some(texture) = texture else {
            self.log(format_args!(
                "promote: the {role_name} replacement could not be created"
            ));
            return None;
        };
        let format = facts.view_format(hint);
        let target = unsafe { make_target(device, texture.as_ptr(), format) };
        let view = target
            .as_ref()
            .and_then(|_| self.view(texture.as_ptr(), Some(format)));
        let Some(view) = view else {
            self.log(format_args!(
                "promote: the {role_name} replacement views could not be created with view format {format}"
            ));
            return None;
        };
        Some(Replacement {
            original,
            texture,
            target: target?,
            view,
            format: facts.format,
        })
    }

    fn make_set<const N: usize>(
        &self,
        originals: &[Handle; N],
        offered: u32,
        hint: u32,
        role: &CStr,
    ) -> Option<[Option<Replacement>; N]> {
        let mut set: [Option<Replacement>; N] = std::array::from_fn(|_| None);
        let mut count = 0;
        for &original in originals.iter().take(offered as usize) {
            if original.is_null() || set.iter().flatten().any(|entry| entry.original == original) {
                continue;
            }
            set[count] = Some(self.make_replacement(original, hint, role)?);
            count += 1;
        }
        Some(set)
    }

    fn build_frame(&self, tail: &Tail, seed_pass: &mut Option<FullscreenBlit>) -> Option<Frame> {
        let composite =
            self.make_replacement(tail.composite, tail.composite_view_format, c"composite")?;
        let ui = self.make_set(
            &tail.ui_targets,
            tail.ui_target_count,
            tail.ui_target_view_format,
            c"interface layer",
        )?;
        let chain = self.make_set(
            &tail.chain_targets,
            tail.chain_target_count,
            tail.chain_view_format,
            c"chain target",
        )?;
        let recombine = if tail.composed.is_null() {
            None
        } else {
            let composed = self.make_replacement(tail.composed, 0, c"recombined colour")?;
            let scene = self.make_replacement(tail.scene_color, 0, c"scene colour")?;
            let scratch =
                self.make_replacement(tail.scene_color, 0, c"scene colour's discarded writes")?;
            let source_view = self.source_view(tail.scene_color, true);
            let Some(source_view) = source_view else {
                self.log("promote: scene colour could not be made readable for the fallback seed");
                return None;
            };
            if seed_pass.is_none() {
                *seed_pass = unsafe {
                    FullscreenBlit::new(
                        &self.device,
                        create_blit,
                        draw_blit,
                        rsf_fullscreen_pass_destroy,
                    )
                };
                if seed_pass.is_none() {
                    self.log("promote: the seed pass could not be created");
                    return None;
                }
            }
            let layer = (!tail.layer.is_null() && !tail.layer_view.is_null()).then_some(Item {
                texture: tail.layer,
                shader_view: tail.layer_view,
                render_view: null_mut(),
                after_target: composed.original,
            });
            Some(Recombine {
                composed,
                scene,
                scratch,
                source_view,
                layer,
            })
        };
        if ui[0].is_none() {
            self.log(
                "promote: no interface layer named; the interface is magnified with the scene",
            );
        }
        if chain[0].is_none() {
            self.log("promote: no chain target named; the scene is downsampled before the interface composite");
        }
        let reconstruction = self.source_view(tail.reconstruction, false);
        let Some(reconstruction) = reconstruction else {
            self.log("promote: the reconstruction could not be made readable");
            return None;
        };
        Some(Frame {
            composite,
            ui,
            chain,
            scene_color: tail.scene_color,
            reconstruction,
            recombine,
        })
    }

    fn prepare(&self, tail: &Tail) -> Result<(), i32> {
        if tail.composite.is_null()
            || tail.scene_color.is_null()
            || tail.reconstruction.is_null()
            || tail.render_width == 0
            || tail.render_height == 0
        {
            return Err(INVALID_ARGUMENT);
        }
        let rounded = |output| u64::from(output).next_multiple_of(4);
        let (width, height) = (self.setup.output_width, self.setup.output_height);
        if u64::from(tail.render_width) > rounded(width)
            || u64::from(tail.render_height) > rounded(height)
        {
            self.log(format_args!(
                "promote: the game is rendering at {}x{} against an output of {width}x{height}, so there is nothing to promote",
                tail.render_width, tail.render_height
            ));
            return Err(NOT_SCALED);
        }

        // Keep the previous frame until all of its replacements have been built.
        let mut state = self.state()?;
        let frame = self
            .build_frame(tail, &mut state.seed_pass)
            .ok_or(RESOURCE_FAILED)?;
        state.render = [tail.render_width, tail.render_height];
        let ui = frame.ui.iter().flatten().count();
        let chain = frame.chain.iter().flatten().count();
        state.frame = Some(frame);
        drop(state);
        self.log(format_args!(
            "promote: ready, {}x{} up to {width}x{height}, {ui} interface layers, {chain} chain targets",
            tail.render_width, tail.render_height
        ));
        Ok(())
    }

    fn plan(&self, size: u32) -> Result<Plan, i32> {
        let state = self.state()?;
        let frame = state.frame.as_ref().ok_or(INVALID_ARGUMENT)?;
        let mut plan = Plan {
            struct_size: size,
            viewport_scale_x: self.setup.output_width as f32 / state.render[0] as f32,
            viewport_scale_y: self.setup.output_height as f32 / state.render[1] as f32,
            ..Plan::default()
        };
        let mut push = |item| {
            plan.items[plan.count as usize] = item;
            plan.count += 1;
        };
        // A pooled post-process target can be a scene attachment before its gate.
        let composite_gate = frame.composite.original;
        push(frame.composite.substitution(composite_gate));
        for item in frame.ui.iter().flatten() {
            push(item.substitution(null_mut()));
        }
        for item in frame.chain.iter().flatten() {
            push(item.substitution(composite_gate));
        }
        if let Some(recombine) = &frame.recombine {
            let recombine_gate = recombine.composed.original;
            push(recombine.composed.substitution(null_mut()));
            push(Item {
                render_view: recombine.scratch.target.as_ptr(),
                ..recombine.scene.substitution(recombine_gate)
            });
            // This stand-in is never bound by the caller. Its entry opens the finish gate.
            push(Item {
                texture: recombine.scratch.texture.as_ptr(),
                shader_view: recombine.scratch.view.as_ptr(),
                render_view: null_mut(),
                after_target: composite_gate,
            });
            if let Some(layer) = recombine.layer {
                push(layer);
            }
        } else {
            push(Item {
                texture: frame.scene_color,
                shader_view: frame.reconstruction.as_ptr(),
                render_view: null_mut(),
                after_target: composite_gate,
            });
        }
        Ok(plan)
    }

    fn draw(&self, context: Handle, draw: Draw) -> Result<(), i32> {
        if context.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        let state = self.state()?;
        let frame = state.frame.as_ref().ok_or(INVALID_ARGUMENT)?;
        let Some(recombine) = &frame.recombine else {
            return Ok(());
        };
        let source = match draw {
            Draw::Finish if recombine.scene.format == recombine.composed.format => {
                if !unsafe {
                    rsf_d3d11_output_is_immediate(context) != 0
                        && same_device(self.device.as_ptr(), context)
                } {
                    return Err(RESOURCE_FAILED);
                }
                unsafe {
                    rsf_d3d11_output_copy_resource(
                        context,
                        recombine.scene.texture.as_ptr(),
                        recombine.composed.texture.as_ptr(),
                    );
                }
                return Ok(());
            }
            Draw::Finish => &recombine.composed.view,
            Draw::Seed {
                reconstructed: true,
            } => &frame.reconstruction,
            Draw::Seed {
                reconstructed: false,
            } => &recombine.source_view,
        };
        let pass = state.seed_pass.as_ref().ok_or(RESOURCE_FAILED)?;
        if unsafe {
            pass.draw(
                context,
                recombine.scene.target.as_ptr(),
                source.as_ptr(),
                0,
                0,
            )
        } {
            Ok(())
        } else {
            Err(RESOURCE_FAILED)
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_promote_create(setup: *const Setup, out: *mut *mut Promote) -> i32 {
    if out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { setup.read() };
    if setup.device.is_null() || setup.output_width == 0 || setup.output_height == 0 {
        return INVALID_ARGUMENT;
    }
    if setup.abi_version != RSF_PROMOTE_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let promote = crate::allocate_handle(Promote {
        state: Mutex::default(),
        device: unsafe { ComRef::retain(setup.device) },
        setup,
    });
    if promote.is_null() {
        return RESOURCE_FAILED;
    }
    unsafe { out.write(promote) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_promote_prepare(promote: *const Promote, tail: *const Tail) -> i32 {
    let Some(promote) = (unsafe { promote.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(tail) } {
        return INVALID_ARGUMENT;
    }
    promote.prepare(unsafe { &*tail }).err().unwrap_or(OK)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_promote_fill_plan(promote: *const Promote, plan: *mut Plan) -> i32 {
    let Some(promote) = (unsafe { promote.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(plan) } {
        return INVALID_ARGUMENT;
    }
    match promote.plan(unsafe { plan.cast::<u32>().read() }) {
        Ok(value) => {
            unsafe { plan.write(value) };
            OK
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_promote_seed(
    promote: *const Promote,
    context: Handle,
    reconstructed: u32,
) -> i32 {
    let Some(promote) = (unsafe { promote.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    promote
        .draw(
            context,
            Draw::Seed {
                reconstructed: reconstructed != 0,
            },
        )
        .err()
        .unwrap_or(OK)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_promote_finish(promote: *const Promote, context: Handle) -> i32 {
    let Some(promote) = (unsafe { promote.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    promote.draw(context, Draw::Finish).err().unwrap_or(OK)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_promote_get_stand_ins(
    promote: *const Promote,
    scene: *mut Handle,
    composed: *mut Handle,
) -> i32 {
    let Some(promote) = (unsafe { promote.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if scene.is_null() || composed.is_null() {
        return INVALID_ARGUMENT;
    }
    let state = match promote.state() {
        Ok(state) => state,
        Err(error) => return error,
    };
    let (scene_texture, composed_texture) = state
        .frame
        .as_ref()
        .and_then(|frame| frame.recombine.as_ref())
        .map_or((null_mut(), null_mut()), |recombine| {
            (
                recombine.scene.texture.as_ptr(),
                recombine.composed.texture.as_ptr(),
            )
        });
    unsafe {
        scene.write(scene_texture);
        composed.write(composed_texture);
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_promote_get_status(promote: *const Promote, status: *mut Status) -> i32 {
    let Some(promote) = (unsafe { promote.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(status) } {
        return INVALID_ARGUMENT;
    }
    let state = match promote.state() {
        Ok(state) => state,
        Err(error) => return error,
    };
    let mut value = Status {
        struct_size: unsafe { status.cast::<u32>().read() },
        render_width: state.render[0],
        render_height: state.render[1],
        output_width: promote.setup.output_width,
        output_height: promote.setup.output_height,
        ..Status::default()
    };
    if let Some(frame) = &state.frame {
        value.ready = 1;
        value.at_recombine = frame.recombine.is_some().into();
        value.ui_targets_promoted = frame.ui.iter().flatten().count() as u32;
        value.chain_targets_promoted = frame.chain.iter().flatten().count() as u32;
    }
    unsafe { status.write(value) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_promote_destroy(promote: *mut Promote) {
    if !promote.is_null() {
        // The caller must clear the tap before releasing the views borrowed by its plan.
        drop(unsafe { Box::from_raw(promote) });
    }
}
