// SPDX-License-Identifier: MIT
//! Engine settings ownership for `ReScaleFrame.RDR2.asi`. The game's own DLSS or FSR2 selection is
//! the input carrier for RSF's SR adapter, so the settings collection is pinned to an active
//! carrier, the native marker mode stays on, and a saved refresh rate stays storable. RSF's quality
//! preference arrives from the ScriptHook fiber and is applied through the engine's own graphics
//! reconfiguration. RSF settings and other graphics settings are never changed here.
use crate::asi::{self, AsiPlatform, MH_ERROR_ALREADY_INITIALIZED, MH_OK};
use crate::layout::asi::{self as site, collection, manager};
use crate::platform::trampoline;
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::{CString, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};

pub(crate) const NVIDIA: u32 = 0x10de;
/// DLSS renderer modes per RSF quality (Native, Quality, Balanced, Performance, Ultra Performance).
/// RAGE has no DLAA mode; Native keeps the Quality carrier and replaces its dimensions.
const DLSS_MODES: [u32; 5] = [2, 2, 1, 0, 3];
/// Displayed DLSS selector indices for the measured active capability list.
const DLSS_INDICES: [u32; 5] = [2, 2, 3, 4, 5];
/// RAGE maps only FSR2 modes 1 to 3 to the SDK; Native and Ultra Performance keep an active
/// carrier and override its dimension producer instead.
const FSR_MODES: [u32; 5] = [1, 1, 2, 3, 3];
const DLSS_DISABLED: u32 = 5;
const NATIVE_MARKERS: u32 = 1;
const QUALITY_LIMIT: u32 = 4;
const DEFAULT_QUALITY: u32 = 2;
/// RAGE saves the refresh rational clamped to this value.
pub(crate) const REFRESH_LIMIT: u32 = 262_144;
const MAX_EXTENT: u32 = 16384;

/// The index in an active DLSS capability list whose mode serves `quality`.
pub(crate) fn dlss_choice(modes: &[u32], quality: u32) -> Option<u32> {
    if modes.is_empty() || modes.len() > manager::DLSS_CHOICE_LIMIT {
        return None;
    }
    let wanted = *DLSS_MODES.get(quality as usize)?;
    modes
        .iter()
        .position(|&mode| mode == wanted)
        .map(|index| index as u32)
}

