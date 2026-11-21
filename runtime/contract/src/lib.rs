// SPDX-License-Identifier: MIT
//! Vendor-neutral capability negotiation over the existing C contract.

use std::ffi::c_char;
use std::slice;

const REGISTRY_ABI: u32 = 1;
const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const VENDOR_NONE: u32 = 0;
const D3D11: u32 = 1;
const D3D12: u32 = 2;
const ROUTE_NATIVE_D3D11: u32 = 1;
const ROUTE_BRIDGE_D3D12: u32 = 2;
const UI_NONE: u32 = 1;
const UI_LAYER: u32 = 2;
const UI_HUDLESS: u32 = 4;
const UI_HUDLESS_AND_LAYER: u32 = 8;
const REQUESTED_UNAVAILABLE: u32 = 0x1;
const FG_NEEDS_BRIDGE: u32 = 0x2;
const SR_FG_SESSION_CONFLICT: u32 = 0x4;
const SR_API_UNAVAILABLE: u32 = 0x8;
const UI_MODE_UNSUPPORTED: u32 = 0x10;
const FG_MULTIPLIER_CAPPED: u32 = 0x20;
const NEEDS_RESTART: u32 = 0x40;
const NO_FG_AVAILABLE: u32 = 0x80;

#[repr(C)]
#[derive(Clone, Copy)]
struct Request {
    struct_size: u32,
    abi_version: u32,
    sr_vendor: u32,
    fg_vendor: u32,
    want_sr: u32,
    want_fg: u32,
    fg_multiplier: u32,
    ui_mode: u32,
    latency: u32,
    game_api: u32,
    bridge_available: u32,
}

#[repr(C)]
struct Caps {
    struct_size: u32,
    vendor: u32,
    name: *const c_char,
    available: u32,
    sr_apis: u32,
    fg_apis: u32,
    reconstructs_camera_motion: u32,
    needs_exposure: u32,
    max_generated_frames: u32,
    ui_modes: u32,
    supported_lifetimes: u32,
    fg_owns_swapchain: u32,
    sr_fg_share_session: u32,
    latency_modes: u32,
    fg_requires_latency_markers: u32,
    supports_dynamic_fg: u32,
    refusal_utf8: *const c_char,
}

#[repr(C)]
#[derive(Default)]
struct Choice {
    struct_size: u32,
    sr_vendor: u32,
    fg_vendor: u32,
    sr_route: u32,
    ui_mode: u32,
    latency: u32,
    generated_frames: u32,
    restart_required: u32,
    reasons: u32,
}

const _: () = {
    assert!(size_of::<Request>() == 44 && std::mem::offset_of!(Request, abi_version) == 4);
    assert!(size_of::<Caps>() == 80 && std::mem::offset_of!(Caps, refusal_utf8) == 72);
    assert!(size_of::<Choice>() == 36 && std::mem::offset_of!(Choice, reasons) == 32);
};

fn available(caps: &[Caps], vendor: u32) -> Option<&Caps> {
    if vendor == VENDOR_NONE {
        return None;
    }
    caps.iter()
        .find(|cap| cap.vendor == vendor && cap.available != 0)
}

fn ui_mode(wanted: u32, supported: u32) -> u32 {
    if wanted & supported != 0 {
        return wanted & supported;
    }
    [UI_HUDLESS_AND_LAYER, UI_LAYER, UI_HUDLESS, UI_NONE]
        .into_iter()
        .find(|mode| mode & supported != 0)
        .unwrap_or(UI_NONE)
}

fn can_generate(cap: &Caps) -> bool {
    cap.fg_apis != 0 && cap.max_generated_frames != 0
}

