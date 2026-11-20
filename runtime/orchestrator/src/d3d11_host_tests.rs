// SPDX-License-Identifier: MIT
// Synthetic D3D11 host contracts: fake COM identities, a fake native leaf table
// and fake auxiliary products. No device, window or SDK is touched.
use super::*;
use crate::auxiliary_sr_abi::rsf_aux_platform;
use crate::native_sr_abi::rsf_motion_decode_params;
use rsf_game_sys::rsf_game_log_fn;
use std::cell::Cell;

#[repr(C)]
struct Vtable {
    query_interface: usize,
    add_ref: unsafe extern "system" fn(*mut Fake) -> u32,
    release: unsafe extern "system" fn(*mut Fake) -> u32,
}

#[repr(C)]
struct Fake {
    vtable: &'static Vtable,
    refs: Cell<u32>,
}

unsafe extern "system" fn add_ref(object: *mut Fake) -> u32 {
    let refs = unsafe { &(*object).refs };
    refs.set(refs.get() + 1);
    refs.get()
}

unsafe extern "system" fn release(object: *mut Fake) -> u32 {
    let refs = unsafe { &(*object).refs };
    refs.set(refs.get() - 1);
    refs.get()
}

static VTABLE: Vtable = Vtable {
    query_interface: 0,
    add_ref,
    release,
};

fn fake() -> Box<Fake> {
    Box::new(Fake {
        vtable: &VTABLE,
        refs: Cell::new(1),
    })
}

fn raw(object: &Fake) -> Raw {
    ptr::from_ref(object).cast_mut().cast()
}

thread_local! {
    static DEVICE: Cell<Raw> = const { Cell::new(null_mut()) };
    static CONTEXT: Cell<Raw> = const { Cell::new(null_mut()) };
    static EVALUATE: Cell<i32> = const { Cell::new(0) };
    static RESERVE_OK: Cell<bool> = const { Cell::new(true) };
    static RESERVES: Cell<u32> = const { Cell::new(0) };
    static OWNER_FACADE: Cell<i32> = const { Cell::new(0) };
}

fn owned(cell: &'static std::thread::LocalKey<Cell<Raw>>) -> Raw {
    let object = cell.get();
    if !object.is_null() {
        unsafe { add_ref(object.cast()) };
    }
    object
}

