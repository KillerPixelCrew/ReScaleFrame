// SPDX-License-Identifier: MIT
//! Loads the managed helper into Unity's scripting domain through the Mono embedding API and
//! calls its Bootstrap entry points. Every call runs on a fresh native thread, so the Mono thread
//! attachment is ours to detach; a caller's Unity/Mono thread and its current domain are never
//! touched.
use crate::plugin_abi::rsf_unity_native_api;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::{self, null_mut};
use std::sync::{Mutex, PoisonError};
use std::thread;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::core::w;

type Handle = *mut c_void;
type DomainVisit = unsafe extern "C" fn(domain: Handle, user: *mut c_void);

const UNAVAILABLE: &CStr = c"Unity Mono scripting domain is not available.";
const MISSING_EXPORTS: &CStr = c"Required Mono embedding exports are missing.";
const HARMONY_REFUSED: &CStr = c"Harmony runtime could not load into Unity's script domain.";
const ENTRY_MISSING: &CStr = c"Managed Unity bootstrap entry points are missing.";
const MANAGED_REFUSED: &CStr = c"Managed bootstrap refused; see the managed diagnostics.";
const LOADED: &CStr =
    c"Mono helper loaded; main-thread adapter activation and GPU validation pending.";
const WORKER_FAILED: &CStr = c"Mono bootstrap worker could not start.";
const ALREADY_LOADED: &CStr = c"Managed Unity helper is already loaded.";

const CHILD_DOMAIN: &[u8] = b"Unity Child Domain";
const ROOT_DOMAIN: &[u8] = b"Unity Root Domain";

/// The Mono embedding exports the bootstrap calls, typed as the Mono C API declares them.
#[derive(Clone, Copy)]
struct Mono {
    root: unsafe extern "C" fn() -> Handle,
    domains: unsafe extern "C" fn(visit: DomainVisit, user: *mut c_void),
    domain_name: unsafe extern "C" fn(domain: Handle) -> *const c_char,
    attach: unsafe extern "C" fn(domain: Handle) -> Handle,
    detach: unsafe extern "C" fn(thread: Handle),
    set_domain: unsafe extern "C" fn(domain: Handle, force: c_int) -> c_int,
    open: unsafe extern "C" fn(domain: Handle, path: *const c_char) -> Handle,
    image: unsafe extern "C" fn(assembly: Handle) -> Handle,
    find_class:
        unsafe extern "C" fn(image: Handle, space: *const c_char, name: *const c_char) -> Handle,
    method: unsafe extern "C" fn(class: Handle, name: *const c_char, arguments: c_int) -> Handle,
    invoke: unsafe extern "C" fn(
        method: Handle,
        object: *mut c_void,
        arguments: *mut *mut c_void,
        error: *mut Handle,
    ) -> Handle,
    unbox: unsafe extern "C" fn(object: Handle) -> *mut c_void,
}

impl Mono {
    /// Resolves every export from the Mono module Unity already loaded, or none.
    unsafe fn resolve(module: HMODULE) -> Option<Self> {
        unsafe {
            Some(Self {
                root: export(module, c"mono_get_root_domain")?,
                domains: export(module, c"mono_domain_foreach")?,
                domain_name: export(module, c"mono_domain_get_friendly_name")?,
                attach: export(module, c"mono_thread_attach")?,
                detach: export(module, c"mono_thread_detach")?,
                set_domain: export(module, c"mono_domain_set")?,
                open: export(module, c"mono_domain_assembly_open")?,
                image: export(module, c"mono_assembly_get_image")?,
                find_class: export(module, c"mono_class_from_name")?,
                method: export(module, c"mono_class_get_method_from_name")?,
                invoke: export(module, c"mono_runtime_invoke")?,
                unbox: export(module, c"mono_object_unbox")?,
            })
        }
    }

