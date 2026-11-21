// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Write;
use std::fs;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

type Handle = *mut c_void;
type Immediate = unsafe extern "C" fn(Handle) -> Handle;
type WriteDump = unsafe extern "C" fn(&Dump, &mut DumpResult);
const OK: i32 = 0;
const INVALID: i32 = -1;
const NOT_READY: i32 = -6;
const CONSTANT_BUFFER: u32 = 0x4;
const RENDER_TARGET: u32 = 0x20;
const CONSTANT_LIMIT: usize = 16;
const HISTOGRAM_LIMIT: usize = 128;

#[repr(C)]
#[derive(Clone, Copy)]
struct Filter {
    format: u32,
    minimum_width: u32,
    capacity: u32,
    constant_min: u32,
    constant_max: u32,
}

#[repr(C)]
struct CaptureStatus {
    have_device: u32,
    textures_matched: u32,
    textures_created: u32,
    constant_matches: u32,
    distinct_buffer_sizes: u32,
    dumps_completed: u32,
    textures_written: u32,
    constant_bytes_written: u32,
}

#[repr(C)]
struct DumpBuffer {
    bytes: u32,
    buffer: Handle,
}

#[repr(C)]
struct Dump {
    device: Handle,
    context: Handle,
    prefix: *const c_char,
    textures: *const Handle,
    texture_count: usize,
    buffers: *const DumpBuffer,
    buffer_count: usize,
    view: u32,
}

#[repr(C)]
#[derive(Default)]
struct DumpResult {
    textures: u32,
    constant_bytes: u32,
}

const _: () = {
    assert!(size_of::<Filter>() == 20);
    assert!(size_of::<CaptureStatus>() == 32);
    assert!(size_of::<DumpBuffer>() == 16);
    assert!(size_of::<Dump>() == 64);
    assert!(size_of::<DumpResult>() == 8);
};

struct Request {
    prefix: CString,
    view: u32,
}

#[derive(Clone)]
struct Constant {
    bytes: u32,
    buffer: ComRef,
}

struct Retained {
    textures: Vec<ComRef>,
    constants: Vec<Constant>,
    context: Option<ComRef>,
    device: Option<ComRef>,
}

impl Retained {
    const fn new() -> Self {
        Self {
            textures: Vec::new(),
            constants: Vec::new(),
            context: None,
            device: None,
        }
    }
}

