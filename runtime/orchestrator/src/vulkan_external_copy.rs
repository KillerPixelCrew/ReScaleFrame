// SPDX-License-Identifier: MIT

pub(super) const UNDEFINED: u32 = 0;
pub(super) const GENERAL: u32 = 1;
pub(super) const TRANSFER_SRC: u32 = 6;
pub(super) const TRANSFER_DST: u32 = 7;
pub(super) const FAMILY_IGNORED: u32 = !0;
pub(super) const FAMILY_EXTERNAL: u32 = !1;
const MEMORY_ACCESS: u32 = 0x8000 | 0x1_0000;
pub(super) const ALL_COMMANDS: u32 = 0x1_0000;

/// Mirrors rsf_dx12gvk_barrier in native_fg_dx12_vulkan_platform.h.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Barrier {
    pub image: u64,
    pub old_layout: u32,
    pub new_layout: u32,
    pub source_family: u32,
    pub target_family: u32,
    pub source_access: u32,
    pub target_access: u32,
    pub source_stages: u32,
    pub target_stages: u32,
}

impl Barrier {
    pub(super) fn new(image: u64, before: u32, after: u32, from: u32, to: u32) -> Self {
        Self {
            image,
            old_layout: before,
            new_layout: after,
            source_family: from,
            target_family: to,
            source_access: if before == UNDEFINED || from == FAMILY_EXTERNAL {
                0
            } else {
                MEMORY_ACCESS
            },
            target_access: if to == FAMILY_EXTERNAL {
                0
            } else {
                MEMORY_ACCESS
            },
            source_stages: ALL_COMMANDS,
            target_stages: ALL_COMMANDS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Transfer {
    pub source: u64,
    pub target: u64,
    pub source_extent: [u32; 2],
    pub target_extent: [u32; 2],
}

const _: () = {
    assert!(size_of::<Barrier>() == 40);
    assert!(size_of::<Transfer>() == 32);
};

pub(super) struct Plan {
    pub before: [Barrier; 2],
    pub transfer: Transfer,
    pub after: [Barrier; 2],
}

/// Copy or blit between the two barrier pairs. The producer stays on its queue
/// and returns to its layout. The shared image ends in external GENERAL.
/// First use discards contents with equal queue families chosen by the caller.
pub(super) fn plan(
    transfer: Transfer,
    source_layout: u32,
    family: u32,
    initialized: bool,
    discard_family: u32,
) -> Plan {
    let Transfer { source, target, .. } = transfer;
    let (old, from, to) = if initialized {
        (GENERAL, FAMILY_EXTERNAL, family)
    } else {
        (UNDEFINED, discard_family, discard_family)
    };
    Plan {
        before: [
            Barrier::new(
                source,
                source_layout,
                TRANSFER_SRC,
                FAMILY_IGNORED,
                FAMILY_IGNORED,
            ),
            Barrier::new(target, old, TRANSFER_DST, from, to),
        ],
        transfer,
        after: [
            Barrier::new(
                source,
                TRANSFER_SRC,
                source_layout,
                FAMILY_IGNORED,
                FAMILY_IGNORED,
            ),
            Barrier::new(target, TRANSFER_DST, GENERAL, family, FAMILY_EXTERNAL),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discards_on_first_use_then_acquires_and_releases() {
        let transfer = Transfer {
            source: 10,
            target: 11,
            source_extent: [128, 64],
            target_extent: [100, 50],
        };
        let source_layout = 5;
        let family = 2;
        for discard in [FAMILY_IGNORED, family] {
            let first = plan(transfer, source_layout, family, false, discard);
            assert_eq!(first.transfer, transfer);
            assert_eq!(first.before[0].image, 10);
            assert_eq!(first.before[0].new_layout, TRANSFER_SRC);
            let target = first.before[1];
            assert_eq!(target.image, 11);
            assert_eq!(target.old_layout, UNDEFINED);
            assert_eq!(target.new_layout, TRANSFER_DST);
            assert_eq!(
                (target.source_family, target.target_family),
                (discard, discard)
            );
            assert_eq!(
                (target.source_access, target.target_access),
                (0, MEMORY_ACCESS)
            );

            let next = plan(transfer, source_layout, family, true, discard);
            assert_eq!(next.before[1].old_layout, GENERAL);
            assert_eq!(
                (next.before[1].source_family, next.before[1].target_family),
                (FAMILY_EXTERNAL, family)
            );
            assert_eq!(next.before[1].source_access, 0);
            assert_eq!(first.after, next.after);
            assert_eq!(next.after[0].new_layout, source_layout);
            let release = next.after[1];
            assert_eq!(release.new_layout, GENERAL);
            assert_eq!(
                (release.source_family, release.target_family),
                (family, FAMILY_EXTERNAL)
            );
            assert_eq!(release.target_access, 0);
        }
    }
}
