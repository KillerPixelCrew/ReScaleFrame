// SPDX-License-Identifier: MIT
use super::{Device, Handle, Pass, TestResult, check, near, run_warp};
use std::ptr;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;

use crate::sampler_bias::{SamplerBias, rsf_sampler_bias_apply, rsf_sampler_bias_create};

// The Runtime keeps its cache for the process; the fixture frees its own.
unsafe extern "C" fn destroy(cache: Handle) {
    drop(unsafe { Box::from_raw(cache.cast::<SamplerBias>()) });
}

fn sampler(gpu: &Device, desc: &D3D11_SAMPLER_DESC) -> TestResult<ID3D11SamplerState> {
    let mut state = None;
    unsafe { gpu.device.CreateSamplerState(desc, Some(&mut state)) }?;
    state.ok_or_else(|| "CreateSamplerState returned null".into())
}

fn run(gpu: &Device) -> TestResult {
    let mut desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_ANISOTROPIC,
        AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_MIRROR,
        MaxAnisotropy: 8,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        MaxLOD: 12.0,
        MipLODBias: 0.25,
        ..Default::default()
    };
    let native = sampler(gpu, &desc)?;
    desc.Filter = D3D11_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT;
    desc.ComparisonFunc = D3D11_COMPARISON_LESS;
    let comparison = sampler(gpu, &desc)?;
    desc.MipLODBias = 2.0;
    let foreign = sampler(gpu, &desc)?;
    let initial = [Some(native.clone()), Some(comparison.clone())];
    let foreign_binding = [Some(foreign.clone())];
    unsafe { gpu.context.PSSetSamplers(0, Some(&initial)) };
    let get = |slot| {
        let mut state = None;
        unsafe {
            gpu.context
                .PSGetSamplers(slot, Some(std::slice::from_mut(&mut state)))
        };
        state
    };
    let raw = rsf_sampler_bias_create();
    check(!raw.is_null(), "sampler cache creation")?;
    let cache = Pass {
        raw: raw.cast(),
        destroy,
    };
    let mut baselines = [ptr::null_mut(); D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT as usize];
    baselines[..2].copy_from_slice(&[native.as_raw(), comparison.as_raw()]);
    let apply = |context: &ID3D11DeviceContext, entries: &[Handle], mask, bias| {
        let mut applied = u32::MAX;
        let ok = unsafe {
            rsf_sampler_bias_apply(
                cache.raw.cast(),
                context.as_raw(),
                entries.as_ptr(),
                mask,
                bias,
                &mut applied,
            )
        } != 0;
        (ok, applied)
    };
    let context = &gpu.context;
    check(
        apply(context, &baselines, 1, -1.5) == (true, 1),
        "selected binding",
    )?;
    let changed = get(0).ok_or("no replacement sampler")?;
    let mut actual = D3D11_SAMPLER_DESC::default();
    let mut original = D3D11_SAMPLER_DESC::default();
    unsafe {
        changed.GetDesc(&mut actual);
        native.GetDesc(&mut original);
    }
    near(actual.MipLODBias, -1.25, 1e-6, "additive bias")?;
    actual.MipLODBias = original.MipLODBias;
    check(actual == original, "all other fields preserved")?;
    check(
        get(1).as_ref() == Some(&comparison),
        "unselected comparison untouched",
    )?;
    check(
        apply(context, &baselines, 1, -1.5) == (true, 1) && get(0).as_ref() == Some(&changed),
        "immutable cached object reused",
    )?;
    unsafe { context.PSSetSamplers(0, Some(&foreign_binding)) };
    check(
        apply(context, &baselines, 1, -1.5) == (true, 0) && get(0).as_ref() == Some(&foreign),
        "foreign renderer binding preserved",
    )?;
    unsafe { context.PSSetSamplers(0, Some(&initial[..1])) };
    check(
        apply(context, &baselines, 1, -1.5) == (true, 1),
        "reacquire native binding",
    )?;
    for i in 1..=80 {
        check(
            apply(context, &baselines, 1, -0.01 * i as f32) == (true, 1),
            "cache eviction while bound",
        )?;
    }
    let prior = get(0);
    unsafe { context.PSSetSamplers(1, Some(&foreign_binding)) };
    baselines[1] = foreign.as_raw();
    check(
        apply(context, &baselines, 3, 15.0) == (false, 0)
            && get(0) == prior
            && get(1).as_ref() == Some(&foreign),
        "invalid later slot preserves bindings",
    )?;
    check(
        apply(context, &baselines, 0, 15.0) == (true, 0) && get(0).as_ref() == Some(&native),
        "drain restores exact baseline after eviction",
    )?;
    check(
        apply(context, &baselines, 0x10000, 15.0) == (false, 0) && get(0).as_ref() == Some(&native),
        "invalid mask leaves bindings unchanged",
    )?;
    let mut deferred = None;
    unsafe { gpu.device.CreateDeferredContext(0, Some(&mut deferred)) }?;
    let deferred = deferred.ok_or("no deferred context")?;
    check(
        apply(&deferred, &baselines, 0, 15.0) == (false, 0),
        "deferred context refused",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sampler_bias_fixture() -> i32 {
    run_warp("Sampler bias", run)
}