    /// Invokes a static managed method returning a boxed int and reports whether it returned 0.
    unsafe fn invoke_ok(&self, method: Handle, arguments: *mut *mut c_void) -> bool {
        let mut error = null_mut();
        let result = unsafe { (self.invoke)(method, null_mut(), arguments, &mut error) };
        if !error.is_null() || result.is_null() {
            return false;
        }
        let value = unsafe { (self.unbox)(result) };
        !value.is_null() && unsafe { value.cast::<i32>().read() } == 0
    }
}

/// F must be the named export's C signature.
unsafe fn export<F: Copy>(module: HMODULE, name: &CStr) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let function = unsafe { GetProcAddress(module, name.as_ptr().cast()) }?;
    Some(unsafe { std::mem::transmute_copy(&function) })
}

/// The helper stays loaded after a successful start; this retains what Stop needs.
#[derive(Clone, Copy)]
struct Loaded {
    mono: Mono,
    domain: Handle,
    stop: Handle,
}

// The Mono handles are process-global runtime objects; each use attaches its own thread.
unsafe impl Send for Loaded {}

static LOADED_HELPER: Mutex<Option<Loaded>> = Mutex::new(None);

/// Unity 6000.3 runs scripts in its named root domain; older players use a child domain. Only
/// domains Unity names are eligible, and a child domain is preferred wherever it appears.
pub(crate) fn selects_domain(name: &[u8], already_selected: bool) -> bool {
    name == CHILD_DOMAIN || (!already_selected && name == ROOT_DOMAIN)
}

/// 0Harmony.dll lives beside the helper assembly.
pub(crate) fn harmony_path(helper: &str) -> String {
    let directory = helper
        .rfind(['/', '\\'])
        .map_or(0, |separator| separator + 1);
    format!("{}0Harmony.dll", &helper[..directory])
}

struct Selection {
    mono: Mono,
    log: Option<unsafe extern "C" fn(message: *const c_char)>,
    domain: Handle,
}

unsafe extern "C" fn visit_domain(domain: Handle, user: *mut c_void) {
    // mono_domain_foreach passes back the Selection lent for the duration of the call.
    let selection = unsafe { &mut *user.cast::<Selection>() };
    let name = unsafe { (selection.mono.domain_name)(domain) };
    if name.is_null() {
        return;
    }
    if let Some(log) = selection.log {
        unsafe { log(name) };
    }
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    if selects_domain(name, !selection.domain.is_null()) {
        selection.domain = domain;
    }
}

struct Attachment<'a> {
    mono: &'a Mono,
    thread: Handle,
}

impl Drop for Attachment<'_> {
    fn drop(&mut self) {
        unsafe { (self.mono.detach)(self.thread) };
    }
}

struct Api(*mut rsf_unity_native_api);

// The table lives in the lifecycle state, which holds its lock across the joined worker.
unsafe impl Send for Api {}

fn start_attached(helper: &str, api: Api) -> Result<Loaded, &'static CStr> {
    let mono_module = unsafe { GetModuleHandleW(w!("mono-2.0-bdwgc.dll")) };
    if mono_module.is_null() || unsafe { GetModuleHandleW(w!("UnityPlayer.dll")) }.is_null() {
        return Err(UNAVAILABLE);
    }
    let mono = unsafe { Mono::resolve(mono_module) }.ok_or(MISSING_EXPORTS)?;
    let root = unsafe { (mono.root)() };
    if root.is_null() {
        return Err(UNAVAILABLE);
    }
    let thread = unsafe { (mono.attach)(root) };
    if thread.is_null() {
        return Err(UNAVAILABLE);
    }
    let _attachment = Attachment {
        mono: &mono,
        thread,
    };
    let mut selection = Selection {
        mono,
        log: unsafe { (*api.0).log },
        domain: null_mut(),
    };
    unsafe { (mono.domains)(visit_domain, (&raw mut selection).cast()) };
    let domain = selection.domain;
    if domain.is_null() || unsafe { (mono.set_domain)(domain, 0) } == 0 {
        return Err(UNAVAILABLE);
    }
    let harmony = CString::new(harmony_path(helper)).map_err(|_| UNAVAILABLE)?;
    if unsafe { (mono.open)(domain, harmony.as_ptr()) }.is_null() {
        return Err(HARMONY_REFUSED);
    }
    let helper = CString::new(helper).map_err(|_| UNAVAILABLE)?;
    let assembly = unsafe { (mono.open)(domain, helper.as_ptr()) };
    let class = if assembly.is_null() {
        null_mut()
    } else {
        let image = unsafe { (mono.image)(assembly) };
        unsafe { (mono.find_class)(image, c"ReScaleFrame.Unity".as_ptr(), c"Bootstrap".as_ptr()) }
    };
    if class.is_null() {
        return Err(ENTRY_MISSING);
    }
    let start = unsafe { (mono.method)(class, c"Start".as_ptr(), 1) };
    let stop = unsafe { (mono.method)(class, c"Stop".as_ptr(), 0) };
    if start.is_null() || stop.is_null() {
        return Err(ENTRY_MISSING);
    }
    // Bootstrap.Start(IntPtr): Mono takes a pointer to each argument's value.
    let mut address = api.0.cast::<c_void>();
    let mut arguments = [(&raw mut address).cast::<c_void>()];
    if !unsafe { mono.invoke_ok(start, arguments.as_mut_ptr()) } {
        return Err(MANAGED_REFUSED);
    }
    Ok(Loaded { mono, domain, stop })
}

