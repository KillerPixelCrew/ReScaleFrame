// SPDX-License-Identifier: MIT
use super::*;
use crate::native_regions::{rsf_native_regions_prepare, rsf_native_regions_temporal_mask};
use rsf_game_sys::{
    RSF_DEPTH_DOMAIN_INCLUDE_MAX, RSF_DEPTH_DOMAIN_INCLUDE_MIN, RSF_DEPTH_DOMAINS_VERSION,
    rsf_game_depth_domains as Domains,
};

fn bind(rig: &Device, target: Option<&ID3D11RenderTargetView>) {
    let targets = [target.cloned()];
    unsafe {
        rig.context.OMSetRenderTargets(
            target.map(|_| &targets[..]),
            None::<&ID3D11DepthStencilView>,
        )
    };
}

fn retained(rig: &Device, target: &ID3D11RenderTargetView) -> bool {
    let mut current = [None];
    unsafe { rig.context.OMGetRenderTargets(Some(&mut current), None) };
    current[0]
        .as_ref()
        .is_some_and(|view| view.as_raw() == target.as_raw())
}

fn read_output(rig: &Device, raw: Texture, stride: usize) -> Vec<u8> {
    // Region outputs are borrowed from the cache through the next normalization.
    unsafe { Texture2D::from_raw_borrowed(&raw) }
        .map_or_else(Vec::new, |source| rig.read(source, stride))
}

