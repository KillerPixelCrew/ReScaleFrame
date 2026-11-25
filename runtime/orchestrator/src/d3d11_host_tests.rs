// SPDX-License-Identifier: MIT
// Synthetic D3D11 host contracts: fake COM identities, fake native leaves
// and fake auxiliary products. No device, window or SDK is touched.
use super::*;
use crate::com_ref::test::{Fake, add_ref, fake, raw};
use crate::sys::rsf_backend_log_fn;
use crate::sys::rsf_motion_decode_params;
use crate::sys::rsf_observer_present_event_fn;
use crate::sys::rsf_streamline_host;
use crate::wide::from_str as wide;
use rsf_game_sys::rsf_game_log_fn;
use std::cell::Cell;

#[test]
fn failure_captures_count_only_failed_frames_with_no_pending_dump() {
    let mut budget = CaptureBudget::default();
    for result in [NOT_READY, NATIVE_REFUSED, INVALID] {
        assert!(!budget.claim_failure(false, false, result));
        assert!(!budget.claim_failure(true, true, result));
        assert!(!budget.claim_failure(true, false, OK));
        assert!(budget.claim_failure(true, false, result));
    }
    budget.request_sequence();
    assert!(!budget.claim_failure(true, false, NOT_READY));
}

#[test]
fn capture_sequence_waits_for_ready_identified_frames_and_can_restart() {
    let mut budget = CaptureBudget::default();
    assert!(!budget.claim_source(7, 1, true, false));
    budget.request_sequence();
    for source in 1..=4 {
        assert!(!budget.claim_source(7, source, false, false));
        assert!(!budget.claim_source(7, source, true, true));
        assert!(!budget.claim_source(0, source, true, false));
        assert!(!budget.claim_source(7, 0, true, false));
        assert!(budget.claim_source(7, source, true, false));
        assert!(!budget.claim_source(7, source, true, false));
    }
    assert!(!budget.claim_source(7, 5, true, false));
    budget.request_sequence();
    assert!(budget.claim_source(7, 4, true, false));
    assert!(budget.claim_source(8, 4, true, false));
}

#[test]
fn the_panel_offers_the_common_set_and_greys_what_the_machine_lacks() {
    use crate::sr_choices::{COMMON, Packages};
    let packages = Packages::all("d3d11-host");
    let fixture = Fixture::rooted(packages.0.to_str().unwrap());
    // NVIDIA: every provider, Auto and the pipeline's FSR1. AMD: the same minus DLSS.
    let fsr1 = 1 << rsf_game_sys::RSF_SR_SPATIAL;
    assert_eq!(fixture.host.sr_offer(0x10de), COMMON | fsr1);
    assert_eq!(
        fixture.host.sr_offer(0x1002),
        (COMMON | fsr1) & !(1 << RSF_SR_DLSS)
    );
    // A host feeding generation offers every provider and Auto; one without offers none.
    assert_eq!(fg_choices(true, false), RSF_FG_CHOICES_ALL);
    assert_eq!(fg_choices(false, false), 0);
    assert_eq!(fg_choices(false, true), 0);
    // The rendered-source fixtures have no input timeline, which DLSS-G and XeSS FG need.
    assert_eq!(
        fg_choices(true, true),
        (1 << FG_BACKEND_FSR3) | (1 << FG_BACKEND_FSR4)
    );
    // No package files: only FSR1, which needs none; no Auto.
    assert_eq!(Fixture::new().host.sr_offer(0x10de), fsr1);
}

thread_local! {
    static DEVICE: Cell<Raw> = const { Cell::new(null_mut()) };
    static CONTEXT: Cell<Raw> = const { Cell::new(null_mut()) };
    static EVALUATE: Cell<i32> = const { Cell::new(0) };
    static EVALUATE_ORDERED: Cell<u32> = const { Cell::new(1) };
    static RESERVE_OK: Cell<bool> = const { Cell::new(true) };
    static RESERVES: Cell<u32> = const { Cell::new(0) };
    static OWNER_FACADE: Cell<i32> = const { Cell::new(0) };
    static CHAIN_EXTENT: Cell<[u32; 2]> = const { Cell::new([64, 64]) };
    // Bits: 1 translucency, 2 UI composition, 4 texture binding, 8 window end.
    static ROUTED: Cell<u32> = const { Cell::new(0) };
    // What the host offered the overlay: SR, FG and latency choices.
    static OFFERED: Cell<[u32; 3]> = const { Cell::new([0; 3]) };
    static INSTALLS: Cell<i32> = const { Cell::new(1) };
}

