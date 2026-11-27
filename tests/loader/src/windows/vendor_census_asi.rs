// SPDX-License-Identifier: MIT
use crate::sys::{RSF_CENSUS_ABI_VERSION, rsf_census_options};
use std::ptr;
use std::sync::atomic::{AtomicI32, Ordering};
use windows_sys::Win32::Foundation::MAX_PATH;
use windows_sys::Win32::System::Environment::GetEnvironmentVariableA;

static INSTALL_RESULT: AtomicI32 = AtomicI32::new(-100);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_attach(
    install: unsafe extern "C" fn(*const rsf_census_options) -> i32,
) {
    let mut directory = [0u8; MAX_PATH as usize];
    let length = unsafe {
        GetEnvironmentVariableA(
            c"RSF_CENSUS_OUT".as_ptr().cast(),
            directory.as_mut_ptr(),
            MAX_PATH,
        )
    };
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: if length > 0 && length < MAX_PATH {
            directory.as_ptr().cast()
        } else {
            ptr::null()
        },
        full_record_cap: 8,
        summary_period_ms: 50,
        ..Default::default()
    };
    INSTALL_RESULT.store(unsafe { install(&options) }, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn asi_install_result() -> i32 {
    INSTALL_RESULT.load(Ordering::Acquire)
}