fn mask_scenarios(
    rig: &Device,
    aux_target: &ID3D11RenderTargetView,
) -> Option<Option<ID3D11Device>> {
    let values: [[u8; 2]; 48] = std::array::from_fn(|i| [(i * 19) as u8, (i * 7) as u8]);
    let temporal = rig.texture(DXGI_FORMAT_R8G8_UNORM, &values, 2, NO_BIND)?;
    let mut reactive = ptr::null_mut();
    let mut bias = ptr::null_mut();
    let rect = [2, 1, 6, 4];
    let weights = [0.1, 1.0, 0.0, 0.0];
    let normalize = |mask: &Texture2D,
                     rect: &[i32; 4],
                     previous_r,
                     previous_b,
                     reactive: &mut Texture,
                     bias: &mut Texture| {
        unsafe {
            rsf_native_regions_temporal_mask(
                rig.context.as_raw(),
                mask.as_raw(),
                rect,
                &weights,
                previous_r,
                previous_b,
                reactive,
                bias,
            )
        }
    };
    let absent = ptr::null_mut();
    let normalized = normalize(&temporal, &rect, absent, absent, &mut reactive, &mut bias);
    rig.check(normalized, "normalize native temporal mask");
    if normalized {
        let view = unsafe { Texture2D::from_raw_borrowed(&reactive) }?;
        let desc = description(view);
        rig.check(
            desc.Width == 4 && desc.Height == 3 && desc.Format == DXGI_FORMAT_R32_FLOAT,
            "mask render-local R32 format and size",
        );
        let original = rig.read(view, 4);
        rig.check(
            original == read_output(rig, bias, 4) && original.len() == 48,
            "same explicit hint feeds reactive and bias",
        );
        for y in 0..3 {
            for x in 0..4 {
                let [r, g] = values[(y + 1) * 8 + x + 2];
                let expected = (0.1 * f32::from(r) / 255.0 + f32::from(g) / 255.0).min(1.0);
                rig.check(
                    (scalar(&original, y * 4 + x) - expected).abs() < 1e-5,
                    "mask channels and crop origin have native numeric semantics",
                );
            }
        }
        let previous_r = rig.initial_texture(
            &rig.device,
            [4, 3],
            DXGI_FORMAT_R32_FLOAT,
            Some((&[0.8_f32; 12], 4)),
            SRV,
            D3D11_USAGE_DEFAULT,
        );
        let previous_b = rig.initial_texture(
            &rig.device,
            [4, 3],
            DXGI_FORMAT_R32_FLOAT,
            Some((&[0.6_f32; 12], 4)),
            SRV,
            D3D11_USAGE_DEFAULT,
        );
        rig.check(
            previous_r.is_some() && previous_b.is_some(),
            "existing temporal hint fixtures",
        );
        let (previous_r, previous_b) = (previous_r?, previous_b?);
        let combined = normalize(
            &temporal,
            &rect,
            previous_r.as_raw(),
            previous_b.as_raw(),
            &mut reactive,
            &mut bias,
        );
        rig.check(combined, "compose explicit and existing temporal hints");
        if combined {
            let combined_r = read_output(rig, reactive, 4);
            let combined_b = read_output(rig, bias, 4);
            rig.check(
                combined_r.len() == 48 && combined_b.len() == 48,
                "combined hints readable",
            );
            if original.len() == 48 && combined_r.len() == 48 && combined_b.len() == 48 {
                for i in 0..12 {
                    let initial = scalar(&original, i);
                    rig.check(
                        (scalar(&combined_r, i) - initial.max(0.8)).abs() < 1e-5
                            && (scalar(&combined_b, i) - initial.max(0.6)).abs() < 1e-5,
                        "existing reactive and bias compose independently by maximum",
                    );
                }
            }
        }
    }
    rig.check(
        !normalize(
            &temporal,
            &[2, 1, 9, 4],
            absent,
            absent,
            &mut reactive,
            &mut bias,
        ),
        "out-of-resource temporal mask rectangle refused",
    );
    let second = create_warp(None);
    rig.check(
        second.is_some(),
        "second device for foreign temporal resource checks",
    );
    if let Some(other) = &second {
        let foreign_mask = rig.initial_texture(
            other,
            [8, 6],
            DXGI_FORMAT_R8G8_UNORM,
            Some((&values, 2)),
            NO_BIND,
            D3D11_USAGE_DEFAULT,
        );
        let foreign_hint = rig.initial_texture::<f32>(
            other,
            [4, 3],
            DXGI_FORMAT_R32_FLOAT,
            None,
            SRV,
            D3D11_USAGE_DEFAULT,
        );
        rig.check(foreign_hint.is_some(), "foreign scalar hint fixture");
        bind(rig, Some(aux_target));
        let saved = (reactive, bias);
        if let (Some(foreign_mask), Some(foreign_hint)) = (foreign_mask, foreign_hint) {
            rig.check(
                !normalize(
                    &foreign_mask,
                    &rect,
                    absent,
                    absent,
                    &mut reactive,
                    &mut bias,
                ),
                "foreign non-SRV mask refused before copy",
            );
            rig.check(
                !normalize(
                    &temporal,
                    &rect,
                    foreign_hint.as_raw(),
                    absent,
                    &mut reactive,
                    &mut bias,
                ),
                "foreign reactive hint refused",
            );
            rig.check(
                !normalize(
                    &temporal,
                    &rect,
                    absent,
                    foreign_hint.as_raw(),
                    &mut reactive,
                    &mut bias,
                ),
                "foreign bias hint refused",
            );
        }
        rig.check(
            retained(rig, aux_target) && (reactive, bias) == saved,
            "foreign resources leave bindings and output pointers unchanged",
        );
        bind(rig, None);
    }
    Some(second)
}

fn depth_domains(split: f32) -> Domains {
    let mut domains: Domains = unsafe { std::mem::zeroed() };
    domains.struct_size = size_of::<Domains>() as u32;
    domains.version = RSF_DEPTH_DOMAINS_VERSION;
    domains.domain_count = 2;
    domains.clear_enabled = 1;
    domains.clear_value = 1.0;
    for (index, domain) in domains.domains.iter_mut().enumerate() {
        let (min, max, endpoints) = if index == 0 {
            (0.0, split, 3)
        } else {
            (split, 1.0, RSF_DEPTH_DOMAIN_INCLUDE_MAX)
        };
        domain.classify_min = min;
        domain.viewport_min_depth = min;
        domain.classify_max = max;
        domain.viewport_max_depth = max;
        domain.endpoint_flags = endpoints;
        domain.viewport_xywh[2] = 8.0;
        domain.viewport_xywh[3] = 6.0;
        for index in [0, 5, 10, 15] {
            domain.source_clip_to_target_clip[index] = 1.0;
        }
    }
    // Changed target X, with Z/W written back at the original sample.
    domains.domains[0].source_clip_to_target_clip[12] = 0.3;
    domains.domains[0].source_clip_to_target_clip[10] = 0.5;
    domains.domains[0].source_clip_to_target_clip[11] = 0.5;
    domains.domains[1].source_clip_to_target_clip[10] = 2.0;
    domains.domains[1].source_clip_to_target_clip[14] = -0.5;
    domains
}

