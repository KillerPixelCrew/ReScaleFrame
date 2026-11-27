// SPDX-License-Identifier: MIT
//! D01's checks on the actual scoped fixture's inner and outer receivers.
use super::Raw;
use crate::sys::{rsf_d3d11_host_dispatch_chain, rsf_d3d11_host_dispatch_present};
use rsf_game_sys::rsf_d3d11_present_dispatch;
use rsf_game_sys::*;
use std::ffi::c_ulong;
use std::ptr::{dangling_mut, from_ref};
use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709;
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_INVALID_CALL, DXGI_PRESENT_PARAMETERS, DXGI_PRESENT_TEST,
};

const INVALID_CALL: i32 = DXGI_ERROR_INVALID_CALL.0;

fn chain_request(receiver: Raw, method: u32) -> rsf_d3d11_chain_dispatch {
    rsf_d3d11_chain_dispatch {
        struct_size: size_of::<rsf_d3d11_chain_dispatch>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        receiver,
        method,
        ..Default::default()
    }
}

fn present_request(receiver: Raw) -> rsf_d3d11_present_dispatch {
    rsf_d3d11_present_dispatch {
        struct_size: size_of::<rsf_d3d11_present_dispatch>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        receiver,
        ..Default::default()
    }
}

fn fail(label: &str) -> i32 {
    eprintln!("FAIL: {label}");
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_check_unadmitted(
    user: Raw,
    inner: Raw,
    held_references: u32,
) -> i32 {
    let mut call = chain_request(inner, RSF_D3D11_DISPATCH_RELEASE);
    if unsafe { rsf_d3d11_host_dispatch_chain(user, &mut call) } != RSF_OK
        || call.handled == 0
        || held_references.checked_sub(1) != Some(unsafe { call.result.references })
    {
        return fail("unadmitted Release consumes exactly one reference");
    }
    for method in RSF_D3D11_DISPATCH_PRESENT..=RSF_D3D11_DISPATCH_RESIZE_BUFFERS1 {
        call.method = method;
        call.handled = 0;
        if unsafe { rsf_d3d11_host_dispatch_chain(user, &mut call) } != RSF_OK
            || call.handled == 0
            || unsafe { call.result.hresult } != INVALID_CALL
        {
            return fail("known unadmitted method cannot fall through");
        }
    }
    let mut present = present_request(inner);
    if unsafe { rsf_d3d11_host_dispatch_present(user, &mut present) } != RSF_OK
        || present.handled == 0
        || present.native_result != INVALID_CALL
    {
        return fail("unadmitted facade remains handled with failure");
    }
    present.flags = DXGI_PRESENT_TEST.0;
    if unsafe { rsf_d3d11_host_dispatch_present(user, &mut present) } != RSF_OK
        || present.handled == 0
        || present.native_result != INVALID_CALL
    {
        return fail("unadmitted TEST cannot fall through to native code");
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_check_outer(
    user: Raw,
    outer: Raw,
    references: *const c_ulong,
) -> i32 {
    let mut alias = present_request(outer);
    alias.handled = 1;
    alias.native_result = 123;
    if unsafe { rsf_d3d11_host_dispatch_present(user, &mut alias) } != RSF_OK
        || alias.handled != 0
        || alias.native_result != 123
    {
        return fail("canonical outer alias cannot bypass its own Present work");
    }
    alias.receiver = dangling_mut();
    alias.handled = 1;
    if unsafe { rsf_d3d11_host_dispatch_present(user, &mut alias) } != RSF_OK
        || alias.handled != 0
        || alias.native_result != 123
    {
        return fail("unknown receiver is not dereferenced with a live owner");
    }
    let mut call = chain_request(outer, RSF_D3D11_DISPATCH_RELEASE);
    call.handled = 1;
    let original_references = unsafe { references.read() };
    for method in RSF_D3D11_DISPATCH_RELEASE..=RSF_D3D11_DISPATCH_RESIZE_BUFFERS1 {
        call.method = method;
        call.result.references = 123;
        if unsafe { rsf_d3d11_host_dispatch_chain(user, &mut call) } != RSF_OK
            || call.handled != 0
            || unsafe { call.result.references } != 123
            || unsafe { references.read() } != original_references
        {
            return fail("typed dispatch preserves every outer wrapper method and reference");
        }
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_check_inner(user: Raw, inner: Raw) -> i32 {
    let dirty = DXGI_PRESENT_PARAMETERS {
        DirtyRectsCount: 1,
        ..Default::default()
    };
    let mut call = chain_request(inner, RSF_D3D11_DISPATCH_PRESENT1);
    call.args.present1 = rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_2 {
        sync_interval: 0,
        flags: 0,
        parameters: from_ref(&dirty).cast(),
    };
    let native_status = |call: &mut rsf_d3d11_chain_dispatch| {
        let status = unsafe { rsf_d3d11_host_dispatch_chain(user, call) };
        (status == RSF_OK && call.handled != 0).then_some(unsafe { call.result.hresult })
    };
    if native_status(&mut call) != Some(INVALID_CALL) {
        return fail("Present1 preserves dirty-region refusal");
    }
    call.args = Default::default();
    call.method = RSF_D3D11_DISPATCH_RESIZE_BUFFERS;
    call.args.resize_buffers = rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_4 {
        buffer_count: 99,
        ..Default::default()
    };
    if native_status(&mut call) != Some(INVALID_CALL) {
        return fail("ResizeBuffers preserves invalid-count refusal");
    }
    let node_mask = 1u32;
    call.args = Default::default();
    call.method = RSF_D3D11_DISPATCH_RESIZE_BUFFERS1;
    call.args.resize_buffers1 = rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_6 {
        node_masks: &node_mask,
        ..Default::default()
    };
    if native_status(&mut call) != Some(INVALID_CALL) {
        return fail("ResizeBuffers1 preserves native-array refusal");
    }
    call.args = Default::default();
    call.method = RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE;
    if native_status(&mut call).is_none_or(|result| result < 0) {
        return fail("SetFullscreenState uses the actual physical owner");
    }
    call.method = RSF_D3D11_DISPATCH_SET_COLOR_SPACE1;
    call.args.color_space = rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_5 {
        color_space: DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0 as u32,
    };
    if native_status(&mut call).is_none_or(|result| result < 0) {
        return fail("SetColorSpace1 uses the actual physical owner");
    }
    0
}
