// SPDX-License-Identifier: MIT
//! Verified Fallout 4 AE menu, freeze, camera and world-lease observations.

use crate::cpu_events::render_source_is_current;
use crate::discovery::{MENUS, Site, rva, verify_all};
pub(crate) use crate::sys::NativeScreen;
use crate::sys::rsf_fallout4_cpu_identity;
use crate::{image, read};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_PRIMARY,
    rsf_game_render_pass,
};
use rsf_game_sys::{RSF_SCREEN_GAMEPLAY, RSF_SCREEN_LOADING, RSF_SCREEN_MENU, RSF_SCREEN_UNKNOWN};
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};

const LOADING_FACT: u32 = 1;
const MENU_FACTS: u32 = 2 | 4 | 8 | 16 | 32;
const FROZEN_BACKGROUND: u32 = 64;
const FROZEN_PAUSE: u32 = 128;
const FREE_CAMERA: u32 = 256;
const VATS_CAMERA: u32 = 512;
const QUERY_UNAVAILABLE: u32 = 1;
const SOURCE_CHANGED: u32 = 2;
const NOT_ORDINARY_WORLD: u32 = 4;
const MAIN_INELIGIBLE: u32 = 8;
pub(crate) const WORLD_LEASE_INVALID: u32 = 16;
const CAMERA_INELIGIBLE: u32 = 32;
const MENU_OR_FREEZE: u32 = 64;
const OBSERVATION_CHANGED: u32 = 128;

/// The facts each discovered menu query reports, in discovery::MENUS order.
const MENU_FACTS_IN_ORDER: [u32; 6] = [LOADING_FACT, 2, 4, 8, 16, 32];
impl NativeScreen {
    pub(crate) const EMPTY: Self = Self::refused(0);

    const fn refused(refusals: u32) -> Self {
        Self {
            screen: RSF_SCREEN_UNKNOWN,
            facts: 0,
            refusals,
            camera_index: u32::MAX,
            camera_state: 0,
        }
    }
}

const _: () = {
    assert!(
        size_of::<NativeScreen>() == 24 && std::mem::offset_of!(NativeScreen, camera_state) == 16
    );
};

static VERIFIED_IMAGE: AtomicUsize = AtomicUsize::new(0);

/// The six menu queries with their name objects, the menu open query with its flags test, the two
/// freeze counters' unsigned tests and the camera state compare.
pub(crate) fn verify_queries() -> bool {
    VERIFIED_IMAGE.store(0, Ordering::Release);
    let base = image();
    let valid = base != 0
        && MENUS
            .iter()
            .all(|&(query, name, _)| verify_all(&[query, name]))
        && verify_all(&[
            Site::MenuOpen,
            Site::FreezeBackground,
            Site::FreezePause,
            Site::CameraEquals,
            Site::Ui,
            Site::PlayerCamera,
            Site::CameraVtable,
        ]);
    if valid {
        VERIFIED_IMAGE.store(base, Ordering::Release);
    }
    valid
}
#[repr(C)]
#[derive(Default, PartialEq, Eq)]
struct Observation {
    ui: usize,
    main: usize,
    player_camera: usize,
    camera_state: usize,
    camera_vtable: usize,
    world_graph: usize,
    world_camera: usize,
    main_flags: [u8; 8],
    in_menu: u8,
    freeze_background: u32,
    freeze_pause: u32,
    native_frame: u32,
}

