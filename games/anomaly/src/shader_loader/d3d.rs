// SPDX-License-Identifier: MIT
use crate::runtime_module::Module;
use std::ffi::CStr;
use windows_sys::Win32::Foundation::FARPROC;
use windows_sys::core::w;

struct ModuleExport {
    _module: Module,
    entry: unsafe extern "system" fn() -> isize,
}

impl ModuleExport {
    fn new(module: Module, name: &CStr) -> Option<Self> {
        let entry = module.symbol(name)?;
        Some(Self {
            _module: module,
            entry,
        })
    }
}

#[derive(Default)]
pub(super) struct Modules {
    compiler: Option<ModuleExport>,
    reflector: Option<ModuleExport>,
}

impl Modules {
    pub(super) unsafe fn load(name: *const u16, allow_system_fallback: bool) -> Self {
        if name.is_null() {
            return Self::default();
        }
        let mut compiler = unsafe { Module::loaded(name) };
        if compiler.is_none() && allow_system_fallback {
            compiler = unsafe { Module::system(name) };
        }
        let compiler = compiler.and_then(|module| ModuleExport::new(module, c"D3DCompile"));
        // _43 exposes an older reflection IID. Reflect with _47 for the current SDK interface.
        let reflector = unsafe { Module::system(w!("d3dcompiler_47.dll")) }
            .and_then(|module| ModuleExport::new(module, c"D3DReflect"));
        Self {
            compiler,
            reflector,
        }
    }

    pub(super) fn compile(&self) -> FARPROC {
        self.compiler.as_ref().map(|export| export.entry)
    }

    pub(super) fn reflect(&self) -> FARPROC {
        self.reflector.as_ref().map(|export| export.entry)
    }
}
