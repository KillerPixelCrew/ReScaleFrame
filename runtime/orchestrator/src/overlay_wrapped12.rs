// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats};
use crate::overlay_native12_abi::{
    rsf_overlay11on12_device, rsf_overlay11on12_platform as Platform, rsf_overlay11on12_target,
    rsf_overlay12_log,
};
use crate::sr_session::has_capacity;
use std::ffi::{CStr, c_void};
use std::ptr;
use std::sync::Mutex;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

const STARTUP_HINT_MS: u64 = 8000;
const NOT_SUPPORTED: i32 = -7;

type Handle = *mut c_void;

struct Binding {
    interop: ComRef,
    context: ComRef,
    device: ComRef,
    owner: ComRef,
    queue: ComRef,
    started: Option<u64>,
}

// These COM handles move between calls. The owner serializes immediate-context
// work and teardown, and the D3D12 queue remains the one supplied at creation.
unsafe impl Send for Binding {}

impl Binding {
    fn matches(&self, device: Handle, queue: Handle) -> bool {
        self.owner.as_ptr() == device && self.queue.as_ptr() == queue
    }
}

struct Wrapped<'a> {
    platform: &'a Platform,
    binding: &'a Binding,
    view: ComRef,
    resource: ComRef,
    _backbuffer: ComRef,
}

impl Drop for Wrapped<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.platform.release.unwrap())(self.binding.interop.as_ptr(), self.resource.as_ptr());
            (self.platform.flush.unwrap())(self.binding.context.as_ptr());
        }
    }
}

struct Frame<'a> {
    device: Handle,
    queue: Handle,
    chain: Handle,
    stats: &'a rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: rsf_overlay12_log,
    user: Handle,
}

impl Frame<'_> {
    fn log(&self, message: &CStr) {
        if let Some(logger) = self.logger {
            unsafe { logger(self.user, message.as_ptr()) };
        }
    }
}

#[derive(Default)]
struct State {
    binding: Option<Binding>,
    failed: bool,
}

static OWNER: Mutex<State> = Mutex::new(State {
    binding: None,
    failed: false,
});

impl State {
    fn refuse(&mut self, frame: &Frame<'_>, message: &CStr) -> i32 {
        self.failed = true;
        frame.log(message);
        0
    }

    fn render(&mut self, platform: &Platform, frame: &Frame<'_>) -> i32 {
        if self.failed {
            return 0;
        }
        if self.binding.is_none() {
            let mut native: rsf_overlay11on12_device = unsafe { std::mem::zeroed() };
            let result =
                unsafe { (platform.create.unwrap())(frame.device, frame.queue, &mut native) };
            if result == NOT_SUPPORTED {
                return self.refuse(
                    frame,
                    c"Shared overlay unavailable: system D3D11-on-12 entry point not available.",
                );
            }
            let (0, Some(device), Some(context), Some(interop)) = (
                result,
                unsafe { ComRef::from_owned(native.device) },
                unsafe { ComRef::from_owned(native.context) },
                unsafe { ComRef::from_owned(native.interop) },
            ) else {
                return self.refuse(
                    frame,
                    c"Shared overlay unavailable: the D3D11-on-12 device could not be created.",
                );
            };
            self.binding = Some(Binding {
                interop,
                context,
                device,
                owner: unsafe { ComRef::retain(frame.device) },
                queue: unsafe { ComRef::retain(frame.queue) },
                started: None,
            });
        }
        let binding = self.binding.as_mut().unwrap();
        if !binding.matches(frame.device, frame.queue) {
            return 0;
        }
        let started = if let Some(started) = binding.started {
            started
        } else {
            let mut window = ptr::null_mut();
            if unsafe { (platform.window.unwrap())(frame.chain, &mut window) } == 0 {
                return 0;
            }
            if unsafe {
                (platform.start.unwrap())(binding.device.as_ptr(), window, frame.logger, frame.user)
            } == 0
            {
                return self.refuse(
                    frame,
                    c"Shared overlay did not start; it will not be retried.",
                );
            }
            let started = unsafe { GetTickCount64() };
            binding.started = Some(started);
            frame.log(c"Shared overlay attached to D3D12 queue and window; Insert opens presets.");
            started
        };

        let now = unsafe { GetTickCount64() };
        if frame.stats.show_performance_hud == 0
            && now.saturating_sub(started) >= STARTUP_HINT_MS
            && unsafe { (platform.visible.unwrap())() } == 0
        {
            return 0;
        }
        let mut native: rsf_overlay11on12_target = unsafe { std::mem::zeroed() };
        if unsafe {
            (platform.target.unwrap())(
                binding.device.as_ptr(),
                binding.interop.as_ptr(),
                frame.chain,
                &mut native,
            )
        } == 0
        {
            return 0;
        }
        let (Some(backbuffer), Some(resource), Some(view)) = (
            unsafe { ComRef::from_owned(native.backbuffer) },
            unsafe { ComRef::from_owned(native.wrapped) },
            unsafe { ComRef::from_owned(native.view) },
        ) else {
            return 0;
        };
        unsafe { (platform.acquire.unwrap())(binding.interop.as_ptr(), resource.as_ptr()) };
        let wrapped = Wrapped {
            platform,
            binding,
            view,
            resource,
            _backbuffer: backbuffer,
        };
        unsafe {
            (platform.draw.unwrap())(
                binding.context.as_ptr(),
                wrapped.view.as_ptr(),
                native.width,
                native.height,
                frame.stats,
                frame.intent,
            )
        }
    }