fn get(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Pins the native input carrier and marker pump of an exact 1491.50 collection. Only dlssIndex,
/// dlssQuality, fsr2Index and the Reflex marker mode change.
pub(crate) fn pin(bytes: &mut [u8], vendor: u32, quality: u32) -> bool {
    if bytes.len() < collection::BYTES || quality > QUALITY_LIMIT {
        return false;
    }
    let version = u16::from_le_bytes([bytes[collection::VERSION], bytes[collection::VERSION + 1]]);
    if !collection::VERSIONS.contains(&version) {
        return false;
    }
    let quality = quality as usize;
    let nvidia = vendor == NVIDIA;
    // dlssIndex is the displayed selector; dlssQuality is the renderer's actual mode
    // (Balanced 1, disabled 5). Both sides of that conversion are pinned.
    put(
        bytes,
        collection::DLSS,
        if nvidia { DLSS_INDICES[quality] } else { 0 },
    );
    put(
        bytes,
        collection::DLSS_QUALITY,
        if nvidia {
            DLSS_MODES[quality]
        } else {
            DLSS_DISABLED
        },
    );
    put(
        bytes,
        collection::FSR,
        if nvidia { 0 } else { FSR_MODES[quality] },
    );
    put(bytes, collection::REFLEX, NATIVE_MARKERS);
    true
}

/// RAGE saves the refresh rational clamped to 262144, so a panel mode such as 31550000/262912
/// (120.002 Hz) comes back as 262144/262144 and boots at the first listed mode. The game matches a
/// centihertz rational to the real mode, so that is stored instead. A value already clamped takes
/// the desktop refresh.
pub(crate) fn storable_refresh(
    numerator: &mut u32,
    denominator: &mut u32,
    desktop_hz: u32,
) -> bool {
    if *numerator == REFRESH_LIMIT && *denominator == REFRESH_LIMIT {
        if desktop_hz == 0 || desktop_hz > 1000 {
            return false;
        }
        *numerator = desktop_hz * 100;
        *denominator = 100;
        return true;
    }
    if *denominator == 0 || (*numerator <= REFRESH_LIMIT && *denominator <= REFRESH_LIMIT) {
        return false;
    }
    let centihertz =
        (u64::from(*numerator) * 100 + u64::from(*denominator / 2)) / u64::from(*denominator);
    if centihertz == 0 || centihertz > u64::from(REFRESH_LIMIT) {
        return false;
    }
    *numerator = centihertz as u32;
    *denominator = 100;
    true
}

type CopyFn = unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void;
type MethodFn = unsafe extern "C" fn(*mut c_void);
type OptimalFn = unsafe extern "C" fn(*const u32, *mut c_void);
type DimensionsFn = unsafe extern "C" fn(*mut u32, *mut u32, u32, u32, u32);
type RefreshFn = unsafe extern "C" fn();
type LogFn = unsafe extern "C" fn(*mut c_void, *const c_char);

static ORIGINAL_COPY: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_LOAD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_APPLY: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_OPTIMAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_DIMENSIONS: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_REFRESH: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_RECONFIGURE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

static IMAGE: AtomicUsize = AtomicUsize::new(0);
/// Low 32 bits: RSF quality. High 32 bits: native selector index in the active capability list.
static SELECTION: AtomicU64 = AtomicU64::new(((u32::MAX as u64) << 32) | DEFAULT_QUALITY as u64);
/// High 32 bits: DXGI adapter index. Low 32 bits: its vendor, 0 when unavailable.
static ADAPTER: AtomicU64 = AtomicU64::new(u64::MAX);
static OVERRIDES: AtomicU32 = AtomicU32::new(0);
static FAILURES: AtomicU32 = AtomicU32::new(0);
static REFRESH_FIXES: AtomicU32 = AtomicU32::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Set once by the first quality tick that pinned the pending owner.
static PENDING_INITIALIZED: AtomicBool = AtomicBool::new(false);
static LOGGER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static LOGGER_USER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

fn requested_quality() -> u32 {
    SELECTION.load(Ordering::SeqCst) as u32
}

/// Logs through the ASI writer once the worker announced it, outside the loader lock.
fn log(message: &str) {
    let pointer = LOGGER.load(Ordering::Acquire);
    if pointer.is_null() {
        return;
    }
    // LOGGER only ever holds a `LogFn` stored by `rsf_rdr2_report_engine_settings`.
    let log = unsafe { std::mem::transmute::<*mut c_void, LogFn>(pointer) };
    if let Ok(text) = CString::new(message) {
        unsafe { log(LOGGER_USER.load(Ordering::Acquire), text.as_ptr()) };
    }
}

fn logging() -> bool {
    !LOGGER.load(Ordering::Acquire).is_null()
}

/// # Safety
/// `address` is readable for four bytes.
unsafe fn word(address: *const u8) -> u32 {
    unsafe { address.cast::<u32>().read_unaligned() }
}

fn adapter_vendor(cache: &AtomicU64, index: u32, detect: impl FnOnce(u32) -> u32) -> u32 {
    let cached = cache.load(Ordering::SeqCst);
    if index == (cached >> 32) as u32 && cached as u32 != 0 {
        return cached as u32;
    }
    let vendor = detect(index);
    if vendor != 0 {
        cache.store(
            (u64::from(index) << 32) | u64::from(vendor),
            Ordering::SeqCst,
        );
    }
    vendor
}

type Collection = [u8; collection::BYTES];

fn snapshot(platform: &AsiPlatform, object: *mut u8) -> Option<Collection> {
    let mut current = [0; collection::BYTES];
    if !unsafe { (platform.read)(object as usize, current.as_mut_ptr().cast(), current.len()) } {
        return None;
    }
    let version = u16::from_le_bytes([
        current[collection::VERSION],
        current[collection::VERSION + 1],
    ]);
    collection::VERSIONS.contains(&version).then_some(current)
}

fn collection_vendor(platform: &AsiPlatform, current: &Collection) -> u32 {
    adapter_vendor(
        &ADAPTER,
        get(current, collection::VIDEO_ADAPTER),
        |index| unsafe { (platform.adapter_vendor)(index) },
    )
}

/// Enforces the carrier on a collection the engine handed to a verified settings method.
///
/// # Safety
/// `object` points at a live settings collection of at least `collection::BYTES`.
unsafe fn own(object: *mut u8) {
    let Some(platform) = asi::platform() else {
        FAILURES.fetch_add(1, Ordering::SeqCst);
        return;
    };
    let Some(current) = snapshot(platform, object) else {
        FAILURES.fetch_add(1, Ordering::SeqCst);
        return;
    };
    let vendor = collection_vendor(platform, &current);
    let selection = SELECTION.load(Ordering::SeqCst);
    let Some(expected) = plan_collection(current, vendor, selection) else {
        FAILURES.fetch_add(1, Ordering::SeqCst);
        return;
    };
    unsafe { apply_collection(platform, object, &current, &expected, vendor) };
}

fn plan_collection(mut expected: Collection, vendor: u32, selection: u64) -> Option<Collection> {
    // Native DLSS and FSR2 are input carriers for the same RSF renderer adapter.
    if !pin(&mut expected, vendor, selection as u32) {
        return None;
    }
    let native_index = (selection >> 32) as u32;
    if vendor == NVIDIA && native_index != u32::MAX {
        put(&mut expected, collection::DLSS, native_index);
    }
    Some(expected)
}

/// The snapshot and requested quality were validated before any engine writes.
unsafe fn apply_collection(
    platform: &AsiPlatform,
    object: *mut u8,
    current: &Collection,
    expected: &Collection,
    vendor: u32,
) {
    let (dlss, mode, fsr) = (
        get(expected, collection::DLSS),
        get(expected, collection::DLSS_QUALITY),
        get(expected, collection::FSR),
    );
    let (old_dlss, old_mode, old_fsr, old_reflex) = (
        get(current, collection::DLSS),
        get(current, collection::DLSS_QUALITY),
        get(current, collection::FSR),
        get(current, collection::REFLEX),
    );
    let changed =
        old_dlss != dlss || old_mode != mode || old_fsr != fsr || old_reflex != NATIVE_MARKERS;
    for offset in [
        collection::DLSS,
        collection::DLSS_QUALITY,
        collection::FSR,
        collection::REFLEX,
    ] {
        unsafe { ptr::copy_nonoverlapping(expected.as_ptr().add(offset), object.add(offset), 4) };
    }
    let mut numerator = get(current, collection::REFRESH_NUMERATOR);
    let mut denominator = get(current, collection::REFRESH_DENOMINATOR);
    let (old_numerator, old_denominator) = (numerator, denominator);
    let desktop_hz = unsafe { (platform.desktop_refresh)() };
    if storable_refresh(&mut numerator, &mut denominator, desktop_hz) {
        unsafe {
            object
                .add(collection::REFRESH_NUMERATOR)
                .cast::<u32>()
                .write_unaligned(numerator);
            object
                .add(collection::REFRESH_DENOMINATOR)
                .cast::<u32>()
                .write_unaligned(denominator);
        }
        if REFRESH_FIXES.fetch_add(1, Ordering::SeqCst) < 16 {
            log(&format!(
                "RDR2 refresh rate stored as {numerator}/{denominator} (was {old_numerator}/{old_denominator}, desktop {desktop_hz} Hz) so the saved mode survives RAGE's 262144 limit."
            ));
        }
    }
    if changed && OVERRIDES.fetch_add(1, Ordering::SeqCst) < 32 {
        log(&format!(
            "RDR2 input carrier enforced: vendor={vendor:x} DLSS={old_dlss}->{dlss} quality={old_mode}->{mode} FSR2={old_fsr}->{fsr} native_marker_mode={old_reflex}->1; RSF settings unchanged."
        ));
    }
}

unsafe extern "C" fn copy(destination: *mut c_void, source: *const c_void) -> *mut c_void {
    let Some(native) = (unsafe { trampoline::<CopyFn>(&ORIGINAL_COPY) }) else {
        return destination;
    };
    let result = unsafe { native(destination, source) };
    unsafe { own(destination.cast()) };
    result
}

unsafe extern "C" fn load(owner: *mut c_void) {
    if let Some(native) = unsafe { trampoline::<MethodFn>(&ORIGINAL_LOAD) } {
        unsafe { native(owner) };
    }
    unsafe { own(owner.cast::<u8>().add(manager::OWNER_COLLECTION)) };
}

unsafe extern "C" fn apply(owner: *mut c_void) {
    unsafe { own(owner.cast::<u8>().add(manager::OWNER_COLLECTION)) };
    if let Some(native) = unsafe { trampoline::<MethodFn>(&ORIGINAL_APPLY) } {
        unsafe { native(owner) };
    }
}

unsafe extern "C" fn optimal(output: *const u32, result: *mut c_void) {
    if let Some(native) = unsafe { trampoline::<OptimalFn>(&ORIGINAL_OPTIMAL) } {
        unsafe { native(output, result) };
    }
    // RAGE has no DLAA mode. Keep its active Quality carrier, but supply native dimensions at
    // the producer used by allocations and NGX feature setup. Table: output[2], then five optimal,
    // five minimum and five maximum pairs.
    if requested_quality() != 0 {
        return;
    }
    let (width, height) = unsafe { (output.read(), output.add(1).read()) };
    if width == 0 || height == 0 || width > MAX_EXTENT || height > MAX_EXTENT {
        return;
    }
    for offset in (8..0x80).step_by(8) {
        unsafe {
            ptr::copy_nonoverlapping(output.cast::<u8>(), result.cast::<u8>().add(offset), 8)
        };
    }
}

unsafe extern "C" fn fsr_dimensions(
    width: *mut u32,
    height: *mut u32,
    output_width: u32,
    output_height: u32,
    mode: u32,
) {
    if let Some(native) = unsafe { trampoline::<DimensionsFn>(&ORIGINAL_DIMENSIONS) } {
        unsafe { native(width, height, output_width, output_height, mode) };
    }
    let quality = requested_quality();
    if output_width == 0
        || output_height == 0
        || output_width > MAX_EXTENT
        || output_height > MAX_EXTENT
    {
        return;
    }
    let size = match quality {
        0 => (output_width, output_height),
        4 => (output_width / 3, output_height / 3),
        _ => return,
    };
    unsafe {
        width.write(size.0);
        height.write(size.1);
    }
}

unsafe extern "C" fn refresh_dimensions() {
    if let Some(native) = unsafe { trampoline::<RefreshFn>(&ORIGINAL_REFRESH) } {
        unsafe { native() };
    }
    let Some(platform) = asi::platform() else {
        return;
    };
    if !logging() {
        return;
    }
    let image = IMAGE.load(Ordering::Acquire) as *const u8;
    let (input, output) = unsafe {
        (
            image.add(site::RENDER_INPUT),
            image.add(site::RENDER_OUTPUT),
        )
    };
    let thread = unsafe { (platform.current_thread)() };
    let message = unsafe {
        format!(
            "RDR2 renderer dimensions refreshed: quality={} input={}x{} output={}x{} thread={thread}.",
            requested_quality(),
            word(input),
            word(input.add(4)),
            word(output),
            word(output.add(4)),
        )
    };
    log(&message);
}

/// Pins the pending graphics owner, which native Apply copies from manager+b8. The renderer reads
/// manager+8, and a change to the active collection is overwritten at reset.
///
/// # Safety
/// `manager_object` is the engine's live settings manager.
unsafe fn pin_pending(platform: &AsiPlatform, manager_object: *mut c_void, quality: u32) -> bool {
    let image = IMAGE.load(Ordering::Acquire);
    let ids = [manager::GRAPHICS_ID];
    let (active, pending) = unsafe {
        (
            (platform.engine_owner)(image + site::OWNER.rva, manager_object).cast::<u8>(),
            (platform.engine_find)(
                image + site::FIND.rva,
                manager_object,
                ids.as_ptr(),
                ids.len() as u64,
                manager_object
                    .cast::<u8>()
                    .add(manager::PENDING_SCRATCH)
                    .cast(),
            )
            .cast::<u8>(),
        )
    };
    if active.is_null() || pending.is_null() || active == pending {
        return false;
    }
    let first = unsafe { pending.add(manager::OWNER_COLLECTION) };
    let second = unsafe { pending.add(manager::PENDING_SECOND_COLLECTION) };
    let (Some(first_values), Some(second_values)) =
        (snapshot(platform, first), snapshot(platform, second))
    else {
        return false;
    };
    let first_vendor = collection_vendor(platform, &first_values);
    let second_vendor = if get(&first_values, collection::VIDEO_ADAPTER)
        == get(&second_values, collection::VIDEO_ADAPTER)
    {
        first_vendor
    } else {
        collection_vendor(platform, &second_values)
    };
    let mut native_index = u32::MAX;
    if first_vendor == NVIDIA || second_vendor == NVIDIA {
        let count = unsafe { word(active.add(manager::DLSS_CHOICE_COUNT)) } as usize;
        if count == 0 || count > manager::DLSS_CHOICE_LIMIT {
            return false;
        }
        let choices: Vec<u32> = (0..count)
            .map(|index| unsafe { word(active.add(manager::DLSS_CHOICES + index * 4)) })
            .collect();
        let Some(index) = dlss_choice(&choices, quality) else {
            return false;
        };
        native_index = index;
    }
    let selection = (u64::from(native_index) << 32) | u64::from(quality);
    let (Some(first_plan), Some(second_plan)) = (
        plan_collection(first_values, first_vendor, selection),
        plan_collection(second_values, second_vendor, selection),
    ) else {
        return false;
    };
    unsafe {
        apply_collection(platform, first, &first_values, &first_plan, first_vendor);
        apply_collection(
            platform,
            second,
            &second_values,
            &second_plan,
            second_vendor,
        );
    }
    SELECTION.store(selection, Ordering::SeqCst);
    true
}

unsafe extern "C" fn reconfigure(manager_object: *mut c_void) {
    // Also covers stock Apply: native Off or another native quality cannot replace the RSF
    // carrier when the game consumes its pending settings.
    let pinned = asi::platform().is_some_and(|platform| unsafe {
        pin_pending(platform, manager_object, requested_quality())
    });
    if !pinned {
        FAILURES.fetch_add(1, Ordering::SeqCst);
    }
    if let Some(native) = unsafe { trampoline::<MethodFn>(&ORIGINAL_RECONFIGURE) } {
        unsafe { native(manager_object) };
    }
}

struct Hook {
    site: &'static crate::layout::Site,
    detour: *const c_void,
    original: &'static AtomicPtr<c_void>,
}

/// Installs the seven settings hooks after exact-image decryption, before native settings load.
/// Every entry and engine function is checked first; any refusal leaves no hook behind.
pub(crate) fn install(platform: &AsiPlatform, configured_quality: u32) -> bool {
    if INSTALLED.load(Ordering::SeqCst) {
        return true;
    }
    let quality = if configured_quality <= QUALITY_LIMIT {
        configured_quality
    } else {
        DEFAULT_QUALITY
    };
    SELECTION.store(
        ((u32::MAX as u64) << 32) | u64::from(quality),
        Ordering::SeqCst,
    );
    let image = unsafe { (platform.main_module)() };
    IMAGE.store(image, Ordering::Release);
    let hooks = [
        Hook {
            site: &site::COPY,
            detour: copy as *const c_void,
            original: &ORIGINAL_COPY,
        },
        Hook {
            site: &site::LOAD,
            detour: load as *const c_void,
            original: &ORIGINAL_LOAD,
        },
        Hook {
            site: &site::APPLY,
            detour: apply as *const c_void,
            original: &ORIGINAL_APPLY,
        },
        Hook {
            site: &site::OPTIMAL,
            detour: optimal as *const c_void,
            original: &ORIGINAL_OPTIMAL,
        },
        Hook {
            site: &site::FSR_DIMENSIONS,
            detour: fsr_dimensions as *const c_void,
            original: &ORIGINAL_DIMENSIONS,
        },
        Hook {
            site: &site::REFRESH_DIMENSIONS,
            detour: refresh_dimensions as *const c_void,
            original: &ORIGINAL_REFRESH,
        },
        Hook {
            site: &site::RECONFIGURE,
            detour: reconfigure as *const c_void,
            original: &ORIGINAL_RECONFIGURE,
        },
    ];
    if !hooks
        .iter()
        .all(|hook| platform.matches(image + hook.site.rva, &hook.site.expected))
    {
        return false;
    }
    if ![site::MANAGER, site::OWNER, site::FIND]
        .iter()
        .all(|function| platform.matches(image + function.rva, function.expected))
    {
        return false;
    }
    let initialized = unsafe { (platform.hook_initialize)() };
    if initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED {
        return false;
    }
    let remove = |hooks: &[Hook]| {
        for hook in hooks {
            unsafe { (platform.hook_remove)(image + hook.site.rva) };
        }
    };
    for (created, hook) in hooks.iter().enumerate() {
        let mut native = ptr::null_mut();
        if unsafe { (platform.hook_create)(image + hook.site.rva, hook.detour, &mut native) }
            != MH_OK
        {
            remove(&hooks[..created]);
            return false;
        }
        hook.original.store(native, Ordering::Release);
    }
    if hooks
        .iter()
        .any(|hook| unsafe { (platform.hook_queue_enable)(image + hook.site.rva) } != MH_OK)
    {
        remove(&hooks[..]);
        return false;
    }
    let installed = unsafe { (platform.hook_apply_queued)() } == MH_OK;
    if !installed {
        for hook in &hooks {
            unsafe { (platform.hook_disable)(image + hook.site.rva) };
        }
        remove(&hooks[..]);
    }
    INSTALLED.store(installed, Ordering::SeqCst);
    installed
}

/// RSF's quality preference from the registered ScriptHook game fiber, never the presentation or
/// job workers. A change pins the pending owner and requests the engine's own reconfiguration.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_engine_quality_tick(quality: u32) {
    if !INSTALLED.load(Ordering::SeqCst) || quality > QUALITY_LIMIT {
        return;
    }
    let Some(platform) = asi::platform() else {
        return;
    };
    let changed = requested_quality() != quality;
    if PENDING_INITIALIZED.load(Ordering::SeqCst) && !changed {
        return;
    }
    let image = IMAGE.load(Ordering::Acquire);
    let manager_object = unsafe { (platform.engine_manager)(image + site::MANAGER.rva) };
    if manager_object.is_null() {
        return;
    }
    let stage = unsafe { word(manager_object.cast::<u8>().add(manager::STAGE)) };
    // Native initialization and an existing restart/apply operation own their state until
    // completion. Retain the request and retry on a later game tick.
    if stage <= 2 || stage == 8 || stage == 9 {
        return;
    }
    if !unsafe { pin_pending(platform, manager_object, quality) } {
        return;
    }
    PENDING_INITIALIZED.store(true, Ordering::SeqCst);
    if !changed {
        // Startup already allocated the selected input size.
        return;
    }
    // Enter the native graphics reconfiguration state 9. This calls the concrete manager's +60
    // method (+23b570), which requests the renderer's synchronized reset through its +188 method
    // (+266dd1c). Settings notifications alone (+2c7d190) do not request this reset and leave
    // old allocations in use. The engine completes state 9 and refreshes dimensions at its own
    // safe point.
    if let Some(native) = unsafe { trampoline::<MethodFn>(&ORIGINAL_RECONFIGURE) } {
        unsafe { native(manager_object) };
    }
    if logging() {
        let after = unsafe { word(manager_object.cast::<u8>().add(manager::STAGE)) };
        log(&format!(
            "RDR2 quality request={quality}: pending owner updated, manager={:016X} stage={stage}->{after}; native graphics reconfiguration requested.",
            manager_object as usize
        ));
    }
}

