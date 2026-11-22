// SPDX-License-Identifier: MIT

use crate::vulkan_fg_copy::TRANSFER_SRC;
pub(super) use crate::vulkan_fg_copy::{
    ALL_COMMANDS, Barrier, FAMILY_EXTERNAL, FAMILY_IGNORED, UNDEFINED,
};

pub(super) const GENERAL: u32 = 1;
pub(super) const TRANSFER_DST: u32 = 7;

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
    use crate::vulkan_fg_copy::MEMORY_ACCESS;

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
