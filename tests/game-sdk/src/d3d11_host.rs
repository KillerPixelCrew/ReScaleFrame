// SPDX-License-Identifier: MIT
use crate::sys::*;
use rsf_game_sys::{
    RSF_ERROR_ABI_MISMATCH, RSF_ERROR_BUSY, RSF_ERROR_INVALID_ARGUMENT, RSF_ERROR_NOT_READY, RSF_OK,
};
use std::ffi::{OsStr, c_char, c_void};
use std::mem::offset_of;
use std::ptr::{dangling_mut, null_mut};
use std::sync::Mutex;

struct Callbacks {
    cpu_calls: u32,
    logs: u32,
    session: u64,
    foreign_user: bool,
}

static CALLBACKS: Mutex<Callbacks> = Mutex::new(Callbacks {
    cpu_calls: 0,
    logs: 0,
    session: 0,
    foreign_user: false,
});

fn cookie() -> *mut c_void {
    (&raw const CALLBACKS).cast_mut().cast()
}

unsafe extern "C" fn cpu(user: *mut c_void, event: *const rsf_game_cpu_event) {
    let mut callbacks = CALLBACKS.lock().unwrap();
    callbacks.cpu_calls += 1;
    callbacks.foreign_user |= user != cookie();
    callbacks.session = unsafe { event.as_ref() }.map_or(0, |event| event.session_id);
}

unsafe extern "C" fn log(user: *mut c_void, _: *const c_char) {
    let mut callbacks = CALLBACKS.lock().unwrap();
    callbacks.logs += 1;
    callbacks.foreign_user |= user != cookie();
}

/// A size-checked call refuses a short struct and a newer ABI version. `$call` is evaluated twice,
/// against `$value` with each field changed in turn; the field is restored afterwards.
macro_rules! refuses_malformed {
    ($value:expr, $call:expr, $label:literal) => {{
        $value.struct_size -= 1;
        assert!(
            $call == RSF_ERROR_INVALID_ARGUMENT,
            concat!("Short ", $label, " refused")
        );
        $value.struct_size += 1;
        $value.abi_version += 1;
        assert!(
            $call == RSF_ERROR_ABI_MISMATCH,
            concat!($label, " ABI checked")
        );
        $value.abi_version -= 1;
    }};
}