fn owned(cell: &'static std::thread::LocalKey<Cell<Raw>>) -> Raw {
    let object = cell.get();
    if !object.is_null() {
        unsafe { add_ref(object.cast()) };
    }
    object
}

// The fakes `d3d11_host` calls in place of d3d11_host_platform.
pub(super) mod native {
    pub(in crate::d3d11_host) use super::{
        auxiliary_products, begin as creation_begin, bind_window as latency_window_bind,
        capture_fg as fg_capture, chain_extent, colour as pipeline_colour_correction,
        colour as pipeline_colour_transport, composition as composition_capture,
        context_of as immediate_context, cpu_dispatch as cpu_dispatch_fg, cpu_event,
        device_of as chain_device, device_of as child_device, dispatch as chain_dispatch,
        dump_begin, dump_complete, dump_configure, dump_inputs, dump_pass, dump_result,
        dump_texture, dump_wants, end as creation_end, evaluate as sr_evaluate,
        facade as present_is_owner, fg_latency_event, fg_options_set, fg_prepare,
        fg_rendered_source_set, fg_retire, fg_set_log, final_fact as fg_final, ini_read, ini_write,
        last_present as window_last_present, listen as observer_listen,
        native_present as dlss_native_present, none as chain_buffer, none as chain_present,
        none as com_identity, nothing as dump_poll, nothing as dump_request,
        observe as observer_install, output as pipeline_output, overlay_configure, overlay_present,
        owner_exists as present_has_owner, pass_fact as fg_frame, pass_fact as fg_submission,
        pass_result as scene_pass, pass_result as window_scope, poll_toggle as overlay_poll_toggle,
        present_path as scene_present_path, preset as pipeline_preset,
        refuse_bridge as present_install, refuse_bridge as present_install_scoped,
        refuse_bridge as present_prepare, release_count as com_release, resize as pipeline_resize,
        same as same_object, select as pipeline_quality, select as pipeline_select,
        sharpen as pipeline_sharpen, start as pipeline_start, status as pipeline_status,
        texture_binding as scene_texture_binding, translucency as translucency_pass,
        vendor as device_vendor, window as chain_window, window_end as fg_window_end, window_match,
        window_present, wrap as wrap_native, yes as device_ok, yes as is_immediate,
        zero as dump_requested,
    };
}