unsafe extern "C" fn same(left: Raw, right: Raw) -> i32 {
    (!left.is_null() && left == right) as i32
}
unsafe extern "C" fn device_of(_: Raw) -> Raw {
    owned(&DEVICE)
}
unsafe extern "C" fn context_of(_: Raw) -> Raw {
    owned(&CONTEXT)
}
unsafe extern "C" fn yes(_: Raw) -> u32 {
    1
}
unsafe extern "C" fn vendor(_: Raw, value: *mut u32) -> i32 {
    unsafe { *value = 0x10de };
    1
}
unsafe extern "C" fn window(_: Raw) -> Raw {
    ptr::without_provenance_mut(0x1000)
}
unsafe extern "C" fn none(_: Raw) -> Raw {
    null_mut()
}
unsafe extern "C" fn refuse_bridge(_: *const rsf_d3d11h_bridge) -> i32 {
    0
}
unsafe extern "C" fn observe(_: rsf_game_log_fn, _: Raw, _: PresentEvent) -> i32 {
    1
}
unsafe extern "C" fn facade(_: Raw) -> i32 {
    OWNER_FACADE.get()
}
unsafe extern "C" fn dispatch(_: *mut rsf_d3d11_chain_dispatch) {}
unsafe extern "C" fn begin(_: Raw, _: *mut u64) -> i32 {
    0
}
unsafe extern "C" fn end(_: Raw, _: u64, _: Raw, _: Raw, _: Raw) -> i32 {
    0
}
unsafe extern "C" fn wrap(_: *const rsf_d3d11_native_chain, _: rsf_game_log_fn, _: Raw) -> i32 {
    NOT_READY
}
unsafe extern "C" fn pass_fact(_: *const Pass, _: u32) {}
unsafe extern "C" fn final_fact(_: Raw, _: *const Pass) {}
unsafe extern "C" fn capture_fg(_: Raw, _: *const Pass, _: *const rsf_motion_decode_params) -> i32 {
    0
}
unsafe extern "C" fn pass_result(_: *const Pass, _: u32) -> i32 {
    0
}
unsafe extern "C" fn present_path(_: *const Pass) -> u32 {
    0
}
unsafe extern "C" fn window_present(_: *const rsf_observer_present_event) -> i32 {
    0
}
unsafe extern "C" fn last_present(_: *mut rsf_native_present_record) -> i32 {
    0
}
unsafe extern "C" fn window_match(_: Raw, _: *mut Pass) -> i32 {
    0
}
unsafe extern "C" fn bind_window(_: u64, _: Raw) {}
unsafe extern "C" fn cpu_dispatch(_: *const rsf_game_cpu_event) -> i32 {
    0
}
unsafe extern "C" fn cpu_event(_: *const rsf_game_cpu_event) {}
unsafe extern "C" fn native_present(_: Raw, _: u32, _: u32, _: u32) -> i32 {
    0
}
unsafe extern "C" fn start(_: Raw, _: *const rsf_d3d11h_pipeline_start) -> i32 {
    0
}
unsafe extern "C" fn resize(_: u32, _: u32) -> i32 {
    0
}
unsafe extern "C" fn select(_: u32) -> i32 {
    0
}
unsafe extern "C" fn sharpen(_: *const rsf_render_settings) {}
unsafe extern "C" fn status(output: *mut rsf_d3d11h_pipeline_status) -> i32 {
    unsafe {
        *output = rsf_d3d11h_pipeline_status {
            running: 1,
            output_width: 64,
            output_height: 64,
            render_width: 32,
            render_height: 32,
        }
    };
    0
}
unsafe extern "C" fn colour(_: u32, _: u32) {}
unsafe extern "C" fn output() -> Raw {
    null_mut()
}
unsafe extern "C" fn evaluate(_: Raw, _: *const Pass) -> i32 {
    EVALUATE.get()
}
unsafe extern "C" fn ordered() -> i32 {
    1
}
unsafe extern "C" fn dump_configure(_: *const rsf_d3d11h_strings, _: rsf_game_log_fn, _: Raw) {}
unsafe extern "C" fn nothing() {}
unsafe extern "C" fn zero() -> i32 {
    0
}
unsafe extern "C" fn dump_begin(_: *const rsf_frame_record, _: u32) -> i32 {
    0
}
unsafe extern "C" fn dump_wants(_: *const rsf_frame_record) -> i32 {
    0
}
unsafe extern "C" fn dump_inputs(_: Raw, _: *const Pass, _: *const rsf_frame_record, _: Raw) {}
unsafe extern "C" fn dump_pass(_: Raw, _: *const Pass, _: u32) {}
unsafe extern "C" fn dump_texture(
    _: Raw,
    _: *const rsf_frame_record,
    _: *const c_char,
    _: *const c_char,
    _: Raw,
    _: u32,
    _: f32,
) -> i32 {
    0
}
unsafe extern "C" fn dump_result(_: *const rsf_frame_record, _: *const c_char, _: u32, _: i32) {}
unsafe extern "C" fn dump_complete(_: *const rsf_frame_record, _: i32) {}
unsafe extern "C" fn overlay_configure(
    _: *const rsf_overlay_service_setup,
    applied: *mut rsf_render_settings,
) -> i32 {
    unsafe {
        *applied = rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: 1,
            sr_backend: SR_DLSS,
            ..zeroed_settings()
        }
    };
    OK
}
unsafe extern "C" fn overlay_present(_: Raw) {}
unsafe extern "C" fn ini_read(_: *const u16, _: *const u16, _: *const u16, fallback: i32) -> i32 {
    fallback
}
unsafe extern "C" fn ini_write(_: *const u16, _: *const u16, _: *const u16, _: *const u16) -> i32 {
    1
}
unsafe extern "C" fn failure_claim(_: u32, _: u32, _: i32) -> i32 {
    0
}
unsafe extern "C" fn sequence_claim(_: u64, _: u64, _: u32, _: u32) -> i32 {
    0
}

