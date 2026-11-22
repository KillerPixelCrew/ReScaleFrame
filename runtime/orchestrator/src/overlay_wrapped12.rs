// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats};
use crate::overlay_native12_abi::{
    rsf_overlay11on12_device, rsf_overlay11on12_platform as RawPlatform, rsf_overlay11on12_target,
    rsf_overlay12_log,
};
use crate::sr_session::has_capacity;
use std::ffi::{CStr, c_void};
use std::ptr;
use std::sync::Mutex;
#[cfg(not(test))]
pub(super) mod native;

// Production constructs this table with required pointers. Only the legacy
// C entry points admit a nullable table.
#[derive(Clone, Copy)]
struct Platform {
    create: unsafe extern "C" fn(
        device: *mut c_void,
        queue: *mut c_void,
        out: *mut rsf_overlay11on12_device,
    ) -> i32,
    window: unsafe extern "C" fn(chain: *mut c_void, window: *mut *mut c_void) -> u32,
    start: unsafe extern "C" fn(
        device: *mut c_void,
        window: *mut c_void,
        log: rsf_overlay12_log,
        user: *mut c_void,
    ) -> i32,
    admit: unsafe extern "C" fn(show_hud: u32, visible: *mut u32, hint: *mut f32) -> i32,
    target: unsafe extern "C" fn(
        device: *mut c_void,
        interop: *mut c_void,
        chain: *mut c_void,
        out: *mut rsf_overlay11on12_target,
    ) -> u32,
    acquire: unsafe extern "C" fn(interop: *mut c_void, wrapped: *mut c_void),
    draw: unsafe extern "C" fn(
        context: *mut c_void,
        view: *mut c_void,
        width: u32,
        height: u32,
        stats: *const rsf_overlay_stats,
        intent: *mut rsf_overlay_intent,
        visible: u32,
        hint: f32,
    ) -> i32,
    release: unsafe extern "C" fn(interop: *mut c_void, wrapped: *mut c_void),
    flush: unsafe extern "C" fn(context: *mut c_void),
    stop: unsafe extern "C" fn(device: *mut c_void),
}

impl Platform {
    fn admit(raw: &RawPlatform) -> Option<Self> {
        Some(Self {
            create: raw.create?,
            window: raw.window?,
            start: raw.start?,
            admit: raw.admit?,
            target: raw.target?,
            acquire: raw.acquire?,
            draw: raw.draw?,
            release: raw.release?,
            flush: raw.flush?,
            stop: raw.stop?,
        })
    }
}

const NOT_SUPPORTED: i32 = -7;

type Handle = *mut c_void;

struct Binding {
    interop: ComRef,
    context: ComRef,
    device: ComRef,
    owner: ComRef,
    queue: ComRef,
    started: bool,
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
    view: Option<ComRef>,
    resource: Option<ComRef>,
    _backbuffer: ComRef,
}

impl Drop for Wrapped<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.platform.release)(
                self.binding.interop.as_ptr(),
                self.resource.as_ref().unwrap().as_ptr(),
            );
            // Release D3D11 references first. Flush also retires their deferred
            // destruction, so they do not hold a backbuffer across ResizeBuffers.
            self.view = None;
            self.resource = None;
            (self.platform.flush)(self.binding.context.as_ptr());
        }
    }
}

struct Frame<'a> {
    device: Handle,
    queue: Handle,
    chain: Handle,
    input_window: Handle,
    stats: &'a rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: rsf_overlay12_log,
    user: Handle,
}

