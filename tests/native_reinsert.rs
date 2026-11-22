// SPDX-License-Identifier: MIT
use super::{Device, Handle, TestResult, check, near};
use std::ptr;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_native_reinsert(
        context: Handle,
        source: Handle,
        destination: Handle,
        readable: Handle,
        rect: *const [i32; 4],
    ) -> bool;
    fn rsf_native_reinsert_release();
}

struct Retire;
impl Drop for Retire {
    fn drop(&mut self) {
        unsafe { rsf_native_reinsert_release() };
    }
}

fn unsigned_float(bits: u32, mantissa: u32) -> f32 {
    let exponent = bits >> mantissa;
    let fraction = (bits & ((1 << mantissa) - 1)) as f32 / (1 << mantissa) as f32;
    if exponent == 0 {
        fraction * 2.0f32.powi(-14)
    } else {
        (1.0 + fraction) * 2.0f32.powi(exponent as i32 - 15)
    }
}

fn run(gpu: &Device) -> TestResult {
    let _retire = Retire;
    let pixels = [
        [4.0f32, 2.0, 0.5, 0.25],
        [0.25, 16.0, 1.0, 0.5],
        [8.0, 0.5, 32.0, 0.75],
        [1.0, 4.0, 2.0, 1.0],
    ];
    let source_desc = Device::desc(
        [2, 2],
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS,
    );
    let source = gpu.initialized(&source_desc, &pixels, 32)?;
    let empty_desc = |format| Device::desc([5, 4], format, D3D11_BIND_FLAG(0));
    let target_desc = empty_desc(DXGI_FORMAT_R11G11B10_FLOAT);
    let mirror_desc = empty_desc(DXGI_FORMAT_R16G16B16A16_FLOAT);
    let direct_desc = empty_desc(DXGI_FORMAT_R32G32B32A32_FLOAT);
    let target = gpu.initialized(&target_desc, &[0u32; 20], 20)?;
    let mirror = gpu.initialized(&mirror_desc, &[0u16; 80], 40)?;
    let direct = gpu.initialized(&direct_desc, &[0u32; 80], 80)?;
    let mut poison = [0u32; 20];
    poison[0] = 0x7f7f7f7f;
    let unsupported = gpu.initialized(&empty_desc(DXGI_FORMAT_R8G8B8A8_UNORM), &poison, 20)?;

    let mut source_uav = None;
    unsafe {
        gpu.device
            .CreateUnorderedAccessView(&source, None, Some(&mut source_uav))
    }?;
    check(source_uav.is_some(), "source UAV creation")?;
    unsafe {
        gpu.context
            .CSSetUnorderedAccessViews(0, 1, Some(&source_uav), None)
    };
    let reinsert = |destination: &ID3D11Texture2D,
                    readable: Option<&ID3D11Texture2D>,
                    rect: &[i32; 4]| unsafe {
        rsf_native_reinsert(
            gpu.context.as_raw(),
            source.as_raw(),
            destination.as_raw(),
            readable.map_or(ptr::null_mut(), Interface::as_raw),
            rect,
        )
    };
    let rect = [1, 1, 3, 3];
    check(
        !reinsert(&target, Some(&unsupported), &rect),
        "unsupported mirror refused before writes",
    )?;
    check(
        unsafe { gpu.read::<u32>(&target, &target_desc, 1) }? == [0; 20],
        "target untouched after refusal",
    )?;
    check(
        !reinsert(&target, None, &[4, 3, 6, 5]),
        "out-of-bounds rect refused",
    )?;
    check(
        reinsert(&target, Some(&mirror), &rect),
        "two different HDR destinations",
    )?;
    let mut restored = None;
    unsafe {
        gpu.context
            .CSGetUnorderedAccessViews(0, Some(std::slice::from_mut(&mut restored)))
    };
    check(
        restored == source_uav,
        "source UAV restored after conversion",
    )?;
    let packed = unsafe { gpu.read::<u32>(&target, &target_desc, 1) }?;
    let halves = unsafe { gpu.read::<u16>(&mirror, &mirror_desc, 4) }?;
    for y in 0..4 {
        for x in 0..5 {
            let i = y * 5 + x;
            let value = packed[i];
            let half = &halves[i * 4..i * 4 + 4];
            if (1..3).contains(&x) && (1..3).contains(&y) {
                let expected = pixels[(y - 1) * 2 + x - 1];
                let rgb = [
                    unsigned_float(value & 0x7ff, 6),
                    unsigned_float((value >> 11) & 0x7ff, 6),
                    unsigned_float(value >> 22, 5),
                ];
                for (actual, expected) in rgb.into_iter().zip(expected) {
                    near(
                        actual,
                        expected,
                        0.001,
                        "linear HDR values and point texel mapping",
                    )?;
                }
                for (&actual, expected) in half.iter().zip(expected) {
                    near(
                        unsigned_float(u32::from(actual), 10),
                        expected,
                        0.001,
                        "RGBA16F mirror including alpha",
                    )?;
                }
            } else {
                check(value == 0 && half == [0; 4], "outside pixels preserved")?;
            }
        }
    }

    check(
        reinsert(&direct, Some(&direct), &rect),
        "same-format copy with aliased mirror",
    )?;
    let exact = unsafe { gpu.read::<u32>(&direct, &direct_desc, 4) }?;
    for (y, row) in pixels.chunks_exact(2).enumerate() {
        let start = ((y + 1) * 5 + 1) * 4;
        let expected: Vec<_> = row.iter().flatten().map(|value| value.to_bits()).collect();
        check(
            exact[start..start + 8] == expected,
            "same-format copy is bit exact",
        )?;
    }
    check(
        reinsert(&target, Some(&mirror), &rect),
        "cached conversion resources reused",
    )?;
    unsafe { rsf_native_reinsert_release() };
    check(
        reinsert(&target, None, &rect),
        "conversion recreated after retirement",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_reinsert_fixture() -> i32 {
    let Some(gpu) = Device::create(D3D_DRIVER_TYPE_WARP, Some(&[D3D_FEATURE_LEVEL_11_0])) else {
        return 77;
    };
    match run(&gpu) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Native reinsertion: {error}");
            1
        }
    }
}
