// SPDX-License-Identifier: MIT

/// D3D12 reserves UINT64_MAX as the device-removed completion value.
pub(crate) fn next_local_completion(current: u64) -> Option<u64> {
    current.checked_add(1).filter(|&value| value != u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_the_removed_device_sentinel() {
        assert_eq!(next_local_completion(0), Some(1));
        assert_eq!(next_local_completion(u64::MAX - 2), Some(u64::MAX - 1));
        assert_eq!(next_local_completion(u64::MAX - 1), None);
        assert_eq!(next_local_completion(u64::MAX), None);
    }
}