fn admissions(
    rig: &Device,
    domains: Domains,
    split: f32,
    mut normalize: impl FnMut(&Domains) -> bool,
) {
    let mut invalid = domains;
    invalid.struct_size -= 1;
    rig.check(!normalize(&invalid), "short depth descriptor refused");
    invalid = domains;
    invalid.version += 1;
    rig.check(!normalize(&invalid), "unknown depth version refused");
    invalid = domains;
    invalid.domain_count = 3;
    rig.check(!normalize(&invalid), "excess depth domains refused");
    invalid = domains;
    invalid.reserved[0] = 1;
    rig.check(!normalize(&invalid), "reserved depth field refused");
    invalid = domains;
    invalid.domains[1].classify_min += 0.01;
    rig.check(!normalize(&invalid), "partial-domain gap refused");
    invalid = domains;
    invalid.domains[1].endpoint_flags |= 1;
    rig.check(!normalize(&invalid), "ambiguous included boundary refused");
    invalid = domains;
    invalid.domains[0].viewport_max_depth = split - 0.001;
    rig.check(
        !normalize(&invalid),
        "unmapping outside quantization tolerance refused",
    );
    invalid = domains;
    invalid.domains[1].viewport_min_depth = split.next_up();
    rig.check(
        normalize(&invalid),
        "one UNORM step endpoint tolerance accepted",
    );
    invalid = domains;
    invalid.domains[0].source_clip_to_target_clip[11] = -1.0;
    rig.check(
        !normalize(&invalid),
        "possible zero W at box corner refused",
    );
    for (value, label) in [(1e-8, "near-zero W refused"), (-1.0, "negative W refused")] {
        invalid = domains;
        invalid.domains[0].source_clip_to_target_clip[15] = value;
        rig.check(!normalize(&invalid), label);
    }
    for (value, label) in [
        (f32::INFINITY, "nonfinite matrix refused"),
        (0.0, "singular matrix refused"),
    ] {
        invalid = domains;
        invalid.domains[0].source_clip_to_target_clip[0] = value;
        rig.check(!normalize(&invalid), label);
    }
    invalid = domains;
    invalid.domains[0].source_clip_to_target_clip[10] = 1e30;
    rig.check(!normalize(&invalid), "overflow-risk matrix refused");
    invalid = domains;
    invalid.domains[0].viewport_xywh[0] = 3.0;
    rig.check(
        !normalize(&invalid),
        "viewport missing part of crop refused",
    );
}