fn choose_fg<'a>(request: &Request, caps: &'a [Caps], choice: &mut Choice) -> Option<&'a Caps> {
    if request.want_fg == 0 {
        return None;
    }
    let preferred = available(caps, request.fg_vendor);
    if preferred.is_none() && request.fg_vendor != VENDOR_NONE {
        choice.reasons |= REQUESTED_UNAVAILABLE;
    }
    let selected = preferred.or_else(|| {
        caps.iter()
            .find(|cap| cap.available != 0 && can_generate(cap))
    });
    let Some(fg) = selected.filter(|cap| can_generate(cap)) else {
        choice.reasons |= NO_FG_AVAILABLE;
        return None;
    };
    if request.bridge_available == 0 && fg.fg_apis & D3D11 == 0 {
        choice.reasons |= FG_NEEDS_BRIDGE | NO_FG_AVAILABLE;
        return None;
    }

    choice.fg_vendor = fg.vendor;
    choice.ui_mode = ui_mode(request.ui_mode, fg.ui_modes);
    if request.ui_mode != 0 && choice.ui_mode != request.ui_mode {
        choice.reasons |= UI_MODE_UNSUPPORTED;
    }
    if fg.fg_apis & request.game_api == 0 {
        choice.reasons |= FG_NEEDS_BRIDGE;
    }
    let frames = request.fg_multiplier.saturating_sub(1).max(1);
    choice.generated_frames = frames.min(fg.max_generated_frames);
    if choice.generated_frames < frames {
        choice.reasons |= FG_MULTIPLIER_CAPPED;
    }
    if fg.latency_modes & request.latency != 0 {
        choice.latency = request.latency;
    }
    Some(fg)
}

fn choose_sr(request: &Request, caps: &[Caps], fg: Option<&Caps>, choice: &mut Choice) {
    if request.want_sr == 0 {
        return;
    }
    let mut preferred = available(caps, request.sr_vendor);
    if preferred.is_none() && request.sr_vendor != VENDOR_NONE {
        choice.reasons |= REQUESTED_UNAVAILABLE;
    }
    let shared = fg.filter(|cap| cap.sr_fg_share_session != 0);
    if let Some(partner) = shared
        .and_then(|fg| available(caps, fg.vendor))
        .filter(|cap| cap.sr_apis != 0)
    {
        if preferred.is_some_and(|sr| sr.vendor != partner.vendor) {
            choice.reasons |= SR_FG_SESSION_CONFLICT;
        }
        preferred = Some(partner);
    }
    let Some(sr) = preferred.or_else(|| {
        caps.iter()
            .find(|cap| cap.available != 0 && cap.sr_apis != 0)
    }) else {
        return;
    };
    let native = sr.sr_apis & request.game_api != 0;
    if native && shared.is_none() {
        choice.sr_vendor = sr.vendor;
        choice.sr_route = ROUTE_NATIVE_D3D11;
    } else if request.bridge_available != 0 && sr.sr_apis & D3D12 != 0 {
        choice.sr_vendor = sr.vendor;
        choice.sr_route = ROUTE_BRIDGE_D3D12;
        choice.reasons |= if native {
            SR_FG_SESSION_CONFLICT
        } else {
            SR_API_UNAVAILABLE
        };
    } else {
        choice.reasons |= SR_API_UNAVAILABLE;
    }
}

/// The C caller lends readable size prefixes. Admitted records must be fully
/// readable or writable as appropriate, and a non-empty capability array must be valid.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_contract_negotiate(
    request: *const Request,
    caps: *const Caps,
    caps_count: u32,
    choice: *mut Choice,
) -> i32 {
    if request.is_null() || choice.is_null() {
        return INVALID_ARGUMENT;
    }
    let request_size = unsafe { request.cast::<u32>().read() };
    if (request_size as usize) < size_of::<Request>() {
        return INVALID_ARGUMENT;
    }
    let choice_size = unsafe { choice.cast::<u32>().read() };
    if (choice_size as usize) < size_of::<Choice>() {
        return INVALID_ARGUMENT;
    }
    let version = unsafe { request.cast::<u32>().add(1).read() };
    if version != REGISTRY_ABI {
        return ABI_MISMATCH;
    }
    let count = caps_count as usize;
    let caps = if count == 0 {
        &[]
    } else if caps.is_null() || count > isize::MAX as usize / size_of::<Caps>() {
        return INVALID_ARGUMENT;
    } else {
        unsafe { slice::from_raw_parts(caps, count) }
    };
    let request = unsafe { request.read() };

    let mut result = Choice {
        struct_size: choice_size,
        ui_mode: UI_NONE,
        ..Choice::default()
    };
    let fg = choose_fg(&request, caps, &mut result);
    choose_sr(&request, caps, fg, &mut result);
    if result.sr_route == ROUTE_BRIDGE_D3D12 || result.fg_vendor != VENDOR_NONE {
        result.restart_required = 1;
        result.reasons |= NEEDS_RESTART;
    }
    unsafe { choice.write(result) };
    OK
}
