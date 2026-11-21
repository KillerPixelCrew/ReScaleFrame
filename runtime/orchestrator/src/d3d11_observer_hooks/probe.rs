// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use std::ffi::c_void;
use std::ptr::null_mut;

type Handle = *mut c_void;
type Table = *mut Handle;

const HARDWARE: u32 = 1;
const WARP: u32 = 5;

#[repr(C)]
struct Description {
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    buffer_count: u32,
    sample_count: u32,
    feature_level: u32,
}

const DESCRIPTION: Description = Description {
    width: 8,
    height: 8,
    format: 28,  // DXGI_FORMAT_R8G8B8A8_UNORM
    usage: 0x20, // DXGI_USAGE_RENDER_TARGET_OUTPUT
    buffer_count: 1,
    sample_count: 1,
    feature_level: 0xb000, // D3D_FEATURE_LEVEL_11_0
};

#[repr(C)]
#[derive(Default)]
struct Attempt {
    context: Handle,
    chain: Handle,
    device: Handle,
    result: i32,
    returned: u32,
}

#[repr(C)]
struct Tables {
    device: Table,
    chain: Table,
    chain1: Table,
}

#[repr(C)]
struct Platform {
    window: unsafe extern "C" fn(u32, u32) -> Handle,
    destroy_window: unsafe extern "C" fn(Handle),
    create: unsafe extern "C" fn(Handle, u32, &Description, &mut Attempt),
    query_chain1: unsafe extern "C" fn(Handle, &mut Handle, &mut i32) -> u32,
    vtable: unsafe extern "C" fn(Handle) -> Table,
}

const _: () = {
    assert!(size_of::<Description>() == 28);
    assert!(size_of::<Attempt>() == 32);
    assert!(std::mem::offset_of!(Attempt, result) == 24);
    assert!(size_of::<Tables>() == 3 * size_of::<Handle>());
    assert!(size_of::<Platform>() == 5 * size_of::<Handle>());
};

// The context, chain and device retire in the order used by the native probe.
struct Objects {
    _context: Option<ComRef>,
    chain: Option<ComRef>,
    device: Option<ComRef>,
}

enum CreateFailure {
    DriverRefused,
    Exception,
}

impl Objects {
    fn create(platform: &Platform, window: Handle, driver: u32) -> Result<Self, CreateFailure> {
        let mut attempt = Attempt::default();
        unsafe { (platform.create)(window, driver, &DESCRIPTION, &mut attempt) };
        let objects = unsafe {
            Self {
                _context: ComRef::from_owned(attempt.context),
                chain: ComRef::from_owned(attempt.chain),
                device: ComRef::from_owned(attempt.device),
            }
        };
        if attempt.returned == 0 {
            return Err(CreateFailure::Exception);
        }
        if attempt.result < 0 {
            return Err(CreateFailure::DriverRefused);
        }
        Ok(objects)
    }
}