const PLATFORM: rsf_d3d11h_platform = rsf_d3d11h_platform {
    same_object: same,
    child_device: device_of,
    is_immediate: yes,
    device_ok: yes,
    immediate_context: context_of,
    device_vendor: vendor,
    chain_device: device_of,
    chain_window: window,
    chain_buffer: none,
    install_bridge: refuse_bridge,
    observer_install: observe,
    present_is_owner: facade,
    chain_dispatch: dispatch,
    creation_begin: begin,
    creation_end: end,
    wrap_chain: wrap,
    fg_frame: pass_fact,
    fg_submission: pass_fact,
    fg_final: final_fact,
    fg_capture: capture_fg,
    scene_pass: pass_result,
    scene_present_path: present_path,
    window_scope: pass_result,
    window_present,
    window_last_present: last_present,
    window_match,
    latency_window_bind: bind_window,
    cpu_dispatch_fg: cpu_dispatch,
    cpu_event,
    dlss_native_present: native_present,
    pipeline_start: start,
    pipeline_resize: resize,
    pipeline_select: select,
    pipeline_quality: select,
    pipeline_preset: select,
    pipeline_sharpen: sharpen,
    pipeline_status: status,
    pipeline_colour: colour,
    pipeline_output: output,
    sr_evaluate: evaluate,
    sr_destinations_ordered: ordered,
    dump_configure,
    dump_request: nothing,
    dump_requested: zero,
    dump_poll: nothing,
    dump_begin,
    dump_wants,
    dump_inputs,
    dump_pass,
    dump_texture,
    dump_result,
    dump_complete,
    overlay_configure,
    overlay_present,
    overlay_poll_toggle: nothing,
    ini_read,
    ini_write,
    failure_claim,
    sequence_request: nothing,
    sequence_claim,
    auxiliary: ptr::null(),
};

#[derive(Default)]
struct FakeProducts {
    frame: bool,
    sealed: bool,
}

impl Products for FakeProducts {
    fn pending(&self) -> bool {
        self.frame
    }
    fn matches(&self, _: [u64; 3]) -> bool {
        self.frame
    }
    fn reserve(
        &mut self,
        _: Raw,
        _: &Pass,
        _: &[Surface],
        _: rsf_game_log_fn,
        _: Raw,
        _: bool,
    ) -> bool {
        RESERVES.set(RESERVES.get() + 1);
        if !RESERVE_OK.get() || self.frame {
            return false;
        }
        self.frame = true;
        self.sealed = false;
        true
    }
    fn refine(&mut self, _: &[Surface]) -> bool {
        self.frame && !self.sealed
    }
    fn seal(&mut self) -> bool {
        self.sealed = self.frame;
        self.frame
    }
    fn cancel(&mut self) -> bool {
        let unused = self.frame && !self.sealed;
        if unused {
            self.frame = false;
        }
        unused
    }
    fn original_colour(&self) -> Raw {
        null_mut()
    }
    fn temporal_prepare(&mut self, _: &[Surface]) -> bool {
        self.sealed
    }
    fn spatial_prepare(&mut self) -> bool {
        self.sealed
    }
    fn commit(&mut self, _: bool) -> bool {
        self.sealed
    }
    fn restore(&mut self) -> bool {
        self.frame = false;
        self.sealed = false;
        true
    }
}

