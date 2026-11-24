// SPDX-License-Identifier: MIT
//! AC7 policy shared by the dedicated and combined DirectInput carriers.
use crate::discovery::{self, Site};
use crate::sites::depth_gates;
use crate::sys::{
    RSF_AC7_CARRIER_ABI_VERSION, RSF_AC7_CARRIER_START_OPTION_COUNT,
    rsf_ac7_carrier_leaves as CarrierLeaves,
};
use crate::{AMD64, EXECUTABLE};
use rsf_game_sys::ffi_util::ascii_wide;
use rsf_game_sys::{rsf_game_log_fn, rsf_game_probe};
use std::ffi::{CStr, CString, c_char, c_int, c_ulong};
use std::mem::offset_of;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

struct StartupOption {
    key: &'static CStr,
    label: &'static CStr,
    fallback: u32,
    maximum: u32,
}

impl StartupOption {
    const fn new(key: &'static CStr, label: &'static CStr, fallback: u32, maximum: u32) -> Self {
        Self {
            key,
            label,
            fallback,
            maximum,
        }
    }
}

const JITTER: StartupOption = StartupOption::new(
    c"RSF_ENABLE_JITTER",
    c"Jitter mode (0 Off, 1 Follow SR, 2 Always)",
    1,
    2,
);

const TRANSLUCENT_VELOCITY: StartupOption = StartupOption::new(
    c"RSF_TRANSLUCENT_VELOCITY",
    c"Allow translucent velocity",
    0,
    1,
);
const FULL_TRANSLUCENCY: StartupOption = StartupOption::new(
    c"RSF_FULL_TRANSLUCENCY",
    c"Enable full-resolution translucency",
    1,
    1,
);
const MOTION_CAPTURE: StartupOption =
    StartupOption::new(c"RSF_MOTION_CAPTURE", c"Install motion capture hooks", 0, 1);
const OUTPUT_WIDTH: StartupOption = StartupOption::new(
    c"RSF_DLSS_OUTPUT_WIDTH",
    c"Startup output width (0 Auto)",
    0,
    16384,
);
const OUTPUT_HEIGHT: StartupOption = StartupOption::new(
    c"RSF_DLSS_OUTPUT_HEIGHT",
    c"Startup output height (0 Auto)",
    0,
    16384,
);
const STARTUP_OPTIONS: [StartupOption; RSF_AC7_CARRIER_START_OPTION_COUNT as usize] = [
    JITTER,
    TRANSLUCENT_VELOCITY,
    FULL_TRANSLUCENCY,
    MOTION_CAPTURE,
    OUTPUT_WIDTH,
    OUTPUT_HEIGHT,
    StartupOption::new(
        c"RSF_AC7_PIXEL_DIAGNOSTICS",
        c"Pixel binding diagnostics",
        0,
        1,
    ),
];

/// The output pointers are writable for one value each.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_carrier_start_option(
    index: u32,
    label: *mut *const c_char,
    fallback: *mut u32,
    maximum: *mut u32,
) -> *const c_char {
    let Some(option) = STARTUP_OPTIONS.get(index as usize) else {
        return null();
    };
    if label.is_null() || fallback.is_null() || maximum.is_null() {
        return null();
    }
    unsafe {
        label.write(option.label.as_ptr());
        fallback.write(option.fallback);
        maximum.write(option.maximum);
    }
    option.key.as_ptr()
}

const PATCH_OK: i32 = 0;
const PATCH_INVALID_ARGUMENT: i32 = -1;
const SCALE_IMMEDIATE_OFFSET: usize = 7;
const NOP6: &[u8; 6] = b"\x66\x0f\x1f\x44\x00\x00";
// Replaces `jae; test dil,dil; je; movss xmm1,[rip+half]`: a NOP, then mov eax, 1.0f and
// movd xmm1, eax. The immediate starts at RVA + 7.
const SEPARATE_TRANSLUCENCY_PATCH: &[u8; 15] =
    b"\x66\x0f\x1f\x44\x00\x00\xb8\x00\x00\x80\x3f\x66\x0f\x6e\xc8";

static GAME_DLL: [u16; b"ReScaleFrame.Game.AC7.dll\0".len()] =
    ascii_wide(b"ReScaleFrame.Game.AC7.dll\0");
static WINDOW_CLASS: [u16; b"UnrealWindow\0".len()] = ascii_wide(b"UnrealWindow\0");
static STARTUP_READY: AtomicBool = AtomicBool::new(false);
static STARTUP_ATTEMPTED: AtomicBool = AtomicBool::new(false);
/// The jitter gate's native `jne rel32`, read at its discovered site before the first toggle.
static JITTER_NATIVE: OnceLock<[u8; 6]> = OnceLock::new();

