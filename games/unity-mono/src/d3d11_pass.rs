// SPDX-License-Identifier: MIT
//! Pass assembly and route decisions of the Unity package's D3D11 bridge
//! (bridge_d3d11.rs).
//!
//! For each render event that took a packet, the plan chooses the route from
//! the event offset (reconstruction, window, Editor preview window or HUD-less scene), assembles
//! the host render pass from the packet, and lists which shared surface each input is copied into
//! and the D3D12 state the pass uses it in. `shared_format` chooses the shared surface format for
//! a surface slot and its source description. `apply_pending_change` applies a pending
//! engine policy change to a reconstruction pass once every submission has completed, and
//! `retire_submission` decides whether a submission whose queue work went out is retired or
//! stops the adapter.
//!
//! D3D11, direct D3D12 and Vulkan share scalar pass fields. Direct D3D12 binds packet
//! resources and supplies camera-valid FG inputs on inputs-only probes.
use crate::sys::{
    RSF_UNITY_PACKET_INPUTS_ONLY, RSF_UNITY_PACKET_PROBE, RSF_UNITY_PACKET_RESET, rsf_unity_packet,
};
use rsf_game_sys::{RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_SR, rsf_game_render_pass};

/// RSF_SCREEN_FLIGHT, a cast macro bindgen does not evaluate. The package's SR pass replaces it
/// from its gameplay setting (package_state.rs).
const SCREEN_FLIGHT: u32 = 4;

/// The C++ `make_pass`: one SR pass for the packet's view, frame and camera. A probe packet
/// carries camera constants only, so its camera is not valid for reconstruction.
fn base_pass(packet: &rsf_unity_packet) -> rsf_game_render_pass {
    // Every field is an integer, float, pointer or array of them.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = packet.session_id;
    pass.family_key = packet.view_key;
    pass.view_key = packet.view_key;
    pass.history_key = packet.view_key;
    pass.native_frame = packet.frame_id;
    pass.source_frame_id = packet.frame_id;
    pass.resource_generation = packet.generation;
    pass.screen = SCREEN_FLIGHT;
    pass.flags = packet.flags & RSF_UNITY_PACKET_RESET;
    pass.camera = packet.camera;
    pass.camera_valid = u32::from(packet.flags & RSF_UNITY_PACKET_PROBE == 0);
    pass.render_rect[2] = packet.camera.render_width as i32;
    pass.render_rect[3] = packet.camera.render_height as i32;
    pass.output_rect[2] = packet.camera.output_width as i32;
    pass.output_rect[3] = packet.camera.output_height as i32;
    pass.motion_to_uv = [1.0, 1.0];
    pass.motion_camera_included = 1;
    pass.previous_clip_to_clip = packet.previous_clip_to_clip;
    pass
}

/// Copies the shared scalar fields. The caller supplies the route and resources.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_build_scalar_pass(
    packet: *const rsf_unity_packet,
    output: *mut rsf_game_render_pass,
) {
    if let (Some(packet), Some(output)) = unsafe { (packet.as_ref(), output.as_mut()) } {
        *output = base_pass(packet);
    }
}

pub(crate) fn build_d3d12_pass(packet: &rsf_unity_packet) -> rsf_game_render_pass {
    let mut pass = base_pass(packet);
    pass.color_input = packet.color;
    pass.color_output = packet.output;
    pass.depth = packet.depth;
    pass.motion = packet.motion;
    if packet.flags & RSF_UNITY_PACKET_INPUTS_ONLY != 0 {
        pass.role = RSF_GAME_RENDER_FG_INPUTS;
        pass.camera_valid = 1;
    }
    pass
}

#[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
mod d3d11 {
    use super::{base_pass, rsf_game_render_pass, rsf_unity_packet};
    #[cfg(all(windows, not(feature = "mono-plugin")))]
    use crate::bridge_interop::D3D11;
    #[cfg(all(windows, not(feature = "mono-plugin")))]
    use crate::bridge_state::{
        rsf_unity_frames_activate, rsf_unity_frames_pending_change, rsf_unity_frames_settle_change,
    };
    #[cfg(all(windows, not(feature = "mono-plugin")))]
    use crate::d3d11_state::wait_completion;
    use crate::sys::RSF_UNITY_PREVIEW_SOURCE_COMMON;
    use rsf_game_sys::{
        RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_WINDOW,
    };

