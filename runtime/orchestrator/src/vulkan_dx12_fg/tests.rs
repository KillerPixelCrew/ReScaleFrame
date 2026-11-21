// SPDX-License-Identifier: MIT
// Expected values are the Vulkan header constants written out, not the
// module's names for them, so a wrong constant cannot agree with itself.
use super::*;

const FAMILY: u32 = 3;
const IGNORED: u32 = 0xffff_ffff;
const EXTERNAL: u32 = 0xffff_fffe;
const MEMORY: u32 = 0x8000 | 0x1_0000;
const ALL: u32 = 0x1_0000;

fn expected(image: u64, old: u32, new: u32, from: u32, to: u32, src: u32, dst: u32) -> Step {
    Step::Barrier(Barrier {
        image,
        old_layout: old,
        new_layout: new,
        source_family: from,
        target_family: to,
        source_access: src,
        target_access: dst,
        source_stages: ALL,
        target_stages: ALL,
    })
}

fn source(format: u32, width: u32, height: u32) -> Image {
    Image {
        device: ptr::null_mut(),
        image: 0x10,
        format,
        width,
        height,
        layout: 5,
        copy_source: 1,
    }
}

fn shared(format: u32, width: u32, height: u32) -> SharedImage {
    SharedImage {
        image: 0x20,
        dx12_resource: ptr::null_mut(),
        format,
        width,
        height,
    }
}

#[test]
fn first_guide_copy_discards_target_and_releases_it_external() {
    let steps = copy_steps(
        FAMILY,
        &source(100, 640, 360),
        &shared(100, 640, 360),
        false,
    );
    assert_eq!(
        steps,
        [
            expected(0x10, 5, 6, IGNORED, IGNORED, MEMORY, MEMORY),
            expected(0x20, 0, 7, FAMILY, FAMILY, 0, MEMORY),
            Step::Copy(Transfer {
                source: 0x10,
                target: 0x20,
                source_extent: [640, 360],
                target_extent: [640, 360],
            }),
            expected(0x10, 6, 5, IGNORED, IGNORED, MEMORY, MEMORY),
            expected(0x20, 7, 1, FAMILY, EXTERNAL, MEMORY, 0),
        ]
    );
}

#[test]
fn initialized_target_is_acquired_from_external_general() {
    let steps = copy_steps(FAMILY, &source(83, 640, 360), &shared(83, 640, 360), true);
    assert_eq!(steps[1], expected(0x20, 1, 7, EXTERNAL, FAMILY, 0, MEMORY));
    assert!(matches!(steps[2], Step::Copy(_)));
    assert_eq!(steps[4], expected(0x20, 7, 1, FAMILY, EXTERNAL, MEMORY, 0));
}

#[test]
fn rgba_colour_into_bgra_shared_image_blits_full_extents() {
    let steps = copy_steps(
        FAMILY,
        &source(37, 1920, 1080),
        &shared(44, 1920, 1080),
        true,
    );
    assert_eq!(
        steps[2],
        Step::Blit(Transfer {
            source: 0x10,
            target: 0x20,
            source_extent: [1920, 1080],
            target_extent: [1920, 1080],
        })
    );
    let same = copy_steps(
        FAMILY,
        &source(44, 1920, 1080),
        &shared(44, 1920, 1080),
        true,
    );
    assert!(matches!(same[2], Step::Copy(_)));
}

#[test]
fn placeholder_clear_discards_then_releases_external() {
    assert_eq!(
        clear_steps(FAMILY, 0x30),
        [
            expected(0x30, 0, 7, FAMILY, FAMILY, 0, MEMORY),
            Step::Clear(0x30),
            expected(0x30, 7, 1, FAMILY, EXTERNAL, MEMORY, 0),
        ]
    );
}

#[test]
fn blit_admission_needs_rgba_source_and_bgra_target_features() {
    assert!(blit_formats_supported(0x400, 0x800));
    assert!(blit_formats_supported(0xffff_ffff, 0xffff_ffff));
    assert!(!blit_formats_supported(0, 0x800));
    assert!(!blit_formats_supported(0x400, 0));
    assert!(!blit_formats_supported(0x800, 0x400));
    assert_eq!(rsf_core_dx12gvk_blit_formats_supported(0x400, 0x800), 1);
    assert_eq!(rsf_core_dx12gvk_blit_formats_supported(0x800, 0x400), 0);
}
