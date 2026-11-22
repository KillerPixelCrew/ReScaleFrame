// SPDX-License-Identifier: MIT
use crate::latency_antilag::{AMD_VENDOR_ID, fps_from_interval};
use crate::sr_session::has_capacity;
use crate::sys::rsf_vk_function;
use crate::sys::{
    RSF_LATENCYVK_DEVICE_ABI_VERSION, rsf_latencyvk_created_device as Created,
    rsf_latencyvk_device_query as Query, rsf_latencyvk_device_requirements as Requirements,
};
use ash::vk::{self, Handle as _};
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::{Condvar, Mutex};

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -8;
const NOT_SUPPORTED: i32 = -7;
const FEATURE_FAILED: i32 = -9;
const INPUT: u32 = 0;
const PRESENT: u32 = 1;
const MODE_ONLY: u32 = u32::MAX;
const FRAME_SLOTS: usize = 128;
const FEATURES_2: u32 = 1_000_059_000;
const ANTI_LAG_FEATURE: u32 = 1_000_476_000;
const ANTI_LAG_DATA: i32 = 1_000_476_001;
const ANTI_LAG_PRESENTATION_INFO: i32 = 1_000_476_002;
const ANTI_LAG_ON: i32 = 1;
const ANTI_LAG_OFF: i32 = 2;
const ANTI_LAG_EXTENSION: &CStr = c"VK_AMD_anti_lag";

#[repr(C)]
struct FeatureHeader {
    kind: u32,
    next: *mut FeatureHeader,
}

#[repr(C)]
struct AntiLagFeature {
    header: FeatureHeader,
    enabled: u32,
}

const _: () = {
    assert!(size_of::<FeatureHeader>() == 16);
    assert!(std::mem::offset_of!(FeatureHeader, next) == 8);
    assert!(size_of::<AntiLagFeature>() == 24);
    assert!(std::mem::offset_of!(AntiLagFeature, enabled) == 16);
};

thread_local! {
    // The caller uses the node in its synchronous device create on this thread.
    static FEATURE: UnsafeCell<AntiLagFeature> = const {
        UnsafeCell::new(AntiLagFeature {
            header: FeatureHeader { kind: ANTI_LAG_FEATURE, next: ptr::null_mut() },
            enabled: 1,
        })
    };
}

struct ExtensionNames([*const c_char; 1]);

// The only pointed-to data is the immutable static extension string.
unsafe impl Sync for ExtensionNames {}

static EXTENSIONS: ExtensionNames = ExtensionNames([ANTI_LAG_EXTENSION.as_ptr()]);

unsafe fn feature_chain(
    mut node: *mut FeatureHeader,
) -> Option<(*mut FeatureHeader, *mut AntiLagFeature)> {
    let mut root = ptr::null_mut::<FeatureHeader>();
    let mut feature = ptr::null_mut::<AntiLagFeature>();
    for _ in 0..64 {
        if node.is_null() {
            break;
        }
        match unsafe { (*node).kind } {
            FEATURES_2 if root.is_null() => root = node,
            ANTI_LAG_FEATURE if feature.is_null() => feature = node.cast(),
            _ => {}
        }
        node = unsafe { (*node).next };
    }
    node.is_null().then_some((root, feature))
}

unsafe fn enable_feature(out: *mut Requirements) -> i32 {
    let features = unsafe { (*out).features };
    let Some((root, existing)) = (unsafe { feature_chain(features.cast()) }) else {
        return INVALID;
    };
    if existing.is_null() {
        FEATURE.with(|feature| unsafe {
            let node = feature.get();
            (*node).enabled = 1;
            if root.is_null() {
                (*node).header.next = features.cast();
                (*out).features = node.cast();
            } else {
                (*node).header.next = (*root).next;
                (*root).next = node.cast();
            }
        });
    } else {
        unsafe { (*existing).enabled = 1 };
    }
    unsafe {
        (*out).extensions = EXTENSIONS.0.as_ptr();
        (*out).extension_count = 1;
    }
    0
}

unsafe fn anti_lag_enabled(info: &vk::DeviceCreateInfo<'_>) -> bool {
    let count = info.enabled_extension_count as usize;
    if count == 0 || count > 4096 || info.pp_enabled_extension_names.is_null() {
        return false;
    }
    let Some((_, feature)) =
        (unsafe { feature_chain(info.p_next.cast_mut().cast::<FeatureHeader>()) })
    else {
        return false;
    };
    if feature.is_null() || unsafe { (*feature).enabled } == 0 {
        return false;
    }
    unsafe { std::slice::from_raw_parts(info.pp_enabled_extension_names, count) }
        .iter()
        .any(|&name| !name.is_null() && unsafe { CStr::from_ptr(name) } == ANTI_LAG_EXTENSION)
}

