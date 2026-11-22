// SPDX-License-Identifier: MIT
mod d3d;
mod decode;
mod fsr;
mod hooks;
mod identity;
mod json;
mod misc;
mod ngx;
mod output;
mod record;
mod session;
mod static_scan;
mod stats;
mod vendor;
mod vulkan;

use crate::sys::rsf_census_platform;
use std::sync::OnceLock;

// The C leaves of loader/census/src, which this archive cannot name directly: the thin, shim and
// plugin carriers link it without them. The table is a C static for the process lifetime.
struct Platform(rsf_census_platform);
unsafe impl Send for Platform {}
unsafe impl Sync for Platform {}
static PLATFORM: OnceLock<Platform> = OnceLock::new();

fn publish(platform: &rsf_census_platform) {
    let _ = PLATFORM.set(Platform(*platform));
}

fn platform() -> &'static rsf_census_platform {
    &PLATFORM
        .get()
        .expect("the census platform is published at install")
        .0
}