pub(super) fn run(
    rig: &Device,
    immutable: &Texture2D,
    aux_target: &ID3D11RenderTargetView,
    colour: &[f32; 192],
) -> Option<()> {
    let second = mask_scenarios(rig, aux_target)?;
    let codes = [0_u16, 32767, 32768, 65535, 32769, 65534, 16384, 49152];
    let packed_vectors = [0_u32; 48];
    let depth16: [u16; 48] = std::array::from_fn(|i| codes[(i + 2 * (i / 8) + 6) % 8]);
    let depth = rig.texture(DXGI_FORMAT_R16_TYPELESS, &depth16, 2, DSV)?;
    let motion = rig.texture(DXGI_FORMAT_R16G16_FLOAT, &packed_vectors, 4, SRV)?;
    let before = rig.read(&depth, 2);
    let rect = [2, 1, 6, 5];
    let split = 32768.0_f32 / 65535.0;
    let domains = depth_domains(split);
    let mut outputs = [ptr::null_mut(); 3];
    let normalize = |request: &Domains,
                     color: &Texture2D,
                     source: &Texture2D,
                     vectors: &Texture2D,
                     outputs: &mut [Texture; 3]| {
        let [local_color, local_depth, local_motion] = outputs;
        unsafe {
            rsf_native_regions_prepare(
                rig.context.as_raw(),
                color.as_raw(),
                source.as_raw(),
                vectors.as_raw(),
                &rect,
                local_color,
                local_depth,
                local_motion,
                request,
                false,
            )
        }
    };
    let mapped = normalize(&domains, immutable, &depth, &motion, &mut outputs);
    rig.check(
        mapped,
        "normalize exhaustive UNORM depth domains without readback in production",
    );
    if mapped {
        let output = unsafe { Texture2D::from_raw_borrowed(&outputs[1]) }?;
        let desc = description(output);
        rig.check(
            desc.Width == 4 && desc.Height == 4 && desc.Format == DXGI_FORMAT_R32_FLOAT,
            "domain depth crop is render-local R32",
        );
        let values = rig.read(output, 4);
        rig.check(values.len() == 64, "mapped depth readback");
        for y in 0..4 {
            for x in 0..4 {
                let stored = f64::from(depth16[(y + 1) * 8 + x + 2]) / 65535.0;
                let first = stored as f32 <= split;
                let domain = &domains.domains[usize::from(!first)];
                let z = (stored - f64::from(domain.viewport_min_depth))
                    / f64::from(domain.viewport_max_depth - domain.viewport_min_depth);
                let expected = if stored == 1.0 {
                    1.0
                } else if first {
                    (0.5 * z) / (1.0 + 0.5 * z)
                } else {
                    (2.0 * z - 0.5).clamp(0.0, 1.0)
                };
                rig.check((f64::from(scalar(&values, y*4+x)) - expected).abs() < 2e-5,
                    "UNORM split, offset crop, homogeneous divide, changed XY and explicit clipping");
            }
        }
        let mut clear = domains;
        clear.clear_value = split;
        clear.domains[0].endpoint_flags = RSF_DEPTH_DOMAIN_INCLUDE_MIN;
        rig.check(
            normalize(&clear, immutable, &depth, &motion, &mut outputs),
            "explicit clear owns otherwise excluded shared endpoint",
        );
        let values = read_output(rig, outputs[1], 4);
        for y in 0..4 {
            for x in 0..4 {
                if depth16[(y + 1) * 8 + x + 2] == 32768 {
                    rig.check(
                        values.len() == 64 && scalar(&values, y * 4 + x) == 1.0,
                        "clear takes precedence over homogeneous projection",
                    );
                }
            }
        }
        let mut fractional = domains;
        for domain in &mut fractional.domains {
            domain.viewport_xywh = [2.25, 1.25, 3.5, 3.5];
        }
        fractional.domains[0].source_clip_to_target_clip[2] = 0.1;
        fractional.domains[0].source_clip_to_target_clip[6] = 0.1;
        rig.check(
            normalize(&fractional, immutable, &depth, &motion, &mut outputs),
            "fractional viewport covers sample centers without covering outer texel edges",
        );
        let values = read_output(rig, outputs[1], 4);
        let split64 = f64::from(split);
        for y in 0..4 {
            for x in 0..4 {
                let stored = f64::from(depth16[(y + 1) * 8 + x + 2]) / 65535.0;
                let first = stored as f32 <= split;
                let z = if first {
                    stored / split64
                } else {
                    (stored - split64) / f64::from(1.0 - split)
                };
                let nx = 2.0 * (x as f64 + 2.5 - 2.25) / 3.5 - 1.0;
                let ny = 1.0 - 2.0 * (y as f64 + 1.5 - 1.25) / 3.5;
                let expected = if stored == 1.0 {
                    1.0
                } else if first {
                    ((0.5 * z + 0.1 * nx + 0.1 * ny) / (1.0 + 0.5 * z)).clamp(0.0, 1.0)
                } else {
                    (2.0 * z - 0.5).clamp(0.0, 1.0)
                };
                rig.check(
                    values.len() == 64
                        && (f64::from(scalar(&values, y * 4 + x)) - expected).abs() < 2e-5,
                    "fractional viewport unmaps actual sample-center NDC",
                );
            }
        }
        for (axis, label) in [
            (2, "viewport right edge at last sample center is uncovered"),
            (3, "viewport bottom edge at last sample center is uncovered"),
        ] {
            let mut uncovered = fractional;
            uncovered.domains[0].viewport_xywh[axis] = 3.25;
            rig.check(
                !normalize(&uncovered, immutable, &depth, &motion, &mut outputs),
                label,
            );
        }
        admissions(rig, domains, split, |request| {
            normalize(request, immutable, &depth, &motion, &mut outputs)
        });
        let float_depth = rig.texture(DXGI_FORMAT_R32_FLOAT, &[0.25_f32; 48], 4, SRV)?;
        rig.check(
            !normalize(&domains, immutable, &float_depth, &motion, &mut outputs),
            "mapped float depth has no finite-sample contract",
        );
        let legacy: Domains = unsafe { std::mem::zeroed() };
        rig.check(
            normalize(&legacy, immutable, &float_depth, &motion, &mut outputs),
            "zero descriptor retains legacy R32 exact-copy path",
        );
        if let Some(other) = second {
            let foreign_depth = rig.initial_texture(
                &other,
                [8, 6],
                DXGI_FORMAT_R16_TYPELESS,
                Some((&depth16, 2)),
                DSV,
                D3D11_USAGE_DEFAULT,
            )?;
            bind(rig, Some(aux_target));
            let saved = outputs[1];
            rig.check(
                !normalize(&domains, immutable, &foreign_depth, &motion, &mut outputs),
                "foreign depth refused before readable copy or cache mutation",
            );
            let foreign_color = rig.initial_texture(
                &other,
                [8, 6],
                DXGI_FORMAT_R32G32B32A32_FLOAT,
                Some((colour, 16)),
                SRV,
                D3D11_USAGE_DEFAULT,
            )?;
            let foreign_motion = rig.initial_texture(
                &other,
                [8, 6],
                DXGI_FORMAT_R16G16_FLOAT,
                Some((&packed_vectors, 4)),
                SRV,
                D3D11_USAGE_DEFAULT,
            )?;
            rig.check(
                !normalize(&domains, &foreign_color, &depth, &motion, &mut outputs),
                "foreign region colour refused",
            );
            rig.check(
                !normalize(&domains, immutable, &depth, &foreign_motion, &mut outputs),
                "foreign region motion refused",
            );
            rig.check(
                retained(rig, aux_target) && outputs[1] == saved,
                "foreign depth preserves outputs and native bindings",
            );
            bind(rig, None);
        }
        let depth24: [u32; 48] = std::array::from_fn(|i| (i % 5) as u32 + 0x7ffffe);
        let d24 = rig.texture(DXGI_FORMAT_R24G8_TYPELESS, &depth24, 4, DSV)?;
        let mut mapped24 = domains;
        let split24 = 0x800000 as f32 / 16777215.0;
        mapped24.domains[0].classify_max = split24;
        mapped24.domains[0].viewport_max_depth = split24;
        mapped24.domains[1].classify_min = split24;
        mapped24.domains[1].viewport_min_depth = split24;
        rig.check(
            normalize(&mapped24, immutable, &d24, &motion, &mut outputs),
            "D24 exact decoded split accepted",
        );
        let values = read_output(rig, outputs[1], 4);
        for y in 0..4 {
            for x in 0..4 {
                let stored = f64::from(depth24[(y + 1) * 8 + x + 2]) / 16777215.0;
                let first = stored as f32 <= split24;
                let z = if first {
                    stored / f64::from(split24)
                } else {
                    (stored - f64::from(split24)) / f64::from(1.0 - split)
                };
                let expected = if first {
                    (0.5 * z) / (1.0 + 0.5 * z)
                } else {
                    (2.0 * z - 0.5).clamp(0.0, 1.0)
                };
                rig.check(
                    values.len() == 64
                        && (f64::from(scalar(&values, y * 4 + x)) - expected).abs() < 2e-5,
                    "D24 quantization retains correct split-side depth",
                );
            }
        }
        let d24_readback = rig.read(&d24, 4);
        rig.check(
            d24_readback.len() == 192
                && depth24
                    .iter()
                    .enumerate()
                    .all(|(i, &value)| word(&d24_readback, i) == Some(value)),
            "native D24 bits unchanged",
        );
    }
    rig.check(
        rig.read(&depth, 2) == before,
        "native partitioned depth unchanged",
    );
    Some(())
}
