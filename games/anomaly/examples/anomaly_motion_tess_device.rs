// SPDX-License-Identifier: MIT
//! Device probe for a motion VS/PS pair interrupted by a stock tessellation chain.
#[cfg(windows)]
mod probe {
    use rsf_shader_compiler::{Compiler, Request, compile};
    use std::ffi::CStr;
    use std::ptr::{null_mut, read_unaligned};
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0,
        D3D_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    };
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };
    use windows::core::Interface;

    const SOURCE: &[u8] = include_bytes!("motion_tess_probe.hlsl");

    fn device(debug: bool) -> Option<(ID3D11Device, ID3D11DeviceContext)> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
        for vendor in [Some(0x10de), None] {
            for index in 0.. {
                let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
                    break;
                };
                let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                    continue;
                };
                if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
                    || vendor.is_some_and(|want| want != desc.VendorId)
                {
                    continue;
                }
                let flags = if debug {
                    D3D11_CREATE_DEVICE_DEBUG
                } else {
                    D3D11_CREATE_DEVICE_FLAG(0)
                };
                let created = rsf_test_support::d3d11_device_on(
                    Some(&*adapter),
                    D3D_DRIVER_TYPE_UNKNOWN,
                    flags,
                    Some(&[D3D_FEATURE_LEVEL_11_0]),
                );
                if let Ok((device, context)) = created {
                    let end = desc
                        .Description
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(desc.Description.len());
                    let description = String::from_utf16_lossy(&desc.Description[..end]);
                    println!(
                        "device: {description} (vendor {:#06x}, debug layer {debug})",
                        desc.VendorId
                    );
                    return Some((device, context));
                }
            }
        }
        None
    }

    fn bytecode(compiler: &Compiler, entry: &CStr, target: &CStr) -> Result<Vec<u8>, String> {
        let result = unsafe {
            compile(
                compiler.entry().ok_or("D3DCompile export missing")?,
                Request {
                    source: SOURCE,
                    name: c"rsf_tess_probe",
                    entry,
                    target,
                    flags: 0,
                },
                &[],
                null_mut(),
            )
        };
        if result.status >= 0
            && let Some(code) = result.code
        {
            return Ok(code);
        }
        let errors = String::from_utf8_lossy(result.errors.as_deref().unwrap_or_default());
        Err(format!(
            "compile {} ({:#010x}): {}",
            entry.to_string_lossy(),
            result.status as u32,
            errors.trim_end_matches('\0')
        ))
    }

    #[derive(Default)]
    struct Probe {
        value: [f32; 4],
        errors: u64,
        first_error: String,
    }

    impl Probe {
        fn inspect(&mut self, info: &ID3D11InfoQueue) {
            for index in 0..unsafe { info.GetNumStoredMessages() } {
                let mut size = 0;
                if unsafe { info.GetMessage(index, None, &mut size) }.is_err()
                    || size < size_of::<D3D11_MESSAGE>()
                {
                    continue;
                }
                // The SDK writes the message header and its variable strings in one allocation.
                let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
                let message = storage.as_mut_ptr().cast::<D3D11_MESSAGE>();
                if unsafe { info.GetMessage(index, Some(message), &mut size) }.is_err() {
                    continue;
                }
                let message = unsafe { &*message };
                if matches!(
                    message.Severity,
                    D3D11_MESSAGE_SEVERITY_ERROR | D3D11_MESSAGE_SEVERITY_CORRUPTION
                ) {
                    if self.errors == 0 && !message.pDescription.is_null() {
                        let text = unsafe {
                            std::slice::from_raw_parts(
                                message.pDescription.cast::<u8>(),
                                message.DescriptionByteLength,
                            )
                        };
                        self.first_error =
                            String::from_utf8_lossy(text).trim_end_matches('\0').into();
                    }
                    self.errors += 1;
                }
            }
        }

        fn defined(&self) -> bool {
            self.errors == 0 && self.value[..3] == [0.125, -0.25, 1.0]
        }
    }

    fn draw(
        compiler: &Compiler,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        tessellated: bool,
    ) -> Result<Probe, Box<dyn std::error::Error>> {
        let vs = bytecode(
            compiler,
            if tessellated {
                c"vs_main"
            } else {
                c"vs_direct"
            },
            c"vs_5_0",
        )?;
        let hs = bytecode(compiler, c"hs_main", c"hs_5_0")?;
        let ds = bytecode(compiler, c"ds_main", c"ds_5_0")?;
        let ps = bytecode(compiler, c"ps_main", c"ps_5_0")?;
        let (mut vertex, mut hull, mut domain, mut pixel) = (None, None, None, None);
        let no_linkage = None::<&ID3D11ClassLinkage>;
        unsafe {
            device.CreateVertexShader(&vs, no_linkage, Some(&mut vertex))?;
            device.CreateHullShader(&hs, no_linkage, Some(&mut hull))?;
            device.CreateDomainShader(&ds, no_linkage, Some(&mut domain))?;
            device.CreatePixelShader(&ps, no_linkage, Some(&mut pixel))?;
        }
        let (Some(vertex), Some(hull), Some(domain), Some(pixel)) = (vertex, hull, domain, pixel)
        else {
            return Err("shader creation returned an empty interface".into());
        };

        let desc = D3D11_TEXTURE2D_DESC {
            Width: 8,
            Height: 8,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        let mut target = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut target)) }?;
        let target = target.ok_or("target unavailable")?;
        let mut view = None;
        unsafe { device.CreateRenderTargetView(&target, None, Some(&mut view)) }?;
        let view = view.ok_or("target view unavailable")?;
        let mut staging = None;
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..desc
        };
        unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }?;
        let staging = staging.ok_or("staging texture unavailable")?;

        let info = device.cast::<ID3D11InfoQueue>().ok();
        let views = [Some(view)];
        let mut probe = Probe::default();
        unsafe {
            if let Some(info) = &info {
                info.ClearStoredMessages();
            }
            context.ClearState();
            context.ClearRenderTargetView(
                views[0].as_ref().ok_or("target view unavailable")?,
                &[-7.0; 4],
            );
            context.OMSetRenderTargets(Some(&views), None::<&ID3D11DepthStencilView>);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: 8.0,
                Height: 8.0,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            context.VSSetShader(&vertex, None);
            context.PSSetShader(&pixel, None);
            if tessellated {
                context.HSSetShader(&hull, None);
                context.DSSetShader(&domain, None);
                context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST);
            } else {
                context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            }
            context.Draw(3, 0);
            context.CopyResource(&staging, &target);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .is_ok()
            {
                // Texel (1,1) lies inside the probe triangle.
                if !mapped.pData.is_null() {
                    let point = mapped.pData.cast::<u8>().add(mapped.RowPitch as usize + 16);
                    probe.value = read_unaligned(point.cast());
                }
                context.Unmap(&staging, 0);
            }
            context.ClearState();
        }
        if let Some(info) = &info {
            probe.inspect(info);
        }
        Ok(probe)
    }

    pub fn run() -> i32 {
        let mut any = false;
        let mut failures = 0;
        for debug in [true, false] {
            let Some((device, context)) = device(debug) else {
                continue;
            };
            any = true;
            let Some(compiler) = Compiler::system("d3dcompiler_47.dll") else {
                eprintln!("FAIL: d3dcompiler_47.dll unavailable");
                return 1;
            };
            let (direct, tess) = match (
                draw(&compiler, &device, &context, false),
                draw(&compiler, &device, &context, true),
            ) {
                (Ok(direct), Ok(tess)) => (direct, tess),
                (Err(error), _) | (_, Err(error)) => {
                    eprintln!("FAIL: {error}");
                    failures += 1;
                    continue;
                }
            };
            for (label, probe) in [("VS -> PS", &direct), ("VS -> HS -> DS -> PS", &tess)] {
                let [x, y, validity, _] = probe.value;
                let first_error: String = probe.first_error.chars().take(220).collect();
                println!(
                    "  {label}: previous ({x}, {y}) validity {validity}; debug-layer errors {} {first_error}",
                    probe.errors
                );
            }
            if !direct.defined() {
                eprintln!("FAIL: the direct shader pair lost its defined motion interpolants");
                failures += 1;
            }
            if tess.defined() || (debug && tess.errors == 0) {
                eprintln!("FAIL: a stock hull/domain chain did not reject the motion interface");
                failures += 1;
            }
        }
        if !any {
            println!("SKIP: no D3D11 hardware device");
            return 77;
        }
        println!("{failures} failure(s)");
        i32::from(failures != 0)
    }
}

fn main() {
    #[cfg(windows)]
    std::process::exit(probe::run());
    #[cfg(not(windows))]
    std::process::exit(77);
}
