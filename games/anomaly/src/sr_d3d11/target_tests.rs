// SPDX-License-Identifier: MIT
use super::*;
use crate::sr_d3d11::tests::{Call, DEVICE, PLATFORM, SOURCE, calls, extent, fake, ptr};
use std::ptr::null_mut;

fn twin_input() -> TwinInput {
    fake(|state| {
        let mut desc = Texture::target(extent(24, 16), 27, COLOUR_BINDINGS);
        desc.sample_quality = 2;
        desc.misc = 4;
        state.textures.insert(SOURCE, desc);
    });
    TwinInput {
        texture: ptr(SOURCE),
        rtv: ptr(6),
        srv: ptr(7),
        uav: ptr(8),
        size: extent(12, 8),
        format: 21,
        promote: 0,
        with_uav: 1,
        world: 1,
    }
}

fn device() -> Device<'static> {
    Device {
        platform: &PLATFORM,
        handle: ptr(DEVICE),
    }
}

#[test]
fn twin_error_names_follow_the_refusal_causes() {
    let name = |cause: u8| {
        unsafe { std::ffi::CStr::from_ptr(rsf_anomaly_sr_twin_error_name(cause)) }
            .to_str()
            .unwrap()
            .to_owned()
    };
    let causes = [
        Error::None,
        Error::NotTexture,
        Error::Shape,
        Error::Format,
        Error::Support,
        Error::Create,
        Error::Depth,
    ];
    let names = causes.map(|cause| name(cause as u8));
    assert_eq!(
        names,
        [
            "none",
            "not_texture",
            "shape",
            "format",
            "support",
            "create",
            "depth"
        ]
    );
    assert_eq!(name(7), "unknown");
    assert_eq!(name(u8::MAX), "unknown");
}

#[test]
fn twin_uses_engine_view_templates_and_partial_failure_releases_every_reference() {
    let input = twin_input();
    let (twin, desc, format) = device().twin(&input).ok().unwrap();
    assert_eq!(format, 21);
    assert_eq!(
        desc,
        Texture {
            sample_quality: 2,
            ..Texture::target(extent(12, 8), 27, COLOUR_BINDINGS | BIND_UAV)
        }
    );
    assert_eq!(
        calls(),
        [
            Call::Texture(desc, 0, None),
            Call::View(RTV, Some(100)),
            Call::View(SRV, Some(101)),
            Call::View(UAV, Some(102)),
        ]
    );
    let owner = Box::into_raw(Box::new(twin));
    rsf_anomaly_sr_release_twin_view(unsafe { &mut *owner }, RTV);
    assert_eq!(fake(|state| state.owned.len()), 3);
    rsf_anomaly_sr_drop_target(owner);
    assert!(fake(|state| state.owned.is_empty()));

    fake(|state| state.fail_view = Some(UAV));
    assert_eq!(device().twin(&input).err(), Some(Error::Create));
    assert!(fake(|state| state.owned.is_empty()));
}

#[test]
fn promoted_twins_require_blending_and_typed_uav_support() {
    let input = TwinInput {
        promote: 1,
        ..twin_input()
    };
    fake(|state| state.support = PROMOTED_SUPPORT);
    assert_eq!(device().twin(&input).err(), Some(Error::Support));
    assert_eq!(calls(), [Call::Support(RGBA16F)]);
    assert!(fake(|state| state.owned.is_empty()));

    fake(|state| state.wrong_view = Some(RTV));
    let input = TwinInput {
        with_uav: 0,
        ..input
    };
    let (twin, desc, format) = device().twin(&input).ok().unwrap();
    assert_eq!(format, D3DFMT_A16B16G16R16F);
    assert_eq!(desc.format, RGBA16F);
    assert_eq!(
        calls(),
        [
            Call::Support(RGBA16F),
            Call::Texture(desc, 0, None),
            Call::View(RTV, None),
            Call::View(SRV, None),
        ]
    );
    drop(twin);
    assert!(fake(|state| state.owned.is_empty()));
}

#[test]
fn depth_and_scratch_keep_distinct_view_and_format_contracts() {
    twin_input();
    fake(|state| {
        state.view_format = D24_UNORM_S8_UINT;
        state.view_flags = 1;
    });
    let render = extent(12, 8);
    let output = extent(24, 16);
    assert_eq!(
        device().depth(ptr(6), output, render).err(),
        Some(Error::Depth)
    );
    fake(|state| state.view_flags = 0);
    assert_eq!(
        device().depth(ptr(6), render, render).err(),
        Some(Error::Depth)
    );
    assert!(calls().is_empty());

    let (depth, desc) = device().depth(ptr(6), output, render).ok().unwrap();
    assert_eq!(
        desc,
        Texture::target(render, R24G8_TYPELESS, BIND_DSV | BIND_SRV)
    );
    assert_eq!(
        calls(),
        [
            Call::Texture(desc, 0, None),
            Call::View(DSV, None),
            Call::View(DEPTH_SRV, None)
        ]
    );
    drop(depth);

    fake(|state| state.view_format = 87);
    let (scratch, desc) = device().scratch(ptr(6), render).ok().unwrap();
    assert_eq!(desc, Texture::target(render, 87, BIND_RTV));
    assert_eq!(
        calls(),
        [Call::Texture(desc, 0, None), Call::View(RTV, None)]
    );
    drop(scratch);
    assert!(fake(|state| state.owned.is_empty()));
}

#[test]
fn temporal_targets_are_keyed_by_device_both_extents_and_output_format() {
    let render = extent(12, 8);
    let output = extent(24, 16);
    let format = 26;
    let targets = TemporalTargets::new(&PLATFORM, ptr(DEVICE), render, output, format)
        .ok()
        .unwrap();
    let colour = Texture::target(render, RGBA16F, COLOUR_BINDINGS);
    let empty = Texture {
        usage: IMMUTABLE,
        ..Texture::target(render, RGBA16F, BIND_SRV)
    };
    assert_eq!(
        calls(),
        [
            Call::Texture(colour, 0, None),
            Call::Texture(colour, 0, None),
            Call::View(RTV, None),
            Call::View(RTV, None),
            Call::View(SRV, None),
            Call::View(SRV, None),
            Call::Texture(empty, render.width * 8, Some(0)),
            Call::View(SRV, None),
            Call::Texture(Texture::target(output, format, BIND_SRV), 0, None),
            Call::View(SRV, None),
            Call::Texture(
                Texture::target(extent(1, 1), R32_FLOAT, BIND_SRV),
                4,
                Some(1.0f32.to_bits())
            ),
        ]
    );
    assert!(targets.matches(ptr(DEVICE), render, output, format));
    assert!(!targets.matches(ptr(2), render, output, format));
    assert!(!targets.matches(ptr(DEVICE), output, output, format));
    assert!(!targets.matches(ptr(DEVICE), render, render, format));
    assert!(!targets.matches(ptr(DEVICE), render, output, RGBA16F));
    assert!(!targets.matches(null_mut(), render, output, format));
    assert_eq!(fake(|state| state.owned.len()), 11);
    drop(targets);
    assert!(fake(|state| state.owned.is_empty()));
}

#[test]
fn temporal_empty_motion_rejects_an_overflowing_row_pitch_before_allocation() {
    let render = extent(u32::MAX / 8 + 1, 1);
    assert_eq!(render.width.wrapping_mul(8), 0);
    assert_eq!(
        TemporalTargets::new(&PLATFORM, ptr(DEVICE), render, render, RGBA16F).err(),
        Some(Error::Create)
    );
    assert!(calls().is_empty());
}