    // Offsets in the bridge's reserved event range (ReserveEventIDRange(3)). The managed package
    // issues the reconstruction packet on EventId, the window packet on EventId + 1 and the HUD-less
    // packet on EventId + 2.
    pub(super) const EVENT_RECONSTRUCT: u32 = 0;
    pub(crate) const EVENT_WINDOW: u32 = 1;
    pub(super) const EVENT_HUDLESS: u32 = 2;

    // D3D11 pass routes.
    pub(crate) const ROUTE_NONE: u32 = 0;
    pub(crate) const ROUTE_WINDOW: u32 = 1;
    pub(crate) const ROUTE_PREVIEW: u32 = 2;
    pub(super) const ROUTE_RECONSTRUCT: u32 = 3;
    pub(super) const ROUTE_HUDLESS: u32 = 4;

    // Submission retirement results.
    pub(crate) const RETIRED: u32 = 0;
    pub(crate) const HANDBACK_FAILED: u32 = 1;
    pub(super) const SIGNAL_FAILED: u32 = 2;

    /// Shared surface slots of the graphics record (d3d11_state.rs): colour, depth, motion and output
    /// keep their packet order, then the HUD-less scene and the Editor preview source.
    pub(super) const SURFACE_DEPTH: u32 = 1;
    pub(super) const SURFACE_MOTION: u32 = 2;
    pub(super) const SURFACE_HUDLESS: u32 = 4;
    pub(super) const SURFACE_PREVIEW: u32 = 5;

    // D3D12_RESOURCE_STATES values. Every planned state starts and ends the
    // pass in D3D12_RESOURCE_STATE_COMMON (0).
    pub(super) const STATE_RENDER_TARGET: u32 = 0x4;
    pub(super) const STATE_NON_PIXEL_SHADER_RESOURCE: u32 = 0x40;
    pub(super) const STATE_COPY_DEST: u32 = 0x400;

    // DXGI_FORMAT values.
    pub(super) const R16G16B16A16_TYPELESS: u32 = 9;
    pub(super) const R16G16B16A16_FLOAT: u32 = 10;
    pub(super) const R8G8B8A8_TYPELESS: u32 = 27;
    pub(super) const R8G8B8A8_UNORM: u32 = 28;
    pub(super) const R8G8B8A8_UNORM_SRGB: u32 = 29;
    pub(super) const R16G16_TYPELESS: u32 = 33;
    pub(super) const R16G16_FLOAT: u32 = 34;
    pub(super) const R32_TYPELESS: u32 = 39;
    pub(super) const D32_FLOAT: u32 = 40;
    pub(super) const R32_FLOAT: u32 = 41;
    pub(super) const B8G8R8A8_UNORM: u32 = 87;
    pub(super) const B8G8R8A8_TYPELESS: u32 = 90;
    pub(super) const B8G8R8A8_UNORM_SRGB: u32 = 91;