fn fake_products(_: *const rsf_aux_platform) -> Option<Box<dyn Products>> {
    Some(Box::<FakeProducts>::default())
}

struct Fixture {
    host: Host,
    // Fake COM objects outlive the host fields that retain them.
    chain: Box<Fake>,
    input: Box<Fake>,
    output: Box<Fake>,
    context: Box<Fake>,
    _device: Box<Fake>,
}

impl Fixture {
    fn new() -> Self {
        let (device, context) = (fake(), fake());
        DEVICE.set(raw(&device));
        CONTEXT.set(raw(&context));
        let (path, section, root) = (wide("unused.ini"), wide("Test"), wide("unused"));
        let mut motion: rsf_motion_decode_params = unsafe { std::mem::zeroed() };
        motion.struct_size = size_of::<rsf_motion_decode_params>() as u32;
        motion.scale_x = 1.0;
        motion.scale_y = 1.0;
        let setup = rsf_d3d11_host_setup {
            struct_size: size_of::<rsf_d3d11_host_setup>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            configuration_path: path.as_ptr(),
            configuration_section: section.as_ptr(),
            runtime_directory: root.as_ptr(),
            identity_utf8: c"contract".as_ptr(),
            executable_sha256: c"unused".as_ptr(),
            engine_type: 0,
            engine_version_utf8: c"1".as_ptr(),
            project_id_utf8: c"project".as_ptr(),
            units_to_meters: 1.0,
            depth_inverted: 0,
            motion,
            quality_choices: 1,
            presentation_bridge: 0,
            accept_window: None,
            presentation_user: null_mut(),
            capabilities: 0,
            colour_transport_default: 0,
        };
        let upstream = rsf_game_host_services {
            struct_size: size_of::<rsf_game_host_services>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            session_id: 91,
            ..unsafe { std::mem::zeroed() }
        };
        assert!(validate_setup(&setup, &upstream));
        let host = Host::new(PLATFORM, fake_products, &setup, upstream).expect("host");
        host.active.store(true, Ordering::SeqCst);
        Self {
            host,
            chain: fake(),
            input: fake(),
            output: fake(),
            context,
            _device: device,
        }
    }

    fn context(&self) -> Raw {
        raw(&self.context)
    }

    fn pass(&self, source: u64) -> Pass {
        let mut pass = zeroed_pass();
        pass.struct_size = size_of::<Pass>() as u32;
        pass.role = RSF_GAME_RENDER_SR;
        pass.session_id = 91;
        pass.swapchain = raw(&self.chain);
        pass.color_input = raw(&self.input);
        pass.color_output = raw(&self.output);
        pass.camera_valid = 1;
        pass.camera.render_width = 32;
        pass.camera.render_height = 32;
        pass.camera.output_width = 64;
        pass.camera.output_height = 64;
        pass.source_frame_id = source;
        pass.native_frame = source + 2;
        pass.view_key = 9;
        pass.render_rect = [0, 0, 32, 32];
        pass.output_rect = [0, 0, 64, 64];
        pass
    }

    fn evaluate(&self) -> i32 {
        self.host.process(self.context(), &self.pass(5), None)
    }

    fn reduced(&self, operation: u32, token: u64, owner: &Pass) -> (i32, rsf_d3d11_reduced_frame) {
        self.reduced_with(operation, token, owner, &[])
    }

    fn reduced_with(
        &self,
        operation: u32,
        token: u64,
        owner: &Pass,
        surfaces: &[Surface],
    ) -> (i32, rsf_d3d11_reduced_frame) {
        let mut request = rsf_d3d11_reduced_frame {
            struct_size: size_of::<rsf_d3d11_reduced_frame>() as u32,
            abi_version: RSF_D3D11_REDUCED_FRAME_ABI_VERSION,
            operation,
            disposition: 0,
            token,
            owner,
            surface_count: surfaces.len() as u32,
            reserved: 0,
            surfaces: if surfaces.is_empty() {
                ptr::null()
            } else {
                surfaces.as_ptr()
            },
            temporal_result: 0,
            result_flags: 0,
        };
        let result = self.host.reduced_frame(self.context(), &mut request);
        (result, request)
    }
}

