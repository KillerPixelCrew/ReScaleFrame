// SPDX-License-Identifier: MIT
//! Rendered-source provider transitions and their actual application receipts.
use super::*;
use rsf_game_sys::RSF_OK;

#[derive(Clone, Copy)]
enum Phase {
    ReducedWarmup,
    LateEnable,
    NativeSrOff,
    NativeSrOn,
    FgOff,
}

struct Scenario<'a> {
    resources: &'a rsf_d3d11_fixture_resources,
    calls: &'a rsf_d3d11_fixture_fg_calls,
    frame: Frame,
    state: State,
}

impl Scenario<'_> {
    fn submit(&mut self) {
        if let Err(label) = unsafe { run(self.resources, self.calls, &self.frame, &mut self.state) }
        {
            self.state.verify(false, label);
        }
    }

    fn submit_if_ready(&mut self) {
        if !self.state.failed {
            self.submit();
        }
    }

    fn phase(&mut self, phase: Phase) {
        let count = match phase {
            Phase::ReducedWarmup | Phase::LateEnable => 10,
            Phase::NativeSrOff | Phase::NativeSrOn => 12,
            Phase::FgOff => 3,
        };
        self.frame.primary = true;
        for index in 0..count {
            let first = index == 0;
            let (reconstruct, reset, expect_capture, cleanup) = match phase {
                Phase::ReducedWarmup => (true, first, 0, self.frame.scoped && index == 9),
                Phase::LateEnable => (true, false, if index < 2 { -1 } else { 1 }, false),
                Phase::NativeSrOff => (false, first, if first { -1 } else { 1 }, false),
                Phase::NativeSrOn => (true, first, 1, false),
                Phase::FgOff => (false, false, 0, false),
            };
            self.frame.reconstruct = reconstruct;
            self.frame.reset = reset;
            self.frame.expect_capture = expect_capture;
            self.frame.cleanup = cleanup;
            if self.state.failed {
                break;
            }
            self.submit();
        }
        self.frame.cleanup = false;
    }

    fn settings(
        &self,
        settings: &mut rsf_render_settings,
        label: &'static str,
    ) -> Result<(), &'static str> {
        let get = self
            .calls
            .settings_get
            .ok_or("overlay settings get callback")?;
        require(unsafe { get(self.calls.user, settings) } == RSF_OK, label)
    }

    fn apply(
        &self,
        settings: &rsf_render_settings,
        label: &'static str,
    ) -> Result<(), &'static str> {
        let apply = self
            .calls
            .settings_apply
            .ok_or("overlay settings apply callback")?;
        require(unsafe { apply(self.calls.user, settings) } == RSF_OK, label)
    }

    fn generation(&self) -> Result<u64, &'static str> {
        let query = self
            .calls
            .generation
            .ok_or("provider generation callback")?;
        let mut generation = 0;
        require(
            unsafe { query(self.calls.user, &mut generation) } == RSF_OK,
            "read actual provider generation",
        )?;
        Ok(generation)
    }

    fn backend(&self) -> Result<u32, &'static str> {
        let query = self
            .calls
            .backend
            .ok_or("active provider backend callback")?;
        let backend = unsafe { query(self.calls.user) };
        require(backend != u32::MAX, "read actual provider backend")?;
        Ok(backend)
    }

    fn switch_result(&self) -> Result<i32, &'static str> {
        let query = self
            .calls
            .switch_result
            .ok_or("provider switch result callback")?;
        let mut result = 0;
        require(
            unsafe { query(self.calls.user, &mut result) } == RSF_OK,
            "read actual provider switch result",
        )?;
        Ok(result)
    }
}

fn fg_status() -> (bool, rsf_native_fg_status) {
    let mut status = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    (
        unsafe { rsf_native_fg_status_get(&mut status) } != 0,
        status,
    )
}