    #[cfg(windows)]
    const _: () = {
        use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};
        assert!(STATE_RENDER_TARGET == D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32);
        assert!(
            STATE_NON_PIXEL_SHADER_RESOURCE
                == D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32
        );
        assert!(STATE_COPY_DEST == D3D12_RESOURCE_STATE_COPY_DEST.0 as u32);
        assert!(R16G16B16A16_TYPELESS == DXGI_FORMAT_R16G16B16A16_TYPELESS.0 as u32);
        assert!(R16G16B16A16_FLOAT == DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32);
        assert!(R8G8B8A8_TYPELESS == DXGI_FORMAT_R8G8B8A8_TYPELESS.0 as u32);
        assert!(R8G8B8A8_UNORM == DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32);
        assert!(R8G8B8A8_UNORM_SRGB == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32);
        assert!(R16G16_TYPELESS == DXGI_FORMAT_R16G16_TYPELESS.0 as u32);
        assert!(R16G16_FLOAT == DXGI_FORMAT_R16G16_FLOAT.0 as u32);
        assert!(R32_TYPELESS == DXGI_FORMAT_R32_TYPELESS.0 as u32);
        assert!(D32_FLOAT == DXGI_FORMAT_D32_FLOAT.0 as u32);
        assert!(R32_FLOAT == DXGI_FORMAT_R32_FLOAT.0 as u32);
        assert!(B8G8R8A8_UNORM == DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32);
        assert!(B8G8R8A8_TYPELESS == DXGI_FORMAT_B8G8R8A8_TYPELESS.0 as u32);
        assert!(B8G8R8A8_UNORM_SRGB == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as u32);
    };

    /// The plan of one render event. The recording path fills its native resources.
    pub(crate) struct PassPlan {
        pub(crate) route: u32,
        /// HUD-less without a packet colour: the colour is buffer 0 of Unity's swapchain.
        pub(crate) swapchain_color: u32,
        /// The output surface is copied back into the packet's output after the hand-back.
        pub(crate) copy_back: u32,
        /// A pending policy change may apply to this pass.
        pub(super) reconfigurable: u32,
        /// The shared surface slot each input (colour, depth, motion, output) is copied into.
        pub(crate) surfaces: [u32; 4],
        /// The D3D12 state each input surface is in while the pass records.
        pub(crate) states: [u32; 4],
        pub(crate) pass: rsf_game_render_pass,
    }

    /// The route and pass for event `offset` in the reserved range. `editor` is the package running
    /// in the Unity Editor, whose window pass presents a copy of the packet colour instead of the
    /// engine swapchain.
    pub(crate) fn plan(offset: u32, packet: &rsf_unity_packet, editor: bool) -> PassPlan {
        let mut plan = PassPlan {
            route: ROUTE_NONE,
            swapchain_color: 0,
            copy_back: 0,
            reconfigurable: 0,
            surfaces: [0, SURFACE_DEPTH, SURFACE_MOTION, 3],
            states: [0; 4],
            pass: base_pass(packet),
        };
        match offset {
            EVENT_RECONSTRUCT => {
                plan.route = ROUTE_RECONSTRUCT;
                plan.copy_back = 1;
                plan.reconfigurable = 1;
                plan.states = [
                    STATE_NON_PIXEL_SHADER_RESOURCE,
                    STATE_NON_PIXEL_SHADER_RESOURCE,
                    STATE_NON_PIXEL_SHADER_RESOURCE,
                    STATE_COPY_DEST,
                ];
            }
            EVENT_WINDOW => {
                plan.pass.role = RSF_GAME_RENDER_WINDOW;
                if editor && !packet.color.is_null() {
                    plan.route = ROUTE_PREVIEW;
                    plan.surfaces[0] = SURFACE_PREVIEW;
                    plan.pass.flags |= RSF_UNITY_PREVIEW_SOURCE_COMMON;
                } else {
                    plan.route = ROUTE_WINDOW;
                }
            }
            EVENT_HUDLESS => {
                plan.route = ROUTE_HUDLESS;
                plan.swapchain_color = u32::from(packet.color.is_null());
                plan.surfaces = [SURFACE_HUDLESS; 4];
                plan.states = [STATE_RENDER_TARGET; 4];
                plan.pass.role = RSF_GAME_RENDER_FINAL_SCENE;
            }
            _ => {}
        }
        plan
    }

    /// The shared surface format for slot `index` copied from a source of `source` format: depth is
    /// R32_FLOAT and motion R16G16_FLOAT, typeless and sRGB colour formats take their linear typed
    /// view, any other format is kept.
    pub(crate) fn shared_format(index: u32, source: u32) -> Option<u32> {
        match index {
            SURFACE_DEPTH => {
                matches!(source, R32_TYPELESS | D32_FLOAT | R32_FLOAT).then_some(R32_FLOAT)
            }
            SURFACE_MOTION => {
                matches!(source, R16G16_TYPELESS | R16G16_FLOAT).then_some(R16G16_FLOAT)
            }
            _ => Some(match source {
                R16G16B16A16_TYPELESS => R16G16B16A16_FLOAT,
                R8G8B8A8_TYPELESS | R8G8B8A8_UNORM_SRGB => R8G8B8A8_UNORM,
                B8G8R8A8_TYPELESS | B8G8R8A8_UNORM_SRGB => B8G8R8A8_UNORM,
                format => format,
            }),
        }
    }

    /// A pending policy change applies on a reconstruction once every submission completed: read the
    /// pending generation, wait for the last completion value handed out, settle that generation and
    /// mark the pass. False when the wait refused; the change stays pending and the pass is dropped.
    pub(super) fn apply_change(
        plan: &mut PassPlan,
        pending: impl FnOnce() -> Option<u32>,
        wait: impl FnOnce() -> bool,
        settle: impl FnOnce(u32),
    ) -> bool {
        if plan.reconfigurable == 0 {
            return true;
        }
        let Some(generation) = pending() else {
            return true;
        };
        if !wait() {
            return false;
        }
        settle(generation);
        plan.pass.flags |= RSF_GAME_RENDER_RECONFIGURE;
        plan.pass.camera_valid = 0;
        true
    }

    /// A submission whose queue work went out. `signalled` is the completion signal on the bridge's
    /// fence, which the native side publishes even when the D3D11 hand-back refused, so release still
    /// waits for the work. Either failure stops the adapter.
    pub(super) fn retire(signalled: bool, handed_back: bool) -> u32 {
        if !signalled {
            SIGNAL_FAILED
        } else if !handed_back {
            HANDBACK_FAILED
        } else {
            RETIRED
        }
    }

    /// The caller holds a frame callback, which keeps the graphics record published for the wait.
    #[cfg(all(windows, not(feature = "mono-plugin")))]
    pub(crate) fn apply_pending_change(plan: &mut PassPlan) -> bool {
        apply_change(
            plan,
            || {
                let mut generation = 0;
                (unsafe { rsf_unity_frames_pending_change(&mut generation) } != 0)
                    .then_some(generation)
            },
            || {
                let submitted = D3D11.lock().submitted();
                wait_completion(submitted)
            },
            |generation| {
                rsf_unity_frames_settle_change(generation);
            },
        )
    }

    #[cfg(all(windows, not(feature = "mono-plugin")))]
    pub(crate) fn retire_submission(signalled: bool, handed_back: bool) -> u32 {
        let verdict = retire(signalled, handed_back);
        if verdict != RETIRED {
            rsf_unity_frames_activate(0);
        }
        verdict
    }
}

