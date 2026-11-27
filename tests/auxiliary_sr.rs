// SPDX-License-Identifier: MIT
use super::*;
use crate::native_regions::rsf_native_regions_release;
use rsf_game_sys::{
    RSF_D3D11_AUXILIARY_COLOUR as COLOUR, RSF_D3D11_AUXILIARY_DEPTH_STENCIL as DEPTH,
    RSF_D3D11_AUXILIARY_RESTORE_CONTENTS as RESTORE,
};
use std::cell::{Cell, RefCell};
use windows::Win32::Graphics::{
    Direct3D::{D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_POINTLIST},
    Direct3D11::*,
    Dxgi::Common::*,
};
use windows::core::Interface;

#[path = "auxiliary_sr/regions.rs"]
mod regions;

type Texture2D = ID3D11Texture2D;
type Pass = rsf_game_render_pass;
type Surface = rsf_d3d11_auxiliary_surface;
const CLEAR: [f32; 4] = [0.0; 4];
const RECONSTRUCTED: [f32; 4] = [7.0, 3.0, 2.0, 0.5];
const FAILED: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
const OVERWRITTEN: [f32; 4] = [99.0, 98.0, 97.0, 1.0];
const RTV: D3D11_BIND_FLAG = D3D11_BIND_RENDER_TARGET;
const DSV: D3D11_BIND_FLAG = D3D11_BIND_DEPTH_STENCIL;
const SRV: D3D11_BIND_FLAG = D3D11_BIND_SHADER_RESOURCE;
const NO_BIND: D3D11_BIND_FLAG = D3D11_BIND_FLAG(0);
const PACKED: DXGI_FORMAT = DXGI_FORMAT_R11G11B10_FLOAT;

struct Device {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    failures: Cell<u32>,
}

impl Device {
    fn check(&self, value: bool, label: &str) {
        if !value {
            self.failures.set(self.failures.get() + 1);
            eprintln!("FAIL {label}");
        }
    }

    fn texture<T>(
        &self,
        format: DXGI_FORMAT,
        pixels: &[T],
        stride: u32,
        bind: D3D11_BIND_FLAG,
    ) -> Option<Texture2D> {
        self.initial_texture(
            &self.device,
            [8, 6],
            format,
            Some((pixels, stride)),
            bind,
            D3D11_USAGE_DEFAULT,
        )
    }

    fn initial_texture<T>(
        &self,
        device: &ID3D11Device,
        [width, height]: [u32; 2],
        format: DXGI_FORMAT,
        pixels: Option<(&[T], u32)>,
        bind: D3D11_BIND_FLAG,
        usage: D3D11_USAGE,
    ) -> Option<Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BindFlags: bind.0 as u32,
            Usage: usage,
            ..Default::default()
        };
        let data = pixels.map(|(pixels, stride)| {
            assert!(size_of_val(pixels) >= (width * height * stride) as usize);
            D3D11_SUBRESOURCE_DATA {
                pSysMem: pixels.as_ptr().cast(),
                SysMemPitch: width * stride,
                SysMemSlicePitch: 0,
            }
        });
        let mut output = None;
        let result = unsafe {
            device.CreateTexture2D(&desc, data.as_ref().map(ptr::from_ref), Some(&mut output))
        };
        self.check(result.is_ok() && output.is_some(), "fixture texture");
        result.ok()?;
        output
    }

    fn read(&self, source: &Texture2D, stride: usize) -> Vec<u8> {
        let gpu = rsf_test_support::device::Device {
            device: self.device.clone(),
            context: self.context.clone(),
        };
        unsafe { gpu.read(source, &description(source), stride) }.unwrap_or_default()
    }
    fn target(&self, source: &Texture2D, label: &str) -> Option<ID3D11RenderTargetView> {
        let mut target = None;
        let ok = unsafe {
            self.device
                .CreateRenderTargetView(source, None, Some(&mut target))
        }
        .is_ok();
        self.check(ok && target.is_some(), label);
        target
    }

    fn clear(&self, target: &ID3D11RenderTargetView, values: &[f32; 4]) {
        unsafe { self.context.ClearRenderTargetView(target, values) };
    }
}