/// Loads Harmony and the helper at `helper` (UTF-8) into Unity's script domain and starts its
/// Bootstrap with `api`.
///
/// # Safety
/// `api` must point to the initialized bridge table until this returns. The helper copies it.
pub(crate) unsafe fn start(
    helper: &str,
    api: *mut rsf_unity_native_api,
) -> Result<&'static CStr, &'static CStr> {
    let mut loaded = LOADED_HELPER.lock().unwrap_or_else(PoisonError::into_inner);
    if loaded.is_some() {
        return Err(ALREADY_LOADED);
    }
    let api = Api(api);
    let result = thread::scope(|scope| -> Result<Loaded, &'static CStr> {
        thread::Builder::new()
            .name("rsf-unity-mono-start".into())
            .spawn_scoped(scope, move || start_attached(helper, api))
            .map_err(|_| WORKER_FAILED)?
            .join()
            .map_err(|_| WORKER_FAILED)?
    })?;
    *loaded = Some(result);
    Ok(LOADED)
}

fn stop_attached(loaded: &Loaded) -> bool {
    let mono = &loaded.mono;
    let thread = unsafe { (mono.attach)(loaded.domain) };
    if thread.is_null() {
        return false;
    }
    let _attachment = Attachment { mono, thread };
    unsafe { mono.invoke_ok(loaded.stop, ptr::null_mut()) }
}

/// Calls Bootstrap.Stop. True when no helper is loaded or the helper stopped its producers.
pub(crate) fn stop() -> bool {
    let mut loaded = LOADED_HELPER.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(helper) = *loaded else {
        return true;
    };
    let stopped = thread::scope(|scope| {
        thread::Builder::new()
            .name("rsf-unity-mono-stop".into())
            .spawn_scoped(scope, move || stop_attached(&helper))
            .ok()
            .and_then(|worker| worker.join().ok())
            .unwrap_or(false)
    });
    if stopped {
        *loaded = None;
    }
    stopped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_domain_wins_over_root() {
        assert!(selects_domain(b"Unity Root Domain", false));
        assert!(!selects_domain(b"Unity Root Domain", true));
        assert!(selects_domain(b"Unity Child Domain", true));
        assert!(!selects_domain(b"Unity Script Domain", false));
    }

    #[test]
    fn harmony_sits_beside_the_helper() {
        assert_eq!(
            harmony_path(r"C:\Game\RSF\Helper.dll"),
            r"C:\Game\RSF\0Harmony.dll"
        );
        assert_eq!(harmony_path("C:/Game/Helper.dll"), "C:/Game/0Harmony.dll");
        assert_eq!(harmony_path("Helper.dll"), "0Harmony.dll");
    }
}