#[cfg(test)]
pub(crate) use d3d11::EVENT_WINDOW;
#[cfg(all(windows, not(feature = "mono-plugin")))]
pub(crate) use d3d11::{
    HANDBACK_FAILED, PassPlan, RETIRED, ROUTE_NONE, ROUTE_PREVIEW, ROUTE_WINDOW,
    apply_pending_change, retire_submission,
};
#[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
pub(crate) use d3d11::{plan, shared_format};
#[cfg(test)]
mod tests {
    use super::d3d11::*;
    use super::*;
    use crate::sys::RSF_UNITY_PREVIEW_SOURCE_COMMON;
    use rsf_game_sys::{
        RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_WINDOW,
        rsf_camera_frame,
    };
    use std::ffi::c_void;
    use std::ptr;

    fn texture(value: usize) -> *mut c_void {
        ptr::without_provenance_mut(value)
    }

    fn packet() -> rsf_unity_packet {
        let mut camera: rsf_camera_frame = unsafe { std::mem::zeroed() };
        camera.render_width = 1280;
        camera.render_height = 720;
        camera.output_width = 2560;
        camera.output_height = 1440;
        let mut clip = [0.0; 16];
        clip[5] = 2.0;
        rsf_unity_packet {
            struct_size: size_of::<rsf_unity_packet>() as u32,
            abi_version: 2,
            session_id: 7,
            frame_id: 42,
            view_key: 9,
            generation: 3,
            flags: RSF_UNITY_PACKET_RESET | 8,
            color: texture(1),
            depth: texture(2),
            motion: texture(3),
            output: texture(4),
            camera,
            previous_clip_to_clip: clip,
        }
    }

