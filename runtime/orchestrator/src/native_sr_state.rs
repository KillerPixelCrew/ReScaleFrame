// SPDX-License-Identifier: MIT
use crate::sys::{RSF_FRAME_ASSEMBLY_ABI_VERSION, rsf_pipeline_camera_frame as Camera};
use rsf_game_sys::{
    RSF_GAME_RENDER_RESET, rsf_game_depth_domain, rsf_game_depth_domains,
    rsf_game_render_config as Config, rsf_game_render_pass as Pass,
};
use std::{ffi::c_void, sync::Mutex};

#[derive(Clone, Copy, PartialEq, Eq)]
struct DomainLayout {
    classify: [u32; 2],
    endpoint_flags: u32,
    reserved: u32,
    viewport: [u32; 4],
    viewport_depth: [u32; 2],
}

impl From<rsf_game_depth_domain> for DomainLayout {
    fn from(domain: rsf_game_depth_domain) -> Self {
        Self {
            classify: [domain.classify_min.to_bits(), domain.classify_max.to_bits()],
            endpoint_flags: domain.endpoint_flags,
            reserved: domain.reserved,
            viewport: domain.viewport_xywh.map(f32::to_bits),
            viewport_depth: [
                domain.viewport_min_depth.to_bits(),
                domain.viewport_max_depth.to_bits(),
            ],
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct DepthLayout {
    version: u32,
    count: u32,
    clear_enabled: u32,
    clear_value: f32,
    domains: [DomainLayout; 2],
}

impl From<rsf_game_depth_domains> for DepthLayout {
    fn from(depth: rsf_game_depth_domains) -> Self {
        Self {
            version: depth.version,
            count: depth.domain_count,
            clear_enabled: depth.clear_enabled,
            clear_value: depth.clear_value,
            // The current source's projection may change without changing its layout.
            domains: depth.domains.map(DomainLayout::from),
        }
    }
}

#[derive(Clone, Copy)]
struct History {
    session: u64,
    owner: u64,
    frame: u64,
    extent: [u32; 2],
    render_rect: [i32; 4],
    output_rect: [i32; 4],
    depth: DepthLayout,
}

impl History {
    fn from_pass(pass: &Pass) -> Self {
        Self {
            session: pass.session_id,
            owner: pass.history_key,
            frame: pass.native_frame,
            extent: [pass.camera.render_width, pass.camera.render_height],
            render_rect: pass.render_rect,
            output_rect: pass.output_rect,
            depth: pass.depth_domains.into(),
        }
    }

    fn continues(self, next: Self) -> bool {
        let follows = self.frame.wrapping_add(1) == next.frame
            || (self.frame == u64::from(u32::MAX) && next.frame == 0);
        self.session == next.session
            && self.owner == next.owner
            && follows
            && self.extent == next.extent
            && self.render_rect == next.render_rect
            && self.output_rect == next.output_rect
            && self.depth == next.depth
    }
}

// SR publication and explicit FG capture commit independently on success.
static HISTORY: Mutex<[Option<History>; 2]> = Mutex::new([None; 2]);
const EMPTY_CONFIG: Config = Config {
    struct_size: size_of::<Config>() as u32,
    enabled: 0,
    output_width: 0,
    output_height: 0,
    render_width: 0,
    render_height: 0,
};
static SURFACE: Mutex<Config> = Mutex::new(EMPTY_CONFIG);

fn camera(pass: &Pass, history: Option<History>) -> Camera {
    let source = &pass.camera;
    let [right, up, forward, position] = source.view_to_world.as_chunks::<4>().0 else {
        unreachable!("4×4 camera matrix")
    };
    let xyz = |row: &[f32; 4]| [row[0], row[1], row[2]];
    let reset = pass.flags & RSF_GAME_RENDER_RESET != 0
        || history.is_none_or(|previous| !previous.continues(History::from_pass(pass)));
    Camera {
        struct_size: size_of::<Camera>() as u32,
        abi_version: RSF_FRAME_ASSEMBLY_ABI_VERSION,
        view_to_clip: source.view_to_clip,
        clip_to_view: source.clip_to_view,
        clip_to_prev_clip: source.clip_to_previous_clip,
        prev_clip_to_clip: pass.previous_clip_to_clip,
        camera_position: xyz(position),
        camera_right: xyz(right),
        camera_up: xyz(up),
        camera_forward: xyz(forward),
        jitter_pixels: source.jitter_pixels,
        has_jitter: 1,
        depth_inverted: source.depth_inverted,
        near_plane: source.near_plane,
        far_plane: source.far_plane,
        vertical_fov: source.vertical_fov_radians,
        aspect_ratio: source.view_to_clip[5] / source.view_to_clip[0],
        camera_motion_included: pass.motion_camera_included,
        motion_scale: pass.motion_to_uv,
        reset: u32::from(reset),
        motion_decoded: 0,
        has_motion_sentinel: 0,
        motion_sentinel: 0.0,
    }
}

pub(super) fn camera_for(kind: u32, pass: &Pass) -> Camera {
    let previous = HISTORY
        .lock()
        .unwrap()
        .get(kind as usize)
        .copied()
        .flatten();
    camera(pass, previous)
}

pub(super) fn commit_history(kind: u32, pass: &Pass) {
    if let Some(slot) = HISTORY.lock().unwrap().get_mut(kind as usize) {
        *slot = Some(History::from_pass(pass));
    }
}

pub(super) fn reset_history(kind: u32) {
    if let Some(slot) = HISTORY.lock().unwrap().get_mut(kind as usize) {
        *slot = None;
    }
}

pub(super) fn publish_surface(
    [output_width, output_height]: [u32; 2],
    [render_width, render_height]: [u32; 2],
) {
    let mut surface = SURFACE.lock().unwrap();
    *surface = Config {
        output_width,
        output_height,
        render_width,
        render_height,
        enabled: surface.enabled,
        ..EMPTY_CONFIG
    };
}

pub(super) fn clear_surface() {
    *SURFACE.lock().unwrap() = EMPTY_CONFIG;
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_native_sr_set_enabled(enabled: u32) {
    SURFACE.lock().unwrap().enabled = u32::from(enabled != 0);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_sr_render_config(_: *mut c_void, out: *mut Config) -> i32 {
    if out.is_null() || unsafe { out.cast::<u32>().read() } < EMPTY_CONFIG.struct_size {
        return 0;
    }
    unsafe { out.write(*SURFACE.lock().unwrap()) };
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_continuity_preserves_projection_updates_and_rejects_layout_changes() {
        let mut pass: Pass = unsafe { std::mem::zeroed() };
        pass.session_id = 3;
        pass.history_key = 7;
        pass.native_frame = u64::from(u32::MAX);
        pass.camera.view_to_world = std::array::from_fn(|i| i as f32);
        pass.camera.view_to_clip[0] = 2.0;
        pass.camera.view_to_clip[5] = 4.0;
        pass.camera.render_width = 32;
        pass.camera.render_height = 20;
        pass.output_rect = [0, 0, 64, 40];
        pass.render_rect = [0, 0, 32, 20];
        let first = camera(&pass, None);
        assert_eq!(first.reset, 1);
        assert_eq!(first.camera_position, [12.0, 13.0, 14.0]);
        assert_eq!(first.aspect_ratio, 2.0);

        let previous = History::from_pass(&pass);
        pass.native_frame = 0;
        pass.depth_domains.domains[0].source_clip_to_target_clip[0] = 2.0;
        assert_eq!(camera(&pass, Some(previous)).reset, 0);
        pass.depth_domains.domains[0].viewport_xywh[2] = 1.0;
        assert_eq!(camera(&pass, Some(previous)).reset, 1);
        pass.depth_domains.domains[0].viewport_xywh[2] = 0.0;
        pass.output_rect[2] = 63;
        assert_eq!(camera(&pass, Some(previous)).reset, 1);
        pass.output_rect[2] = 64;
        pass.native_frame = u64::from(u32::MAX) + 1;
        assert_eq!(camera(&pass, Some(previous)).reset, 0);
        let wide = History::from_pass(&pass);
        pass.native_frame = 1;
        assert_eq!(camera(&pass, Some(wide)).reset, 1);
    }
}
