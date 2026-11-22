// SPDX-License-Identifier: MIT
//! Verified Fallout 4 AE menu, freeze, camera and world-lease observations.

use crate::cpu_abi::rsf_fallout4_cpu_identity;
use crate::cpu_events::rsf_fallout4_cpu_render_source_is_current;
use crate::{image, read};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_PRIMARY,
    rsf_game_render_pass,
};
use rsf_game_sys::{RSF_SCREEN_GAMEPLAY, RSF_SCREEN_LOADING, RSF_SCREEN_MENU, RSF_SCREEN_UNKNOWN};
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

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

const MENU_OPEN: usize = 0x1a8_0320;
const FREEZE_BACKGROUND: usize = 0x1a8_0480;
const FREEZE_PAUSE: usize = 0x1a8_04a0;
const CAMERA_EQUALS: usize = 0x102_b5c0;

struct MenuQuery {
    entry: usize,
    literal_lea: usize,
    return_lea: usize,
    literal: usize,
    object: usize,
    name: &'static CStr,
    literal_bytes: [u8; 7],
    return_bytes: [u8; 7],
    fact: u32,
}

const MENUS: [MenuQuery; 6] = [
    MenuQuery {
        entry: 0x1a8_7520,
        literal_lea: 0x1a8_7575,
        return_lea: 0x1a8_7549,
        literal: 0x25b_0548,
        object: 0x3db_d830,
        name: c"LoadingMenu",
        literal_bytes: [0x48, 0x8d, 0x15, 0xcc, 0x8f, 0xb2, 0x00],
        return_bytes: [0x48, 0x8d, 0x05, 0xe0, 0x62, 0x33, 0x02],
        fact: LOADING_FACT,
    },
    MenuQuery {
        entry: 0x1a8_79a0,
        literal_lea: 0x1a8_79f5,
        return_lea: 0x1a8_79c9,
        literal: 0x253_f640,
        object: 0x3db_d8b0,
        name: c"MainMenu",
        literal_bytes: [0x48, 0x8d, 0x15, 0x44, 0x7c, 0xab, 0x00],
        return_bytes: [0x48, 0x8d, 0x05, 0xe0, 0x5e, 0x33, 0x02],
        fact: 2,
    },
    MenuQuery {
        entry: 0x1a8_7e20,
        literal_lea: 0x1a8_7e75,
        return_lea: 0x1a8_7e49,
        literal: 0x271_de40,
        object: 0x3db_d930,
        name: c"PauseMenu",
        literal_bytes: [0x48, 0x8d, 0x15, 0xc4, 0x5f, 0xc9, 0x00],
        return_bytes: [0x48, 0x8d, 0x05, 0xe0, 0x5a, 0x33, 0x02],
        fact: 4,
    },
    MenuQuery {
        entry: 0x1a8_7fd0,
        literal_lea: 0x1a8_8025,
        return_lea: 0x1a8_7ff9,
        literal: 0x253_cde8,
        object: 0x3db_d960,
        name: c"PipboyMenu",
        literal_bytes: [0x48, 0x8d, 0x15, 0xbc, 0x4d, 0xab, 0x00],
        return_bytes: [0x48, 0x8d, 0x05, 0x60, 0x59, 0x33, 0x02],
        fact: 8,
    },
    MenuQuery {
        entry: 0x1a8_7a30,
        literal_lea: 0x1a8_7a85,
        return_lea: 0x1a8_7a59,
        literal: 0x271_ddf0,
        object: 0x3db_d8d0,
        name: c"MapMenu",
        literal_bytes: [0x48, 0x8d, 0x15, 0x64, 0x63, 0xc9, 0x00],
        return_bytes: [0x48, 0x8d, 0x05, 0x70, 0x5e, 0x33, 0x02],
        fact: 16,
    },
    MenuQuery {
        entry: 0x1a8_90b0,
        literal_lea: 0x1a8_9105,
        return_lea: 0x1a8_90d9,
        literal: 0x254_1f48,
        object: 0x3db_db40,
        name: c"VATSMenu",
        literal_bytes: [0x48, 0x8d, 0x15, 0x3c, 0x8e, 0xab, 0x00],
        return_bytes: [0x48, 0x8d, 0x05, 0x60, 0x4a, 0x33, 0x02],
        fact: 32,
    },
];

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct NativeScreen {
    pub(crate) screen: u32,
    pub(crate) facts: u32,
    pub(crate) refusals: u32,
    pub(crate) camera_index: u32,
    pub(crate) camera_state: usize,
}

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

fn matches<const N: usize>(base: usize, rva: usize, expected: &[u8; N]) -> bool {
    read::<[u8; N]>(base + rva).as_ref() == Some(expected)
}

