// SPDX-License-Identifier: MIT
//! The platform fakes the callback and render host tests share word for word. Each host's tests
//! keep their own thread-local `WORLD` of their own `World`; the expansion reads it through the
//! names in scope there (`WORLD`, `World`, `Handle`, `Pass`).

macro_rules! host_test_fakes {
    () => {
        fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
            WORLD.with(|world| f(&mut world.borrow_mut()))
        }
        fn event(text: String) {
            with(|world| world.events.push(text));
        }
        fn events() -> Vec<String> {
            with(|world| std::mem::take(&mut world.events))
        }

        pub(super) unsafe extern "C" fn tick() -> u64 {
            with(|world| world.now)
        }
        pub(super) extern "C" fn dlss_preset_valid(preset: u32) -> u32 {
            u32::from(with(|world| world.refused_preset != Some(preset)))
        }
        pub(super) unsafe extern "C" fn sr12_destroy(context: Handle) {
            event(format!("destroy {}", context as usize));
        }
        pub(super) unsafe extern "C" fn fg_submitted(session: u64, frame: u64) {
            event(format!("fg submitted {session} {frame}"));
        }
        pub(super) unsafe extern "C" fn fg_hudless(_: Handle, pass: *const Pass) {
            event(format!("fg hudless {}", unsafe { (*pass).source_frame_id }));
        }
        pub(super) unsafe extern "C" fn fg_window(pass: *const Pass) {
            event(format!("fg window {}", unsafe { (*pass).source_frame_id }));
        }
        pub(super) extern "C" fn present_request(backend: u32) -> i32 {
            event(format!("fg request {backend}"));
            with(|world| world.request_result)
        }
    };
}

pub(crate) use host_test_fakes;
