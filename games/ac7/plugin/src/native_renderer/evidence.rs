// SPDX-License-Identifier: MIT
//! Evidence-only pixel uniform bind hooks, installed only with the pixel binding diagnostic.
//! They log and always preserve the native result.
use super::translucency::{TRANSLUCENCY_RENDERER, ui_active};
use super::{
    Guard, PixelUniformBinding, game_module, hook, hook_rva, installed, key, original, read,
};
use rsf_ac7::discovery::Site;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
use windows::Win32::System::Threading::GetCurrentThreadId;

/// A pointer as MSVC's %p printed it: sixteen uppercase hex digits.
fn pointer(value: *const c_void) -> String {
    format!("{:016X}", value as usize)
}

static NULL_VIEW_REPORTS: AtomicU32 = AtomicU32::new(0);

pub(super) unsafe extern "C" fn hooked_pixel_view_uniform(
    shader: *mut c_void,
    list: *mut c_void,
    rhi_shader: *mut c_void,
    uniform: *mut c_void,
) {
    let _entry = Guard::entry();
    if uniform.is_null()
        && let Some(owner) = installed()
        && owner.active()
        && NULL_VIEW_REPORTS.fetch_add(1, Ordering::SeqCst) < 8
    {
        // The detour's own frame is skipped, so the first frame is its caller's return address.
        let mut frames = [null_mut(); 1];
        let captured = unsafe { RtlCaptureStackBackTrace(1, &mut frames, None) };
        let caller = if captured == 1 { frames[0] as usize } else { 0 };
        let base = game_module() as usize;
        owner.log_bounded(
            &format!(
                "native null pixel View uniform enqueue: caller RVA 0x{:x}, shader {}, UI scope {}, renderer {}, thread {}",
                caller.saturating_sub(base),
                pointer(shader),
                u32::from(ui_active()),
                pointer(TRANSLUCENCY_RENDERER.get()),
                unsafe { GetCurrentThreadId() }
            ),
            224,
        );
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) =
        unsafe { original(hook::PIXEL_VIEW_UNIFORM) };
    unsafe { forward(shader, list, rhi_shader, uniform) };
}

pub(super) unsafe extern "C" fn hooked_rhi_pixel_uniform(
    context: *mut c_void,
    shader: *mut c_void,
    slot: u32,
    uniform: *mut c_void,
) {
    let _entry = Guard::entry();
    if let Some(owner) = installed()
        && owner.active()
    {
        let mut command = 0u64;
        read(
            key(game_module().cast()),
            rsf_ac7::discovery::rva(Site::CurrentCommand.into()),
            &mut command,
        );
        let mut bindings = owner
            .pixel_bindings
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        bindings.sequence = bindings.sequence.wrapping_add(1);
        let sequence = bindings.sequence;
        let index = (sequence % bindings.ring.len() as u64) as usize;
        bindings.ring[index] = PixelUniformBinding {
            sequence,
            context: key(context).wrapping_sub(0x18),
            shader: key(shader),
            uniform: key(uniform),
            command,
            slot,
        };
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut c_void) =
        unsafe { original(hook::RHI_PIXEL_UNIFORM) };
    unsafe { forward(context, shader, slot, uniform) };
}

