// SPDX-License-Identifier: MIT
use crate::com_ref;
use crate::d3d11_device_identity::identity as device_identity;
use crate::d3d11_observer_hooks::installed;
use crate::d3d11_observer_present::{frames, present_size};
use crate::sr_session::has_capacity;
use crate::sys::{rsf_dump_log_fn, rsf_observer_options, rsf_observer_status};
use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Write;
use std::fs;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

mod dump;
use dump::DumpSettings;

use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::core::{IUnknown, Interface};

type Handle = *mut c_void;
const OK: i32 = 0;
const INVALID: i32 = -1;
const NOT_READY: i32 = -6;
const CONSTANT_BUFFER: u32 = 0x4;
const RENDER_TARGET: u32 = 0x20;
const CANDIDATE_LIMIT: usize = 4;
const CONSTANT_LIMIT: usize = 16;
const HISTOGRAM_LIMIT: usize = 128;

#[derive(Clone, Copy)]
struct Filter {
    format: u32,
    minimum_width: u32,
    capacity: u32,
    constant_min: u32,
    constant_max: u32,
}

#[derive(Default)]
struct DumpResult {
    textures: u32,
    constant_bytes: u32,
}

struct Request {
    prefix: CString,
    view: u32,
}

#[derive(Clone)]
struct Constant {
    bytes: u32,
    buffer: IUnknown,
}

struct Resources {
    identity: IUnknown,
    textures: Vec<IUnknown>,
    constants: Vec<Constant>,
}

struct Retained {
    candidates: VecDeque<Resources>,
    identity: Option<IUnknown>,
    context: Option<IUnknown>,
    device: Option<IUnknown>,
}

impl Retained {
    const fn new() -> Self {
        Self {
            candidates: VecDeque::new(),
            identity: None,
            context: None,
            device: None,
        }
    }

    fn selected(&self) -> Option<&Resources> {
        let identity = self.identity.as_ref()?;
        self.candidates
            .iter()
            .find(|entry| same_identity(&entry.identity, identity))
    }

    unsafe fn record(
        &mut self,
        device: Handle,
        identity: &IUnknown,
    ) -> (Option<&mut Resources>, Option<Resources>) {
        if PRESENTING_DEVICE.load(Ordering::Relaxed)
            && !self
                .identity
                .as_ref()
                .is_some_and(|selected| same_identity(selected, identity))
        {
            return (None, None);
        }
        if self.device.is_none() {
            self.device = Some(unsafe { com_ref::retain(device) });
            self.identity = Some(identity.clone());
        }
        let mut retired = None;
        let entry = if let Some(index) = self
            .candidates
            .iter()
            .position(|entry| same_identity(&entry.identity, identity))
        {
            self.candidates.remove(index).expect("candidate index")
        } else {
            if self.candidates.len() == CANDIDATE_LIMIT {
                retired = self.candidates.pop_front();
            }
            Resources {
                identity: identity.clone(),
                textures: Vec::new(),
                constants: Vec::new(),
            }
        };
        self.candidates.push_back(entry);
        (self.candidates.back_mut(), retired)
    }
}

fn same_identity(first: &IUnknown, second: &IUnknown) -> bool {
    first.as_raw() == second.as_raw()
}

struct Capture {
    filter: Filter,
    dump_settings: DumpSettings,
    retained: Retained,
    sizes: Vec<(u32, u32)>,
    textures_created: u32,
    constant_matches: u32,
    request: Option<Request>,
    dumps_completed: u32,
    dump_result: DumpResult,
}

// Access is serialized. Only reference management takes place under the
// inventory lock. GPU calls run with retained references on the caller's thread.
unsafe impl Send for Capture {}

static CAPTURE: Mutex<Capture> = Mutex::new(Capture {
    filter: Filter {
        format: 0,
        minimum_width: 0,
        capacity: 0,
        constant_min: 0,
        constant_max: 0,
    },
    dump_settings: DumpSettings::EMPTY,
    retained: Retained::new(),
    sizes: Vec::new(),
    textures_created: 0,
    constant_matches: 0,
    request: None,
    dumps_completed: 0,
    dump_result: DumpResult {
        textures: 0,
        constant_bytes: 0,
    },
});
static REQUESTED: AtomicBool = AtomicBool::new(false);
static PRESENTING_DEVICE: AtomicBool = AtomicBool::new(false);

fn lock() -> MutexGuard<'static, Capture> {
    crate::sync::lock(&CAPTURE)
}

pub(super) fn configure(options: &rsf_observer_options, log: rsf_dump_log_fn) {
    let mut state = lock();
    state.filter = Filter {
        format: options.format,
        minimum_width: options.minimum_width,
        capacity: if options.capacity == 0 {
            16
        } else {
            options.capacity
        },
        constant_min: options.constant_buffer_min_bytes,
        constant_max: options.constant_buffer_max_bytes,
    };
    state.dump_settings = DumpSettings::new(options, log);
}

