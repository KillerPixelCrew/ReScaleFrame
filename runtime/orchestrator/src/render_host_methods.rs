// SPDX-License-Identifier: MIT

// Production calls the Rust owners and Windows operations directly. Host fixtures
// substitute recording callbacks without adding test slots to the native tables.
macro_rules! impl_host_methods {
    ($(fn $name:ident($($arg:ident: $ty:ty),*) $(-> $result:ty)? { $native:expr })*) => {
        impl Platform {
            $(
                unsafe fn $name(&self, $($arg: $ty),*) $(-> $result)? {
                    #[cfg(not(test))]
                    { unsafe { $native } }
                    #[cfg(test)]
                    { unsafe { (self.$name)($($arg),*) } }
                }
            )*
        }
    };
}

pub(super) use impl_host_methods;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Adapter {
    pub(super) vendor: u32,
    pub(super) device: u32,
    pub(super) software: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ResourceDesc {
    pub(super) width: u64,
    pub(super) height: u32,
    pub(super) format: u32,
    pub(super) samples: u32,
}
