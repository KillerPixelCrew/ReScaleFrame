// SPDX-License-Identifier: MIT
//! The Unity package's D3D12 SR views (integrations/unity/native/package.cpp's render callback):
//! one Runtime SR context per Unity view key with the extents and backend it was made for, and
//! the recording slots bound to the command lists the renderer bridge records with. Every
//! bridge (D3D12, D3D11 interop and Vulkan interop) records SR on a ring of three D3D12 lists and
//! retires a list's previous submission before recording it again, which is what
//! rsf_sr12_evaluate's slot contract (0..2) needs. package_state.rs sequences creation,
//! evaluation and release against the stored settings through package.cpp's typed leaves; this
//! module holds the table and its rules, and never touches a context itself.
use crate::package_abi::rsf_sr12;
use std::ffi::c_void;
use std::ptr::null_mut;

/// rsf_sr12_evaluate's recording slots.
const SLOTS: usize = 3;

#[derive(Clone, Copy)]
pub(crate) struct View {
    /// Null until a context is created.
    pub(crate) sr: *mut rsf_sr12,
    output: (u32, u32),
    /// The context's planned input extents.
    pub(crate) render: (u32, u32),
    /// The backend the context was made with; zero is the spatial or native path.
    pub(crate) backend: u32,
    lists: [*mut c_void; SLOTS],
}

impl View {
    pub(crate) const EMPTY: Self = Self {
        sr: null_mut(),
        output: (0, 0),
        render: (0, 0),
        backend: 0,
        lists: [null_mut(); SLOTS],
    };

    /// A context made for other output extents no longer fits. A backend-zero context also
    /// takes the engine's render extents as they are, so it no longer fits other ones either.
    pub(crate) fn stale(&self, output: (u32, u32), render: (u32, u32)) -> bool {
        !self.sr.is_null()
            && (self.output != output || (self.backend == 0 && self.render != render))
    }

    /// A new context's identity. Its slots start unbound.
    pub(crate) fn created(sr: *mut rsf_sr12, backend: u32, output: (u32, u32)) -> Self {
        Self {
            sr,
            output,
            backend,
            ..Self::EMPTY
        }
    }

    /// The slot already bound to this command list, else the first free one, now bound to it.
    /// None once three other lists hold every slot. commands is never null.
    pub(crate) fn slot(&mut self, commands: *mut c_void) -> Option<u32> {
        let index = self
            .lists
            .iter()
            .position(|&list| list == commands)
            .or_else(|| {
                let free = self.lists.iter().position(|list| list.is_null())?;
                self.lists[free] = commands;
                Some(free)
            })?;
        Some(index as u32)
    }
}

/// The views by Unity view key, in first-use order. A pass's key gets an empty view on first
/// use, as std::unordered_map::operator[] gave it; views leave the table only all together.
pub(crate) struct Views(Vec<(u64, View)>);

// SAFETY: the handles are Runtime SR contexts and command-list identities that package.cpp's
// leaves use under the view lock; nothing here dereferences them.
unsafe impl Send for Views {}

impl Views {
    pub(crate) const fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn entry(&mut self, key: u64) -> &mut View {
        let index = match self.0.iter().position(|(k, _)| *k == key) {
            Some(index) => index,
            None => {
                self.0.push((key, View::EMPTY));
                self.0.len() - 1
            }
        };
        &mut self.0[index].1
    }

    /// Every view's context (null for a view that has none), leaving the table empty.
    pub(crate) fn take(&mut self) -> impl Iterator<Item = *mut rsf_sr12> {
        std::mem::take(&mut self.0)
            .into_iter()
            .map(|(_, view)| view.sr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::without_provenance_mut;

    fn handle<T>(address: usize) -> *mut T {
        without_provenance_mut(address)
    }

    #[test]
    fn slots_bind_the_first_three_lists() {
        let mut view = View::created(handle(1), 3, (8, 8));
        assert_eq!(view.slot(handle(0x10)), Some(0));
        assert_eq!(view.slot(handle(0x20)), Some(1));
        assert_eq!(view.slot(handle(0x10)), Some(0));
        assert_eq!(view.slot(handle(0x30)), Some(2));
        assert_eq!(view.slot(handle(0x40)), None);
        assert_eq!(view.slot(handle(0x30)), Some(2));
        // A replacement context starts with every slot free.
        view = View::created(handle(2), 3, (8, 8));
        assert_eq!(view.slot(handle(0x40)), Some(0));
    }

    #[test]
    fn only_a_context_goes_stale() {
        assert!(!View::EMPTY.stale((1, 1), (1, 1)));
        let mut temporal = View::created(handle(1), 3, (1920, 1080));
        temporal.render = (1280, 720);
        assert!(!temporal.stale((1920, 1080), (960, 540)));
        assert!(temporal.stale((1920, 1081), (1280, 720)));
        let mut native = View::created(handle(1), 0, (1920, 1080));
        native.render = (1280, 720);
        assert!(!native.stale((1920, 1080), (1280, 720)));
        assert!(native.stale((1920, 1080), (1280, 721)));
    }

    #[test]
    fn views_keep_their_keys_until_taken() {
        let mut views = Views::new();
        views.entry(7).sr = handle(0x70);
        views.entry(9);
        assert_eq!(views.entry(7).sr, handle(0x70));
        assert_eq!(
            views.take().collect::<Vec<_>>(),
            [handle::<rsf_sr12>(0x70), null_mut()]
        );
        assert!(views.entry(7).sr.is_null());
    }
}
