// SPDX-License-Identifier: MIT
//! Guarded Skyrim CPU producer and consumer observations.
//! Evidence: docs/research/skyrim-se-integration.md.

use crate::platform::{self, Platform};

pub(crate) const MAIN: usize = 0x323_0530;
pub(crate) const PLAYER: usize = 0x323_0778;
pub(crate) const JOB_MANAGER: usize = 0x322_0a90;
pub(crate) const REQUESTED_MODE: usize = 0x68;
pub(crate) const RUNNING_MODE: usize = 0x6c;
const INPUT_MANAGER: usize = 0x320_4b30;
const PLAYER_CONTROLS: usize = 0x31a_5698;
const PLAYER_CAMERA: usize = 0x31a_5478;

#[repr(C)]
#[derive(Default)]
pub(crate) struct ManagerState {
    pub(crate) manager: usize,
    pub(crate) main: usize,
    pub(crate) requested: u32,
    pub(crate) running: u32,
}

const _: () = assert!(size_of::<ManagerState>() == 24);

impl ManagerState {
    fn read(platform: &Platform, base: usize) -> Option<Self> {
        let manager = singleton(platform, base, JOB_MANAGER)?;
        Some(Self {
            manager,
            main: singleton(platform, base, MAIN)?,
            requested: platform.read_u32(manager.checked_add(REQUESTED_MODE)?)?,
            running: platform.read_u32(manager.checked_add(RUNNING_MODE)?)?,
        })
    }
}

fn singleton(platform: &Platform, base: usize, offset: usize) -> Option<usize> {
    (base != 0).then_some(())?;
    platform
        .read_usize(base.checked_add(offset)?)
        .filter(|&value| value != 0)
}

fn byte(platform: &Platform, address: usize) -> Option<u8> {
    let mut value = [0u8; 1];
    platform.read(address, &mut value).then_some(value[0])
}

pub(crate) fn rsf_skyrim_cpu_manager_state(out: &mut ManagerState) -> bool {
    let Some(state) = platform::get().and_then(|p| ManagerState::read(p, p.main_module())) else {
        return false;
    };
    *out = state;
    true
}

pub(crate) fn poll_managers_match(job_manager: usize, poll_manager: usize) -> bool {
    let Some(platform) = platform::get() else {
        return false;
    };
    let base = platform.main_module();
    ManagerState::read(platform, base).is_some_and(|state| {
        state.manager == job_manager && state.requested <= 1 && state.running == 1
    }) && singleton(platform, base, INPUT_MANAGER) == Some(poll_manager)
}

fn matches_singleton(address: usize, offset: usize) -> bool {
    platform::get().is_some_and(|p| singleton(p, p.main_module(), offset) == Some(address))
}

pub(crate) fn rsf_skyrim_cpu_is_input_manager(address: usize) -> bool {
    matches_singleton(address, INPUT_MANAGER)
}

pub(crate) fn rsf_skyrim_cpu_is_player_controls(address: usize) -> bool {
    matches_singleton(address, PLAYER_CONTROLS)
}

pub(crate) fn rsf_skyrim_cpu_is_player(address: usize) -> bool {
    matches_singleton(address, PLAYER)
}

pub(crate) fn rsf_skyrim_cpu_is_player_camera(address: usize) -> bool {
    matches_singleton(address, PLAYER_CAMERA)
}

pub(crate) fn rsf_skyrim_cpu_camera_consumes(address: usize) -> bool {
    platform::get().is_some_and(|p| address.checked_add(0xe0).and_then(|at| byte(p, at)) == Some(0))
}

fn queued_move() -> Option<bool> {
    let p = platform::get()?;
    let player = singleton(p, p.main_module(), PLAYER)?;
    let flags = byte(p, player.checked_add(0xbec)?)?;
    let pending = byte(p, player.checked_add(0x692)?)?;
    Some(flags & 0x40 != 0 || pending != 0)
}

pub(crate) fn rsf_skyrim_cpu_queued_move(queued: &mut bool) -> bool {
    let Some(value) = queued_move() else {
        return false;
    };
    *queued = value;
    true
}

fn free_camera() -> Option<bool> {
    let p = platform::get()?;
    let camera = singleton(p, p.main_module(), PLAYER_CAMERA)?;
    let state = p.read_usize(camera.checked_add(0x28)?)?;
    let free = p.read_usize(camera.checked_add(0xd0)?)?;
    // The matched engine compares both pointers, including when both are null.
    Some(state == free)
}

pub(crate) fn rsf_skyrim_cpu_free_camera(active: &mut bool) -> bool {
    let Some(value) = free_camera() else {
        return false;
    };
    *active = value;
    true
}