fn description(source: &Texture2D) -> D3D11_TEXTURE2D_DESC {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };
    desc
}

fn word(bytes: &[u8], index: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(
        bytes.get(index * 4..index * 4 + 4)?.try_into().ok()?,
    ))
}

fn scalar(bytes: &[u8], index: usize) -> f32 {
    f32::from_bits(word(bytes, index).unwrap_or_default())
}

// One auxiliary SR owner driven through the transaction methods the reduced host calls.
struct Transaction(RefCell<AuxiliarySr>);

impl Transaction {
    fn new() -> Self {
        Self(RefCell::new(AuxiliarySr::new()))
    }

    fn reserve(&self, rig: &Device, pass: &Pass, surfaces: &[Surface], recovery: bool) -> bool {
        let context = rig.context.as_raw();
        let user = ptr::null_mut();
        self.0
            .borrow_mut()
            .reserve(context, pass, surfaces, None, user, recovery)
    }

    fn prepare(&self, rig: &Device, pass: &Pass, surfaces: &[Surface]) -> bool {
        self.reserve(rig, pass, surfaces, false) && self.seal()
    }

    fn pending(&self) -> bool {
        self.0.borrow().pending()
    }
    fn seal(&self) -> bool {
        self.0.borrow_mut().seal()
    }
    fn cancel(&self) -> bool {
        self.0.borrow_mut().cancel()
    }
    fn spatial(&self) -> bool {
        self.0.borrow_mut().spatial_prepare()
    }
    fn commit(&self) -> bool {
        self.0.borrow_mut().commit(false)
    }
    fn restore(&self) -> bool {
        self.0.borrow_mut().restore()
    }
    fn matches(&self, frame: u64) -> bool {
        self.0.borrow().matches([1, frame, 91])
    }

    fn temporal(&self, surfaces: &[Surface]) -> bool {
        self.0.borrow_mut().temporal_prepare(surfaces)
    }
}
fn pass(output: &Texture2D, frame: u64, in_place: bool) -> Pass {
    let mut pass: Pass = unsafe { std::mem::zeroed() };
    pass.color_output = output.as_raw();
    if in_place {
        pass.color_input = pass.color_output;
    }
    pass.session_id = 1;
    pass.native_frame = frame;
    pass.view_key = 91;
    pass
}

fn surface(
    texture: &Texture2D,
    kind: u32,
    source_rect: [f32; 4],
    output_rect: [i32; 4],
    jitter_pixels: [f32; 2],
    flags: u32,
) -> Surface {
    Surface {
        struct_size: size_of::<Surface>() as u32,
        kind,
        texture: texture.as_raw(),
        source_rect,
        output_rect,
        jitter_pixels,
        name: ptr::null(),
        source_name: ptr::null(),
        flags,
    }
}

fn viewport_bits(viewport: D3D11_VIEWPORT) -> [u32; 6] {
    [
        viewport.TopLeftX,
        viewport.TopLeftY,
        viewport.Width,
        viewport.Height,
        viewport.MinDepth,
        viewport.MaxDepth,
    ]
    .map(f32::to_bits)
}

fn saved_viewport(context: &ID3D11DeviceContext) -> (u32, [u32; 6]) {
    let mut viewport = D3D11_VIEWPORT::default();
    let mut count = 1;
    unsafe { context.RSGetViewports(&mut count, Some(&mut viewport)) };
    (count, viewport_bits(viewport))
}