pub(super) unsafe extern "C" fn same(left: Raw, right: Raw) -> i32 {
    (!left.is_null() && left == right) as i32
}
pub(super) unsafe extern "C" fn device_of(_: Raw) -> Raw {
    owned(&DEVICE)
}
pub(super) unsafe extern "C" fn context_of(_: Raw) -> Raw {
    owned(&CONTEXT)
}
pub(super) unsafe extern "C" fn yes(_: Raw) -> u32 {
    1
}
pub(super) unsafe extern "C" fn vendor(_: Raw, value: *mut u32) -> i32 {
    unsafe { *value = 0x10de };
    1
}
pub(super) unsafe extern "C" fn window(_: Raw) -> Raw {
    ptr::without_provenance_mut(0x1000)
}
pub(super) unsafe extern "C" fn none(_: Raw) -> Raw {
    null_mut()
}
pub(super) unsafe extern "C" fn refuse_bridge(_: *const rsf_d3d11_present_setup) -> c_int {
    0
}
pub(super) unsafe extern "C" fn fg_prepare(
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: Raw,
    _: *mut rsf_streamline_host,
    _: Raw,
    _: u32,
) {
}
pub(super) unsafe extern "C" fn fg_retire(_: Raw, _: Raw) {}
pub(super) unsafe extern "C" fn fg_latency_event(_: Raw, _: *const rsf_observer_present_event) {}
pub(super) extern "C" fn fg_rendered_source_set(_: u32) {}
pub(super) unsafe extern "C" fn fg_set_log(_: rsf_backend_log_fn, _: Raw) {}
pub(super) unsafe extern "C" fn fg_options_set(_: *const rsf_native_fg_options) {}
pub(super) unsafe extern "C" fn observe(
    _: rsf_game_log_fn,
    _: Raw,
    _: rsf_observer_present_event_fn,
) -> i32 {
    INSTALLS.get()
}
// An observer the game installed carries the host's listener.
pub(super) unsafe fn listen(_: rsf_observer_present_event_fn, _: Raw) -> i32 {
    1
}
pub(super) fn owner_exists() -> i32 {
    OWNER_FACADE.get()
}
pub(super) unsafe extern "C" fn facade(_: Raw) -> i32 {
    OWNER_FACADE.get()
}
pub(super) unsafe extern "C" fn dispatch(_: *mut rsf_d3d11_chain_dispatch) {}
pub(super) unsafe extern "C" fn begin(_: Raw, _: *mut u64) -> i32 {
    0
}
pub(super) unsafe extern "C" fn end(_: Raw, _: u64, _: Raw, _: Raw, _: Raw) -> i32 {
    0
}
pub(super) unsafe extern "C" fn wrap(_: Raw, _: Raw, _: *mut Raw) -> i32 {
    0x8000_4005_u32 as i32 // E_FAIL
}
pub(super) unsafe extern "C" fn release_count(_: Raw) -> u32 {
    0
}
pub(super) unsafe fn chain_extent(_: Raw, width: *mut u32, height: *mut u32) -> i32 {
    let [w, h] = CHAIN_EXTENT.get();
    unsafe {
        width.write(w);
        height.write(h);
    }
    1
}
fn route(bit: u32) {
    ROUTED.set(ROUTED.get() | bit);
}
pub(super) unsafe extern "C" fn translucency(_: Raw, _: *const Pass, _: u32) {
    route(1);
}
pub(super) unsafe extern "C" fn composition(_: Raw, _: *const Pass, _: u32) -> i32 {
    route(2);
    1
}
pub(super) unsafe extern "C" fn texture_binding(_: *const Pass) -> i32 {
    route(4);
    1
}
pub(super) unsafe extern "C" fn window_end(_: *const Pass) {
    route(8);
}
pub(super) unsafe extern "C" fn pass_fact(_: *const Pass, _: u32) {}
pub(super) unsafe extern "C" fn final_fact(_: Raw, _: *const Pass) {}
pub(super) unsafe extern "C" fn capture_fg(
    _: Raw,
    _: *const Pass,
    _: *const rsf_motion_decode_params,
) -> i32 {
    0
}
pub(super) unsafe extern "C" fn pass_result(_: *const Pass, _: u32) -> i32 {
    0
}
pub(super) unsafe extern "C" fn present_path(_: *const Pass) -> u32 {
    0
}
pub(super) unsafe extern "C" fn window_present(_: *const rsf_observer_present_event) -> i32 {
    0
}
pub(super) unsafe extern "C" fn last_present(_: *mut rsf_native_present_record) -> i32 {
    0
}
pub(super) unsafe extern "C" fn window_match(_: Raw, _: *mut Pass) -> i32 {
    0
}
pub(super) fn bind_window(_: u64, _: Raw) {}
pub(super) unsafe extern "C" fn cpu_dispatch(_: *const rsf_game_cpu_event) -> i32 {
    0
}
pub(super) unsafe extern "C" fn cpu_event(_: *const rsf_game_cpu_event) {}
pub(super) unsafe extern "C" fn native_present(_: Raw, _: u32, _: u32, _: u32) -> i32 {
    0
}
pub(super) unsafe extern "C" fn start(_: Raw, _: *const rsf_dlss_pipeline_setup, _: u32) -> i32 {
    0
}
pub(super) unsafe extern "C" fn resize(_: u32, _: u32, _: *mut u32, _: *mut u32) -> i32 {
    0
}
pub(super) unsafe extern "C" fn select(_: u32, _: *mut u32, _: *mut u32) -> i32 {
    0
}
pub(super) unsafe extern "C" fn preset(_: u32) -> i32 {
    0
}
pub(super) extern "C" fn sharpen(_: &rsf_render_settings) {}
pub(super) unsafe extern "C" fn status(output: *mut rsf_dlss_pipeline_status) -> i32 {
    let output = unsafe { &mut *output };
    output.running = 1;
    [output.output_width, output.output_height] = [64, 64];
    [output.render_width, output.render_height] = [32, 32];
    0
}
pub(super) extern "C" fn colour(_: u32) {}
pub(super) extern "C" fn output() -> Raw {
    null_mut()
}
pub(super) unsafe extern "C" fn evaluate(_: Raw, _: *const Pass, ordered: *mut u32) -> i32 {
    if !ordered.is_null() {
        unsafe { ordered.write(EVALUATE_ORDERED.get()) };
    }
    EVALUATE.get()
}
pub(super) unsafe extern "C" fn dump_configure(
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: rsf_game_log_fn,
    _: Raw,
) {
}
pub(super) unsafe extern "C" fn nothing() {}
pub(super) extern "C" fn poll_toggle() {}
pub(super) unsafe extern "C" fn zero() -> i32 {
    0
}
pub(super) unsafe extern "C" fn dump_begin(_: *const rsf_frame_record, _: u32) -> i32 {
    0
}
pub(super) unsafe extern "C" fn dump_wants(_: *const rsf_frame_record) -> i32 {
    0
}
pub(super) unsafe extern "C" fn dump_inputs(
    _: Raw,
    _: *const Pass,
    _: *const rsf_frame_record,
    _: Raw,
) {
}
pub(super) unsafe extern "C" fn dump_pass(_: Raw, _: *const Pass, _: u32) {}
pub(super) unsafe extern "C" fn dump_texture(
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
pub(super) unsafe extern "C" fn dump_result(
    _: *const rsf_frame_record,
    _: *const c_char,
    _: u32,
    _: i32,
) {
}
pub(super) unsafe extern "C" fn dump_complete(_: *const rsf_frame_record, _: i32) {}
pub(super) unsafe extern "C" fn overlay_configure(
    setup: *const rsf_overlay_service_setup,
    applied: *mut rsf_render_settings,
) -> i32 {
    let setup = unsafe { &*setup };
    OFFERED.set([
        setup.sr_backend_choices,
        setup.fg_backend_choices,
        setup.latency_choices,
    ]);
    unsafe {
        *applied = rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: 1,
            sr_backend: RSF_SR_DLSS,
            ..zeroed_settings()
        }
    };
    OK
}
pub(super) unsafe extern "C" fn overlay_present(_: Raw) {}
pub(super) unsafe extern "C" fn ini_read(
    _: *const u16,
    _: *const u16,
    _: *const c_char,
    fallback: u32,
) -> u32 {
    fallback
}
pub(super) unsafe extern "C" fn ini_write(
    _: *const u16,
    _: *const u16,
    _: *const c_char,
    _: u32,
) -> i32 {
    OK
}
#[derive(Default)]
pub(crate) struct FakeProducts {
    frame: bool,
    sealed: bool,
}