#[test]
fn first_scene_lease_configures_and_evaluates() {
    let fixture = Fixture::new();
    assert_eq!(fixture.evaluate(), OK);
    let g = fixture.host.lock_graphics();
    assert!(g.configured && g.scene_bound && g.observer && g.running);
    assert_eq!(g.execution_thread, current_thread());
    assert_eq!((g.evaluated, g.refused, g.reset), (1, 0, false));
    drop(g);
    let planned = fixture.host.shared().planned;
    assert_eq!(
        (planned.enabled, planned.render_width, planned.output_width),
        (1, 32, 64)
    );
}

#[test]
fn suspended_and_contended_leases_refuse_before_graphics() {
    let fixture = Fixture::new();
    fixture.host.active.store(false, Ordering::SeqCst);
    assert_eq!(fixture.evaluate(), NOT_READY);
    assert!(!fixture.host.lock_graphics().configured);
    fixture.host.active.store(true, Ordering::SeqCst);
    let held = fixture.host.lock_graphics();
    assert_eq!(fixture.evaluate(), BUSY);
    assert_eq!(fixture.host.bind_presentation(raw(&fixture.chain)), BUSY);
    let (result, _) = fixture.reduced(RSF_D3D11_REDUCED_RESERVE, 0, &fixture.pass(5));
    assert_eq!(result, BUSY);
    drop(held);
    assert_eq!(fixture.evaluate(), OK);
}

#[test]
fn reduced_ticket_runs_reserve_seal_finish_retire_and_acknowledges() {
    let fixture = Fixture::new();
    assert_eq!(fixture.evaluate(), OK);
    let owner = fixture.pass(5);
    let (result, reserved) = fixture.reduced(RSF_D3D11_REDUCED_RESERVE, 0, &owner);
    assert_eq!(
        (result, reserved.disposition, reserved.token),
        (OK, RSF_D3D11_REDUCED_RESERVED, 1)
    );
    assert_eq!(fixture.input.refs.get(), 2, "colour input leased");
    // Another evaluation cannot run against an unsealed reservation.
    assert_eq!(fixture.evaluate(), BUSY);
    let (result, sealed) = fixture.reduced(RSF_D3D11_REDUCED_SEAL, 1, &owner);
    assert_eq!((result, sealed.disposition), (OK, RSF_D3D11_REDUCED_SEALED));
    let (result, _) = fixture.reduced(RSF_D3D11_REDUCED_CANCEL, 1, &owner);
    assert_eq!(
        result, NOT_READY,
        "a sealed ticket is not an unused reservation"
    );
    let (result, _) = fixture.reduced(RSF_D3D11_REDUCED_RETIRE, 1, &owner);
    assert_eq!(result, BUSY, "retire requires a defined output");
    let (result, finished) = fixture.reduced(RSF_D3D11_REDUCED_FINISH, 1, &owner);
    assert_eq!(
        (result, finished.disposition),
        (OK, RSF_D3D11_REDUCED_SPATIAL)
    );
    assert_eq!(
        finished.result_flags,
        RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED
            | RSF_D3D11_REDUCED_CLEANUP_REQUIRED
            | RSF_D3D11_REDUCED_SPATIAL_OUTPUT
    );
    assert!(
        fixture.host.lock_graphics().reset,
        "spatial recovery resets history"
    );
    let (result, retired) = fixture.reduced(RSF_D3D11_REDUCED_RETIRE, 1, &owner);
    assert_eq!(result, OK);
    assert_eq!(
        retired.result_flags & RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED,
        RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED
    );
    assert_eq!(fixture.input.refs.get(), 1, "retirement releases the lease");
    let (result, _) = fixture.reduced(RSF_D3D11_REDUCED_RETIRE, 1, &owner);
    assert_eq!(result, OK, "retired ticket acknowledges RETIRE");
    let (result, _) = fixture.reduced(RSF_D3D11_REDUCED_FINISH, 1, &owner);
    assert_eq!(
        result, OK,
        "retired ticket acknowledges FINISH of a defined output"
    );
    let (result, _) = fixture.reduced(RSF_D3D11_REDUCED_CANCEL, 1, &owner);
    assert_eq!(result, NOT_READY);
    let (result, _) = fixture.reduced(RSF_D3D11_REDUCED_RETIRE, 2, &owner);
    assert_eq!(result, NOT_READY, "token identity is exact");
}