impl Frame<'_> {
    fn log(&self, message: &CStr) {
        unsafe {
            crate::graphics_d3d11::rsf_call_native_log(self.logger, self.user, message.as_ptr())
        };
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
            let result = unsafe { (platform.create)(frame.device, frame.queue, &mut native) };
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
                started: false,
            });
        }
        let binding = self.binding.as_mut().unwrap();
        if !binding.matches(frame.device, frame.queue) {
            return 0;
        }
        if !binding.started {
            let mut window = frame.input_window;
            if window.is_null() && unsafe { (platform.window)(frame.chain, &mut window) } == 0 {
                return 0;
            }
            if unsafe {
                (platform.start)(binding.device.as_ptr(), window, frame.logger, frame.user)
            } == 0
            {
                return self.refuse(
                    frame,
                    c"Shared overlay did not start; it will not be retried.",
                );
            }
            binding.started = true;
            frame.log(c"Shared overlay attached to D3D12 queue and window; Insert opens presets.");
        }
        let (mut visible, mut hint) = (0, 0.0);
        if unsafe { (platform.admit)(frame.stats.show_performance_hud, &mut visible, &mut hint) }
            == 0
        {
            return 0;
        }
        let mut native: rsf_overlay11on12_target = unsafe { std::mem::zeroed() };
        if unsafe {
            (platform.target)(
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
        unsafe { (platform.acquire)(binding.interop.as_ptr(), resource.as_ptr()) };
        let wrapped = Wrapped {
            platform,
            binding,
            view: Some(view),
            resource: Some(resource),
            _backbuffer: backbuffer,
        };
        unsafe {
            (platform.draw)(
                binding.context.as_ptr(),
                wrapped.view.as_ref().unwrap().as_ptr(),
                native.width,
                native.height,
                frame.stats,
                frame.intent,
                visible,
                hint,
            )
        }
    }

    fn stop(&mut self, platform: &Platform) {
        if let Some(binding) = &self.binding
            && binding.started
        {
            unsafe { (platform.stop)(binding.device.as_ptr()) };
        }
        *self = Self::default();
    }
}

unsafe fn render_frame(
    platform: &Platform,
    device: Handle,
    queue: Handle,
    chain: Handle,
    input_window: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: rsf_overlay12_log,
    user: Handle,
) -> i32 {
    if device.is_null()
        || queue.is_null()
        || chain.is_null()
        || !unsafe { has_capacity(stats) }
        || (!intent.is_null() && !unsafe { has_capacity(intent) })
    {
        return 0;
    }
    let Ok(mut owner) = OWNER.try_lock() else {
        return 0;
    };
    let drawn = owner.render(
        platform,
        &Frame {
            device,
            queue,
            chain,
            input_window,
            stats: unsafe { &*stats },
            intent,
            logger,
            user,
        },
    );
    if !input_window.is_null() && owner.failed {
        -1
    } else {
        drawn
    }
}

fn stop(platform: &Platform) {
    OWNER.lock().unwrap().stop(platform);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay11on12_frame(
    raw: &RawPlatform,
    device: Handle,
    queue: Handle,
    chain: Handle,
    input_window: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: rsf_overlay12_log,
    user: Handle,
) -> i32 {
    let Some(platform) = Platform::admit(raw) else {
        return 0;
    };
    unsafe {
        render_frame(
            &platform,
            device,
            queue,
            chain,
            input_window,
            stats,
            intent,
            logger,
            user,
        )
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay11on12_stop(raw: &RawPlatform) {
    if let Some(platform) = Platform::admit(raw) {
        stop(&platform);
    }
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
    unsafe extern "C" fn admit(_: u32, visible: *mut u32, hint: *mut f32) -> i32 {
        call("admit");
        unsafe {
            visible.write(1);
            hint.write(0.25);
        }
        1
    }
    unsafe extern "C" fn deny(_: u32, _: *mut u32, _: *mut f32) -> i32 {
        call("deny");
        0
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
        visible: u32,
        hint: f32,
    ) -> i32 {
        assert_eq!((visible, hint), (1, 0.25));
        call("draw");
        0
    }
    unsafe extern "C" fn release(_: Handle, _: Handle) {
        call("release");
    }
    unsafe extern "C" fn flush(_: Handle) {
        call("flush");
    }
    unsafe extern "C" fn stop(_: Handle) {
        call("stop");
    }

    #[test]
    fn refused_draw_releases_the_backbuffer_and_foreign_queue_cannot_wrap() {
        let platform = Platform {
            create,
            window,
            start,
            admit,
            target,
            acquire,
            draw,
            release,
            flush,
            stop,
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
            input_window: ptr::null_mut(),
            stats: &stats,
            intent: ptr::null_mut(),
            logger: None,
            user: ptr::null_mut(),
        };
        let mut owner = State::default();
        owner.stop(&platform);
        assert_eq!(owner.render(&platform, &frame), 0);
        let hidden = Platform {
            admit: deny,
            ..platform
        };
        assert_eq!(owner.render(&hidden, &frame), 0);
        frame.queue = device;
        assert_eq!(owner.render(&platform, &frame), 0);
        owner.stop(&platform);
        assert!(owner.binding.is_none());
        CALLS.with_borrow(|calls| {
            assert_eq!(
                calls,
                &[
                    "create", "start", "admit", "target", "acquire", "draw", "release", "flush",
                    "deny", "stop"
                ]
            )
        });
    }
}