pub(super) unsafe extern "C" fn hooked_rhi_pixel_tables(context: *mut c_void, shader: *mut c_void) {
    let _entry = Guard::entry();
    let (mut required, mut dirty, mut missing) = (0u32, 0u16, 0u32);
    let context_key = key(context);
    if let Some(owner) = installed()
        && owner.active()
        && read(key(shader), 0x30, &mut required)
        && read(context_key, 0x40e6, &mut dirty)
    {
        let selected = required & u32::from(dirty);
        for slot in 0..14u32 {
            if selected & (1 << slot) != 0 {
                let mut uniform = 0u64;
                if read(context_key, 0x3f90 + slot as usize * 8, &mut uniform) && uniform == 0 {
                    missing |= 1 << slot;
                }
            }
        }
        if missing != 0 && owner.missing_pixel_reports.fetch_add(1, Ordering::SeqCst) < 4 {
            let mut hashes = 0u64;
            read(key(shader), 0x68, &mut hashes);
            owner.log_bounded(
                &format!(
                    "native missing PS uniform table: context {} shader {} required 0x{:x} dirty 0x{:x} missing 0x{:x}",
                    pointer(context),
                    pointer(shader),
                    required,
                    dirty,
                    missing
                ),
                224,
            );
            for slot in 0..14u32 {
                if missing & (1 << slot) != 0 {
                    let mut expected_hash = 0u32;
                    read(hashes, slot as usize * 4, &mut expected_hash);
                    owner.log_bounded(
                        &format!(
                            "native missing PS slot {slot} expected layout hash 0x{expected_hash:x}"
                        ),
                        224,
                    );
                }
            }
            let mut recent = [PixelUniformBinding::default(); 16];
            {
                let bindings = owner
                    .pixel_bindings
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                for (index, binding) in recent.iter_mut().enumerate() {
                    if bindings.sequence > index as u64 {
                        let sequence = bindings.sequence - index as u64;
                        *binding = bindings.ring[(sequence % bindings.ring.len() as u64) as usize];
                    }
                }
            }
            for binding in recent.iter().filter(|binding| binding.sequence != 0) {
                owner.log_bounded(
                    &format!(
                        "native preceding PS bind: seq {} context {:x} shader {:x} slot {} uniform {:x} command {:x}",
                        binding.sequence,
                        binding.context,
                        binding.shader,
                        binding.slot,
                        binding.uniform,
                        binding.command
                    ),
                    224,
                );
            }
        }
    }
    // Evidence only: preserve the native failure rather than hiding an incomplete material draw.
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { original(hook::RHI_PIXEL_TABLES) };
    unsafe { forward(context, shader) };
}

/// One report budget per enqueue hook, as each former template instance had its own.
static NULL_CREATOR_REPORTS: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];

pub(super) unsafe extern "C" fn hooked_pixel_enqueue<const INDEX: u32>(
    list: *mut c_void,
    shader: *mut c_void,
    parameter: *mut c_void,
    input: *mut c_void,
) {
    let _entry = Guard::entry();
    let owner = installed();
    let (mut bound, mut slot) = (0u8, 0u16);
    let selected = owner.is_some_and(|owner| owner.active())
        && read(key(parameter), 6, &mut bound)
        && bound != 0
        && read(key(parameter), 4, &mut slot)
        && slot == 1;
    let mut uniform = key(input);
    if INDEX == hook::PIXEL_ENQUEUE_B {
        read(key(input), 0x38, &mut uniform);
    }
    if INDEX == hook::PIXEL_ENQUEUE_C {
        read(key(input), 0, &mut uniform);
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) =
        unsafe { original(INDEX) };
    if INDEX == hook::PIXEL_ENQUEUE_D {
        unsafe { forward(list, shader, parameter, input) };
        let mut command = 0u64;
        if !selected || !read(key(list), 8, &mut command) || !read(command, 0x20, &mut uniform) {
            return;
        }
    }
    if selected
        && uniform == 0
        && let Some(owner) = owner
        && NULL_CREATOR_REPORTS[(INDEX - hook::PIXEL_ENQUEUE_A) as usize]
            .fetch_add(1, Ordering::SeqCst)
            < 4
    {
        let mut frames = [null_mut(); 10];
        let count = unsafe { RtlCaptureStackBackTrace(0, &mut frames, None) };
        let base = key(game_module().cast());
        owner.log_bounded(
            &format!(
                "native null PS slot1 CPU creator: RVA 0x{:x} shader {} parameter {} input {} UI {} renderer {}",
                hook_rva(INDEX),
                pointer(shader),
                pointer(parameter),
                pointer(input),
                u32::from(ui_active()),
                pointer(TRANSLUCENCY_RENDERER.get())
            ),
            224,
        );
        for (index, frame) in frames.iter().take(usize::from(count)).enumerate() {
            let frame = key(*frame);
            let rva = if frame >= base && frame - base < 0x500_0000 {
                frame - base
            } else {
                0
            };
            owner.log_bounded(
                &format!("native null PS CPU stack {index}: address {frame:x} game RVA {rva:x}"),
                224,
            );
        }
    }
    if INDEX != hook::PIXEL_ENQUEUE_D {
        unsafe { forward(list, shader, parameter, input) };
    }
}