/// Announces the ownership state and keeps `log` for later engine-settings messages.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_rdr2_report_engine_settings(
    log_fn: rsf_game_log_fn,
    user: *mut c_void,
) {
    let Some(writer) = log_fn else { return };
    LOGGER_USER.store(user, Ordering::Release);
    LOGGER.store(writer as *mut c_void, Ordering::Release);
    let cached = ADAPTER.load(Ordering::SeqCst);
    log(&format!(
        "RDR2 engine settings ownership {}: copy+{:x}/load+{:x}/apply+{:x}; adapter={} vendor={:x} overrides={} refused={}; RSF overlay owns SR/latency.",
        if INSTALLED.load(Ordering::SeqCst) {
            "armed"
        } else {
            "REFUSED"
        },
        site::COPY.rva,
        site::LOAD.rva,
        site::APPLY.rva,
        (cached >> 32) as u32,
        cached as u32,
        OVERRIDES.load(Ordering::SeqCst),
        FAILURES.load(Ordering::SeqCst),
    ));
}

/// C ABI of the pure policy for the native engine-settings fixture.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_dlss_choice_abi(modes: *const u32, count: u32, quality: u32) -> u32 {
    if modes.is_null() || count == 0 || count as usize > manager::DLSS_CHOICE_LIMIT {
        return u32::MAX;
    }
    let modes = unsafe { std::slice::from_raw_parts(modes, count as usize) };
    dlss_choice(modes, quality).unwrap_or(u32::MAX)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_pin_engine_settings_abi(
    collection: *mut c_void,
    bytes: usize,
    vendor: u32,
    quality: u32,
) -> bool {
    if collection.is_null() {
        return false;
    }
    let collection = unsafe { std::slice::from_raw_parts_mut(collection.cast::<u8>(), bytes) };
    pin(collection, vendor, quality)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_storable_refresh_abi(
    numerator: *mut u32,
    denominator: *mut u32,
    desktop_hz: u32,
) -> bool {
    if numerator.is_null() || denominator.is_null() {
        return false;
    }
    unsafe { storable_refresh(&mut *numerator, &mut *denominator, desktop_hz) }
}

#[cfg(test)]
mod tests;
