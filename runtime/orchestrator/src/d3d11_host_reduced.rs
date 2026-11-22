// SPDX-License-Identifier: MIT
// Graphics-owner products and the reduced-frame ticket of the D3D11 host.
use crate::auxiliary_sr::{AuxiliarySr, SurfaceKey};
use crate::auxiliary_sr_abi::rsf_aux_platform;
use crate::com_ref::ComRef;
use crate::d3d11_host_abi::{
    Pass, RSF_D3D11_REDUCED_CANCELLED, RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED, Surface,
    rsf_d3d11_reduced_frame,
};
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::c_void;
use std::ptr::NonNull;

pub(crate) type Raw = *mut c_void;
pub(crate) const MAX_RESERVED: usize = 8;
const MAX_SURFACES: usize = 10;
const NOT_READY: i32 = -3;

/// The auxiliary SR transaction used by temporal auxiliaries and reduced recovery.
pub(crate) trait Products {
    fn pending(&self) -> bool;
    fn matches(&self, scope: [u64; 3]) -> bool;
    fn reserve(
        &mut self,
        context: Raw,
        pass: &Pass,
        surfaces: &[Surface],
        log: rsf_game_log_fn,
        user: Raw,
        recovery: bool,
    ) -> bool;
    fn refine(&mut self, surfaces: &[Surface]) -> bool;
    fn seal(&mut self) -> bool;
    fn cancel(&mut self) -> bool;
    fn original_colour(&self) -> Raw;
    fn temporal_prepare(&mut self, surfaces: &[Surface]) -> bool;
    fn spatial_prepare(&mut self) -> bool;
    fn commit(&mut self, temporal: bool) -> bool;
    fn restore(&mut self) -> bool;

    fn prepare(
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

pub(crate) type ProductsFactory = fn(*const rsf_aux_platform) -> Option<Box<dyn Products>>;

pub(crate) fn auxiliary_products(platform: *const rsf_aux_platform) -> Option<Box<dyn Products>> {
    let platform = unsafe { *platform.as_ref()? };
    let owner = NonNull::new(crate::allocate_handle(AuxiliarySr::new(platform)))?;
    Some(unsafe { Box::from_raw(owner.as_ptr()) })
}

pub(crate) fn zeroed_pass() -> Pass {
    // SAFETY: the render pass is a C aggregate of integers, floats, nullable
    // pointers and nested aggregates of the same kinds.
    unsafe { std::mem::zeroed() }
}

/// One reduced-frame obligation. Scalar identity and explicit resource
/// leases survive the callback; borrowed pass resources do not.
pub(crate) struct Ticket {
    pub(crate) products: Box<dyn Products>,
    pub(crate) owner: Pass,
    pub(crate) surfaces: [Surface; MAX_SURFACES],
    pub(crate) leases: Vec<ComRef>,
    pub(crate) context: Option<ComRef>,
    pub(crate) chain: Option<ComRef>,
    pub(crate) token: u64,
    pub(crate) generation: u32,
    pub(crate) count: u32,
    pub(crate) flags: u32,
    pub(crate) disposition: u32,
    pub(crate) temporal_result: i32,
    pub(crate) pending: bool,
    pub(crate) attempted: bool,
    pub(crate) ordered: bool,
    pub(crate) temporal: bool,
    pub(crate) sealed: bool,
}

impl Ticket {
    pub(crate) fn new(products: Box<dyn Products>) -> Self {
        Self {
            products,
            owner: zeroed_pass(),
            surfaces: [Surface::default(); MAX_SURFACES],
            leases: Vec::new(),
            context: None,
            chain: None,
            token: 0,
            generation: 0,
            count: 0,
            flags: 0,
            disposition: 0,
            temporal_result: NOT_READY,
            pending: false,
            attempted: false,
            ordered: true,
            temporal: false,
            sealed: false,
        }
    }

    pub(crate) fn context_ptr(&self) -> Raw {
        self.context
            .as_ref()
            .map_or(std::ptr::null_mut(), ComRef::as_ptr)
    }

    pub(crate) fn matches(&self, p: &Pass, crop: bool) -> bool {
        let a = &self.owner;
        p.role == a.role
            && p.session_id == a.session_id
            && p.source_frame_id == a.source_frame_id
            && p.submission_id == a.submission_id
            && p.native_frame == a.native_frame
            && p.view_key == a.view_key
            && p.family_key == a.family_key
            && p.scope_id == a.scope_id
            && p.pass_key == a.pass_key
            && p.viewport_key == a.viewport_key
            && p.window_key == a.window_key
            && p.rhi_viewport_key == a.rhi_viewport_key
            && p.resource_generation == a.resource_generation
            && p.swapchain == a.swapchain
            && p.color_input == a.color_input
            && p.color_output == a.color_output
            && p.color_output_readable == a.color_output_readable
            && p.output_rect == a.output_rect
            && (!crop || p.render_rect == a.render_rect)
    }

    pub(crate) fn temporal_surfaces(&self, request: Option<&[Surface]>) -> bool {
        let count = self.count as usize;
        if count == 0 {
            return request.is_none_or(<[Surface]>::is_empty);
        }
        let Some(request) = request else {
            return false;
        };
        request.len() == count
            && request
                .iter()
                .zip(&self.surfaces)
                .all(|(p, a)| SurfaceKey::from(a).matches(p))
    }

    pub(crate) fn output(&self, request: &mut rsf_d3d11_reduced_frame) {
        request.token = self.token;
        request.disposition = self.disposition;
        request.result_flags = self.flags;
        request.temporal_result = self.temporal_result;
    }

    /// Keep exact context and chain identity for idempotent acknowledgement.
    pub(crate) fn release(&mut self) {
        self.leases.clear();
        self.pending = false;
    }

    /// A retired ticket stays an exact idempotent acknowledgement until the
    /// next successful RESERVE.
    pub(crate) fn acknowledges(&self, operation: u32) -> bool {
        use crate::d3d11_host_abi::{
            RSF_D3D11_REDUCED_CANCEL, RSF_D3D11_REDUCED_FINISH, RSF_D3D11_REDUCED_RETIRE,
        };
        operation == RSF_D3D11_REDUCED_RETIRE
            || (operation == RSF_D3D11_REDUCED_FINISH
                && self.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED != 0)
            || (operation == RSF_D3D11_REDUCED_CANCEL
                && self.disposition == RSF_D3D11_REDUCED_CANCELLED)
    }
}
