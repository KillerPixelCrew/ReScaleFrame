// SPDX-License-Identifier: MIT
use crate::{Handle, NVIDIA, ProbeResult, sys::*};
use std::ffi::c_char;
use std::ptr::NonNull;
use windows::Win32::System::Environment::GetEnvironmentVariableW;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{PCWSTR, s, w};

pub(super) struct Capture(NonNull<std::ffi::c_void>);

impl Capture {
    pub(super) fn open(prefix: *const c_char) -> ProbeResult<Option<Self>> {
        if unsafe { rsf_probe_renderdoc_available() } == 0 {
            return Ok(None);
        }
        let mut path = [0; 1024];
        let size = unsafe { GetEnvironmentVariableW(w!("RSF_PROBE_RENDERDOC"), Some(&mut path)) };
        if size == 0 {
            return Ok(None);
        }
        if size as usize >= path.len() {
            return Err("could not initialize RenderDoc".into());
        }
        // RenderDoc stays loaded through process exit to keep its interception alive.
        let library = unsafe { LoadLibraryW(PCWSTR(path.as_ptr())) }
            .map_err(|_| "could not initialize RenderDoc")?;
        let get_api = unsafe { GetProcAddress(library, s!("RENDERDOC_GetAPI")) }
            .ok_or("could not initialize RenderDoc")?;
        let api = unsafe { rsf_probe_renderdoc_connect(get_api as Handle, prefix, NVIDIA) };
        let api = NonNull::new(api).ok_or("could not initialize RenderDoc")?;
        eprintln!("RenderDoc enabled before D3D11 device creation");
        Ok(Some(Self(api)))
    }

    pub(super) fn start(&self, device: Handle) {
        unsafe { rsf_probe_renderdoc_start(self.0.as_ptr(), device) };
    }

    pub(super) fn finish(&self, device: Handle) -> bool {
        let (mut saved, mut count) = (0, 0);
        let active = unsafe {
            rsf_probe_renderdoc_finish(self.0.as_ptr(), device, &mut saved, &mut count) != 0
        };
        if active {
            eprintln!("RenderDoc capture saved={saved} count={count}");
        }
        !active || saved != 0
    }
}