    #[test]
    fn the_base_pass_is_the_native_make_pass() {
        let pass = base_pass(&packet());
        assert_eq!(pass.struct_size as usize, size_of::<rsf_game_render_pass>());
        assert_eq!(pass.role, RSF_GAME_RENDER_SR);
        assert_eq!(pass.session_id, 7);
        assert_eq!(
            (pass.family_key, pass.view_key, pass.history_key),
            (9, 9, 9)
        );
        assert_eq!((pass.native_frame, pass.source_frame_id), (42, 42));
        assert_eq!(pass.resource_generation, 3);
        assert_eq!(pass.screen, SCREEN_FLIGHT);
        // Only the reset flag carries over.
        assert_eq!(pass.flags, RSF_UNITY_PACKET_RESET);
        assert_eq!(pass.camera_valid, 1);
        assert_eq!(pass.render_rect, [0, 0, 1280, 720]);
        assert_eq!(pass.output_rect, [0, 0, 2560, 1440]);
        assert_eq!(pass.motion_to_uv, [1.0, 1.0]);
        assert_eq!(pass.motion_camera_included, 1);
        assert_eq!(pass.previous_clip_to_clip[5], 2.0);
        assert!(pass.color_input.is_null() && pass.swapchain.is_null());
        let mut probe = packet();
        probe.flags = RSF_UNITY_PACKET_PROBE;
        let pass = base_pass(&probe);
        assert_eq!((pass.flags, pass.camera_valid), (0, 0));
    }

    #[test]
    fn reconstruction_reads_inputs_and_copies_its_output_back() {
        let plan = plan(EVENT_RECONSTRUCT, &packet(), true);
        assert_eq!(plan.route, ROUTE_RECONSTRUCT);
        assert_eq!(plan.pass.role, RSF_GAME_RENDER_SR);
        assert_eq!(plan.surfaces, [0, 1, 2, 3]);
        assert_eq!(
            plan.states,
            [
                STATE_NON_PIXEL_SHADER_RESOURCE,
                STATE_NON_PIXEL_SHADER_RESOURCE,
                STATE_NON_PIXEL_SHADER_RESOURCE,
                STATE_COPY_DEST
            ]
        );
        assert_eq!(
            (plan.copy_back, plan.reconfigurable, plan.swapchain_color),
            (1, 1, 0)
        );
    }

    #[test]
    fn the_window_presents_the_swapchain_or_the_editor_copy() {
        let player = plan(EVENT_WINDOW, &packet(), false);
        assert_eq!(player.route, ROUTE_WINDOW);
        assert_eq!(player.pass.role, RSF_GAME_RENDER_WINDOW);
        assert_eq!(player.pass.flags & RSF_UNITY_PREVIEW_SOURCE_COMMON, 0);
        let preview = plan(EVENT_WINDOW, &packet(), true);
        assert_eq!(preview.route, ROUTE_PREVIEW);
        assert_eq!(preview.surfaces[0], SURFACE_PREVIEW);
        assert_ne!(preview.pass.flags & RSF_UNITY_PREVIEW_SOURCE_COMMON, 0);
        // The Editor without a colour presents nothing to copy and keeps the plain window pass.
        let mut empty = packet();
        empty.color = ptr::null_mut();
        assert_eq!(plan(EVENT_WINDOW, &empty, true).route, ROUTE_WINDOW);
        assert_eq!(
            (
                preview.copy_back,
                preview.reconfigurable,
                player.reconfigurable
            ),
            (0, 0, 0)
        );
    }

    #[test]
    fn the_hudless_scene_uses_its_own_surface_as_a_render_target() {
        let mut hudless = packet();
        hudless.flags = RSF_UNITY_PACKET_PROBE | 8;
        hudless.color = ptr::null_mut();
        let plan = plan(EVENT_HUDLESS, &hudless, false);
        assert_eq!(plan.route, ROUTE_HUDLESS);
        assert_eq!(plan.pass.role, RSF_GAME_RENDER_FINAL_SCENE);
        assert_eq!(plan.swapchain_color, 1);
        assert_eq!(plan.surfaces, [SURFACE_HUDLESS; 4]);
        assert_eq!(plan.states, [STATE_RENDER_TARGET; 4]);
        assert_eq!((plan.copy_back, plan.reconfigurable), (0, 0));
        assert_eq!(plan.pass.camera_valid, 0);
        let with_color = super::plan(EVENT_HUDLESS, &packet(), false);
        assert_eq!(with_color.swapchain_color, 0);
    }

