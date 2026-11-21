// SPDX-License-Identifier: MIT
//! AC7 policy shared by the dedicated and combined DirectInput carriers.
use crate::sites::DEPTH_GATES;
use crate::{AMD64, EXECUTABLE};
use rsf_game_sys::{rsf_game_log_fn, rsf_game_probe};
use std::ffi::{CStr, CString, c_char, c_int, c_ulong};
use std::mem::offset_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};

const ABI_VERSION: u32 = 2;
const PATCH_OK: i32 = 0;
const PATCH_INVALID_ARGUMENT: i32 = -1;
const JITTER_RVA: u32 = 0x112b1f3;
const TRANSLUCENT_VELOCITY_RVA: u32 = 0x11823de;
const SEPARATE_TRANSLUCENCY_RVA: u32 = 0x10be329;
const SCALE_IMMEDIATE_OFFSET: u32 = 7;
const JITTER_GATE: &[u8; 6] = b"\x0f\x85\xdd\x03\x00\x00";
const NOP6: &[u8; 6] = b"\x66\x0f\x1f\x44\x00\x00";
const TRANSLUCENT_VELOCITY_GATE: &[u8; 6] = b"\x0f\x87\x68\x03\x00\x00";
const SEPARATE_TRANSLUCENCY_GATE: &[u8; 15] =
    b"\x73\x0d\x40\x84\xff\x74\x08\xf3\x0f\x10\x0d\x58\x61\x4b\x01";
// A NOP, then mov eax, 1.0f and movd xmm1, eax. The immediate starts at RVA + 7.
const SEPARATE_TRANSLUCENCY_PATCH: &[u8; 15] =
    b"\x66\x0f\x1f\x44\x00\x00\xb8\x00\x00\x80\x3f\x66\x0f\x6e\xc8";

const fn widen<const N: usize>(text: &[u8]) -> [u16; N] {
    assert!(text.len() == N);
    let mut wide = [0; N];
    let mut index = 0;
    while index < N {
        wide[index] = text[index] as u16;
        index += 1;
    }
    wide
}

static EXECUTABLE_WIDE: [u16; EXECUTABLE.to_bytes_with_nul().len()] =
    widen(EXECUTABLE.to_bytes_with_nul());
static GAME_DLL: [u16; b"ReScaleFrame.Game.AC7.dll\0".len()] =
    widen(b"ReScaleFrame.Game.AC7.dll\0");
static WINDOW_CLASS: [u16; b"UnrealWindow\0".len()] = widen(b"UnrealWindow\0");
static STARTUP_READY: AtomicBool = AtomicBool::new(false);
static STARTUP_ATTEMPTED: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_carrier_executable() -> *const u16 {
    EXECUTABLE_WIDE.as_ptr()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_carrier_game_dll() -> *const u16 {
    GAME_DLL.as_ptr()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_carrier_window_class() -> *const u16 {
    WINDOW_CLASS.as_ptr()
}

/// A nonnull `probe` points to at least its leading size field and to a writable probe if full
/// sized. GameSDK borrows the digest provided by the caller when it detects the executable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_carrier_probe(
    probe: *mut rsf_game_probe,
    sha256: *const c_char,
) -> c_int {
    if probe.is_null() || unsafe { probe.cast::<u32>().read() } < size_of::<rsf_game_probe>() as u32
    {
        return 0;
    }
    unsafe {
        probe.write(rsf_game_probe {
            struct_size: size_of::<rsf_game_probe>() as u32,
            pe_machine: AMD64,
            reserved: 0,
            executable_name_utf8: EXECUTABLE.as_ptr(),
            sha256_hex: sha256,
        });
    }
    1
}

type Patch = unsafe extern "C" fn(u32, *const u8, u32, *const u8, u32, *mut u8) -> i32;
type ReadNumber = unsafe extern "C" fn(*const c_char, u32) -> u32;

#[repr(C)]
struct CarrierLeaves {
    struct_size: u32,
    abi_version: u32,
    read_number: Option<ReadNumber>,
    patch: Option<Patch>,
    log: rsf_game_log_fn,
    install_jitter: Option<unsafe extern "C" fn(i32, u32, c_int)>,
    set_depth_conformed: Option<unsafe extern "C" fn(c_int)>,
    register_translucency_scale: Option<unsafe extern "C" fn(u32, u32)>,
    prepare_native_game: Option<unsafe extern "C" fn() -> c_int>,
    install_motion_capture: Option<unsafe extern "C" fn() -> c_int>,
}

const _: () = {
    assert!(size_of::<CarrierLeaves>() == 72);
    assert!(offset_of!(CarrierLeaves, read_number) == 8);
    assert!(offset_of!(CarrierLeaves, prepare_native_game) == 56);
    assert!(offset_of!(CarrierLeaves, install_motion_capture) == 64);
};

struct PatchIo {
    patch: Patch,
    log: rsf_game_log_fn,
}

impl PatchIo {
    fn write(&self, rva: u32, bytes: &[u8], expected: Option<&[u8]>, previous: &mut [u8]) -> i32 {
        let (expected, count) =
            expected.map_or((null(), 0), |guard| (guard.as_ptr(), guard.len() as u32));
        assert!(previous.len() >= bytes.len());
        // The native helper borrows the byte arrays for the call and checks the live image.
        unsafe {
            (self.patch)(
                rva,
                bytes.as_ptr(),
                bytes.len() as u32,
                expected,
                count,
                previous.as_mut_ptr(),
            )
        }
    }

    fn log(&self, message: &str) {
        if let (Some(log), Ok(text)) = (self.log, CString::new(message)) {
            unsafe { log(null_mut(), text.as_ptr()) };
        }
    }
}

/// A supplied `previous` points to a six-byte output. `stub_rel32`, when set, points to the
/// native relay's displacement from the end of the five-byte jump.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_carrier_patch_jitter(
    patch: Option<Patch>,
    rva: u32,
    enabled: c_int,
    stub_rel32: *const i32,
    previous: *mut u8,
) -> i32 {
    let Some(patch) = patch else {
        return PATCH_INVALID_ARGUMENT;
    };
    let mut jump = [0xe9, 0, 0, 0, 0, 0x90];
    let open = match unsafe { stub_rel32.as_ref() } {
        Some(displacement) => {
            jump[1..5].copy_from_slice(&displacement.to_le_bytes());
            &jump
        }
        None => NOP6,
    };
    let (bytes, expected) = if enabled != 0 {
        (open, JITTER_GATE)
    } else {
        (JITTER_GATE, open)
    };
    let count = JITTER_GATE.len() as u32;
    unsafe {
        patch(
            rva,
            bytes.as_ptr(),
            count,
            expected.as_ptr(),
            count,
            previous,
        )
    }
}