fn literal(base: usize, query: &MenuQuery) -> bool {
    let expected = query.name.to_bytes_with_nul();
    let mut bytes = [0_u8; 20];
    let Some(bytes) = bytes.get_mut(..expected.len()) else {
        return false;
    };
    let mut copied = 0;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            (base + query.literal) as *const c_void,
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            &mut copied,
        )
    } != 0;
    ok && copied == bytes.len() && &*bytes == expected
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fallout4_verify_screen_queries() -> bool {
    VERIFIED_IMAGE.store(0, Ordering::Release);
    let base = image();
    let valid = base != 0
        && MENUS.iter().all(|menu| {
            matches(base, menu.entry, &[0x40, 0x53, 0x48, 0x83, 0xec, 0x20])
                && matches(base, menu.literal_lea, &menu.literal_bytes)
                && matches(base, menu.return_lea, &menu.return_bytes)
                && literal(base, menu)
        })
        && matches(
            base,
            MENU_OPEN,
            &[0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x40],
        )
        && matches(base, 0x1a8_0377, &[0xf6, 0x41, 0x58, 0x40])
        && matches(
            base,
            FREEZE_BACKGROUND,
            &[0x83, 0xb9, 0xec, 0x01, 0, 0, 0, 0x0f, 0x97, 0xc0, 0xc3],
        )
        && matches(
            base,
            FREEZE_PAUSE,
            &[0x83, 0xb9, 0xf0, 0x01, 0, 0, 0, 0x0f, 0x97, 0xc0, 0xc3],
        )
        && matches(
            base,
            CAMERA_EQUALS,
            &[
                0x4c, 0x8b, 0x41, 0x28, 0x4d, 0x85, 0xc0, 0x74, 0x0f, 0x8b, 0xc2, 0x4c, 0x3b, 0x84,
                0xc1, 0xe0, 0, 0, 0, 0x75, 0x03, 0xb0, 0x01, 0xc3, 0x32, 0xc0, 0xc3,
            ],
        );
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
        let ui: usize = read(base + 0x30e_8930)?;
        if ui == 0 {
            return None;
        }
        let mut value = Self {
            ui,
            freeze_background: read(ui.checked_add(0x1ec)?)?,
            freeze_pause: read(ui.checked_add(0x1f0)?)?,
            main: read(base + 0x32d_d130)?,
            player_camera: read(base + 0x30e_6e58)?,
            world_graph: read(base + 0x32d_d338)?,
            native_frame: read(base + 0x3d8_71a0 + 0x98)?,
            ..Self::default()
        };
        if value.main != 0 {
            value.main_flags = read(value.main.checked_add(0x24)?)?;
            value.in_menu = read(value.main.checked_add(0x1d0)?)?;
        }
        if value.player_camera != 0 {
            value.camera_vtable = read(value.player_camera)?;
            value.camera_state = read(value.player_camera.checked_add(0x28)?)?;
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
    fn rsf_fallout4_menu_freeze(entry: usize, ui: usize, frozen: &mut bool) -> bool;
    fn rsf_fallout4_camera_equals(
        entry: usize,
        camera: usize,
        index: u32,
        equal: &mut bool,
    ) -> bool;
}

const _: () = assert!(size_of::<Observation>() == 80);

/// The verified entry points run on the engine callback. The C++ leaves contain
/// exceptions from individual native calls. A mismatched name retains the facts
/// from earlier menus and skips all later calls.
fn observe(base: usize, ui: usize, camera: usize) -> Option<(u32, Option<u16>)> {
    let mut facts = 0;
    for menu in MENUS {
        let mut name = 0;
        if !unsafe { rsf_fallout4_menu_name(base + menu.entry, &mut name) } {
            return None;
        }
        if name != base + menu.object {
            return Some((facts, None));
        }
        let mut open = false;
        if !unsafe { rsf_fallout4_menu_open(base + MENU_OPEN, ui, name, &mut open) } {
            return None;
        }
        if open {
            facts |= menu.fact;
        }
    }
    for (entry, fact) in [
        (FREEZE_BACKGROUND, FROZEN_BACKGROUND),
        (FREEZE_PAUSE, FROZEN_PAUSE),
    ] {
        let mut frozen = false;
        if !unsafe { rsf_fallout4_menu_freeze(base + entry, ui, &mut frozen) } {
            return None;
        }
        if frozen {
            facts |= fact;
        }
    }
    let mut cameras = 0;
    if camera != 0 {
        for index in 0..13 {
            let mut equal = false;
            if !unsafe {
                rsf_fallout4_camera_equals(base + CAMERA_EQUALS, camera, index, &mut equal)
            } {
                return None;
            }
            if equal {
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

/// Query only on the engine callback. The native leaf contains the exception
/// guard around the engine calls.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fallout4_screen_classify(
    source: &rsf_fallout4_cpu_identity,
    pass: &rsf_game_render_pass,
    ordinary_world: bool,
    result: &mut NativeScreen,
) {
    let _error = LastError(unsafe { GetLastError() });
    let base = VERIFIED_IMAGE.load(Ordering::Acquire);
    if base == 0 {
        *result = NativeScreen::refused(QUERY_UNAVAILABLE);
        return;
    }
    if !unsafe { rsf_fallout4_cpu_render_source_is_current(source) } {
        *result = NativeScreen::refused(SOURCE_CHANGED);
        return;
    }
    let Some(before) = Observation::read(base) else {
        *result = NativeScreen::refused(QUERY_UNAVAILABLE);
        return;
    };
    let camera = if before.camera_state != 0 && before.camera_vtable == base + 0x25a_a6d8 {
        before.player_camera
    } else {
        0
    };
    let Some((facts, cameras)) = observe(base, before.ui, camera) else {
        *result = NativeScreen::refused(QUERY_UNAVAILABLE);
        return;
    };
    *result = NativeScreen::refused(0);
    result.facts = facts;
    let Some(cameras) = cameras else {
        result.refusals = QUERY_UNAVAILABLE;
        return;
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
        || !unsafe { rsf_fallout4_cpu_render_source_is_current(source) }
        || (result.facts & FROZEN_BACKGROUND != 0) != (before.freeze_background != 0)
        || (result.facts & FROZEN_PAUSE != 0) != (before.freeze_pause != 0)
    {
        result.refusals = OBSERVATION_CHANGED;
        return;
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
}