    #[test]
    fn events_outside_the_range_plan_nothing() {
        assert_eq!(plan(3, &packet(), false).route, ROUTE_NONE);
        assert_eq!(plan(u32::MAX, &packet(), false).route, ROUTE_NONE);
    }

    #[test]
    fn direct_d3d12_accepts_camera_data_for_inputs_only_probes() {
        let mut packet = packet();
        packet.flags = RSF_UNITY_PACKET_PROBE | RSF_UNITY_PACKET_INPUTS_ONLY;
        let base = base_pass(&packet);
        assert_eq!(base.camera_valid, 0);
        assert!(base.motion.is_null());
        let pass = build_d3d12_pass(&packet);
        assert_eq!(pass.role, RSF_GAME_RENDER_FG_INPUTS);
        assert_eq!(pass.camera_valid, 1);
        assert_eq!(pass.motion, packet.motion);
    }

    #[test]
    fn shared_formats_follow_the_slot_and_the_source() {
        // The shared depth and motion surfaces accept only compatible typed families.
        assert_eq!(shared_format(SURFACE_DEPTH, D32_FLOAT), Some(R32_FLOAT));
        assert_eq!(shared_format(SURFACE_DEPTH, 45), None);
        assert_eq!(
            shared_format(SURFACE_MOTION, R16G16_TYPELESS),
            Some(R16G16_FLOAT)
        );
        assert_eq!(shared_format(SURFACE_MOTION, R16G16B16A16_FLOAT), None);
        for index in [0, 3, SURFACE_HUDLESS, SURFACE_PREVIEW] {
            assert_eq!(
                shared_format(index, R16G16B16A16_TYPELESS),
                Some(R16G16B16A16_FLOAT)
            );
            assert_eq!(
                shared_format(index, R8G8B8A8_TYPELESS),
                Some(R8G8B8A8_UNORM)
            );
            assert_eq!(
                shared_format(index, R8G8B8A8_UNORM_SRGB),
                Some(R8G8B8A8_UNORM)
            );
            assert_eq!(
                shared_format(index, B8G8R8A8_TYPELESS),
                Some(B8G8R8A8_UNORM)
            );
            assert_eq!(
                shared_format(index, B8G8R8A8_UNORM_SRGB),
                Some(B8G8R8A8_UNORM)
            );
            assert_eq!(shared_format(index, 24), Some(24));
        }
    }

    #[test]
    fn a_pending_change_waits_for_the_ring_before_it_settles() {
        let mut order = Vec::new();
        let mut reconstruct = plan(EVENT_RECONSTRUCT, &packet(), false);
        let applied = apply_change(
            &mut reconstruct,
            || Some(5),
            || {
                order.push("wait");
                true
            },
            |generation| assert_eq!(generation, 5),
        );
        assert!(applied);
        assert_eq!(order, ["wait"]);
        assert_ne!(reconstruct.pass.flags & RSF_GAME_RENDER_RECONFIGURE, 0);
        assert_eq!(reconstruct.pass.camera_valid, 0);

        // A refused wait keeps the change pending and drops the pass.
        let mut refused = plan(EVENT_RECONSTRUCT, &packet(), false);
        assert!(!apply_change(
            &mut refused,
            || Some(5),
            || false,
            |_| panic!("settled")
        ));
        assert_eq!(refused.pass.flags & RSF_GAME_RENDER_RECONFIGURE, 0);

        // Nothing pending, or a route that never reconfigures, passes unchanged.
        let mut idle = plan(EVENT_RECONSTRUCT, &packet(), false);
        assert!(apply_change(
            &mut idle,
            || None,
            || panic!("waited"),
            |_| panic!("settled")
        ));
        assert_eq!(idle.pass.camera_valid, 1);
        let mut hudless = plan(EVENT_HUDLESS, &packet(), false);
        assert!(apply_change(
            &mut hudless,
            || panic!("read"),
            || panic!("waited"),
            |_| panic!("settled")
        ));
    }

    #[test]
    fn a_partial_hand_back_stops_after_the_completion_signal() {
        assert_eq!(retire(true, true), RETIRED);
        assert_eq!(retire(true, false), HANDBACK_FAILED);
        assert_eq!(retire(false, true), SIGNAL_FAILED);
        assert_eq!(retire(false, false), SIGNAL_FAILED);
    }
}