#[test]
fn refused_reserve_keeps_previous_acknowledgement() {
    let fixture = Fixture::new();
    assert_eq!(fixture.evaluate(), OK);
    let owner = fixture.pass(5);
    assert_eq!(fixture.reduced(RSF_D3D11_REDUCED_RESERVE, 0, &owner).0, OK);
    assert_eq!(fixture.reduced(RSF_D3D11_REDUCED_CANCEL, 1, &owner).0, OK);

    // A malformed surface and a refused products reservation both fail after
    // the request passed admission. Neither may replace the retired identity.
    let next = fixture.pass(6);
    let malformed = [Surface::default()];
    let (result, _) = fixture.reduced_with(RSF_D3D11_REDUCED_RESERVE, 0, &next, &malformed);
    assert_eq!(result, INVALID);
    RESERVE_OK.set(false);
    let (result, _) = fixture.reduced(RSF_D3D11_REDUCED_RESERVE, 0, &next);
    assert_eq!(result, NOT_READY);
    RESERVE_OK.set(true);

    let (result, acknowledged) = fixture.reduced(RSF_D3D11_REDUCED_CANCEL, 1, &owner);
    assert_eq!(
        (result, acknowledged.disposition),
        (OK, RSF_D3D11_REDUCED_CANCELLED)
    );
    assert_eq!(
        fixture.reduced(RSF_D3D11_REDUCED_CANCEL, 1, &next).0,
        NOT_READY
    );
    let (result, reserved) = fixture.reduced(RSF_D3D11_REDUCED_RESERVE, 0, &next);
    assert_eq!((result, reserved.token), (OK, 2));
}

#[test]
fn pending_recovery_admits_only_its_sealed_owner() {
    let fixture = Fixture::new();
    assert_eq!(fixture.evaluate(), OK);
    let owner = fixture.pass(5);
    assert_eq!(fixture.reduced(RSF_D3D11_REDUCED_RESERVE, 0, &owner).0, OK);
    assert_eq!(fixture.reduced(RSF_D3D11_REDUCED_SEAL, 1, &owner).0, OK);
    let other = fixture.pass(6);
    assert_eq!(fixture.host.process(fixture.context(), &other, None), BUSY);
    assert_eq!(fixture.host.process(fixture.context(), &owner, None), OK);
    assert_eq!(
        fixture.host.process(fixture.context(), &owner, None),
        BUSY,
        "one temporal attempt per ticket"
    );
    let (result, finished) = fixture.reduced(RSF_D3D11_REDUCED_FINISH, 1, &owner);
    assert_eq!(
        (result, finished.disposition, finished.temporal_result),
        (OK, RSF_D3D11_REDUCED_TEMPORAL, OK)
    );
    assert_eq!(
        finished.result_flags & RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED,
        RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED
    );
    assert_eq!(fixture.reduced(RSF_D3D11_REDUCED_RETIRE, 1, &owner).0, OK);
}

