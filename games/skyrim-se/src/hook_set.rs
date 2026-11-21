// SPDX-License-Identifier: MIT
//! MinHook bookkeeping shared by the Skyrim hook transactions. The caller chooses the
//! order and decides when the native continuations can be removed.

#[derive(Default)]
pub(crate) struct HookSet<const N: usize> {
    created: u32,
    enabled: u32,
}

impl<const N: usize> HookSet<N> {
    pub(crate) const fn new() -> Self {
        Self {
            created: 0,
            enabled: 0,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.created == 0
    }

    pub(crate) fn is_disabled(&self) -> bool {
        self.enabled == 0
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.created == (1 << N) - 1
    }

    pub(crate) fn first_unavailable(&self) -> Option<usize> {
        (0..N).find(|&index| self.created & !self.enabled & (1 << index) == 0)
    }

    pub(crate) fn create(&mut self, mut create: impl FnMut(usize) -> bool) -> bool {
        for index in 0..N {
            if !create(index) {
                return false;
            }
            self.created |= 1 << index;
        }
        true
    }

    pub(crate) fn remove(
        &mut self,
        order: impl IntoIterator<Item = usize>,
        mut remove: impl FnMut(usize) -> bool,
    ) {
        for index in order {
            let bit = 1 << index;
            if self.created & !self.enabled & bit != 0 && remove(index) {
                self.created &= !bit;
            }
        }
    }

    pub(crate) fn enable(
        &mut self,
        mut enable: impl FnMut(usize) -> i32,
    ) -> Result<(), (usize, i32)> {
        for index in 0..N {
            let status = enable(index);
            if status != 0 {
                return Err((index, status));
            }
            self.enabled |= 1 << index;
        }
        Ok(())
    }

    pub(crate) fn disable(
        &mut self,
        order: impl IntoIterator<Item = usize>,
        mut disable: impl FnMut(usize) -> bool,
    ) -> bool {
        for index in order {
            let bit = 1 << index;
            if self.enabled & bit != 0 && disable(index) {
                self.enabled &= !bit;
            }
        }
        self.is_disabled()
    }
}