    fn stop(&mut self, platform: &Platform) {
        unsafe { (platform.stop.unwrap())() };
        *self = Self::default();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay11on12_frame(
    platform: &Platform,
    device: Handle,
    queue: Handle,
    chain: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: rsf_overlay12_log,
    user: Handle,
) -> i32 {
    if device.is_null() || queue.is_null() || chain.is_null() || !unsafe { has_capacity(stats) } {
        return 0;
    }
    let Ok(mut owner) = OWNER.try_lock() else {
        return 0;
    };
    owner.render(
        platform,
        &Frame {
            device,
            queue,
            chain,
            stats: unsafe { &*stats },
            intent,
            logger,
            user,
        },
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay11on12_stop(platform: &Platform) {
    OWNER.lock().unwrap().stop(platform);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[repr(C)]
    struct Vtable {
        query: unsafe extern "system" fn(Handle, Handle, *mut Handle) -> i32,
        add_ref: unsafe extern "system" fn(Handle) -> u32,
        release: unsafe extern "system" fn(Handle) -> u32,
    }

    unsafe extern "system" fn query(_: Handle, _: Handle, _: *mut Handle) -> i32 {
        -1
    }
    unsafe extern "system" fn reference(_: Handle) -> u32 {
        1
    }
    static VTABLE: Vtable = Vtable {
        query,
        add_ref: reference,
        release: reference,
    };

    thread_local! {
        static CALLS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn call(name: &'static str) {
        CALLS.with_borrow_mut(|calls| calls.push(name));
    }

    unsafe extern "C" fn create(
        device: Handle,
        _: Handle,
        out: *mut rsf_overlay11on12_device,
    ) -> i32 {
        call("create");
        unsafe {
            out.write(rsf_overlay11on12_device {
                device,
                context: device,
                interop: device,
            })
        };
        0
    }
    unsafe extern "C" fn window(chain: Handle, out: *mut Handle) -> u32 {
        unsafe { out.write(chain) };
        1
    }
    unsafe extern "C" fn start(_: Handle, _: Handle, _: rsf_overlay12_log, _: Handle) -> i32 {
        call("start");
        1
    }
    unsafe extern "C" fn visible() -> u32 {
        1
    }
    unsafe extern "C" fn target(
        device: Handle,
        _: Handle,
        _: Handle,
        out: *mut rsf_overlay11on12_target,
    ) -> u32 {
        call("target");
        unsafe {
            out.write(rsf_overlay11on12_target {
                backbuffer: device,
                wrapped: device,
                view: device,
                width: 64,
                height: 32,
            })
        };
        1
    }
    unsafe extern "C" fn acquire(_: Handle, _: Handle) {
        call("acquire");
    }
    unsafe extern "C" fn draw(
        _: Handle,
        _: Handle,
        _: u32,
        _: u32,
        _: *const rsf_overlay_stats,
        _: *mut rsf_overlay_intent,
    ) -> i32 {
        call("draw");
        0
    }
    unsafe extern "C" fn release(_: Handle, _: Handle) {
        call("release");
    }
    unsafe extern "C" fn flush(_: Handle) {
        call("flush");
    }
    unsafe extern "C" fn stop() {
        call("stop");
    }

    #[test]
    fn refused_draw_releases_the_backbuffer_and_foreign_queue_cannot_wrap() {
        let platform = Platform {
            create: Some(create),
            window: Some(window),
            start: Some(start),
            visible: Some(visible),
            target: Some(target),
            acquire: Some(acquire),
            draw: Some(draw),
            release: Some(release),
            flush: Some(flush),
            stop: Some(stop),
        };
        let mut device_vtable = &VTABLE;
        let mut queue_vtable = &VTABLE;
        let device = (&raw mut device_vtable).cast();
        let queue = (&raw mut queue_vtable).cast();
        let stats = rsf_overlay_stats::default();
        let mut frame = Frame {
            device,
            queue,
            chain: device,
            stats: &stats,
            intent: ptr::null_mut(),
            logger: None,
            user: ptr::null_mut(),
        };
        let mut owner = State::default();
        assert_eq!(owner.render(&platform, &frame), 0);
        frame.queue = device;
        assert_eq!(owner.render(&platform, &frame), 0);
        owner.stop(&platform);
        assert!(owner.binding.is_none());
        CALLS.with_borrow(|calls| {
            assert_eq!(
                calls,
                &[
                    "create", "start", "target", "acquire", "draw", "release", "flush", "stop"
                ]
            )
        });
    }
}