fn run_scenario(fixture: &mut Scenario<'_>) -> Result<i32, &'static str> {
    println!("Phase: one complete source-owned bootstrap scene, SR/FG Off");
    fixture.submit();
    let bootstrap_sources = fixture.state.source;
    require(
        bootstrap_sources == 1
            && fixture.state.app_presents == 1
            && fixture.state.captured == 0
            && fixture.state.sr_evaluated == 0,
        "bootstrap must complete exactly one application Present without capture or SR evaluation",
    )?;
    require(
        !fixture.state.failed,
        "complete source-owned bootstrap admission",
    )?;

    let mut settings = rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..unsafe { std::mem::zeroed() }
    };
    fixture.settings(&mut settings, "read initial warmup settings")?;
    settings.sr_enabled = 1;
    settings.sr_backend = RSF_SR_DLSS;
    settings.sr_quality = 1;
    settings.fg_enabled = 0;
    settings.fg_backend = RSF_FG_BACKEND_FSR3;
    settings.generated_frames = 1;
    settings.latency = RSF_REFLEX_OFF;
    settings.frame_limit_us = 0;
    fixture.apply(&settings, "apply reduced DLSS warmup with FG Off")?;
    fixture.frame.active_width = 640;
    fixture.frame.active_height = 360;
    fixture.frame.native_reinsertion = true;
    println!("Phase: bounded 10-source DLSS Quality warmup, 640x360 in 960x540");
    let previous = fixture.state.sr_evaluated;
    fixture.phase(Phase::ReducedWarmup);
    let state = &mut fixture.state;
    state.verify(
        state.sr_evaluated == previous + 10,
        "all bounded reduced warmup sources must evaluate SR",
    );
    let r = fixture.resources;
    let mut outside = 0u32;
    require(
        unsafe {
            rsf_d3d11_host_fixture_read_pixel(
                r.device,
                r.context,
                r.colour,
                r.width - 2,
                r.height - 2,
                size_of::<u32>() as u32,
                (&raw mut outside).cast(),
            )
        } != 0,
        "read native reinsertion outside-active pixel",
    )?;
    println!(
        "Native reinsertion outside-active pixel=0x{outside:08x} original_marker=0x{:08x}",
        fixture.frame.outside_marker
    );
    state.verify(
        outside != fixture.frame.outside_marker,
        "DLSS reinsertion must overwrite native allocation outside reduced input rectangle",
    );
    let (read, warmed) = fg_status();
    state.verify(read, "read warmed FG-Off status");
    state.verify(
        warmed.requested.mode == RSF_FG_OFF && warmed.tagged_frames == 0,
        "warmup must remain FG Off and untagged",
    );

    let presents_before_late_enable = state.app_presents;
    let captured_before_late_enable = state.captured;
    let generation_before_late_enable = fixture.generation()?;
    settings.fg_enabled = 1;
    fixture.apply(
        &settings,
        "enable FSR3 through public overlay after warmed reduced DLSS",
    )?;
    println!("Phase: bounded 10-source late FSR3 activation over warmed reduced in-place DLSS");
    fixture.phase(Phase::LateEnable);
    let (read, late) = fg_status();
    fixture
        .state
        .verify(read, "read post-switch reduced FG status");
    let backend = fixture.backend()?;
    let switch_result = fixture.switch_result()?;
    let generation = fixture.generation()?;
    let state = &mut fixture.state;
    println!(
        "Late-enable receipt: backend={backend} switch_result={switch_result} generation={generation} previous_generation={generation_before_late_enable} captures={} application_calls={} tags={} generated_callbacks={} statistics_flags=0x{:x} vendor_generated_presented={}",
        state.captured - captured_before_late_enable,
        state.app_presents - presents_before_late_enable,
        late.tagged_frames,
        late.vendor.generated_callbacks,
        late.vendor.valid_statistics,
        late.vendor.generated_presented
    );
    state.verify(
        state.app_presents == presents_before_late_enable + 10,
        "late-enable phase must complete ten application Presents",
    );
    if backend != RSF_FG_BACKEND_FSR3 || late.vendor.generated_callbacks == 0 {
        state.unavailable = true;
    } else {
        state.verify(switch_result == RSF_OK, "late FSR switch must succeed");
        state.verify(
            generation > generation_before_late_enable,
            "late FSR switch must advance provider generation",
        );
        state.verify(
            state.captured > captured_before_late_enable
                && late.tagged_frames > warmed.tagged_frames,
            "late activation must capture and tag reduced native inputs",
        );
    }
    let late_generated =
        late.tagged_frames > warmed.tagged_frames && late.vendor.generated_callbacks != 0;

    settings.sr_enabled = 0;
    settings.sr_quality = 0;
    fixture.apply(&settings, "start original SR-Off generation phase")?;
    fixture.frame.active_width = r.width;
    fixture.frame.active_height = r.height;
    fixture.frame.native_reinsertion = false;
    fixture.phase(Phase::NativeSrOff);
    let (read, off_sr) = fg_status();
    require(read, "query SR-Off FG status")?;
    let off_sr_generated = off_sr.tagged_frames > late.tagged_frames
        && off_sr.vendor.generated_callbacks > late.vendor.generated_callbacks;
    if off_sr_generated {
        let backend = fixture.backend()?;
        fixture.state.verify(
            backend == RSF_FG_BACKEND_FSR3,
            "generated SR-Off phase must use FSR3",
        );
    }
    println!(
        "SR-Off receipt: tagged={} generated_callbacks={} provenance={} input_qpc={}",
        off_sr.tagged_frames,
        off_sr.vendor.generated_callbacks,
        off_sr.source_provenance,
        off_sr.input_qpc
    );

    fixture.settings(&mut settings, "read shared rendering settings")?;
    settings.sr_enabled = 1;
    settings.sr_quality = 0;
    fixture.apply(&settings, "apply shared rendering settings")?;
    fixture.phase(Phase::NativeSrOn);
    let (read, before_reset) = fg_status();
    require(read, "query before-reset FG status")?;
    fixture.frame.reset = true;
    fixture.submit_if_ready();
    let (read, after_reset) = fg_status();
    fixture.state.verify(read, "after-reset FG status query");
    println!(
        "Reset tags: before={} after={}",
        before_reset.tagged_frames, after_reset.tagged_frames
    );
    fixture.state.verify(
        after_reset.tagged_frames == before_reset.tagged_frames,
        "explicit reset must not increment tag count",
    );
    fixture.frame.reset = false;
    fixture.submit_if_ready();

    let (read, before_refusal) = fg_status();
    require(read, "query before-late-refusal FG status")?;
    fixture.frame.refuse_inputs_late = true;
    fixture.submit_if_ready();
    fixture.frame.refuse_inputs_late = false;
    let (read, after_refusal) = fg_status();
    fixture
        .state
        .verify(read, "after-late-refusal FG status query");
    fixture.state.verify(
        after_refusal.tagged_frames == before_refusal.tagged_frames
            && after_refusal.reason == RSF_NATIVE_FG_REASON_RESET,
        "genuine late FG refusal remains sticky despite eligible duplicate and WINDOW",
    );
    fixture.submit_if_ready();
    if fixture.frame.scoped {
        let (read, before) = fg_status();
        fixture
            .state
            .verify(read, "query before malformed repeated depth lease");
        fixture.frame.refuse_depth_late = true;
        fixture.submit_if_ready();
        fixture.frame.refuse_depth_late = false;
        let (read, after) = fg_status();
        fixture.state.verify(
            read && after.tagged_frames == before.tagged_frames
                && after.reason == RSF_NATIVE_FG_REASON_RESET,
            "refused repeated depth descriptor cannot authorize retained same-source inputs",
        );
        fixture.submit_if_ready();
    }
    let (read, before_invalid) = fg_status();
    require(read, "query before-nonprimary FG status")?;
    fixture.frame.primary = false;
    fixture.frame.expect_capture = -1;
    fixture.submit_if_ready();
    let (read, after_invalid) = fg_status();
    fixture
        .state
        .verify(read, "after-nonprimary FG status query");
    println!(
        "Nonprimary tags: before={} after={}",
        before_invalid.tagged_frames, after_invalid.tagged_frames
    );
    fixture.state.verify(
        after_invalid.tagged_frames == before_invalid.tagged_frames,
        "nonprimary source must not increment tag count",
    );

    settings.fg_enabled = 0;
    settings.sr_enabled = 0;
    fixture.apply(&settings, "disable shared FG and SR")?;
    fixture.phase(Phase::FgOff);
    let (stopped_ok, stopped) = fg_status();
    let mut latency = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..unsafe { std::mem::zeroed() }
    };
    let latency_ok = unsafe { rsf_native_latency_status_get(&mut latency) } != 0;
    println!(
        "Final FG: query={} requested={} effective={} tagged={} expected_tags={} statistics_flags=0x{:x} present_callbacks={} generated_callbacks={}",
        u32::from(stopped_ok),
        stopped.requested.mode,
        stopped.vendor.effective_mode,
        stopped.tagged_frames,
        after_invalid.tagged_frames,
        stopped.vendor.valid_statistics,
        stopped.vendor.present_callbacks,
        stopped.vendor.generated_callbacks
    );
    println!(
        "Final latency: query={} version={} owner={} available={} paced={} markers={} effective={} PCL_available={} report_available={}",
        u32::from(latency_ok),
        latency.version,
        latency.owner,
        latency.available,
        latency.paced_frames,
        latency.marker_calls,
        latency.effective_mode,
        latency.pcl_available,
        latency.report_available
    );
    let state = &mut fixture.state;
    state.verify(stopped_ok, "final FG status query");
    state.verify(
        stopped.requested.mode == RSF_FG_OFF,
        "final FG request must be Off",
    );
    state.verify(
        stopped.tagged_frames == after_invalid.tagged_frames,
        "FG Off must not increment tag count",
    );
    state.verify(latency_ok, "final latency status query with version 1");
    state.verify(
        latency.paced_frames == 0,
        "final latency paced count must be zero",
    );
    state.verify(
        latency.marker_calls == 0,
        "final latency marker count must be zero",
    );
    state.verify(
        latency.effective_mode == RSF_REFLEX_OFF,
        "final effective latency must be Off",
    );
    state.verify(
        latency.pcl_available == 0,
        "rendered-source latency PCL availability must remain zero",
    );
    state.verify(
        latency.report_available == 0,
        "rendered-source physical latency report availability must remain zero",
    );
    state.failed |= !unsafe { finish_device(r) };
    println!(
        "Receipts: sources={} bootstrap_sources={bootstrap_sources} test_sources={} captured={} SR_evaluated={} application_Present_calls={} SR_Off_generated={} occluded={}",
        state.source,
        state.source - bootstrap_sources,
        state.captured,
        state.sr_evaluated,
        state.app_presents,
        u32::from(off_sr_generated),
        u32::from(state.occluded)
    );
    require(!state.failed, "one or more labeled fixture checks failed")?;
    if !late_generated || !off_sr_generated || state.unavailable {
        println!("INCOMPLETE: missing late-enable or SR-Off callback receipt");
        return Ok(77);
    }
    require(
        after_reset.tagged_frames > off_sr.tagged_frames
            && after_reset.vendor.generated_callbacks > off_sr.vendor.generated_callbacks,
        "SR-On phase must advance both tags and generated callbacks",
    )?;
    require(
        state.sr_evaluated != 0,
        "SR-On phase must evaluate reconstruction",
    )?;
    println!(
        "Rendered-source FSR generated callback regression passed; physical scanout and latency remain unmeasured"
    );
    Ok(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_rendered_fg(
    resources: *const rsf_d3d11_fixture_resources,
    calls: *const rsf_d3d11_fixture_fg_calls,
    scoped: u32,
    first_present: i32,
    native_pixel: u32,
    outside_marker: u32,
) -> i32 {
    let resources = unsafe { &*resources };
    let mut fixture = Scenario {
        resources,
        calls: unsafe { &*calls },
        state: State {
            occluded: first_present == DXGI_STATUS_OCCLUDED.0,
            ..Default::default()
        },
        frame: Frame {
            reconstruct: false,
            primary: true,
            reset: true,
            expect_capture: 0,
            scoped: scoped != 0,
            native_reinsertion: false,
            active_width: resources.width,
            active_height: resources.height,
            native_pixel,
            outside_marker,
            cleanup: false,
            refuse_inputs_late: false,
            refuse_depth_late: false,
        },
    };
    run_scenario(&mut fixture).unwrap_or_else(|label| {
        eprintln!("FAIL: {label}");
        1
    })
}
