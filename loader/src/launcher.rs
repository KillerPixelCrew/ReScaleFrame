// SPDX-License-Identifier: MIT
use std::ffi::{CStr, c_char};
use std::io::{self, Write};

const VERSION: &str = concat!("ReScaleFrame ", env!("CARGO_PKG_VERSION"), "\n");
const HELP: &str = concat!(
    "ReScaleFrame ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "A framework for adding upscaling and frame generation to games.\n\n",
    "Initial scaffold. Game launching and injection are not implemented.\n",
    "Options: --help, --version\n",
);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_launcher_main(argc: i32, argv: *const *const c_char) -> i32 {
    let option = (argc == 2).then(|| unsafe { CStr::from_ptr(*argv.add(1)) });
    match option.map(CStr::to_bytes) {
        Some(b"--version") => {
            let _ = io::stdout().write_all(VERSION.as_bytes());
        }
        Some(b"--help") | None if argc <= 2 => {
            let _ = io::stdout().write_all(HELP.as_bytes());
        }
        _ => {
            let _ = io::stderr().write_all(b"Unknown option. Use --help.\n");
            return 2;
        }
    }
    0
}
