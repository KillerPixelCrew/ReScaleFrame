// SPDX-License-Identifier: MIT
use rsf_test_support::Library;
use std::ffi::{CStr, CString, OsStr, c_char, c_int, c_void};
use std::mem::ManuallyDrop;
use std::process::ExitCode;
use std::ptr::null_mut;
use windows::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};

type Handle = *mut c_void;

/// F is the Mono embedding signature of the named export.
unsafe fn entry<F: Copy>(module: &Library, name: &CStr) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let address = module.symbol(name)?;
    Some(unsafe { std::mem::transmute_copy(&address) })
}

fn run(mono_path: &OsStr, framework: &CStr, assembly: &CStr) -> Option<i32> {
    // Process exit owns the hosted DLL and its runtime-generated code.
    let module = ManuallyDrop::new(
        Library::open(
            mono_path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
        .ok()?,
    );
    let module = &*module;
    unsafe {
        let search: unsafe extern "C" fn(*const c_char) =
            entry(module, c"mono_set_assemblies_path")?;
        let init: unsafe extern "C" fn(*const c_char, *const c_char) -> Handle =
            entry(module, c"mono_jit_init_version")?;
        let open: unsafe extern "C" fn(Handle, *const c_char) -> Handle =
            entry(module, c"mono_domain_assembly_open")?;
        let image: unsafe extern "C" fn(Handle) -> Handle =
            entry(module, c"mono_assembly_get_image")?;
        let class: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> Handle =
            entry(module, c"mono_class_from_name")?;
        let method: unsafe extern "C" fn(Handle, *const c_char, c_int) -> Handle =
            entry(module, c"mono_class_get_method_from_name")?;
        let invoke: unsafe extern "C" fn(Handle, Handle, *mut Handle, *mut Handle) -> Handle =
            entry(module, c"mono_runtime_invoke")?;
        let unbox: unsafe extern "C" fn(Handle) -> Handle = entry(module, c"mono_object_unbox")?;
        let cleanup: unsafe extern "C" fn(Handle) = entry(module, c"mono_jit_cleanup")?;

        search(framework.as_ptr());
        let domain = init(
            c"ReScaleFrame isolated Mono contract fixture".as_ptr(),
            c"v4.0.30319".as_ptr(),
        );
        if domain.is_null() {
            return None;
        }
        let assembly = open(domain, assembly.as_ptr());
        let mut code = 1;
        if !assembly.is_null() {
            let program = class(image(assembly), c"".as_ptr(), c"Program".as_ptr());
            if !program.is_null() {
                let run = method(program, c"Run".as_ptr(), 0);
                if !run.is_null() {
                    let mut error = null_mut();
                    let result = invoke(run, null_mut(), null_mut(), &raw mut error);
                    if !error.is_null() {
                        eprintln!("Managed Mono fixture threw an exception.");
                    } else if !result.is_null() {
                        let value = unbox(result).cast::<i32>();
                        if !value.is_null() {
                            code = value.read();
                        }
                    }
                }
            }
        }
        cleanup(domain);
        Some(code)
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [mono, framework, assembly] = args.as_slice() else {
        eprintln!("Usage: mono-contract mono-dll managed-framework-directory test-assembly");
        return ExitCode::from(2);
    };
    let (Ok(framework), Ok(assembly)) = (
        CString::new(framework.to_string_lossy().as_bytes()),
        CString::new(assembly.to_string_lossy().as_bytes()),
    ) else {
        return ExitCode::FAILURE;
    };
    std::process::exit(run(mono, &framework, &assembly).unwrap_or(1));
}
