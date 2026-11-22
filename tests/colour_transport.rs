// SPDX-License-Identifier: MIT
// Display-range colour transport on WARP, with the analytic curve and highlight recovery.
use super::*;
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_colour_transport_create(device: Handle, pass: *mut Handle) -> i32;
    fn rsf_colour_transport_encode(
        pass: Handle,
        context: Handle,
        source: Handle,
        exposure: Handle,
        width: u32,
        height: u32,
        encoded: *mut Handle,
    ) -> i32;
    fn rsf_colour_transport_decode(
        pass: Handle,
        context: Handle,
        target: Handle,
        exposure: Handle,
        width: u32,
        height: u32,
        scene: Handle,
        scene_width: u32,
        scene_height: u32,
        jitter: *const f32,
    ) -> i32;
    fn rsf_colour_transport_destroy(pass: Handle);
}

fn read_red(warp: &Warp, source: &ID3D11Texture2D) -> TestResult<Vec<f32>> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };
    if desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT {
        Ok(unsafe { warp.read::<u16>(source, &desc, 4) }?
            .chunks_exact(4)
            .map(|pixel| half_to_f32(pixel[0]))
            .collect())
    } else {
        Ok(unsafe { warp.read::<f32>(source, &desc, 4) }?
            .chunks_exact(4)
            .map(|pixel| pixel[0])
            .collect())
    }
}

fn run(warp: &Warp) -> TestResult {
    let values = [0.0, 0.001, 0.02, 0.18, 1.0, 10.0];
    let exposure_value = 5.0_f32;
    let count = values.len() as u32;
    let colour_desc = Warp::desc(
        [count, 1],
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        D3D11_BIND_SHADER_RESOURCE,
    );
    let exposure_desc = Warp::desc([1, 1], DXGI_FORMAT_R32_FLOAT, D3D11_BIND_SHADER_RESOURCE);
    let source = values.map(|value| [value, value, value, 1.0]);
    let colour = warp.initialized(&colour_desc, &source, count * 16)?;
    let exposure = warp.initialized(&exposure_desc, &[exposure_value], 4)?;
    let mut raw = ptr::null_mut();
    if unsafe { rsf_colour_transport_create(warp.device.as_raw(), &mut raw) } == 0 {
        return Err("create refused".into());
    }
    let pass = Pass {
        raw,
        destroy: rsf_colour_transport_destroy,
    };
    let mut encoded = ptr::null_mut();
    let context = warp.context.as_raw();
    if unsafe {
        rsf_colour_transport_encode(
            pass.raw,
            context,
            colour.as_raw(),
            exposure.as_raw(),
            count,
            1,
            &mut encoded,
        )
    } == 0
    {
        return Err("encode refused".into());
    }
    let encoded =
        unsafe { ID3D11Texture2D::from_raw_borrowed(&encoded) }.ok_or("missing encoded texture")?;
    let display = read_red(warp, encoded)?;
    let mut passed = true;
    for (&value, &display) in values.iter().zip(&display) {
        let scaled = value * exposure_value;
        let expected = (scaled / (scaled + 4.0)).powf(1.0 / 2.2);
        passed &= (display - expected).abs() < 1e-3 && (0.0..1.0).contains(&display);
    }

    // Identity reconstruction, with an FP32 target for decoding.
    let staged: Vec<_> = display
        .iter()
        .map(|&value| [value, value, value, 1.0])
        .collect();
    let output_bind = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    let output_desc = Warp::desc([count, 1], DXGI_FORMAT_R32G32B32A32_FLOAT, output_bind);
    let output = warp.initialized(&output_desc, &staged, count * 16)?;
    passed &= unsafe {
        rsf_colour_transport_decode(
            pass.raw,
            context,
            output.as_raw(),
            exposure.as_raw(),
            count,
            1,
            ptr::null_mut(),
            0,
            0,
            ptr::null(),
        )
    } != 0;
    let decoded = read_red(warp, &output)?;
    for ((value, display), decoded) in values.into_iter().zip(display).zip(decoded) {
        eprintln!("{value} -> {display} -> {decoded}");
        passed &= (decoded - value).abs() <= 0.02 * value + 1e-5;
    }

    let pixel_desc = Warp::desc([1, 1], DXGI_FORMAT_R32G32B32A32_FLOAT, output_bind);
    let scene_desc = Warp::desc(
        [1, 1],
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        D3D11_BIND_SHADER_RESOURCE,
    );
    let pixel = warp.initialized(&pixel_desc, &[1.0_f32; 4], 16)?;
    let scene = warp.initialized(&scene_desc, &[500.0_f32, 500.0, 500.0, 1.0], 16)?;
    passed &= unsafe {
        rsf_colour_transport_decode(
            pass.raw,
            context,
            pixel.as_raw(),
            exposure.as_raw(),
            1,
            1,
            scene.as_raw(),
            1,
            1,
            [0.0; 2].as_ptr(),
        )
    } != 0;
    let recovered = read_red(warp, &pixel)?[0];
    eprintln!("saturated -> {recovered}");
    passed &= (recovered - 500.0).abs() < 0.5;
    if !passed {
        return Err("analytic curve, round-trip or highlight recovery mismatch".into());
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_colour_transport_fixture() -> i32 {
    run_warp("colour_transport", run)
}