/// The native measure leaf writes the current entropy on success.
#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_carrier_wait_decrypted(
    measure: Option<unsafe extern "C" fn(*mut f64) -> i32>,
    still_encrypted: i32,
) -> i32 {
    use windows::Win32::System::{SystemInformation::GetTickCount64, Threading::Sleep};

    let Some(measure) = measure else {
        return PATCH_INVALID_ARGUMENT;
    };
    let deadline = unsafe { GetTickCount64() } + 180_000;
    let mut consecutive = 0;
    let mut entropy = 0.0;
    loop {
        let result = unsafe { measure(&mut entropy) };
        if result != PATCH_OK {
            return result;
        }
        consecutive = if entropy < 7.0 { consecutive + 1 } else { 0 };
        if consecutive == 2 {
            return PATCH_OK;
        }
        unsafe { Sleep(250) };
        if unsafe { GetTickCount64() } >= deadline {
            return still_encrypted;
        }
    }
}

/// The separate translucency multiplier on the scene buffer, which the native renderer keeps at
/// output size: `target_percent` of native (zero means the scene's), or `override_percent`
/// directly when nonzero. The second value is a request above 1.0 refused for missing depth.
pub(crate) fn translucency_scale(
    target_percent: u32,
    override_percent: u32,
    depth_conformed: bool,
) -> (f32, Option<f32>) {
    let requested = match (override_percent, target_percent) {
        (0, 0) => 1.0,
        (0, percent) | (percent, _) => percent as f32 / 100.0,
    };
    // The immediate is downstream of the engine's own clamp. Depth can only match a
    // larger colour target after the separate-translucency depth gates are installed.
    if requested > 1.0 && !depth_conformed {
        (1.0, Some(requested))
    } else {
        (requested.clamp(0.25, 4.0), None)
    }
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

const _: () = {
    assert!(size_of::<CarrierLeaves>() == 56);
    assert!(offset_of!(CarrierLeaves, read_number) == 8);
    assert!(offset_of!(CarrierLeaves, prepare_native_game) == 40);
    assert!(offset_of!(CarrierLeaves, install_motion_capture) == 48);
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

/// A supplied `previous` points to a six-byte output.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_carrier_patch_jitter(
    patch: Option<Patch>,
    rva: u32,
    enabled: c_int,
    previous: *mut u8,
) -> i32 {
    let (Some(patch), Some(native)) = (patch, JITTER_NATIVE.get()) else {
        return PATCH_INVALID_ARGUMENT;
    };
    let (bytes, expected) = if enabled != 0 {
        (NOP6, native)
    } else {
        (native, NOP6)
    };
    let count = NOP6.len() as u32;
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

/// The discovered RVA of a carrier patch site and its live native bytes, which its pattern fixes
/// except for displacements; the error names the site.
fn native_site<const N: usize>(site: Site) -> Result<(u32, [u8; N]), String> {
    discovery::admit([site as usize])?;
    let mut native = [0; N];
    if !discovery::live(site as usize, &mut native) {
        return Err(format!(
            "site '{}' bytes differ in memory",
            discovery::name(site as usize)
        ));
    }
    Ok((discovery::rva(site as usize) as u32, native))
}

fn apply_translucent_velocity(io: &PatchIo) {
    let (rva, native) = match native_site::<6>(Site::TranslucentVelocityGate) {
        Ok(found) => found,
        Err(reason) => {
            io.log(&format!("translucent velocity gate NOT patched: {reason}"));
            return;
        }
    };
    let mut previous = [0; 6];
    let result = io.write(rva, NOP6, Some(&native), &mut previous);
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
    let gates = match depth_gates() {
        Ok(gates) => gates,
        Err(reason) => {
            io.log(&format!(
                "translucency depth refused: {reason}; no gates changed. The layer stays at the scene's resolution"
            ));
            return false;
        }
    };
    let rva = |gate: &crate::sites::Gate| gate.rva as u32;
    let mut previous = [0; 9];
    // An identical write performs the same native range and guard checks as a real patch.
    for site in &gates {
        let result = io.write(rva(site), site.native, Some(site.native), &mut previous);
        if result != PATCH_OK {
            io.log(&format!(
                "translucency preflight: {} at rva 0x{:x} refused, result {result}; no gates changed",
                site.description, site.rva
            ));
            return false;
        }
    }
    for (index, site) in gates.iter().enumerate() {
        let result = io.write(rva(site), site.expected, Some(site.native), &mut previous);
        if result != PATCH_OK {
            for patched in gates[..index].iter().rev() {
                let restored = io.write(
                    rva(patched),
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
        || unsafe { header.add(1).read() } != RSF_AC7_CARRIER_ABI_VERSION
    {
        return;
    }
    let leaves = unsafe { &*leaves };
    let (Some(read), Some(patch), Some(jitter), Some(prepare), Some(install_motion_capture)) = (
        leaves.read_number,
        leaves.patch,
        leaves.install_jitter,
        leaves.prepare_native_game,
        leaves.install_motion_capture,
    ) else {
        return;
    };
    let io = PatchIo {
        patch,
        log: leaves.log,
    };
    let number = |name: &CStr, fallback| unsafe { read(name.as_ptr(), fallback) };

    // The decrypted code is copied and every site discovered once, before any patch below.
    let _ = discovery::admit([]);
    io.log(&discovery::summary());
    let mode = number(JITTER.key, JITTER.fallback) as i32;
    let jitter_site = (mode != 0).then(|| native_site::<6>(Site::JitterGate));
    match jitter_site {
        Some(Ok((rva, native))) => {
            let _ = JITTER_NATIVE.set(native);
            unsafe { jitter(mode, rva) };
        }
        refused => {
            if let Some(Err(reason)) = refused {
                io.log(&format!(
                    "jitter refused: {reason}; jitter stays off for this run"
                ));
            }
            // Reset the native mode just as the carrier did before skipping the other jitter
            // settings.
            unsafe { jitter(0, 0) };
        }
    }
    if number(TRANSLUCENT_VELOCITY.key, TRANSLUCENT_VELOCITY.fallback) != 0 {
        apply_translucent_velocity(&io);
    }
    let depth_conformed = apply_depth_gates(&io);
    let translucency = (number(FULL_TRANSLUCENCY.key, FULL_TRANSLUCENCY.fallback) != 0)
        .then(|| native_site::<15>(Site::SeparateTranslucency));
    if let Some(Err(reason)) = &translucency {
        io.log(&format!(
            "separate translucency halving NOT removed: {reason}"
        ));
    }
    if let Some(Ok((rva, native))) = translucency {
        let target = number(c"RSF_TRANSLUCENCY_TARGET", 100);
        let (scale, refused) = translucency_scale(
            target,
            number(c"RSF_TRANSLUCENCY_SCALE", 0),
            depth_conformed,
        );
        if let Some(request) = refused
            && target != 0
        {
            io.log(&format!(
                "separate translucency asked for {}% of the scene, but the depth patches did not \
                 apply, so the engine has no depth to bind at that size. Held at 100%",
                (request * 100.0) as i32
            ));
        }
        // The patch carries the scale as the immediate of its mov eax.
        let mut bytes = *SEPARATE_TRANSLUCENCY_PATCH;
        bytes[SCALE_IMMEDIATE_OFFSET..SCALE_IMMEDIATE_OFFSET + 4]
            .copy_from_slice(&scale.to_bits().to_le_bytes());
        let expected = Some(&native[..]);
        let mut previous = [0; 15];
        let result = io.write(rva, &bytes, expected, &mut previous);
        if result == PATCH_OK {
            io.log(&format!(
                "separate translucency halving removed at rva 0x{rva:x}, scale {}% of the scene",
                (scale * 100.0) as i32
            ));
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
    if number(MOTION_CAPTURE.key, MOTION_CAPTURE.fallback) != 0 {
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
    start: Option<unsafe extern "C" fn(u32, u32)>,
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
        && width != 0
        && height != 0
        && !STARTUP_ATTEMPTED.swap(true, Ordering::Relaxed)
    {
        // A refusal inside start still spends the automatic attempt. Manual start is separate.
        unsafe { start(width, height) };
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
        for (option, dimension) in [(OUTPUT_WIDTH, width), (OUTPUT_HEIGHT, height)] {
            let override_value = unsafe { read(option.key.as_ptr(), option.fallback) };
            if override_value != 0 {
                unsafe { dimension.write(override_value) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static STARTS: Cell<u32> = const { Cell::new(0) };
    }

    unsafe extern "C" fn ready() -> c_ulong {
        1
    }

    unsafe extern "C" fn empty_extent(_: *mut u32, _: *mut u32) -> c_int {
        1
    }

    unsafe extern "C" fn extent(width: *mut u32, height: *mut u32) -> c_int {
        unsafe {
            width.write(2560);
            height.write(1440);
        }
        1
    }

    unsafe extern "C" fn start(width: u32, height: u32) {
        assert_eq!((width, height), (2560, 1440));
        STARTS.with(|starts| starts.set(starts.get() + 1));
    }

    #[test]
    fn automatic_start_spends_the_attempt_on_one_valid_extent() {
        rsf_ac7_carrier_set_startup_attempted(0);
        unsafe {
            rsf_ac7_carrier_try_auto_start(1, 0, Some(ready), Some(empty_extent), Some(start))
        };
        assert_eq!(rsf_ac7_carrier_startup_attempted(), 0);
        for _ in 0..2 {
            unsafe { rsf_ac7_carrier_try_auto_start(1, 0, Some(ready), Some(extent), Some(start)) };
        }
        assert_eq!(rsf_ac7_carrier_startup_attempted(), 1);
        STARTS.with(|starts| assert_eq!(starts.get(), 1));
        rsf_ac7_carrier_set_startup_attempted(0);
    }
}