// VK_AMD_anti_lag is newer than the pinned ash Vulkan registry. These are the
// extension's x64 C layouts from Vulkan-Headers 1.4.357.
#[repr(C)]
struct PresentationInfo {
    kind: i32,
    next: *mut c_void,
    stage: i32,
    frame_index: u64,
}

#[repr(C)]
struct AntiLagData {
    kind: i32,
    next: *const c_void,
    mode: i32,
    max_fps: u32,
    presentation: *const PresentationInfo,
}

const _: () = {
    assert!(size_of::<PresentationInfo>() == 32);
    assert!(std::mem::offset_of!(PresentationInfo, frame_index) == 24);
    assert!(size_of::<AntiLagData>() == 32);
    assert!(std::mem::offset_of!(AntiLagData, presentation) == 24);
};

type UpdateDevice = unsafe extern "system" fn(vk::Device, *const AntiLagData);

#[derive(Clone, Copy)]
struct Device {
    handle: *mut c_void,
    update: UpdateDevice,
}

// The opaque Vulkan handle is borrowed until all admitted SDK calls retire.
unsafe impl Send for Device {}

impl Device {
    fn call(self, stage: u32, frame: u64, enabled: bool, fps: u32) {
        let present = PresentationInfo {
            kind: ANTI_LAG_PRESENTATION_INFO,
            next: ptr::null_mut(),
            stage: stage as i32,
            frame_index: frame,
        };
        let data = AntiLagData {
            kind: ANTI_LAG_DATA,
            next: ptr::null(),
            mode: if enabled { ANTI_LAG_ON } else { ANTI_LAG_OFF },
            max_fps: fps,
            presentation: if stage == MODE_ONLY {
                ptr::null()
            } else {
                &present
            },
        };
        unsafe { (self.update)(vk::Device::from_raw(self.handle as u64), &data) };
    }
}