#[allow(clippy::too_many_arguments)]
fn transaction_scenarios(
    rig: &Device,
    transaction: &Transaction,
    depth: &Texture2D,
    aux: &Texture2D,
    main: &Texture2D,
    immutable: &Texture2D,
    depths: &[u32; 48],
    pixels: &[u32; 48],
) -> Option<[ID3D11RenderTargetView; 2]> {
    let depth_before = rig.read(depth, 4);
    let aux_before = rig.read(aux, 4);
    let colour_before = rig.read(main, 16);
    let mut pass = pass(main, 7, false);
    let mut surfaces = [
        surface(
            depth,
            DEPTH,
            [0.0, 0.0, 4.0, 3.0],
            [0, 0, 8, 6],
            [0.25, -0.125],
            RESTORE,
        ),
        surface(
            aux,
            COLOUR,
            [0.8, 1.2, 5.75, 4.8],
            [1, 0, 8, 6],
            [0.35, -0.4],
            RESTORE,
        ),
    ];
    let viewport = D3D11_VIEWPORT {
        TopLeftX: 2.0,
        TopLeftY: 1.0,
        Width: 3.0,
        Height: 4.0,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    };
    unsafe {
        rig.context.RSSetViewports(Some(&[viewport]));
        rig.context
            .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
    }
    rig.check(
        transaction.reserve(rig, &pass, &surfaces, false),
        "reserve depth and fractional colour allocations",
    );
    rig.check(
        rig.read(depth, 4) == depth_before && rig.read(main, 16) == colour_before,
        "allocation-only auxiliary reserve leaves native contents untouched",
    );
    rig.check(
        saved_viewport(&rig.context).1 == viewport_bits(viewport),
        "reserve does not alter native viewport bindings",
    );
    rig.check(
        transaction.seal(),
        "seal depth and fractional colour region using reserved allocations",
    );
    rig.check(
        transaction.pending() && !transaction.prepare(rig, &pass, &surfaces),
        "overlap refused without cancelling the lease",
    );
    rig.check(
        rig.read(depth, 4) == depth_before
            && rig.read(aux, 4) == aux_before
            && rig.read(main, 16) == colour_before,
        "preparation leaves every native resource untouched",
    );
    rig.check(
        saved_viewport(&rig.context).1 == viewport_bits(viewport)
            && unsafe { rig.context.IAGetPrimitiveTopology() } == D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
        "preparation restores native bindings",
    );
    rig.check(transaction.matches(7), "lease identity matches");
    rig.check(!transaction.matches(8), "different native frame refused");
    let target = rig.target(main, "SR destination RTV")?;
    rig.clear(&target, &RECONSTRUCTED);
    rig.check(transaction.commit(), "commit complete SR auxiliaries");
    let scaled_depth = rig.read(depth, 4);
    let scaled_aux = rig.read(aux, 4);
    rig.check(
        scaled_depth.len() == 192 && scaled_aux.len() == 192,
        "committed readbacks",
    );
    if scaled_depth.len() == 192 && scaled_aux.len() == 192 {
        for y in 0..6 {
            for x in 0..8 {
                let dx = (((x as f32 + 0.5) / 8.0 * 4.0 - 0.25).floor() as usize).min(3);
                let dy = (((y as f32 + 0.5) / 6.0 * 3.0 + 0.125).floor() as usize).min(2);
                rig.check(
                    word(&scaled_depth, y * 8 + x) == Some(depths[dy * 8 + dx]),
                    "depth and stencil point mapping",
                );
                let expected = if x == 0 {
                    pixels[y * 8 + x]
                } else {
                    let sx = ((0.8 + (x as f32 + 0.5 - 1.0) / 7.0 * 4.95 - 0.35).floor() as usize)
                        .clamp(1, 5);
                    let sy =
                        ((1.2 + (y as f32 + 0.5) / 6.0 * 3.6 + 0.4).floor() as usize).clamp(1, 4);
                    pixels[sy * 8 + sx]
                };
                rig.check(
                    word(&scaled_aux, y * 8 + x) == Some(expected),
                    "fractional viewport, jitter, alpha and outside pixels",
                );
            }
        }
    }
    let sr_colour = rig.read(main, 16);
    rig.check(
        transaction.restore() && !transaction.pending(),
        "restore lease after final consumer",
    );
    rig.check(
        saved_viewport(&rig.context) == (1, viewport_bits(viewport))
            && unsafe { rig.context.IAGetPrimitiveTopology() } == D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
        "retirement restores native bindings",
    );
    rig.check(
        rig.read(depth, 4) == depth_before && rig.read(aux, 4) == aux_before,
        "every original auxiliary restored bit for bit",
    );
    rig.check(
        rig.read(main, 16) == sr_colour,
        "successful SR colour survives auxiliary restoration",
    );
    rig.check(
        transaction.prepare(rig, &pass, &surfaces),
        "prepare failed-SR rollback",
    );
    rig.clear(&target, &FAILED);
    rig.check(
        transaction.restore() && rig.read(main, 16) == sr_colour,
        "failed SR restores original colour",
    );
    pass.color_output_readable = immutable.as_raw();
    rig.check(
        !transaction.prepare(rig, &pass, &surfaces) && rig.read(main, 16) == sr_colour,
        "invalid second destination cannot copy an uninitialized snapshot over native colour",
    );
    pass.color_output_readable = ptr::null_mut();
    surfaces[1].flags = 0;
    rig.check(
        transaction.prepare(rig, &pass, &surfaces) && transaction.commit(),
        "prepare current-frame auxiliary retirement",
    );
    let aux_target = rig.target(aux, "native consumer RTV")?;
    rig.clear(&aux_target, &CLEAR);
    rig.check(
        transaction.restore() && rig.read(aux, 4) == [0; 192],
        "later native consumer clear survives auxiliary retirement",
    );
    Some([target, aux_target])
}