fn apply_translucent_velocity(io: &PatchIo, rva: u32) {
    // A custom RVA retains the carrier's existing unguarded diagnostic override.
    let expected = (rva == TRANSLUCENT_VELOCITY_RVA).then_some(&TRANSLUCENT_VELOCITY_GATE[..]);
    let mut previous = [0; 6];
    let result = io.write(rva, NOP6, expected, &mut previous);
    if result == PATCH_OK {
        let [a, b, c, d, e, f] = previous;
        io.log(&format!(
            "translucent velocity gate patched at rva 0x{rva:x}, was {a:02x} {b:02x} {c:02x} {d:02x} {e:02x} {f:02x}. \
             Translucent draws can now reach the velocity pass; whether any does is a question \
             for the velocity target, not for this line"
        ));
    } else {
        io.log(&format!(
            "translucent velocity gate NOT patched at rva 0x{rva:x}, result {result} (expected bytes did not match?)"
        ));
    }
}

fn apply_depth_gates(io: &PatchIo) -> bool {
    let mut previous = [0; 9];
    // An identical write performs the same native range and guard checks as a real patch.
    for site in &DEPTH_GATES {
        let result = io.write(site.rva, site.native, Some(site.native), &mut previous);
        if result != PATCH_OK {
            io.log(&format!(
                "translucency preflight: {} at rva 0x{:x} refused, result {result}; no gates changed",
                site.description, site.rva
            ));
            return false;
        }
    }
    for (index, site) in DEPTH_GATES.iter().enumerate() {
        let result = io.write(site.rva, site.expected, Some(site.native), &mut previous);
        if result != PATCH_OK {
            for patched in DEPTH_GATES[..index].iter().rev() {
                let restored = io.write(
                    patched.rva,
                    patched.native,
                    Some(patched.expected),
                    &mut previous,
                );
                if restored != PATCH_OK {
                    io.log(&format!(
                        "translucency rollback: {} failed, result {restored}",
                        patched.description
                    ));
                }
            }
            io.log(&format!(
                "translucency depth: {} at rva 0x{:x} did NOT match, result {result}. The layer stays at the scene's resolution",
                site.description, site.rva
            ));
            return false;
        }
        io.log(&format!(
            "translucency depth: {} at rva 0x{:x} now asks for exactly 1.0 rather than less",
            site.description, site.rva
        ));
    }
    true
}