#[derive(Clone, Copy)]
struct Key {
    frame: u64,
    input: bool,
    present_owed: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Status {
    pub(crate) available: u32,
    pub(crate) enabled: u32,
    pub(crate) input_updates: u64,
    pub(crate) present_updates: u64,
}

#[derive(PartialEq, Eq)]
enum Phase {
    Open,
    RetiringDevice,
    Closing,
}

struct State {
    device: Option<Device>,
    calls: usize,
    phase: Phase,
    configuring: bool,
    configured: Option<(bool, u32)>,
    session: u64,
    keys: [Option<Key>; FRAME_SLOTS],
    input_updates: u64,
    present_updates: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            device: None,
            calls: 0,
            phase: Phase::Open,
            configuring: false,
            configured: None,
            session: 0,
            keys: [None; FRAME_SLOTS],
            input_updates: 0,
            present_updates: 0,
        }
    }

    fn slot(&mut self, frame: u64) -> &mut Option<Key> {
        &mut self.keys[(frame % FRAME_SLOTS as u64) as usize]
    }

    fn for_device(&self, device: *mut c_void) -> bool {
        self.device.is_some_and(|binding| binding.handle == device)
    }

    fn enabled(&self) -> bool {
        self.configured.is_some_and(|(enabled, _)| enabled)
    }

    fn admit(&mut self, device: Device, stage: u32, frame: u64) -> Update {
        self.calls += 1;
        let (enabled, interval) = self.configured.unwrap_or_default();
        Update {
            device,
            stage,
            frame,
            enabled,
            fps: if enabled {
                fps_from_interval(interval)
            } else {
                0
            },
        }
    }

    fn frame_update(&mut self, session: u64, frame: u64, stage: u32) -> Option<Update> {
        let device = self.device?;
        if !self.enabled()
            || (session != 0 && session != self.session)
            || self.phase == Phase::Closing
            || (stage == INPUT && (self.phase != Phase::Open || self.configuring))
        {
            return None;
        }
        let key = self.slot(frame).as_mut().filter(|key| key.frame == frame)?;
        if stage == INPUT {
            if key.input {
                return None;
            }
            key.input = true;
            key.present_owed = true;
            self.input_updates += 1;
        } else {
            if !key.present_owed {
                return None;
            }
            key.present_owed = false;
            self.present_updates += 1;
        }
        Some(self.admit(device, stage, frame))
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static DRAINED: Condvar = Condvar::new();

struct Update {
    device: Device,
    stage: u32,
    frame: u64,
    enabled: bool,
    fps: u32,
}

impl Update {
    fn call(self) {
        self.device
            .call(self.stage, self.frame, self.enabled, self.fps);
    }
}

impl Drop for Update {
    fn drop(&mut self) {
        let mut state = STATE.lock().unwrap();
        state.calls -= 1;
        if self.stage == MODE_ONLY {
            state.configuring = false;
        }
        if state.calls == 0 {
            DRAINED.notify_all();
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_amd_vk_require(
    enabled: u32,
    query: *const Query,
    out: *mut Requirements,
) -> i32 {
    if !unsafe { has_capacity(query) && has_capacity(out) } {
        return INVALID;
    }
    let query = unsafe { &*query };
    if query.abi_version != RSF_LATENCYVK_DEVICE_ABI_VERSION
        || unsafe { (*out).abi_version } != RSF_LATENCYVK_DEVICE_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    if query.instance.is_null() || query.physical_device.is_null() || query.instance_proc.is_none()
    {
        return INVALID;
    }
    unsafe {
        out.write(Requirements {
            struct_size: size_of::<Requirements>() as u32,
            abi_version: RSF_LATENCYVK_DEVICE_ABI_VERSION,
            extension_count: 0,
            extensions: ptr::null(),
            features: query.features,
        });
    }
    if enabled != 0 && unsafe { amd_device(query) } {
        match unsafe { supported(query) } {
            Ok(true) => return unsafe { enable_feature(out) },
            Ok(false) => {}
            Err(result) => return result,
        }
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_amd_vk_device_created(enabled: u32, facts: *const Created) -> i32 {
    if !unsafe { has_capacity(facts) } {
        return INVALID;
    }
    let facts = unsafe { &*facts };
    if facts.device.is_null() || facts.create_info.is_null() {
        return INVALID;
    }
    if facts.abi_version != RSF_LATENCYVK_DEVICE_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if enabled == 0 {
        return NOT_SUPPORTED;
    }
    if facts.instance.is_null() || facts.physical_device.is_null() || facts.instance_proc.is_none()
    {
        return NOT_SUPPORTED;
    }
    let query = Query {
        struct_size: size_of::<Query>() as u32,
        abi_version: RSF_LATENCYVK_DEVICE_ABI_VERSION,
        instance: facts.instance,
        physical_device: facts.physical_device,
        instance_proc: facts.instance_proc,
        features: ptr::null_mut(),
    };
    if !unsafe { amd_device(&query) } || !unsafe { anti_lag_enabled(&*facts.create_info.cast()) } {
        return NOT_SUPPORTED;
    }
    let Some(get_device) = (unsafe { get_proc(&query, c"vkGetDeviceProcAddr") }) else {
        return NOT_SUPPORTED;
    };
    let get_device = unsafe {
        std::mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkGetDeviceProcAddr>(get_device)
    };
    let handle = vk::Device::from_raw(facts.device as u64);
    let Some(update) = (unsafe { get_device(handle, c"vkAntiLagUpdateAMD".as_ptr()) }) else {
        return NOT_SUPPORTED;
    };
    let update =
        unsafe { std::mem::transmute::<unsafe extern "system" fn(), UpdateDevice>(update) };
    bind(facts.device, update)
}

unsafe fn get_proc(query: &Query, name: &CStr) -> rsf_vk_function {
    unsafe { (query.instance_proc.unwrap())(query.instance, name.as_ptr()) }
}

unsafe fn amd_device(query: &Query) -> bool {
    let Some(properties) = (unsafe { get_proc(query, c"vkGetPhysicalDeviceProperties") }) else {
        return false;
    };
    let properties = unsafe {
        std::mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkGetPhysicalDeviceProperties>(
            properties,
        )
    };
    let mut value = vk::PhysicalDeviceProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(query.physical_device as u64),
            &mut value,
        )
    };
    value.vendor_id == AMD_VENDOR_ID
}

unsafe fn supported(query: &Query) -> Result<bool, i32> {
    let Some(enumerate) = (unsafe { get_proc(query, c"vkEnumerateDeviceExtensionProperties") })
    else {
        return Ok(false);
    };
    let features = unsafe { get_proc(query, c"vkGetPhysicalDeviceFeatures2") }
        .or_else(|| unsafe { get_proc(query, c"vkGetPhysicalDeviceFeatures2KHR") });
    let Some(features) = features else {
        return Ok(false);
    };
    let enumerate = unsafe {
        std::mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkEnumerateDeviceExtensionProperties>(
            enumerate,
        )
    };
    let features = unsafe {
        std::mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkGetPhysicalDeviceFeatures2>(
            features,
        )
    };
    let physical = vk::PhysicalDevice::from_raw(query.physical_device as u64);
    let mut count = 0;
    if unsafe { enumerate(physical, ptr::null(), &mut count, ptr::null_mut()) }
        != vk::Result::SUCCESS
        || count == 0
        || count > 4096
    {
        return Ok(false);
    }
    let mut extensions = Vec::new();
    extensions
        .try_reserve_exact(count as usize)
        .map_err(|_| FEATURE_FAILED)?;
    extensions.resize(count as usize, vk::ExtensionProperties::default());
    if unsafe { enumerate(physical, ptr::null(), &mut count, extensions.as_mut_ptr()) }
        != vk::Result::SUCCESS
        || count as usize > extensions.len()
    {
        return Ok(false);
    }
    if !extensions[..count as usize].iter().any(|entry| {
        entry
            .extension_name_as_c_str()
            .is_ok_and(|name| name == ANTI_LAG_EXTENSION)
    }) {
        return Ok(false);
    }
    let mut supported = AntiLagFeature {
        header: FeatureHeader {
            kind: ANTI_LAG_FEATURE,
            next: ptr::null_mut(),
        },
        enabled: 0,
    };
    let mut root = vk::PhysicalDeviceFeatures2 {
        p_next: (&raw mut supported).cast(),
        ..Default::default()
    };
    unsafe { features(physical, &mut root) };
    Ok(supported.enabled != 0)
}

fn bind(handle: *mut c_void, update: UpdateDevice) -> i32 {
    let mut state = STATE.lock().unwrap();
    if state.phase != Phase::Open || (state.device.is_some() && !state.for_device(handle)) {
        return NOT_READY;
    }
    if state.device.is_none() {
        state.device = Some(Device { handle, update });
    }
    0
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_amd_vk_status() -> Status {
    let state = STATE.lock().unwrap();
    Status {
        available: u32::from(state.device.is_some() && state.phase != Phase::Closing),
        enabled: u32::from(state.enabled()),
        input_updates: state.input_updates,
        present_updates: state.present_updates,
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_amd_vk_options(mode: u32, interval: u32) {
    let update = {
        let mut state = STATE.lock().unwrap();
        let Some(device) = state.device else {
            return;
        };
        let enabled = mode != 0;
        if state.phase != Phase::Open
            || state.configuring
            || state.configured == Some((enabled, interval))
        {
            return;
        }
        if enabled != state.enabled() {
            for key in state.keys.iter_mut().flatten() {
                key.present_owed = false;
            }
        }
        state.configured = Some((enabled, interval));
        state.configuring = true;
        state.admit(device, MODE_ONLY, 0)
    };
    update.call();
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_amd_vk_begin(session: u64, frame: u64) {
    let mut state = STATE.lock().unwrap();
    if state.device.is_none() || state.phase != Phase::Open || session == 0 || frame == 0 {
        return;
    }
    if state.session != session {
        state.session = session;
        state.keys.fill(None);
    }
    let slot = state.slot(frame);
    if !slot.is_some_and(|key| key.frame == frame) {
        *slot = Some(Key {
            frame,
            input: false,
            present_owed: false,
        });
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_amd_vk_cancel(session: u64, frame: u64) {
    let mut state = STATE.lock().unwrap();
    if state.session == session
        && let Some(key) = state.slot(frame).as_mut().filter(|key| key.frame == frame)
    {
        key.present_owed = false;
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_amd_vk_frame(session: u64, frame: u64, stage: u32) {
    if !matches!(stage, INPUT | PRESENT) {
        return;
    }
    let update = STATE.lock().unwrap().frame_update(session, frame, stage);
    if let Some(update) = update {
        // An earlier Present must be able to proceed while a newer input sleeps.
        update.call();
    }
}

fn close(retire_device: bool) {
    let device = {
        let state = STATE.lock().unwrap();
        debug_assert!(state.phase == Phase::Closing && state.calls == 0);
        state.device.filter(|_| state.enabled())
    };
    if let Some(device) = device {
        device.call(MODE_ONLY, 0, false, 0);
    }
    if retire_device {
        crate::vulkan_latency::amd_device_retired();
    }
    let mut state = STATE.lock().unwrap();
    *state = State::new();
    DRAINED.notify_all();
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_amd_vk_shutdown_begin() -> i32 {
    let mut state = STATE.lock().unwrap();
    if state.phase != Phase::Open || state.calls != 0 {
        return NOT_READY;
    }
    state.phase = Phase::Closing;
    0
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_amd_vk_shutdown_end(committed: u32) {
    if committed != 0 {
        close(false);
    } else {
        STATE.lock().unwrap().phase = Phase::Open;
        DRAINED.notify_all();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_amd_vk_device_destroyed(device: *mut c_void) {
    let state = STATE.lock().unwrap();
    let mut state = DRAINED
        .wait_while(state, |state| {
            state.for_device(device) && state.phase != Phase::Open
        })
        .unwrap();
    if !state.for_device(device) {
        return;
    }
    state.phase = Phase::RetiringDevice;
    // The public void notification precedes the real vkDestroyDevice. Drain
    // outstanding borrows, allowing already-owed older Presents to finish.
    let mut state = DRAINED.wait_while(state, |state| state.calls != 0).unwrap();
    state.phase = Phase::Closing;
    drop(state);
    close(true);
}