fn packed_hdr(
    rig: &Device,
    transaction: &Transaction,
    main: &Texture2D,
    immutable: &Texture2D,
    target: &ID3D11RenderTargetView,
    hdr_pixels: &[u32; 48],
    capable: bool,
) -> Option<()> {
    let hdr = rig.texture(PACKED, hdr_pixels, 4, RTV)?;
    let desc = description(&hdr);
    rig.check(
        desc.Format == PACKED && desc.Width == 8 && desc.Height == 6 && desc.SampleDesc.Count == 1,
        "actual packed HDR source descriptor",
    );
    let before = rig.read(&hdr, 4);
    let main_before = rig.read(main, 16);
    let mut pass = pass(main, 7, false);
    let mut input = [surface(
        &hdr,
        COLOUR,
        [1.25, 0.75, 5.5, 4.75],
        [2, 1, 8, 6],
        [0.25, -0.125],
        RESTORE,
    )];
    let prepared = transaction.prepare(rig, &pass, &input);
    rig.check(
        prepared == capable,
        "packed HDR admission follows actual load/RTV capability",
    );
    if !prepared {
        rig.check(
            !transaction.pending()
                && rig.read(&hdr, 4) == before
                && rig.read(main, 16) == main_before,
            "unsupported packed HDR leaves transaction and native contents unchanged",
        );
        return Some(());
    }

    rig.check(
        rig.read(&hdr, 4) == before && rig.read(main, 16) == main_before,
        "packed HDR prepare leaves engine resources untouched",
    );
    rig.clear(target, &RECONSTRUCTED);
    rig.check(
        transaction.commit(),
        "packed HDR nearest transaction commits",
    );
    let expanded = rig.read(&hdr, 4);
    rig.check(expanded.len() == 192, "packed HDR expanded readback");
    let channel = |packed: u32, shift, mantissa| {
        let code = packed >> shift;
        (1.0 + (code & ((1_u32 << mantissa) - 1)) as f32 / (1_u32 << mantissa) as f32)
            * 2.0_f32.powi(((code >> mantissa) & 31_u32) as i32 - 15)
    };
    if expanded.len() == 192 {
        for y in 0..6 {
            for x in 0..8 {
                let expected = if x >= 2 && y >= 1 {
                    let sx = ((1.25 + (x as f32 + 0.5 - 2.0) / 6.0 * 4.25 - 0.25).floor() as usize)
                        .clamp(1, 4);
                    let sy = ((0.75 + (y as f32 + 0.5 - 1.0) / 5.0 * 4.0 + 0.125).floor() as usize)
                        .clamp(1, 4);
                    hdr_pixels[sy * 8 + sx]
                } else {
                    hdr_pixels[y * 8 + x]
                };
                let actual = word(&expanded, y * 8 + x).unwrap_or_default();
                rig.check(
                    actual == expected,
                    "packed HDR nearest crop/expand preserves exact channels and outside texels",
                );
                for (shift, mantissa) in [(0_u32, 6_u32), (11, 6), (22, 5)] {
                    let value = channel(actual, shift, mantissa);
                    rig.check(
                        value.is_finite()
                            && value > 0.0
                            && value == channel(expected, shift, mantissa),
                        "positive finite R11G11B10 values retain native numeric magnitude",
                    );
                }
            }
        }
    }
    let accepted = rig.read(main, 16);
    rig.check(
        transaction.restore() && !transaction.pending() && rig.read(&hdr, 4) == before,
        "packed HDR RESTORE_CONTENTS restores every native source bit",
    );
    rig.check(
        rig.read(main, 16) == accepted,
        "packed HDR restore retains accepted SR colour",
    );
    rig.check(
        transaction.prepare(rig, &pass, &input),
        "packed HDR prepare failed-SR rollback",
    );
    rig.clear(target, &FAILED);
    rig.check(
        transaction.restore()
            && !transaction.pending()
            && rig.read(main, 16) == accepted
            && rig.read(&hdr, 4) == before,
        "failed SR cancels packed HDR transaction and restores native output",
    );
    pass.color_output_readable = immutable.as_raw();
    rig.check(!transaction.prepare(rig, &pass, &input) && !transaction.pending() && rig.read(main, 16) == accepted && rig.read(&hdr, 4) == before,
        "packed HDR preparation failure preserves all source/destination bytes and clears transaction");
    pass.color_output_readable = ptr::null_mut();
    input[0].flags = 0;
    rig.check(
        transaction.prepare(rig, &pass, &input) && transaction.commit(),
        "packed HDR without RESTORE_CONTENTS commits",
    );
    if let Some(hdr_target) = rig.target(&hdr, "packed HDR native consumer RTV") {
        rig.clear(&hdr_target, &CLEAR);
    }
    rig.check(
        transaction.restore() && rig.read(&hdr, 4) == [0; 192],
        "packed HDR later native clear survives non-restoring retirement",
    );
    Some(())
}