struct Capture {
    filter: Filter,
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
    CAPTURE.lock().unwrap_or_else(PoisonError::into_inner)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_capture_configure(filter: &Filter) {
    lock().filter = Filter {
        capacity: if filter.capacity == 0 {
            16
        } else {
            filter.capacity
        },
        ..*filter
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_texture(
    device: Handle,
    texture: Handle,
    width: u32,
    format: u32,
    bindings: u32,
) {
    let mut state = lock();
    state.textures_created = state.textures_created.wrapping_add(1);
    let filter = state.filter;
    if bindings & RENDER_TARGET == 0
        || (filter.format != 0 && filter.format != format)
        || width < filter.minimum_width
        || state.retained.textures.len() >= filter.capacity as usize
    {
        return;
    }
    state
        .retained
        .textures
        .push(unsafe { ComRef::retain(texture) });
    state
        .retained
        .device
        .get_or_insert_with(|| unsafe { ComRef::retain(device) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_buffer(
    device: Handle,
    buffer: Handle,
    bytes: u32,
    bindings: u32,
) {
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
    let mut previous = None;
    if bytes >= filter.constant_min && bytes <= filter.constant_max {
        let buffers = &mut state.retained.constants;
        if let Some(entry) = buffers.iter_mut().find(|entry| entry.bytes == bytes) {
            previous = Some(std::mem::replace(&mut entry.buffer, unsafe {
                ComRef::retain(buffer)
            }));
            state.constant_matches = state.constant_matches.wrapping_add(1);
        } else if buffers.len() < CONSTANT_LIMIT {
            buffers.push(Constant {
                bytes,
                buffer: unsafe { ComRef::retain(buffer) },
            });
            state.constant_matches = state.constant_matches.wrapping_add(1);
        }
    }
    state
        .retained
        .device
        .get_or_insert_with(|| unsafe { ComRef::retain(device) });
    drop(state);
    drop(previous);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_needs_presenting_device() -> i32 {
    i32::from(!PRESENTING_DEVICE.load(Ordering::Acquire))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_adopt_device(incoming: Handle) -> Handle {
    let Some(incoming) = (unsafe { ComRef::from_owned(incoming) }) else {
        return ptr::null_mut();
    };
    let (old, context) = {
        let mut state = lock();
        if PRESENTING_DEVICE.swap(true, Ordering::AcqRel)
            || state
                .retained
                .device
                .as_ref()
                .is_some_and(|device| device.as_ptr() == incoming.as_ptr())
        {
            return ptr::null_mut();
        }
        (
            state.retained.device.replace(incoming),
            state.retained.context.take(),
        )
    };
    drop(context);
    old.map_or(ptr::null_mut(), ComRef::into_raw)
}

pub(super) fn clear() {
    let retained = {
        let mut state = lock();
        state.request = None;
        REQUESTED.store(false, Ordering::Release);
        PRESENTING_DEVICE.store(false, Ordering::Release);
        std::mem::replace(&mut state.retained, Retained::new())
    };
    drop(retained);
}

fn cache_context(retained: &mut Retained, device: &ComRef, context: &ComRef) {
    if retained.context.is_none()
        && retained
            .device
            .as_ref()
            .is_some_and(|selected| selected.as_ptr() == device.as_ptr())
    {
        retained.context = Some(context.clone());
    }
}

fn resolve_context(
    device: &ComRef,
    cached: Option<ComRef>,
    immediate: Immediate,
) -> Option<ComRef> {
    if cached.is_some() {
        return cached;
    }
    let context = unsafe { ComRef::from_owned(immediate(device.as_ptr())) }?;
    cache_context(&mut lock().retained, device, &context);
    Some(context)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_acquire_device(
    device_out: *mut Handle,
    context_out: *mut Handle,
    immediate: Immediate,
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
        resolve_context(&device, context, immediate)
    };
    if !device_out.is_null() {
        unsafe { device_out.write(device.into_raw()) };
    }
    if !context_out.is_null() {
        unsafe { context_out.write(context.map_or(ptr::null_mut(), ComRef::into_raw)) };
    }
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_capture_status(out: &mut CaptureStatus) {
    let state = lock();
    *out = CaptureStatus {
        have_device: state.retained.device.is_some().into(),
        textures_matched: state.retained.textures.len() as u32,
        textures_created: state.textures_created,
        constant_matches: state.constant_matches,
        distinct_buffer_sizes: state.sizes.len() as u32,
        dumps_completed: state.dumps_completed,
        textures_written: state.dump_result.textures,
        constant_bytes_written: state.dump_result.constant_bytes,
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_request_dump(prefix: *const c_char, view: u32) {
    let request = Request {
        prefix: unsafe { CStr::from_ptr(prefix) }.to_owned(),
        view,
    };
    let mut state = lock();
    state.request = Some(request);
    REQUESTED.store(true, Ordering::Release);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_dump(immediate: Immediate, write: WriteDump) {
    if !REQUESTED.load(Ordering::Acquire) {
        return;
    }
    let (request, device, context, textures, buffers) = {
        let mut state = lock();
        let Some(device) = state.retained.device.clone() else {
            return;
        };
        let Some(request) = state.request.take() else {
            return;
        };
        REQUESTED.store(false, Ordering::Release);
        (
            request,
            device,
            state.retained.context.clone(),
            state.retained.textures.clone(),
            state.retained.constants.clone(),
        )
    };
    let context = resolve_context(&device, context, immediate);
    let texture_views: Vec<_> = textures.iter().map(ComRef::as_ptr).collect();
    let buffer_views: Vec<_> = buffers
        .iter()
        .map(|entry| DumpBuffer {
            bytes: entry.bytes,
            buffer: entry.buffer.as_ptr(),
        })
        .collect();
    let dump = Dump {
        device: device.as_ptr(),
        context: context.as_ref().map_or(ptr::null_mut(), ComRef::as_ptr),
        prefix: request.prefix.as_ptr(),
        textures: texture_views.as_ptr(),
        texture_count: texture_views.len(),
        buffers: buffer_views.as_ptr(),
        buffer_count: buffer_views.len(),
        view: request.view,
    };
    let mut result = DumpResult::default();
    unsafe { write(&dump, &mut result) };
    let mut state = lock();
    state.dumps_completed = state.dumps_completed.wrapping_add(1);
    state.dump_result = result;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_write_buffer_sizes(path: *const c_char) -> i32 {
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
        let first_ref = unsafe { ComRef::retain(raw(&first)) };
        let context_ref = unsafe { ComRef::retain(raw(&context)) };
        let mut retained = Retained::new();
        retained.device = Some(unsafe { ComRef::retain(raw(&second)) });
        cache_context(&mut retained, &first_ref, &context_ref);
        assert!(retained.context.is_none());
        let second_ref = retained.device.as_ref().unwrap().clone();
        cache_context(&mut retained, &second_ref, &context_ref);
        assert_eq!(retained.context.as_ref().unwrap().as_ptr(), raw(&context));
    }
}