// `d3d11_host_reduced::Products` under test: the methods `AuxiliarySr` gives the host.
impl FakeProducts {
    pub(crate) fn pending(&self) -> bool {
        self.frame
    }
    pub(crate) fn matches(&self, _: [u64; 3]) -> bool {
        self.frame
    }
    pub(crate) fn reserve(
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
    pub(crate) fn refine(&mut self, _: &[Surface]) -> bool {
        self.frame && !self.sealed
    }
    pub(crate) fn seal(&mut self) -> bool {
        self.sealed = self.frame;
        self.frame
    }
    pub(crate) fn cancel(&mut self) -> bool {
        let unused = self.frame && !self.sealed;
        if unused {
            self.frame = false;
        }
        unused
    }
    pub(crate) fn original_colour(&self) -> Raw {
        null_mut()
    }
    pub(crate) fn temporal_prepare(&mut self, _: &[Surface]) -> bool {
        self.sealed
    }
    pub(crate) fn spatial_prepare(&mut self) -> bool {
        self.sealed
    }
    pub(crate) fn commit(&mut self, _: bool) -> bool {
        self.sealed
    }
    pub(crate) fn restore(&mut self) -> bool {
        self.frame = false;
        self.sealed = false;
        true
    }
    pub(crate) fn prepare(
        &mut self,
        context: Raw,
        pass: &Pass,
        surfaces: &[Surface],
        log: rsf_game_log_fn,
        user: Raw,
    ) -> bool {
        self.reserve(context, pass, surfaces, log, user, false) && self.seal()
    }
}

pub(super) fn auxiliary_products() -> Option<Box<Products>> {
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
        Self::rooted("unused")
    }