fn recovery_source(x: usize, y: usize) -> usize {
    if x == 0 {
        return y * 8;
    }
    let sx = ((1.0 + (x as f32 + 0.5 - 1.0) / 7.0 * 4.0).floor() as usize).clamp(1, 4);
    let sy = ((1.0 + (y as f32 + 0.5) / 6.0 * 3.0).floor() as usize).clamp(1, 3);
    sy * 8 + sx
}

fn raw_surface(texture: &Texture2D, kind: u32, flags: u32) -> Surface {
    surface(
        texture,
        kind,
        [1.0, 1.0, 5.0, 4.0],
        [1, 0, 8, 6],
        [0.0; 2],
        flags,
    )
}

fn recover_raw(rig: &Device, depths: &[u32; 48], pixels: &[u32; 48]) -> Option<()> {
    let mut values: [[f32; 4]; 48] =
        std::array::from_fn(|i| [(i + 1) as f32, (i + 2) as f32, (i + 3) as f32, 1.0]);
    let vectors: [u32; 48] =
        std::array::from_fn(|i| (0x3000 + i as u32) | ((0xb000 + i as u32) << 16));
    let scene = rig.texture(DXGI_FORMAT_R32G32B32A32_FLOAT, &values, 16, RTV)?;
    let motion = rig.texture(DXGI_FORMAT_R16G16_FLOAT, &vectors, 4, RTV)?;
    let depth = rig.texture(DXGI_FORMAT_R24G8_TYPELESS, depths, 4, DSV)?;
    let content = rig.texture(DXGI_FORMAT_R8G8B8A8_UNORM, pixels, 4, RTV)?;
    let raster = pass(&scene, 81, true);
    let raw = [
        raw_surface(&depth, DEPTH, 0),
        raw_surface(&motion, COLOUR, 0),
        raw_surface(&content, COLOUR, RESTORE),
        raw_surface(&scene, COLOUR, 0),
    ];
    let pending = Transaction::new();
    let original_scene = rig.read(&scene, 16);
    let original_content = rig.read(&content, 4);
    rig.check(
        pending.reserve(rig, &raster, &raw, true),
        "recovery preflight includes Main and raw consumers without camera",
    );
    rig.check(
        rig.read(&scene, 16) == original_scene,
        "reserve never snapshots or writes native content",
    );
    rig.check(
        pending.cancel() && !pending.pending(),
        "unused reservation cancels before producer",
    );
    rig.check(
        pending.reserve(rig, &raster, &raw, true),
        "second recovery reservation",
    );
    for pixel in &mut values {
        for value in pixel {
            *value *= 2.0;
        }
    }
    unsafe {
        rig.context
            .UpdateSubresource(&scene, 0, None, values.as_ptr().cast(), 8 * 16, 0)
    };
    rig.check(
        pending.seal(),
        "seal observes actual native production after allocation-only reserve",
    );
    rig.check(
        !pending.cancel(),
        "sealed producer cannot cancel its output obligation",
    );
    let mut fractional = raw;
    fractional[1].source_rect[0] = 0.8;
    fractional[1].jitter_pixels[0] = 0.25;
    rig.check(
        pending.temporal(&fractional[..3]),
        "actual temporal fractional mapping reuses sealed originals",
    );
    let scene_target = rig.target(&scene, "recovery consumer views")?;
    let content_target = rig.target(&content, "recovery consumer views")?;
    rig.clear(&scene_target, &OVERWRITTEN);
    rig.check(
        pending.spatial() && pending.commit(),
        "failed/skipped temporal recovers from immutable original products",
    );
    let output_scene = rig.read(&scene, 16);
    let output_depth = rig.read(&depth, 4);
    let output_motion = rig.read(&motion, 4);
    for y in 0..6 {
        for x in 0..8 {
            let dst = y * 8 + x;
            let src = recovery_source(x, y);
            rig.check(output_scene.len() == 768 && values[src].iter().enumerate().all(|(channel, expected)|
                word(&output_scene, dst*4+channel) == Some(expected.to_bits())),
                "Main nearest crop mapping and untouched outside output rectangle use sealed source");
            rig.check(
                output_depth.len() == 192 && word(&output_depth, dst) == Some(depths[src]),
                "raw D24 partition and all stencil bits retained",
            );
            rig.check(
                output_motion.len() == 192 && word(&output_motion, dst) == Some(vectors[src]),
                "raw motion stored values not rescaled or replaced",
            );
        }
    }
    rig.clear(&content_target, &OVERWRITTEN);
    let motion_target = rig.target(&motion, "later raw consumer views")?;
    let mut depth_target = None;
    let desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
        Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
        ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let ready = unsafe {
        rig.device
            .CreateDepthStencilView(&depth, Some(&desc), Some(&mut depth_target))
    }
    .is_ok();
    rig.check(ready && depth_target.is_some(), "later raw consumer views");
    let depth_target = depth_target?;
    rig.clear(&motion_target, &OVERWRITTEN);
    unsafe {
        rig.context.ClearDepthStencilView(
            &depth_target,
            D3D11_CLEAR_DEPTH.0 | D3D11_CLEAR_STENCIL.0,
            0.2,
            213,
        )
    };
    let later_depth = rig.read(&depth, 4);
    let later_motion = rig.read(&motion, 4);
    rig.check(
        pending.restore() && !pending.pending(),
        "explicit content obligations drain",
    );
    rig.check(rig.read(&content, 4) == original_content && rig.read(&scene, 16) == output_scene &&
        rig.read(&motion, 4) == later_motion && rig.read(&depth, 4) == later_depth,
        "restore-content consumer restores original while accepted Main and flags-zero later contents survive");
    let mut malformed = raw;
    malformed[0].source_rect[2] = 9.0;
    rig.check(
        !pending.reserve(rig, &raster, &malformed, true)
            && !pending.pending()
            && rig.read(&scene, 16) == output_scene,
        "failed complete preflight cannot mutate Main",
    );
    Some(())
}