#[test]
fn presentation_owner_transfers_only_before_scene_admission() {
    let fixture = Fixture::new();
    let chain = raw(&fixture.chain);
    assert_eq!(fixture.host.bind_presentation(chain), OK);
    {
        let g = fixture.host.lock_graphics();
        assert!(g.configured && !g.scene_bound && !g.running);
    }
    let other = ptr::without_provenance_mut(0x2000);
    assert_eq!(fixture.host.bind_presentation(other), NOT_READY);
    let host = &fixture.host;
    let chain_key = chain.expose_provenance();
    let worker = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let result = host.bind_presentation(ptr::with_exposed_provenance_mut(chain_key));
                (result, current_thread())
            })
            .join()
            .expect("worker")
    });
    assert_eq!(worker.0, OK);
    assert_eq!(fixture.host.lock_graphics().execution_thread, worker.1);
    assert_eq!(
        fixture.evaluate(),
        NOT_READY,
        "the previous thread no longer owns graphics"
    );
    fixture.host.lock_graphics().execution_thread = current_thread();
    assert_eq!(fixture.evaluate(), OK);
    let refused = std::thread::scope(|scope| {
        scope
            .spawn(|| host.bind_presentation(ptr::with_exposed_provenance_mut(chain_key)))
            .join()
            .expect("worker")
    });
    assert_eq!(refused, NOT_READY, "scene-admitted SR keeps its owner");
}

#[test]
fn reconfigure_before_scene_lease_is_refused_without_graphics() {
    let fixture = Fixture::new();
    let mut plan = fixture.pass(5);
    plan.flags |= RSF_GAME_RENDER_RECONFIGURE;
    assert_eq!(
        fixture.host.process(fixture.context(), &plan, None),
        NOT_READY
    );
    assert!(!fixture.host.lock_graphics().configured);
    assert_eq!(fixture.evaluate(), OK);
    // An unchanged successful plan publishes no new evaluation result.
    assert_eq!(
        fixture.host.process(fixture.context(), &plan, None),
        NOT_READY
    );
    assert!(!fixture.host.lock_graphics().reset);
}

#[test]
fn failed_evaluation_resets_history_and_reports() {
    let fixture = Fixture::new();
    EVALUATE.set(-7);
    assert_eq!(fixture.evaluate(), -7);
    let g = fixture.host.lock_graphics();
    assert_eq!((g.refused, g.reset, g.last_result), (1, true, -7));
    drop(g);
    let report = fixture.host.shared().report;
    assert_eq!((report.effective_backend, report.last_result), (0, -7));
    let reason = unsafe { CStr::from_ptr(report.reason.as_ptr()) };
    assert_eq!(
        reason.to_str().unwrap(),
        "SR result -7; native temporal processing retained"
    );
    EVALUATE.set(0);
}

#[test]
fn overlay_actions_follow_index_and_value_bounds() {
    let fixture = Fixture::new();
    let host = &fixture.host;
    let generation = host.requested_generation.load(Ordering::SeqCst);
    assert!(!host.action(5, 1) && !host.action(1, 2) && !host.action(0, 0));
    assert!(host.action(1, 0) && !host.transport.load(Ordering::SeqCst));
    assert!(host.action(2, 1) && host.fidelity.load(Ordering::SeqCst));
    assert_eq!(
        host.requested_generation.load(Ordering::SeqCst),
        generation + 2
    );
    assert!(host.action(3, 1) && host.capture_failures.load(Ordering::SeqCst));
    assert!(host.action(4, 1) && host.capture_sequence.load(Ordering::SeqCst));
    assert_eq!(
        host.requested_generation.load(Ordering::SeqCst),
        generation + 2
    );
}

#[test]
fn report_reason_truncates_to_the_c_buffer() {
    let mut report = fresh_report();
    set_reason(&mut report, &"x".repeat(200));
    assert_eq!(report.reason[report.reason.len() - 1], 0);
    assert_eq!(
        unsafe { CStr::from_ptr(report.reason.as_ptr()) }
            .to_bytes()
            .len(),
        95
    );
}
