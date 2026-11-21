// SPDX-License-Identifier: MIT
//! Host services leased by the orchestrator between prepare and a successful stop.
use rsf_game_sys::{
    rsf_game_cpu_event, rsf_game_host_services, rsf_game_latency_event, rsf_game_render_feedback,
    rsf_game_render_pass,
};
use std::cell::UnsafeCell;
use std::ffi::{CString, c_void};

/// One owner's copy of the host services. It is written only by prepare and stop, while that
/// owner's hooks are not installed or enabled, and read by its callbacks in between.
pub(crate) struct HostCell(UnsafeCell<Option<rsf_game_host_services>>);

// Writes are ordered by the lifecycle before hook enable and after hook removal.
unsafe impl Sync for HostCell {}

impl HostCell {
    pub(crate) const fn new() -> Self {
        Self(UnsafeCell::new(None))
    }

    pub(crate) fn set(&self, services: Option<rsf_game_host_services>) {
        unsafe { *self.0.get() = services };
    }

    pub(crate) fn get(&self) -> Option<Host> {
        unsafe { *self.0.get() }.map(Host)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Host(pub(crate) rsf_game_host_services);

impl Host {
    pub(crate) fn session(&self) -> u64 {
        self.0.session_id
    }

    pub(crate) fn log(&self, message: &str) {
        let Some(log) = self.0.log else { return };
        if let Ok(text) = CString::new(message) {
            unsafe { log(self.0.user, text.as_ptr()) };
        }
    }

    pub(crate) fn feedback(&self, reason: u32, native_frame: u64) {
        let Some(feedback) = self.0.render_feedback else {
            return;
        };
        let value = rsf_game_render_feedback {
            struct_size: size_of::<rsf_game_render_feedback>() as u32,
            reason,
            session_id: self.0.session_id,
            native_frame,
        };
        unsafe { feedback(self.0.user, &value) };
    }

    pub(crate) fn latency(&self, marker: u32, frame: u64, qpc: (u64, u64)) -> Option<bool> {
        let process = self.0.process_latency?;
        let event = rsf_game_latency_event {
            struct_size: size_of::<rsf_game_latency_event>() as u32,
            marker,
            session_id: self.0.session_id,
            source_frame_id: frame,
            timestamp_qpc: qpc.0,
            qpc_frequency: qpc.1,
        };
        Some(unsafe { process(self.0.user, &event) } == 0)
    }

    pub(crate) fn cpu_event(&self, event: &rsf_game_cpu_event) -> bool {
        let Some(report) = self.0.cpu_event else {
            return false;
        };
        unsafe { report(self.0.user, event) };
        true
    }

    pub(crate) fn render_pass(
        &self,
        commands: *mut c_void,
        pass: &rsf_game_render_pass,
        begin: u32,
    ) {
        if let Some(render_pass) = self.0.render_pass {
            unsafe { render_pass(self.0.user, commands, pass, begin) };
        }
    }
}