pub(super) unsafe fn record_texture(
    device: Handle,
    texture: Handle,
    width: u32,
    format: u32,
    bindings: u32,
) {
    let identity = unsafe { device_identity(device) };
    let mut state = lock();
    state.textures_created = state.textures_created.wrapping_add(1);
    let filter = state.filter;
    if bindings & RENDER_TARGET == 0
        || (filter.format != 0 && filter.format != format)
        || width < filter.minimum_width
    {
        return;
    }
    let Some(identity) = identity else { return };
    let (resources, retired) = unsafe { state.retained.record(device, &identity) };
    if let Some(resources) = resources
        && resources.textures.len() < filter.capacity as usize
    {
        resources.textures.push(unsafe { com_ref::retain(texture) });
    }
    drop(state);
    drop(retired);
}

pub(super) unsafe fn record_buffer(device: Handle, buffer: Handle, bytes: u32, bindings: u32) {
    let identity = unsafe { device_identity(device) };
    let mut state = lock();
    let filter = state.filter;
    if filter.constant_max == 0 || bindings & CONSTANT_BUFFER == 0 {
        return;
    }
    if let Some((_, created)) = state.sizes.iter_mut().find(|(size, _)| *size == bytes) {
        *created = created.wrapping_add(1);
    } else if state.sizes.len() < HISTOGRAM_LIMIT {
        state.sizes.push((bytes, 1));
    }
    let Some(identity) = identity else { return };
    let (resources, retired) = unsafe { state.retained.record(device, &identity) };
    let mut matched = false;
    let mut previous = None;
    if let Some(resources) = resources
        && bytes >= filter.constant_min
        && bytes <= filter.constant_max
    {
        let buffers = &mut resources.constants;
        if let Some(entry) = buffers.iter_mut().find(|entry| entry.bytes == bytes) {
            previous = Some(std::mem::replace(&mut entry.buffer, unsafe {
                com_ref::retain(buffer)
            }));
            matched = true;
        } else if buffers.len() < CONSTANT_LIMIT {
            buffers.push(Constant {
                bytes,
                buffer: unsafe { com_ref::retain(buffer) },
            });
            matched = true;
        }
    }
    if matched {
        state.constant_matches = state.constant_matches.wrapping_add(1);
    }
    drop(state);
    drop(previous);
    drop(retired);
}

pub(super) fn needs_presenter() -> bool {
    !PRESENTING_DEVICE.load(Ordering::Acquire) || REQUESTED.load(Ordering::Acquire)
}

pub(super) fn adopt_device(incoming: IUnknown) -> Option<IUnknown> {
    let identity = (unsafe { device_identity(incoming.as_raw()) })?;
    let (old, context, old_identity, discarded) = {
        let mut state = lock();
        if PRESENTING_DEVICE.swap(true, Ordering::AcqRel) {
            return None;
        }
        let retained = &mut state.retained;
        let mut discarded = std::mem::take(&mut retained.candidates);
        if let Some(index) = discarded
            .iter()
            .position(|entry| same_identity(&entry.identity, &identity))
        {
            retained
                .candidates
                .push_back(discarded.remove(index).expect("candidate index"));
        }
        let (old, context, old_identity) = if retained
            .identity
            .as_ref()
            .is_some_and(|old| same_identity(old, &identity))
        {
            (None, None, None)
        } else {
            (
                retained.device.replace(incoming),
                retained.context.take(),
                retained.identity.replace(identity),
            )
        };
        (old, context, old_identity, discarded)
    };
    drop((context, old_identity, discarded));
    old
}

pub(super) fn clear() {
    let retained = {
        let mut state = lock();
        state.request = None;
        state.dump_settings = DumpSettings::EMPTY;
        REQUESTED.store(false, Ordering::Release);
        PRESENTING_DEVICE.store(false, Ordering::Release);
        std::mem::replace(&mut state.retained, Retained::new())
    };
    drop(retained);
}

fn cache_context(retained: &mut Retained, device: &IUnknown, context: &IUnknown) {
    if retained.context.is_none()
        && retained
            .device
            .as_ref()
            .is_some_and(|selected| selected.as_raw() == device.as_raw())
    {
        retained.context = Some(context.clone());
    }
}