fn recover_packed(rig: &Device, hdr_pixels: &[u32; 48]) -> Option<()> {
    let native_main = rig.texture(PACKED, hdr_pixels, 4, RTV)?;
    rig.check(
        description(&native_main).Format == PACKED,
        "recovery uses actual packed numeric Main descriptor",
    );
    let owner = pass(&native_main, 82, true);
    let inputs = [raw_surface(&native_main, COLOUR, 0)];
    let recovery = Transaction::new();
    rig.check(
        recovery.reserve(rig, &owner, &inputs, true) && recovery.seal() && recovery.commit(),
        "packed numeric Main reconstructs without camera or temporal inputs",
    );
    let output = rig.read(&native_main, 4);
    for y in 0..6 {
        for x in 0..8 {
            rig.check(
                output.len() == 192
                    && word(&output, y * 8 + x) == Some(hdr_pixels[recovery_source(x, y)]),
                "packed Main exact positive numeric nearest and outside pixels",
            );
        }
    }
    rig.check(
        recovery.restore() && rig.read(&native_main, 4) == output,
        "retirement never restores successful packed Main",
    );
    Some(())
}

fn run(rig: &Device) -> Option<()> {
    let depths: [u32; 48] = std::array::from_fn(|i| {
        let i = i as u32;
        (((i * 17) & 255) << 24) | ((i * 237113) & 0xffffff)
    });
    let pixels: [u32; 48] = std::array::from_fn(|i| {
        let i = i as u32;
        i | ((i * 3) << 8) | ((i * 5) << 16) | ((255 - i * 2) << 24)
    });
    let colour = [0.25_f32; 192];
    let depth = rig.texture(DXGI_FORMAT_R24G8_TYPELESS, &depths, 4, DSV)?;
    let aux = rig.texture(DXGI_FORMAT_R8G8B8A8_UNORM, &pixels, 4, RTV)?;
    let main = rig.texture(DXGI_FORMAT_R32G32B32A32_FLOAT, &colour, 16, RTV)?;
    let immutable = rig.initial_texture(
        &rig.device,
        [8, 6],
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        Some((&colour, 16)),
        SRV,
        D3D11_USAGE_IMMUTABLE,
    )?;
    let transaction = Transaction::new();
    let [target, aux_target] = transaction_scenarios(
        rig,
        &transaction,
        &depth,
        &aux,
        &main,
        &immutable,
        &depths,
        &pixels,
    )?;

    let hdr_pixels: [u32; 48] = std::array::from_fn(|i| {
        let i = i as u32;
        let r = (((i % 9) + 11) << 6) | ((i * 7) & 63);
        let g = (((i % 7) + 12) << 6) | ((i * 11) & 63);
        let b = (((i % 5) + 13) << 5) | ((i * 3) & 31);
        r | (g << 11) | (b << 22)
    });
    let required = (D3D11_FORMAT_SUPPORT_TEXTURE2D.0
        | D3D11_FORMAT_SUPPORT_SHADER_LOAD.0
        | D3D11_FORMAT_SUPPORT_RENDER_TARGET.0) as u32;
    let capable = unsafe { rig.device.CheckFormatSupport(PACKED) }
        .is_ok_and(|support| support & required == required);
    rig.check(
        capable,
        "WARP supports numeric R11G11B10 texture load and render target",
    );
    packed_hdr(
        rig,
        &transaction,
        &main,
        &immutable,
        &target,
        &hdr_pixels,
        capable,
    )?;
    recover_raw(rig, &depths, &pixels)?;
    if capable {
        recover_packed(rig, &hdr_pixels)?;
    }
    regions::run(rig, &immutable, &aux_target, &colour)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_auxiliary_sr_fixture() -> i32 {
    let Ok((device, context)) =
        rsf_test_support::warp_d3d11(D3D11_CREATE_DEVICE_FLAG(0), Some(&[D3D_FEATURE_LEVEL_11_0]))
    else {
        return 77;
    };
    let rig = Device {
        device,
        context,
        failures: Cell::new(0),
    };
    let completed = run(&rig).is_some();
    rsf_native_regions_release();
    i32::from(!completed || rig.failures.get() != 0)
}