impl Observation {
    fn read(base: usize) -> Option<Self> {
        let ui: usize = read(base + rva(Site::Ui))?;
        if ui == 0 {
            return None;
        }
        let mut value = Self {
            ui,
            freeze_background: read(ui.checked_add(FREEZE_BACKGROUND_OFFSET)?)?,
            freeze_pause: read(ui.checked_add(FREEZE_PAUSE_OFFSET)?)?,
            main: read(base + rva(Site::Main))?,
            player_camera: read(base + rva(Site::PlayerCamera))?,
            world_graph: read(base + rva(Site::WorldGraph))?,
            native_frame: read(base + rva(Site::State) + 0x98)?,
            ..Self::default()
        };
        if value.main != 0 {
            value.main_flags = read(value.main.checked_add(0x24)?)?;
            value.in_menu = read(value.main.checked_add(0x1d0)?)?;
        }
        if value.player_camera != 0 {
            value.camera_vtable = read(value.player_camera)?;
            value.camera_state = read(value.player_camera.checked_add(CAMERA_CURRENT)?)?;
        }
        if value.world_graph != 0 {
            value.world_camera = read(value.world_graph.checked_add(0x140)?)?;
        }
        Some(value)
    }

    fn main_eligible(&self) -> bool {
        let [f0, f1, f2, f3, _, f5, f6, f7] = self.main_flags;
        self.main != 0
            && self.in_menu == 0
            && f0 == 0
            && f1 == 0
            && f2 == 0
            && f3 != 0
            && f5 == 0
            && f6 == 0
            && f7 == 0
    }

    fn world_lease(&self, source: &rsf_fallout4_cpu_identity, pass: &rsf_game_render_pass) -> bool {
        let flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
        pass.struct_size as usize >= size_of::<rsf_game_render_pass>()
            && pass.session_id == source.session
            && pass.source_frame_id == source.source
            && pass.submission_id == source.submission
            && pass.scope_id == source.submission
            && pass.flags & flags == flags
            && pass.camera_valid != 0
            && pass.camera.struct_size as usize >= std::mem::size_of_val(&pass.camera)
            && pass.camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
            && !pass.color_input.is_null()
            && !pass.depth.is_null()
            && !pass.motion.is_null()
            && !pass.swapchain.is_null()
            && pass.camera.render_width != 0
            && pass.camera.render_height != 0
            && pass.camera.output_width != 0
            && pass.camera.output_height != 0
            && self.world_camera != 0
            && pass.view_key == self.world_camera as u64
            && pass.family_key == pass.view_key
            && pass.history_key == pass.view_key
            && pass.native_frame == u64::from(self.native_frame)
    }
}

unsafe extern "C" {
    fn rsf_fallout4_menu_name(entry: usize, name: &mut usize) -> bool;
    fn rsf_fallout4_menu_open(entry: usize, ui: usize, name: usize, open: &mut bool) -> bool;

}

const _: () = assert!(size_of::<Observation>() == 80);

const FREEZE_BACKGROUND_OFFSET: usize = 0x1ec;
const FREEZE_PAUSE_OFFSET: usize = 0x1f0;
const CAMERA_CURRENT: usize = 0x28;
const CAMERA_STATES: usize = 0xe0;

fn camera_equals(camera: usize, index: usize) -> Option<bool> {
    let current: usize = read(camera.checked_add(CAMERA_CURRENT)?)?;
    if current == 0 {
        return Some(false);
    }
    let slot = camera.checked_add(CAMERA_STATES + index * size_of::<usize>())?;
    Some(read::<usize>(slot)? == current)
}

/// The verified entry points run on the engine callback. The C leaves contain
/// foreign C++ exceptions from the two native menu calls. A mismatched name retains the facts
/// from earlier menus and skips all later calls.
fn observe(base: usize, ui: usize, camera: usize) -> Option<(u32, Option<u16>)> {
    let mut facts = 0;
    for (&(query, object, _), fact) in MENUS.iter().zip(MENU_FACTS_IN_ORDER) {
        let mut name = 0;
        if !unsafe { rsf_fallout4_menu_name(base + rva(query), &mut name) } {
            return None;
        }
        if name != base + rva(object) {
            return Some((facts, None));
        }
        let mut open = false;
        if !unsafe { rsf_fallout4_menu_open(base + rva(Site::MenuOpen), ui, name, &mut open) } {
            return None;
        }
        if open {
            facts |= fact;
        }
    }
    // The verified leaf bodies are unsigned nonzero tests of the two live counters.
    for (offset, fact) in [
        (FREEZE_BACKGROUND_OFFSET, FROZEN_BACKGROUND),
        (FREEZE_PAUSE_OFFSET, FROZEN_PAUSE),
    ] {
        if read::<u32>(ui.checked_add(offset)?)? != 0 {
            facts |= fact;
        }
    }
    let mut cameras = 0;
    if camera != 0 {
        for index in 0..13 {
            if camera_equals(camera, index)? {
                cameras |= 1 << index;
            }
        }
    }
    Some((facts, Some(cameras)))
}