// The cached immediate context, or the device's, cached for later dumps.
fn resolve_context(device: &IUnknown, cached: Option<IUnknown>) -> Option<IUnknown> {
    if cached.is_some() {
        return cached;
    }
    let raw = device.as_raw();
    let context: IUnknown = unsafe { ID3D11Device::from_raw_borrowed(&raw)?.GetImmediateContext() }
        .ok()?
        .into();
    cache_context(&mut lock().retained, device, &context);
    Some(context)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_observer_acquire_device(
    device_out: *mut Handle,
    context_out: *mut Handle,
) -> i32 {
    if device_out.is_null() && context_out.is_null() {
        return INVALID;
    }
    let (device, context) = {
        let state = lock();
        let Some(device) = state.retained.device.clone() else {
            return NOT_READY;
        };
        (device, state.retained.context.clone())
    };
    let context = if context_out.is_null() {
        None
    } else {
        resolve_context(&device, context)
    };
    if !device_out.is_null() {
        unsafe { device_out.write(device.into_raw()) };
    }
    if !context_out.is_null() {
        unsafe { context_out.write(context.map_or(ptr::null_mut(), Interface::into_raw)) };
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_observer_get_status(out: *mut rsf_observer_status) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID;
    }
    let state = lock();
    let size = present_size();
    let status = rsf_observer_status {
        struct_size: unsafe { out.cast::<u32>().read() },
        installed: installed() as u32,
        have_device: state.retained.device.is_some().into(),
        frames_presented: frames(),
        textures_matched: state
            .retained
            .selected()
            .map_or(0, |entry| entry.textures.len() as u32),
        textures_created: state.textures_created,
        present_width: (size >> 32) as u32,
        present_height: size as u32,
        constant_buffers_matched: state.constant_matches,
        distinct_buffer_sizes: state.sizes.len() as u32,
        dumps_completed: state.dumps_completed,
        textures_written: state.dump_result.textures,
        constant_bytes_written: state.dump_result.constant_bytes,
    };
    unsafe { out.write(status) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_observer_request_dump(prefix: *const c_char, view: u32) -> i32 {
    if prefix.is_null() {
        return INVALID;
    }
    let request = Request {
        prefix: unsafe { CStr::from_ptr(prefix) }.to_owned(),
        view,
    };
    let mut state = lock();
    if installed() == 0 {
        return NOT_READY;
    }
    state.request = Some(request);
    REQUESTED.store(true, Ordering::Release);
    OK
}

pub(super) fn dump_presenter(presenter: &IUnknown) {
    if !REQUESTED.load(Ordering::Acquire) {
        return;
    }
    let Some(presenter) = (unsafe { device_identity(presenter.as_raw()) }) else {
        return;
    };
    let (request, settings, device, context, textures, buffers) = {
        let mut state = lock();
        if !PRESENTING_DEVICE.load(Ordering::Relaxed)
            || !state
                .retained
                .identity
                .as_ref()
                .is_some_and(|selected| same_identity(selected, &presenter))
        {
            return;
        }
        let Some(device) = state.retained.device.clone() else {
            return;
        };
        let Some(request) = state.request.take() else {
            return;
        };
        REQUESTED.store(false, Ordering::Release);
        (
            request,
            state.dump_settings,
            device,
            state.retained.context.clone(),
            state
                .retained
                .selected()
                .map_or_else(Vec::new, |entry| entry.textures.clone()),
            state
                .retained
                .selected()
                .map_or_else(Vec::new, |entry| entry.constants.clone()),
        )
    };
    let context = resolve_context(&device, context);
    let result = dump::write(
        &settings,
        &request,
        &device,
        context.as_ref(),
        &textures,
        &buffers,
    );
    let mut state = lock();
    state.dumps_completed = state.dumps_completed.wrapping_add(1);
    state.dump_result = result;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_observer_write_buffer_sizes(path: *const c_char) -> i32 {
    if path.is_null() {
        return INVALID;
    }
    let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
        return INVALID;
    };
    let sizes = lock().sizes.clone();
    let mut contents = String::from("bytes,created\n");
    for (bytes, created) in sizes {
        let _ = writeln!(contents, "{bytes},{created}");
    }
    if fs::write(path, contents).is_ok() {
        OK
    } else {
        INVALID
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{fake, raw};

    #[test]
    fn context_from_a_replaced_device_is_not_cached() {
        let (first, second, context) = (fake(), fake(), fake());
        let first_ref = unsafe { com_ref::retain(raw(&first)) };
        let context_ref = unsafe { com_ref::retain(raw(&context)) };
        let mut retained = Retained::new();
        retained.device = Some(unsafe { com_ref::retain(raw(&second)) });
        cache_context(&mut retained, &first_ref, &context_ref);
        assert!(retained.context.is_none());
        let second_ref = retained.device.as_ref().unwrap().clone();
        cache_context(&mut retained, &second_ref, &context_ref);
        assert_eq!(retained.context.as_ref().unwrap().as_raw(), raw(&context));
    }
}