fn check() -> i32 {
    let cookie = cookie();
    let ini = crate::wide(OsStr::new("unused.ini"));
    let section = crate::wide(OsStr::new("Test"));
    let directory = crate::wide(OsStr::new("unused"));
    let fake = dangling_mut::<c_void>();
    // The only fake native pointers below are deliberately refused before GPU access.
    unsafe {
        let mut capture_control = rsf_d3d11_capture_control {
            struct_size: size_of::<rsf_d3d11_capture_control>() as u32,
            abi_version: RSF_D3D11_CAPTURE_CONTROL_VERSION,
            operation: RSF_D3D11_CAPTURE_SEQUENCE,
            value: 1,
        };
        assert!(
            rsf_d3d11_host_capture_control(null_mut()) == RSF_ERROR_INVALID_ARGUMENT,
            "Null capture control refused"
        );
        refuses_malformed!(
            capture_control,
            rsf_d3d11_host_capture_control(&capture_control),
            "capture control"
        );
        capture_control.value = 0;
        assert!(
            rsf_d3d11_host_capture_control(&capture_control) == RSF_ERROR_INVALID_ARGUMENT,
            "empty sequence request refused"
        );
        capture_control.value = 1;
        assert!(
            rsf_d3d11_host_capture_control(&capture_control) == RSF_ERROR_NOT_READY,
            "capture_control requires live host"
        );

        let mut setup = rsf_d3d11_host_setup {
            struct_size: size_of::<rsf_d3d11_host_setup>() as u32,
            ..rsf_d3d11_host_setup::default()
        };
        setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
        setup.configuration_path = ini.as_ptr();
        setup.configuration_section = section.as_ptr();
        setup.runtime_directory = directory.as_ptr();
        setup.identity_utf8 = c"contract".as_ptr();
        setup.executable_sha256 = c"unused".as_ptr();
        setup.engine_version_utf8 = c"1".as_ptr();
        setup.project_id_utf8 = c"668392a0-ecfe-4d0c-9f58-c2838c5ca308".as_ptr();
        setup.units_to_meters = 1.0;
        setup.quality_choices = 1;
        setup.motion.struct_size = size_of_val(&setup.motion) as u32;
        setup.motion.scale_x = 1.0;
        setup.motion.scale_y = 1.0;
        let mut host = rsf_game_host_services {
            struct_size: size_of::<rsf_game_host_services>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            session_id: 91,
            log: Some(log),
            user: cookie,
            cpu_event: Some(cpu),
            ..std::mem::zeroed()
        };
        assert!(
            rsf_d3d11_host_create(null_mut(), &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Null setup refused"
        );
        setup.struct_size = offset_of!(rsf_d3d11_host_setup, depth_far_plane) as u32 - 1;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Short setup refused"
        );
        setup.struct_size = size_of_val(&setup) as u32 - 1;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Partial extension refused"
        );
        setup.struct_size = size_of_val(&setup) as u32;
        setup.abi_version += 1;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_ABI_MISMATCH,
            "Setup ABI checked"
        );
        setup.abi_version -= 1;
        refuses_malformed!(host, rsf_d3d11_host_create(&setup, &mut host), "services");
        setup.depth_far_plane = 3;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Unknown far-plane convention refused"
        );
        setup.depth_far_plane = RSF_D3D11_HOST_DEPTH_FINITE;
        setup.colour_transport_default = 2;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Unknown colour transport default refused"
        );
        setup.colour_transport_default = 0; // existing callers preserve the enabled default
        setup.units_to_meters = f32::NAN;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Nonfinite units refused"
        );
        setup.units_to_meters = 1.0;
        setup.motion.scale_x = 0.0;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Zero decode scale refused"
        );
        setup.motion.scale_x = 1.0;
        setup.quality_choices = 0;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Empty capabilities refused"
        );
        setup.quality_choices = 1;
        setup.presentation_bridge = 1;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Cold presentation requires an adapter window filter before any hooks are installed"
        );
        setup.presentation_bridge = 4;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Unknown presentation mode refused"
        );
        setup.presentation_bridge = 0;
        setup.capabilities = 8;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Unknown capability refused"
        );
        setup.capabilities = RSF_D3D11_HOST_CAP_FG_INPUTS;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "FG facts require presentation bridge"
        );
        setup.capabilities = RSF_D3D11_HOST_CAP_INPUT_TIMELINE;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Input timeline requires presentation bridge"
        );
        setup.capabilities = RSF_D3D11_HOST_CAP_RENDERED_SOURCE;
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_ERROR_INVALID_ARGUMENT,
            "Rendered source requires presentation bridge"
        );
        setup.capabilities = 0;
        let mut outer = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            source_frame_id: 42,
            ..std::mem::zeroed()
        };
        assert!(
            rsf_native_fg_scene_swap(&mut outer) == 1,
            "Scene scope saved"
        );
        let mut inner = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            source_frame_id: 43,
            ..std::mem::zeroed()
        };
        assert!(
            rsf_native_fg_scene_swap(&mut inner) == 1 && inner.source_frame_id == 42,
            "Nested scene saved exactly"
        );
        assert!(
            rsf_native_fg_scene_swap(&mut inner) == 1 && inner.source_frame_id == 43,
            "Nested scene restored exactly"
        );
        assert!(
            rsf_native_fg_scene_swap(&mut outer) == 1 && outer.source_frame_id == 42,
            "Outer scene restored exactly"
        );
        let mut bound = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            ..std::mem::zeroed()
        };
        bound.source_frame_id = 101;
        bound.session_id = 91;
        bound.submission_id = 109;
        bound.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
        bound.color_input = (&raw mut bound).cast();
        rsf_native_fg_bind_source(&bound);
        let mut anonymous = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            ..std::mem::zeroed()
        };
        anonymous.role = RSF_GAME_RENDER_SR;
        anonymous.flags = RSF_GAME_RENDER_NO_FG;
        anonymous.color_input = (&raw mut anonymous).cast();
        rsf_native_fg_scene(&anonymous);
        let mut resolved = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            ..std::mem::zeroed()
        };
        assert!(
            rsf_native_fg_scene_swap(&mut resolved) == 1
                && resolved.source_frame_id == 101
                && resolved.session_id == 91
                && resolved.submission_id == 109
                && resolved.flags == bound.flags
                && resolved.color_input == (&raw mut anonymous).cast()
                && anonymous.source_frame_id == 0
                && anonymous.flags == RSF_GAME_RENDER_NO_FG,
            "Bound source supplies only identity and FG admission to the captured copy"
        );
        anonymous.source_frame_id = 102;
        rsf_native_fg_scene(&anonymous);
        resolved = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            ..std::mem::zeroed()
        };
        assert!(
            rsf_native_fg_scene_swap(&mut resolved) == 1
                && resolved.source_frame_id == 102
                && resolved.flags == RSF_GAME_RENDER_NO_FG,
            "A source-owned pass keeps its own identity and admission"
        );
        rsf_native_fg_bind_source(null_mut());
        rsf_native_fg_scene(null_mut());
        let mut inputs = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            ..std::mem::zeroed()
        };
        inputs.role = RSF_GAME_RENDER_FG_INPUTS;
        let invalid_context = fake;
        assert!(
            rsf_native_sr_capture_fg(invalid_context, &inputs, &setup.motion)
                == RSF_ERROR_NOT_READY,
            "Unidentified FG lease refused before graphics dereference"
        );
        inputs.struct_size -= 1;
        assert!(
            rsf_native_sr_capture_fg(invalid_context, &inputs, &setup.motion)
                == RSF_ERROR_INVALID_ARGUMENT,
            "Short FG lease refused before graphics dereference"
        );
        let mut upstream = host;
        setup.struct_size = offset_of!(rsf_d3d11_host_setup, depth_far_plane) as u32;
        setup.depth_far_plane = 3; // Outside the advertised legacy record.
        assert!(
            rsf_d3d11_host_create(&setup, &mut host) == RSF_OK,
            "Graphics-free creation succeeds without SDK directories"
        );
        assert!(
            host.session_id == 91
                && host.user != cookie
                && host.process_upscale.is_some()
                && host.render_config.is_some()
                && host.render_feedback.is_some()
                && host.render_pass.is_some()
                && host.present.is_none()
                && host.submit.is_none()
                && host.process_latency.is_none(),
            "Only admitted host services published; no pacing or generation"
        );
        assert!(
            rsf_d3d11_host_create(&setup, &mut upstream) == RSF_ERROR_BUSY,
            "Second process owner refused"
        );
        assert!(
            rsf_d3d11_host_set_active(cookie, 1) == RSF_ERROR_INVALID_ARGUMENT,
            "Only the returned owner controls activation"
        );
        assert!(
            rsf_d3d11_host_set_active(host.user, 1) == RSF_OK
                && rsf_d3d11_host_set_active(host.user, 0) == RSF_OK,
            "Thread-safe activation and suspension"
        );
        let mut recovery = rsf_d3d11_reduced_frame {
            struct_size: size_of::<rsf_d3d11_reduced_frame>() as u32,
            abi_version: RSF_D3D11_REDUCED_FRAME_ABI_VERSION,
            ..rsf_d3d11_reduced_frame::default()
        };
        let unreadable = fake;
        assert!(
            rsf_d3d11_host_reduced_frame(cookie, unreadable, &mut recovery)
                == RSF_ERROR_INVALID_ARGUMENT,
            "recovery validates host before graphics"
        );
        refuses_malformed!(
            recovery,
            rsf_d3d11_host_reduced_frame(host.user, unreadable, &mut recovery),
            "recovery request"
        );
        recovery.operation = 5;
        assert!(
            rsf_d3d11_host_reduced_frame(host.user, unreadable, &mut recovery)
                == RSF_ERROR_INVALID_ARGUMENT,
            "unknown recovery operation refused"
        );
        recovery.operation = 0;
        recovery.reserved = 1;
        assert!(
            rsf_d3d11_host_reduced_frame(host.user, unreadable, &mut recovery)
                == RSF_ERROR_INVALID_ARGUMENT,
            "reserved recovery bits refused"
        );
        recovery.reserved = 0;
        recovery.owner = unreadable.cast();
        assert!(
            rsf_d3d11_host_reduced_frame(host.user, unreadable, &mut recovery)
                == RSF_ERROR_NOT_READY,
            "unconfigured recovery owner refused before resource reads"
        );
        let mut cleanup = rsf_d3d11_auxiliary_scope {
            struct_size: size_of::<rsf_d3d11_auxiliary_scope>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            session_id: 91,
            native_frame: 1,
            view_key: 1,
        };
        assert!(
            rsf_d3d11_host_restore_auxiliaries(cookie, &cleanup) == RSF_ERROR_INVALID_ARGUMENT,
            "Auxiliary cleanup verifies host user"
        );
        refuses_malformed!(
            cleanup,
            rsf_d3d11_host_restore_auxiliaries(host.user, &cleanup),
            "auxiliary cleanup scope"
        );
        assert!(
            rsf_d3d11_host_restore_auxiliaries(host.user, &cleanup) == RSF_ERROR_NOT_READY,
            "Unbound graphics owner cannot acknowledge empty cleanup"
        );
        let mut event = rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_event>() as u32,
            ..std::mem::zeroed()
        };
        event.session_id = 91;
        host.cpu_event.unwrap()(host.user, &event);
        host.log.unwrap()(host.user, c"contract".as_ptr());
        {
            let callbacks = CALLBACKS.lock().unwrap();
            assert!(
                callbacks.cpu_calls == 1
                    && callbacks.logs == 1
                    && callbacks.session == 91
                    && !callbacks.foreign_user,
                "Callbacks forwarded exactly once"
            );
        }
        let mut config = rsf_game_render_config {
            struct_size: size_of::<rsf_game_render_config>() as u32,
            ..std::mem::zeroed()
        };
        assert!(
            host.render_config.unwrap()(host.user, &mut config) == 1
                && config.enabled == 0
                && config.output_width == 0,
            "Cold config does not invent graphics dimensions"
        );
        let mut early = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            ..std::mem::zeroed()
        };
        early.role = RSF_GAME_RENDER_SR;
        early.flags = RSF_GAME_RENDER_RECONFIGURE;
        early.swapchain = fake;
        early.camera.output_width = 64;
        early.camera.output_height = 64;
        assert!(
            rsf_d3d11_host_set_active(host.user, 1) == RSF_OK,
            "Activate for early planning refusal"
        );
        assert!(
            host.process_upscale.unwrap()(host.user, fake, &early) == RSF_ERROR_INVALID_ARGUMENT,
            "Foreign planning lease refused before graphics access"
        );
        early.session_id = host.session_id;
        assert!(
            host.process_upscale.unwrap()(host.user, fake, &early) == RSF_ERROR_NOT_READY,
            "Early loading-style planning must not consume a graphics context or bind an execution owner"
        );
        assert!(
            host.render_config.unwrap()(host.user, &mut config) == 1
                && config.enabled == 0
                && config.output_width == 0,
            "Refused cold planning publishes no invented size"
        );
        assert!(
            host.process_upscale.unwrap()(host.user, null_mut(), null_mut())
                == RSF_ERROR_INVALID_ARGUMENT,
            "Null graphics lease refused"
        );
        assert!(
            rsf_d3d11_host_request_capture(null_mut()) == 0,
            "Any-thread capture request does no graphics work"
        );
        early.role = RSF_GAME_RENDER_FINAL_SCENE;
        early.session_id = 91;
        early.scene_surface = fake;
        host.render_pass.unwrap()(host.user, fake, &early, 1);
        host.render_pass.unwrap()(host.user, fake, &early, 0);
        assert!(
            host.render_config.unwrap()(host.user, &mut config) == 1 && config.enabled == 0,
            "Cold native pass facts do not touch graphics or activate generation"
        );

        let mut call = rsf_d3d11_chain_dispatch {
            struct_size: size_of::<rsf_d3d11_chain_dispatch>() as u32,
            ..std::mem::zeroed()
        };
        call.abi_version = RSF_D3D11_HOST_ABI_VERSION;
        call.receiver = fake;
        call.method = RSF_D3D11_DISPATCH_RELEASE;
        call.handled = 1;
        call.result.references = 123;
        assert!(
            rsf_d3d11_host_dispatch_chain(cookie, &mut call) == RSF_ERROR_INVALID_ARGUMENT
                && call.handled == 1,
            "Typed chain dispatch checks host before touching outputs"
        );
        refuses_malformed!(
            call,
            rsf_d3d11_host_dispatch_chain(host.user, &mut call),
            "typed dispatch"
        );
        call.method = 0;
        assert!(
            rsf_d3d11_host_dispatch_chain(host.user, &mut call) == RSF_ERROR_INVALID_ARGUMENT,
            "Unknown method cannot forward"
        );
        for method in RSF_D3D11_DISPATCH_RELEASE..=RSF_D3D11_DISPATCH_RESIZE_BUFFERS1 {
            call.method = method;
            call.handled = 1;
            call.args.resize_buffers1 = rsf_d3d11_chain_dispatch__bindgen_ty_1__bindgen_ty_6 {
                node_masks: fake.cast(),
                present_queues: fake.cast(),
                ..std::mem::zeroed()
            };
            assert!(
                rsf_d3d11_host_dispatch_chain(host.user, &mut call) == RSF_OK
                    && call.handled == 0
                    && call.result.references == 123,
                "Every unowned typed method preserves result without argument or receiver dereference"
            );
        }
        let mut dispatch = rsf_d3d11_present_dispatch {
            struct_size: size_of::<rsf_d3d11_present_dispatch>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            receiver: fake,
            sync_interval: 2,
            flags: 0x80,
            handled: 1,
            native_result: 123,
        };
        assert!(
            rsf_d3d11_host_dispatch_present(cookie, &mut dispatch) == RSF_ERROR_INVALID_ARGUMENT
                && dispatch.handled == 1
                && dispatch.native_result == 123,
            "Present dispatch validates owner before outputs"
        );
        refuses_malformed!(
            dispatch,
            rsf_d3d11_host_dispatch_present(host.user, &mut dispatch),
            "Present dispatch"
        );
        assert!(
            rsf_d3d11_host_dispatch_present(host.user, &mut dispatch) == RSF_OK
                && dispatch.handled == 0
                && dispatch.native_result == 123
                && dispatch.receiver == fake
                && dispatch.sync_interval == 2
                && dispatch.flags == 0x80,
            "Unowned Present receiver is not dereferenced and preserves native result and arguments"
        );
        let mut scope = rsf_d3d11_creation_scope {
            struct_size: size_of::<rsf_d3d11_creation_scope>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            window: fake,
            token: 0,
        };
        assert!(
            rsf_d3d11_host_creation_begin(cookie, &mut scope) == RSF_ERROR_INVALID_ARGUMENT,
            "Creation scope owner checked"
        );
        refuses_malformed!(
            scope,
            rsf_d3d11_host_creation_begin(host.user, &mut scope),
            "creation scope"
        );
        assert!(
            rsf_d3d11_host_creation_begin(host.user, &mut scope) == RSF_ERROR_NOT_READY
                && scope.token == 0,
            "Unscoped host preserves scope output without dereferencing native pointers"
        );
        assert!(
            rsf_d3d11_host_creation_end(host.user, &scope, null_mut()) == RSF_ERROR_NOT_READY,
            "Unscoped host cannot close another creation transaction"
        );
        let mut original_chain = fake;
        let mut native_creation_request = rsf_d3d11_native_chain {
            struct_size: size_of::<rsf_d3d11_native_chain>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            device: fake,
            immediate_context: fake,
            swapchain: &raw mut original_chain,
        };
        assert!(
            rsf_d3d11_host_wrap_native_swapchain(cookie, &native_creation_request)
                == RSF_ERROR_INVALID_ARGUMENT,
            "Only returned host owner wraps outer native creation"
        );
        refuses_malformed!(
            native_creation_request,
            rsf_d3d11_host_wrap_native_swapchain(host.user, &native_creation_request),
            "outer creation call"
        );
        assert!(
            rsf_d3d11_host_wrap_native_swapchain(host.user, &native_creation_request)
                == RSF_ERROR_NOT_READY
                && original_chain == fake,
            "Host without outer-root bridge preserves native outputs before graphics access"
        );
        let mut binding = rsf_d3d11_presentation_binding {
            struct_size: size_of::<rsf_d3d11_presentation_binding>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            swapchain: fake,
        };
        assert!(
            rsf_d3d11_host_bind_presentation(cookie, &binding) == RSF_ERROR_INVALID_ARGUMENT,
            "Presentation owner checked"
        );
        refuses_malformed!(
            binding,
            rsf_d3d11_host_bind_presentation(host.user, &binding),
            "presentation binding"
        );
        assert!(
            rsf_d3d11_host_set_active(host.user, 0) == RSF_OK
                && rsf_d3d11_host_bind_presentation(host.user, &binding) == RSF_ERROR_NOT_READY,
            "Suspended binding never dereferences graphics pointers"
        );
        assert!(
            rsf_d3d11_host_set_active(host.user, 1) == RSF_OK,
            "Restore active host"
        );
        let mut capture = rsf_d3d11_host_capture {
            struct_size: size_of::<rsf_d3d11_host_capture>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            session_id: 91,
            native_frame: 1,
            view_key: 2,
            name: c"aux".as_ptr(),
            description: c"Auxiliary".as_ptr(),
            texture: fake,
            view: 0,
            scale: 1.0,
        };
        assert!(
            rsf_d3d11_host_capture_texture(cookie, fake, &capture) == RSF_ERROR_INVALID_ARGUMENT,
            "Capture owner checked"
        );
        refuses_malformed!(
            capture,
            rsf_d3d11_host_capture_texture(host.user, fake, &capture),
            "capture"
        );
        assert!(
            rsf_d3d11_host_capture_texture(host.user, fake, &capture) == RSF_ERROR_NOT_READY,
            "An armed request without a matching scene capture must not dereference graphics pointers"
        );
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11_host_fixture() -> i32 {
    crate::run(check)
}