struct LastError(u32);

impl Drop for LastError {
    fn drop(&mut self) {
        unsafe { SetLastError(self.0) };
    }
}

/// Query only on the engine callback. The native leaf contains the exception guard around the
/// engine calls. Facts only: this neither clears NO_FG nor admits an input owner.
pub(crate) fn classify(
    source: &rsf_fallout4_cpu_identity,
    pass: &rsf_game_render_pass,
    ordinary_world: bool,
) -> NativeScreen {
    let _error = LastError(unsafe { GetLastError() });
    let base = VERIFIED_IMAGE.load(Ordering::Acquire);
    if base == 0 {
        return NativeScreen::refused(QUERY_UNAVAILABLE);
    }
    if !render_source_is_current(source) {
        return NativeScreen::refused(SOURCE_CHANGED);
    }
    let Some(before) = Observation::read(base) else {
        return NativeScreen::refused(QUERY_UNAVAILABLE);
    };
    let camera =
        if before.camera_state != 0 && before.camera_vtable == base + rva(Site::CameraVtable) {
            before.player_camera
        } else {
            0
        };
    let Some((facts, cameras)) = observe(base, before.ui, camera) else {
        return NativeScreen::refused(QUERY_UNAVAILABLE);
    };
    let mut result = NativeScreen::refused(0);
    result.facts = facts;
    let Some(cameras) = cameras else {
        result.refusals = QUERY_UNAVAILABLE;
        return result;
    };
    if cameras & (1 << 2) != 0 {
        result.facts |= VATS_CAMERA;
    }
    if cameras & (1 << 3) != 0 {
        result.facts |= FREE_CAMERA;
    }
    if cameras.count_ones() == 1 {
        result.camera_index = cameras.trailing_zeros();
    }
    result.camera_state = before.camera_state;
    if Observation::read(base).as_ref() != Some(&before)
        || !render_source_is_current(source)
        || (result.facts & FROZEN_BACKGROUND != 0) != (before.freeze_background != 0)
        || (result.facts & FROZEN_PAUSE != 0) != (before.freeze_pause != 0)
    {
        result.refusals = OBSERVATION_CHANGED;
        return result;
    }
    if !ordinary_world {
        result.refusals |= NOT_ORDINARY_WORLD;
    }
    if !before.main_eligible() {
        result.refusals |= MAIN_INELIGIBLE;
    }
    if !before.world_lease(source, pass) {
        result.refusals |= WORLD_LEASE_INVALID;
    }
    if !matches!(result.camera_index, 0 | 4 | 8) {
        result.refusals |= CAMERA_INELIGIBLE;
    }
    if result.facts != 0 {
        result.refusals |= MENU_OR_FREEZE;
    }
    if result.facts & LOADING_FACT != 0 {
        result.screen = RSF_SCREEN_LOADING;
    } else if result.facts & MENU_FACTS != 0 {
        result.screen = RSF_SCREEN_MENU;
    } else if result.refusals == 0 {
        result.screen = RSF_SCREEN_GAMEPLAY;
    }
    result
}

/// Refusal facts for the ordinary World before a render-input lease is available: the screen
/// facts and the expected invalid-world-lease refusal.
pub(crate) fn ordinary_refusals(source: &rsf_fallout4_cpu_identity) -> u32 {
    // SAFETY: the empty pass has no resources.
    classify(source, &unsafe { std::mem::zeroed() }, true).refusals
}