    fn rooted(root: &str) -> Self {
        let (device, context) = (fake(), fake());
        DEVICE.set(raw(&device));
        CONTEXT.set(raw(&context));
        let (path, section, root) = (wide("unused.ini"), wide("Test"), wide(root));
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
            depth_far_plane: 0,
        };
        let upstream = rsf_game_host_services {
            struct_size: size_of::<rsf_game_host_services>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            session_id: 91,
            ..unsafe { std::mem::zeroed() }
        };
        assert!(validate_setup(&setup, &upstream));
        let host = Host::new(&setup, upstream).expect("host");
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
fn unordered_evaluation_keeps_the_recovery_ticket_owned() {
    let fixture = Fixture::new();
    assert_eq!(fixture.evaluate(), OK);
    let owner = fixture.pass(5);
    assert_eq!(fixture.reduced(RSF_D3D11_REDUCED_RESERVE, 0, &owner).0, OK);
    assert_eq!(fixture.reduced(RSF_D3D11_REDUCED_SEAL, 1, &owner).0, OK);

    EVALUATE.set(-7);
    EVALUATE_ORDERED.set(0);
    let result = fixture.host.process(fixture.context(), &owner, None);
    EVALUATE.set(0);
    EVALUATE_ORDERED.set(1);
    assert_eq!(result, -7);

    let (result, ticket) = fixture.reduced(RSF_D3D11_REDUCED_FINISH, 1, &owner);
    assert_eq!(
        (result, ticket.disposition, ticket.temporal_result),
        (BUSY, RSF_D3D11_REDUCED_UNSAFE, -7)
    );
    assert_eq!(
        ticket.result_flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED,
        0
    );
    assert_eq!(fixture.reduced(RSF_D3D11_REDUCED_RETIRE, 1, &owner).0, BUSY);
    assert_eq!(fixture.input.refs.get(), 2);
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

#[test]
fn far_plane_convention_is_independent_of_depth_direction() {
    let fixture = Fixture::new();
    let mut setup = fixture.host.setup;
    for inverted in 0..=1 {
        setup.depth_inverted = inverted;
        for far in [RSF_D3D11_HOST_DEPTH_FINITE, RSF_D3D11_HOST_DEPTH_INFINITE] {
            setup.depth_far_plane = far;
            assert_eq!(depth_infinite(&setup), far);
        }
        setup.struct_size = offset_of!(rsf_d3d11_host_setup, depth_far_plane) as u32;
        assert_eq!(depth_infinite(&setup), inverted);
        setup.struct_size = size_of::<rsf_d3d11_host_setup>() as u32;
    }
}

#[test]
fn foreign_pass_cannot_configure_or_evaluate() {
    let fixture = Fixture::new();
    let mut pass = fixture.pass(5);
    pass.session_id += 1;
    assert_eq!(
        fixture.host.process(fixture.context(), &pass, None),
        INVALID
    );
    assert!(!fixture.host.lock_graphics().configured);
}

#[test]
fn sr_reports_only_inputs_from_actual_evaluation() {
    let fixture = Fixture::new();
    let mut pass = fixture.pass(5);
    pass.camera.struct_size = size_of::<rsf_camera_frame>() as u32;
    pass.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    pass.camera.jitter_pixels = [0.25, -0.25];
    assert_eq!(fixture.host.process(fixture.context(), &pass, None), OK);
    let expected = RSF_OVERLAY_SR_INPUT_COLOR | RSF_OVERLAY_SR_INPUT_JITTER;
    assert_eq!(fixture.host.shared().report.inputs, expected);
    pass.flags |= RSF_GAME_RENDER_RECONFIGURE;
    pass.camera.output_width += 1;
    pass.camera_valid = 0;
    pass.color_input = null_mut();
    assert_eq!(
        fixture.host.process(fixture.context(), &pass, None),
        NOT_READY
    );
    let report = fixture.host.shared().report;
    assert_eq!(report.inputs, expected);
    assert_eq!(report.jitter_pixels, [0.25, -0.25]);
}

impl Fixture {
    fn native_graph() -> Self {
        let mut fixture = Self::new();
        fixture.host.setup.capabilities = RSF_D3D11_HOST_CAP_NATIVE_GRAPH;
        fixture
    }

    fn present(&self) {
        let event = rsf_observer_present_event {
            struct_size: size_of::<rsf_observer_present_event>() as u32,
            completed: 0,
            swapchain: raw(&self.chain),
            sync_interval: 1,
            flags: 0,
            result: 0,
            method: 0,
        };
        unsafe { host_present(self.host.user(), &event) };
    }

    fn report(&self, role: u32, begin: u32, swapchain: Raw) {
        let mut pass = self.pass(5);
        pass.role = role;
        pass.swapchain = swapchain;
        unsafe { host_passes(self.host.user(), self.context(), &pass, begin) };
    }
}

#[test]
fn native_graph_plans_from_the_bound_chain_before_any_scene() {
    for native_graph in [false, true] {
        let fixture = if native_graph {
            Fixture::native_graph()
        } else {
            Fixture::new()
        };
        assert_eq!(fixture.host.bind_presentation(raw(&fixture.chain)), OK);
        fixture.present();
        let g = fixture.host.lock_graphics();
        assert_eq!(g.running, native_graph);
        assert!(!g.scene_bound, "planning binds no scene");
        drop(g);
        let planned = fixture.host.shared().planned;
        let expected = if native_graph { (1, 32, 64) } else { (0, 0, 0) };
        assert_eq!(
            (planned.enabled, planned.render_width, planned.output_width),
            expected
        );
    }
    // A chain whose extent the pipeline does not serve leaves the plan disabled.
    let fixture = Fixture::native_graph();
    assert_eq!(fixture.host.bind_presentation(raw(&fixture.chain)), OK);
    CHAIN_EXTENT.set([80, 64]);
    fixture.present();
    CHAIN_EXTENT.set([64, 64]);
    let planned = fixture.host.shared().planned;
    assert_eq!((planned.enabled, planned.output_width), (0, 80));
}

#[test]
fn native_graph_evaluates_the_sr_pass_it_reports() {
    for native_graph in [false, true] {
        let fixture = if native_graph {
            Fixture::native_graph()
        } else {
            Fixture::new()
        };
        assert_eq!(fixture.host.bind_presentation(raw(&fixture.chain)), OK);
        fixture.present();
        // The native graph's SR pass names no swapchain; the bound chain is implied.
        fixture.report(RSF_GAME_RENDER_SR, 1, null_mut());
        fixture.report(RSF_GAME_RENDER_SR, 0, null_mut());
        let g = fixture.host.lock_graphics();
        assert_eq!(g.evaluated, u64::from(native_graph));
        assert_eq!(g.scene_bound, native_graph);
    }
    // Only a native graph may leave the chain implied.
    let fixture = Fixture::new();
    let mut pass = fixture.pass(5);
    pass.swapchain = null_mut();
    assert_eq!(fixture.evaluate(), OK);
    assert_eq!(
        fixture.host.process(fixture.context(), &pass, None),
        NOT_READY
    );
}

#[test]
fn native_graph_routes_its_inputs_once_sr_runs() {
    for native_graph in [false, true] {
        let fixture = if native_graph {
            Fixture::native_graph()
        } else {
            Fixture::new()
        };
        let chain = raw(&fixture.chain);
        assert_eq!(fixture.host.bind_presentation(chain), OK);
        ROUTED.set(0);
        // Before SR runs, the reconstruction inputs wait; the presentation facts do not.
        for role in [RSF_GAME_RENDER_TRANSLUCENCY, RSF_GAME_RENDER_UI_COMPOSITE] {
            fixture.report(role, 1, chain);
        }
        fixture.report(RSF_GAME_RENDER_TEXTURE_BINDING, 0, chain);
        fixture.report(RSF_GAME_RENDER_WINDOW, 0, chain);
        assert_eq!(ROUTED.get(), if native_graph { 4 | 8 } else { 0 });
        fixture.present();
        assert_eq!(fixture.evaluate(), OK);
        ROUTED.set(0);
        for role in [
            RSF_GAME_RENDER_TRANSLUCENCY,
            RSF_GAME_RENDER_CLOUD_DEPTH,
            RSF_GAME_RENDER_MATERIALS,
            RSF_GAME_RENDER_UI_COMPOSITE,
        ] {
            fixture.report(role, 1, chain);
        }
        fixture.report(RSF_GAME_RENDER_WINDOW, 1, chain);
        assert_eq!(ROUTED.get(), if native_graph { 1 | 2 } else { 0 });
        // A window end on another chain is not this host's.
        ROUTED.set(0);
        fixture.report(RSF_GAME_RENDER_WINDOW, 0, raw(&fixture.output));
        assert_eq!(ROUTED.get(), 0);
    }
}

thread_local! {
    static ADAPTER_CHANGES: Cell<Option<(u32, i32)>> = const { Cell::new(None) };
    static ADAPTER_PRESENTS: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "C" fn adapter_read(_: Raw, values: *mut i32, count: u32) {
    for index in 0..count as usize {
        unsafe { values.add(index).write(70 + index as i32) };
    }
}

unsafe extern "C" fn adapter_changed(_: Raw, index: u32, value: i32) -> c_int {
    ADAPTER_CHANGES.set(Some((index, value)));
    1
}

unsafe extern "C" fn adapter_present(_: Raw, event: *const rsf_observer_present_event) {
    assert_eq!(unsafe { (*event).completed }, 0);
    ADAPTER_PRESENTS.set(ADAPTER_PRESENTS.get() + 1);
}

fn adapter(options: &[rsf_overlay_service_option]) -> rsf_d3d11_host_adapter {
    rsf_d3d11_host_adapter {
        struct_size: size_of::<rsf_d3d11_host_adapter>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        option_count: options.len() as u32,
        options: options.as_ptr(),
        options_read: Some(adapter_read),
        option_changed: Some(adapter_changed),
        present: Some(adapter_present),
        user: null_mut(),
    }
}

#[test]
fn adapter_options_follow_the_host_options_and_present_comes_first() {
    let option = |label: &'static CStr| rsf_overlay_service_option {
        label_utf8: label.as_ptr(),
        kind: RSF_OVERLAY_SERVICE_OPTION_TOGGLE,
        value: 0,
        minimum: 0,
        maximum: 1,
        step: 1,
        divisor: 1,
        restart: 1,
    };
    let options = [option(c"First"), option(c"Second")];
    let fixture = Fixture::new();
    let host = &fixture.host;
    let mut unlabelled = options;
    unlabelled[1].label_utf8 = ptr::null();
    assert_eq!(host.register(&adapter(&unlabelled)), INVALID);
    let too_many = vec![options[0]; RSF_OVERLAY_SERVICE_OPTION_LIMIT as usize];
    assert_eq!(host.register(&adapter(&too_many)), INVALID);
    let mut silent = adapter(&options);
    silent.option_changed = None;
    assert_eq!(host.register(&silent), INVALID);
    assert_eq!(host.register(&adapter(&options)), OK);
    assert_eq!(host.register(&adapter(&options)), BUSY, "registered once");

    let mut values = [-1; ADVANCED.len() + 2];
    unsafe { host_options(host.user(), values.as_mut_ptr(), values.len() as u32) };
    assert_eq!(&values[ADVANCED.len()..], &[70, 71]);
    assert_eq!(
        values[Advanced::Transport as usize],
        ADVANCED[Advanced::Transport as usize].value(host)
    );
    let first = ADVANCED.len() as u32;
    assert_eq!(unsafe { host_action(host.user(), first + 1, 1) }, 1);
    assert_eq!(ADAPTER_CHANGES.get(), Some((1, 1)));
    assert_eq!(unsafe { host_action(host.user(), first + 2, 1) }, 0);
    assert_eq!(
        unsafe { host_action(host.user(), 1, 0) },
        1,
        "host options stay first"
    );

    ADAPTER_PRESENTS.set(0);
    fixture.present();
    assert_eq!(
        ADAPTER_PRESENTS.get(),
        1,
        "the adapter sees Present before binding"
    );
    assert_eq!(host.bind_presentation(raw(&fixture.chain)), OK);
    fixture.present();
    assert_eq!(ADAPTER_PRESENTS.get(), 2);

    let late = Fixture::new();
    assert_eq!(late.host.bind_presentation(raw(&late.chain)), OK);
    assert_eq!(late.host.register(&adapter(&options)), NOT_READY);
}

#[test]
fn sr_and_the_overlay_never_depend_on_the_frame_generation_bridge() {
    // Native Presents reach the adapter before binding, and stand aside for a facade.
    let fixture = Fixture::native_graph();
    ADAPTER_PRESENTS.set(0);
    assert_eq!(fixture.host.register(&adapter(&[])), OK);
    let event = rsf_observer_present_event {
        struct_size: size_of::<rsf_observer_present_event>() as u32,
        swapchain: raw(&fixture.chain),
        ..Default::default()
    };
    OWNER_FACADE.set(0);
    unsafe { host_native_present(fixture.host.user(), &event) };
    assert_eq!(ADAPTER_PRESENTS.get(), 1);
    OWNER_FACADE.set(1);
    unsafe { host_native_present(fixture.host.user(), &event) };
    OWNER_FACADE.set(0);
    assert_eq!(ADAPTER_PRESENTS.get(), 1, "a facade's own events serve it");
    let packages = crate::sr_choices::Packages::all("d3d11-host-bridgeless");
    // Refused at install, or installed while its chain creation fell back to the native chain.
    for installed in [false, true] {
        let mut fixture = Fixture::rooted(packages.0.to_str().unwrap());
        fixture.host.setup.presentation_bridge = 1;
        fixture.host.setup.capabilities =
            RSF_D3D11_HOST_CAP_FG_INPUTS | RSF_D3D11_HOST_CAP_INPUT_TIMELINE;
        fixture
            .host
            .bridge_installed
            .store(installed, Ordering::SeqCst);
        OWNER_FACADE.set(0);
        // The game installed the observer for its own work; the host listens beside it.
        INSTALLS.set(0);
        assert_eq!(fixture.evaluate(), OK, "SR evaluates without the bridge");
        INSTALLS.set(1);
        let g = fixture.host.lock_graphics();
        assert!(g.configured && g.observer && !g.facade && g.running);
        assert_eq!(g.evaluated, 1);
        drop(g);
        let [sr, fg, latency] = OFFERED.get();
        assert_ne!(sr, 0, "the overlay offers SR");
        assert_eq!(
            (fg, latency),
            (0, 1 << REFLEX_OFF),
            "FG and latency unavailable"
        );
        let report = fixture.host.shared().report;
        assert_eq!(report.effective_backend, RSF_SR_DLSS);
        let text = |field: &[c_char]| unsafe { CStr::from_ptr(field.as_ptr()) }.to_owned();
        let expected = if installed {
            c"Frame generation unavailable: the presentation bridge could not create its chain"
        } else {
            c"Frame generation unavailable: the presentation bridge was refused"
        };
        assert_eq!(text(&report.fg_unavailable).as_c_str(), expected);
        assert_eq!(text(&report.reason).as_c_str(), c"", "SR itself is fine");
        assert!(fixture.host.native_presentation.load(Ordering::SeqCst));
        assert!(!fixture.host.bridge_bound.load(Ordering::SeqCst));
    }
}