fn probe(platform: &Platform, window: Handle) -> Option<Tables> {
    let objects = match Objects::create(platform, window, HARDWARE) {
        Ok(objects) => objects,
        Err(CreateFailure::DriverRefused) => Objects::create(platform, window, WARP).ok()?,
        Err(CreateFailure::Exception) => return None,
    };
    let device_object = objects.device.as_ref()?.as_ptr();
    let chain_object = objects.chain.as_ref()?.as_ptr();
    let device = unsafe { (platform.vtable)(device_object) };
    let chain = unsafe { (platform.vtable)(chain_object) };
    if device.is_null() || chain.is_null() {
        return None;
    }
    let (mut extended, mut result) = (null_mut(), 0);
    let returned = unsafe { (platform.query_chain1)(chain_object, &mut extended, &mut result) };
    let extended = unsafe { ComRef::from_owned(extended) };
    if returned == 0 {
        return None;
    }
    let chain1 = extended
        .filter(|_| result >= 0)
        .map_or(null_mut(), |extended| unsafe {
            (platform.vtable)(extended.as_ptr())
        });
    Some(Tables {
        device,
        chain,
        chain1,
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_probe(platform: &Platform, tables: &mut Tables) -> u32 {
    let window = unsafe { (platform.window)(DESCRIPTION.width, DESCRIPTION.height) };
    if window.is_null() {
        return 0;
    }
    let result = probe(platform, window);
    unsafe { (platform.destroy_window)(window) };
    if let Some(result) = result {
        *tables = result;
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{Fake, add_ref, fake, raw};
    use std::cell::RefCell;
    use std::ptr::without_provenance_mut;

    struct Fixture {
        context: Box<Fake>,
        chain: Box<Fake>,
        device: Box<Fake>,
        extended: Box<Fake>,
        drivers: Vec<u32>,
        throw_in_hardware: bool,
        windows_destroyed: u32,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                context: fake(),
                chain: fake(),
                device: fake(),
                extended: fake(),
                drivers: Vec::new(),
                throw_in_hardware: false,
                windows_destroyed: 0,
            }
        }

        fn refs(&self) -> [u32; 4] {
            [
                self.context.refs.get(),
                self.chain.refs.get(),
                self.device.refs.get(),
                self.extended.refs.get(),
            ]
        }
    }

    thread_local! {
        static FIXTURE: RefCell<Fixture> = RefCell::new(Fixture::new());
    }

    fn owned(fake: &Fake) -> Handle {
        let object = raw(fake);
        unsafe { add_ref(object.cast()) };
        object
    }

    unsafe extern "C" fn window(width: u32, height: u32) -> Handle {
        assert_eq!((width, height), (8, 8));
        without_provenance_mut(1)
    }

    unsafe extern "C" fn destroy_window(window: Handle) {
        assert_eq!(window.addr(), 1);
        FIXTURE.with_borrow_mut(|fixture| {
            assert_eq!(fixture.refs(), [1; 4]);
            fixture.windows_destroyed += 1;
        });
    }

    unsafe extern "C" fn create(
        window: Handle,
        driver: u32,
        desc: &Description,
        out: &mut Attempt,
    ) {
        assert_eq!(window.addr(), 1);
        assert_eq!(
            (desc.width, desc.height, desc.format, desc.usage),
            (8, 8, 28, 0x20)
        );
        assert_eq!(
            (desc.buffer_count, desc.sample_count, desc.feature_level),
            (1, 1, 0xb000)
        );
        FIXTURE.with_borrow_mut(|fixture| {
            // The WARP retry must see every hardware reference released.
            assert_eq!(fixture.refs(), [1; 4]);
            fixture.drivers.push(driver);
            *out = Attempt {
                context: owned(&fixture.context),
                chain: owned(&fixture.chain),
                device: if driver == WARP {
                    owned(&fixture.device)
                } else {
                    null_mut()
                },
                result: if driver == WARP { 0 } else { -1 },
                returned: u32::from(driver == WARP || !fixture.throw_in_hardware),
            };
        });
    }

    unsafe extern "C" fn query_chain1(chain: Handle, out: &mut Handle, result: &mut i32) -> u32 {
        FIXTURE.with_borrow(|fixture| {
            assert_eq!(chain, raw(&fixture.chain));
            *out = owned(&fixture.extended);
        });
        *result = 0;
        1
    }

    unsafe extern "C" fn vtable(object: Handle) -> Table {
        object.cast()
    }

    static PLATFORM: Platform = Platform {
        window,
        destroy_window,
        create,
        query_chain1,
        vtable,
    };

    #[test]
    fn probe_releases_partial_hardware_results_before_retry_or_exception_return() {
        for interrupted in [false, true] {
            FIXTURE.with_borrow_mut(|fixture| {
                *fixture = Fixture::new();
                fixture.throw_in_hardware = interrupted;
            });
            let mut tables = Tables {
                device: null_mut(),
                chain: null_mut(),
                chain1: null_mut(),
            };
            assert_eq!(
                rsf_core_observer11_probe(&PLATFORM, &mut tables),
                u32::from(!interrupted)
            );
            FIXTURE.with_borrow(|fixture| {
                assert_eq!(fixture.windows_destroyed, 1);
                if interrupted {
                    assert_eq!(fixture.drivers.as_slice(), &[HARDWARE]);
                    assert!(
                        tables.device.is_null()
                            && tables.chain.is_null()
                            && tables.chain1.is_null()
                    );
                } else {
                    assert_eq!(fixture.drivers.as_slice(), &[HARDWARE, WARP]);
                    assert_eq!(tables.device, raw(&fixture.device).cast());
                    assert_eq!(tables.chain, raw(&fixture.chain).cast());
                    assert_eq!(tables.chain1, raw(&fixture.extended).cast());
                }
            });
        }
    }
}