/// The table is null or points to its two leading fields and to a full table when its size says
/// so. Native callbacks borrow all settings and log strings for the duration of each call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_carrier_install_decrypted(leaves: *const CarrierLeaves) {
    let header = leaves.cast::<u32>();
    if header.is_null()
        || unsafe { header.read() } < size_of::<CarrierLeaves>() as u32
        || unsafe { header.add(1).read() } != ABI_VERSION
    {
        return;
    }
    let leaves = unsafe { &*leaves };
    let (
        Some(read),
        Some(patch),
        Some(jitter),
        Some(set_depth),
        Some(register_scale),
        Some(prepare),
        Some(install_motion_capture),
    ) = (
        leaves.read_number,
        leaves.patch,
        leaves.install_jitter,
        leaves.set_depth_conformed,
        leaves.register_translucency_scale,
        leaves.prepare_native_game,
        leaves.install_motion_capture,
    )
    else {
        return;
    };
    let io = PatchIo {
        patch,
        log: leaves.log,
    };
    let number = |name: &CStr, fallback| unsafe { read(name.as_ptr(), fallback) };

    let mode = number(c"RSF_ENABLE_JITTER", 1) as i32;
    if mode != 0 {
        let rva = number(c"RSF_JITTER_RVA", JITTER_RVA);
        let main_only = rva == JITTER_RVA && number(c"RSF_JITTER_ALL_VIEWS", 0) == 0;
        unsafe { jitter(mode, rva, c_int::from(main_only)) };
    } else {
        // Reset the native mode just as the carrier did before skipping the other jitter settings.
        unsafe { jitter(0, 0, 0) };
    }
    if number(c"RSF_TRANSLUCENT_VELOCITY", 0) != 0 {
        apply_translucent_velocity(
            &io,
            number(c"RSF_TRANSLUCENT_VELOCITY_RVA", TRANSLUCENT_VELOCITY_RVA),
        );
    }
    // Publish depth conformance before the scale callback uses it to choose the allowed scale.
    unsafe { set_depth(c_int::from(apply_depth_gates(&io))) };
    if number(c"RSF_FULL_TRANSLUCENCY", 1) != 0 {
        let rva = number(c"RSF_FULL_TRANSLUCENCY_RVA", SEPARATE_TRANSLUCENCY_RVA);
        let expected =
            (rva == SEPARATE_TRANSLUCENCY_RVA).then_some(&SEPARATE_TRANSLUCENCY_GATE[..]);
        let mut previous = [0; 15];
        let result = io.write(rva, SEPARATE_TRANSLUCENCY_PATCH, expected, &mut previous);
        if result == PATCH_OK {
            unsafe { register_scale(rva, SCALE_IMMEDIATE_OFFSET) };
        } else {
            io.log(&format!(
                "separate translucency halving NOT removed at rva 0x{rva:x}, result {result} (expected bytes did not match?)"
            ));
        }
    }
    io.log(if unsafe { prepare() } != 0 {
        "native AC7 plugin preparation: ready"
    } else {
        "native AC7 plugin preparation: refused"
    });
    if number(c"RSF_MOTION_CAPTURE", 0) != 0 {
        unsafe { install_motion_capture() };
    }
    // Compatibility SR can start after native preparation refuses.
    STARTUP_READY.store(true, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_carrier_startup_ready() -> c_ulong {
    c_ulong::from(STARTUP_READY.load(Ordering::SeqCst))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_carrier_startup_attempted() -> c_int {
    c_int::from(STARTUP_ATTEMPTED.load(Ordering::Relaxed))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_carrier_set_startup_attempted(attempted: c_int) {
    STARTUP_ATTEMPTED.store(attempted != 0, Ordering::Relaxed);
}

/// The Present owner lends the worker readiness action, synchronous observer extent query,
/// and backend start action. The query only borrows the two output pointers for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_carrier_try_auto_start(
    enabled: c_int,
    started: c_int,
    ready: Option<unsafe extern "C" fn() -> c_ulong>,
    present_extent: Option<unsafe extern "C" fn(*mut u32, *mut u32) -> c_int>,
    start: Option<unsafe extern "C" fn()>,
) {
    if enabled == 0 || started != 0 || STARTUP_ATTEMPTED.load(Ordering::Relaxed) {
        return;
    }
    if ready.is_none_or(|ready| unsafe { ready() } == 0) {
        return;
    }
    let (Some(present_extent), Some(start)) = (present_extent, start) else {
        return;
    };
    let (mut width, mut height) = (0, 0);
    if unsafe { present_extent(&mut width, &mut height) } != 0
        && !STARTUP_ATTEMPTED.swap(true, Ordering::Relaxed)
    {
        // A refusal inside start still spends the automatic attempt. Manual start is separate.
        unsafe { start() };
    }
}

/// Width and height point to writable dimensions from the observer. The native reader borrows
/// the setting names for the call and keeps its existing parsing and fallback behavior.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_carrier_override_start_extent(
    read: Option<ReadNumber>,
    width: *mut u32,
    height: *mut u32,
) {
    if let Some(read) = read
        && !width.is_null()
        && !height.is_null()
    {
        unsafe {
            width.write(read(c"RSF_DLSS_OUTPUT_WIDTH".as_ptr(), width.read()));
            height.write(read(c"RSF_DLSS_OUTPUT_HEIGHT".as_ptr(), height.read()));
        }
    }
}
