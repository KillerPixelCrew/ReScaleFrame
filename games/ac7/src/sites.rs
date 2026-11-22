// SPDX-License-Identifier: MIT
//! Exact-build code sites of Ace7Game.exe (SHA-256 c7da97f5...58f4f) that the native renderer
//! hooks or depends on, and the admission that verifies them before any hook is made.
//!
//! native_renderer.rs keeps MinHook, the detours and the original pointers, and lends the guarded
//! reads and protected code writes through `rsf_ac7_code_leaves`.
//! The renderer verifies post-patch depth gates installed by the carrier. The research for these
//! sites is in docs/research/ue418-hook-map.md and games/ac7/engine.json.
use crate::sys::{
    RSF_AC7_EVIDENCE_FIRST, RSF_AC7_EVIDENCE_LAST, RSF_AC7_HOOK_SITE_COUNT, RSF_AC7_PROCESS_HOOKS,
    RSF_AC7_ROLE_AA, RSF_AC7_ROLE_COMPOSITE, RSF_AC7_ROLE_HUD, RSF_AC7_ROLE_MATERIAL,
    RSF_AC7_ROLE_OUTPUT, RSF_AC7_ROLE_TONEMAP, RSF_AC7_SITE_BYTES, rsf_ac7_code_leaves,
};
use std::ffi::CString;

/// Read-only access needed for exact-build admission.
pub(crate) trait CodeView {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool;
    fn log(&self, message: &str);
}

/// A writer replaces verified code, leaving the code unchanged on refusal.
pub(crate) trait Code: CodeView {
    fn write(&self, site: *mut u8, expected: &[u8], replacement: &[u8]) -> bool;
}

type CompareFn = unsafe extern "C" fn(*const u8, *const u8, usize) -> bool;
type WriteFn = unsafe extern "C" fn(*mut u8, *const u8, *const u8, usize) -> bool;

pub(crate) struct NativeCodeView {
    compare: CompareFn,
    pub(crate) log: rsf_game_sys::rsf_game_log_fn,
    pub(crate) user: *mut std::ffi::c_void,
}

impl NativeCodeView {
    fn new(leaves: &rsf_ac7_code_leaves) -> Option<Self> {
        Some(Self {
            compare: leaves.matches?,
            log: leaves.log,
            user: leaves.user,
        })
    }
}

impl CodeView for NativeCodeView {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        // The native leaf guards access to the live image.
        unsafe { (self.compare)(address, expected.as_ptr(), expected.len()) }
    }

    fn log(&self, message: &str) {
        if let (Some(log), Ok(text)) = (self.log, CString::new(message)) {
            unsafe { log(self.user, text.as_ptr()) };
        }
    }
}

pub(crate) struct NativeCode {
    pub(crate) view: NativeCodeView,
    write: WriteFn,
}

impl NativeCode {
    /// # Safety
    /// The table, user and callbacks must be valid for all uses of the returned code access.
    pub(crate) unsafe fn new(leaves: *const rsf_ac7_code_leaves) -> Option<Self> {
        let leaves = unsafe { leaves.as_ref() }?;
        Some(Self {
            view: NativeCodeView::new(leaves)?,
            write: leaves.write?,
        })
    }
}

impl CodeView for NativeCode {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        self.view.matches(address, expected)
    }

    fn log(&self, message: &str) {
        self.view.log(message);
    }
}

impl Code for NativeCode {
    fn write(&self, site: *mut u8, expected: &[u8], replacement: &[u8]) -> bool {
        if expected.len() != replacement.len() {
            return false;
        }
        // The native leaf writes only after checking the live bytes.
        unsafe {
            (self.write)(
                site,
                expected.as_ptr(),
                replacement.as_ptr(),
                expected.len(),
            )
        }
    }
}

const SITE_BYTES: usize = RSF_AC7_SITE_BYTES as usize;

pub(crate) const POSTPROCESS_RVA: u32 = 0xffb900;
pub(crate) const POSTPROCESS_ENTRY: &[u8; 18] =
    b"\x4c\x8b\xdc\x55\x49\x8d\xab\xe8\xfc\xff\xff\x48\x81\xec\x10\x04\x00\x00";
pub(crate) const WIDGET_QUEUE_RVA: u32 = 0x4d6340;
pub(crate) const WIDGET_QUEUE_ENTRY: &[u8; 16] =
    b"\x40\x55\x57\x48\x8d\x6c\x24\xa8\x48\x81\xec\x58\x01\x00\x00\x0f";

/// A function entry compared over its first 16 bytes.
pub(crate) struct Site {
    pub(crate) rva: u32,
    pub(crate) expected: [u8; SITE_BYTES],
}

impl Site {
    const fn new(rva: u32, expected: &[u8; SITE_BYTES]) -> Self {
        Self {
            rva,
            expected: *expected,
        }
    }
}

/// An in-function branch or comparison that enlarged separate translucency depends on.
pub(crate) struct Gate {
    pub(crate) rva: u32,
    pub(crate) native: &'static [u8],
    pub(crate) expected: &'static [u8],
    pub(crate) description: &'static str,
}

impl Gate {
    const fn new<const N: usize>(
        rva: u32,
        native: &'static [u8; N],
        expected: &'static [u8; N],
        description: &'static str,
    ) -> Self {
        Self {
            rva,
            native,
            expected,
            description,
        }
    }
}

/// Hooked entry sites, indexed by native_renderer.rs's `hook` constants.
#[rustfmt::skip]
pub(crate) static HOOK_SITES: [Site; RSF_AC7_HOOK_SITE_COUNT as usize] = [
    Site::new(0x10b0220, b"\x40\x55\x53\x56\x57\x41\x55\x41\x57\x48\x8d\x6c\x24\xb8\x48\x81"), // 0 process_tonemap
    Site::new(0xfbfe90, b"\x40\x55\x56\x41\x56\x48\x8d\x6c\x24\xb9\x48\x81\xec\x00\x01\x00"), // 1 process_aa
    Site::new(0x1004960, b"\x40\x55\x53\x56\x57\x41\x54\x41\x55\x41\x56\x48\x8d\xac\x24\xe0"), // 2 process_material
    Site::new(0xfcbc90, b"\x40\x55\x53\x56\x41\x55\x41\x56\x41\x57\x48\x8d\xac\x24\xe8\xfe"), // 3 process_hud
    Site::new(0xfc86c0, b"\x48\x89\x5c\x24\x18\x55\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57"), // 4 process_composite
    Site::new(0x10b1650, b"\x40\x55\x56\x57\x48\x8d\x6c\x24\xf0\x48\x81\xec\x10\x01\x00\x00"), // 5 process_output
    Site::new(0x1098ff0, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x40\x48\x8b\x01\x48\x8b\xda"), // 6 descriptor_tonemap
    Site::new(0xfb6f20, b"\x40\x53\x48\x83\xec\x20\x48\x8b\x01\x48\x8b\xda\x33\xd2\xff\x50"), // 7 descriptor_aa
    Site::new(0xff7b50, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x40\x48\x8b\x01\x48\x8b\xfa"), // 8 descriptor_material
    Site::new(0xfb7bc0, b"\x40\x53\x48\x83\xec\x20\x48\x8b\x01\x48\x8b\xda\x33\xd2\xff\x50"), // 9 descriptor_hud
    Site::new(0xfb7b50, b"\x40\x53\x48\x83\xec\x20\x48\x8b\x01\x48\x8b\xda\x33\xd2\xff\x50"), // 10 descriptor_composite
    Site::new(0x10991e0, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\x01\x48\x8b\xda"), // 11 descriptor_output
    Site::new(POSTPROCESS_RVA, POSTPROCESS_ENTRY.first_chunk::<SITE_BYTES>().unwrap()), // 12 postprocess
    Site::new(0x10b3970, b"\x40\x56\x57\x41\x57\x48\x83\xec\x30\x65\x48\x8b\x04\x25\x58\x00"), // 13 context
    Site::new(0x112afa0, b"\x40\x55\x53\x41\x54\x41\x57\x48\x8d\xac\x24\x68\xff\xff\xff\x48"), // 14 visibility
    Site::new(0x1113080, b"\x48\x89\x5c\x24\x20\x4c\x89\x44\x24\x18\x48\x89\x4c\x24\x08\x55"), // 15 construct
    Site::new(WIDGET_QUEUE_RVA, WIDGET_QUEUE_ENTRY), // 16 widget_queue
    Site::new(0x4d6660, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x50\x80\x79\x78\x00\x48\x8b"), // 17 widget_targets
    Site::new(0x1380d80, b"\x48\x8b\xc4\x48\x89\x58\x08\x48\x89\x70\x10\x48\x89\x78\x18\x4c"), // 18 widget_draw
    Site::new(0x3a3f70, b"\x48\x8b\xc4\x48\x89\x58\x10\x48\x89\x70\x18\x55\x57\x41\x54\x41"), // 19 engine_tick
    Site::new(0x113a0c0, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\xfa\x48\x8b\xd9"), // 20 renderer_retire
    Site::new(0xcea130, b"\x83\xb9\xb0\x01\x00\x00\x00\x48\x8b\xd1\x75\x22\xf2\x0f\x10\x82"), // 21 poll_input
    Site::new(0x3cd130, b"\x48\x89\x5c\x24\x10\x48\x89\x74\x24\x18\x48\x89\x7c\x24\x20\x55"), // 22 game_instance_init
    Site::new(0x1ab0200, b"\x0f\xb6\x44\x24\x28\x83\xe0\x01\x89\x91\xd0\x00\x00\x00\x44\x89"), // 23 target_init, also called directly by the viewport target path
    Site::new(0x3c7a30, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x30\x0f\x29\x74\x24\x20\x41"), // 24 simulation_a
    Site::new(0x1781e80, b"\x48\x8b\xc4\x44\x88\x40\x18\x48\x89\x48\x08\x55\x53\x56\x57\x41"), // 25 simulation_b
    Site::new(0x177cdd0, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\xd9\x0f\xb6\xfa"), // 26 redraw
    Site::new(0x1ac3600, b"\x40\x55\x56\x57\x41\x54\x41\x55\x48\x8d\xac\x24\x70\xfe\xff\xff"), // 27 viewport_draw
    Site::new(0x1345310, b"\x48\x8b\xc4\x48\x89\x50\x10\x55\x53\x48\x8d\xa8\x88\xfe\xff\xff"), // 28 slate_private
    Site::new(0x133c6c0, b"\x40\x56\x57\x48\x83\xec\x48\x48\x89\x5c\x24\x40\x48\x8b\xf1\x48"), // 29 slate_allocate
    Site::new(0x1346520, b"\x48\x89\x5c\x24\x10\x48\x89\x6c\x24\x18\x48\x89\x74\x24\x20\x57"), // 30 slate_task
    Site::new(0x1343660, b"\x4c\x8b\xdc\x55\x53\x57\x49\x8d\xab\x08\xfa\xff\xff\x48\x81\xec"), // 31 slate_window
    Site::new(0x1351e10, b"\x48\x89\x5c\x24\x18\x55\x56\x57\x41\x55\x41\x56\x48\x83\xec\x20"), // 32 slate_texture
    Site::new(0x10be2b0, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x65\x48\x8b\x04\x25\x58"), // 33 translucency_size
    Site::new(0x1097d20, b"\x48\x89\x5c\x24\x18\x55\x56\x57\x48\x83\xec\x30\x83\xb9\x58\x02"), // 34 unmodified_begin
    Site::new(0x109d320, b"\x40\x57\x48\x83\xec\x40\x83\xb9\x58\x02\x00\x00\x02\x48\x8b\xfa"), // 35 unmodified_resolve
    Site::new(0x1168d40, b"\x40\x55\x56\x57\x41\x56\x48\x8d\x6c\x24\xc1\x48\x81\xec\xb8\x00"), // 36 translucency_render
    Site::new(0xe97d80, b"\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10\x48\x89\x74\x24\x18\x57"), // 37 pixel_view_uniform
    Site::new(0xe39520, b"\x48\x89\x5c\x24\x10\x55\x56\x57\x41\x56\x41\x57\x48\x83\xec\x20"), // 38 rhi_pixel_uniform
    Site::new(0xe1f8d0, b"\x48\x89\x5c\x24\x20\x55\x41\x56\x41\x57\x48\x83\xec\x30\x0f\xb7"), // 39 rhi_pixel_tables
    Site::new(0xe9b880, b"\x48\x89\x5c\x24\x18\x48\x89\x6c\x24\x20\x56\x48\x83\xec\x20\x41"), // 40 pixel_enqueue_a
    Site::new(0xedb9d0, b"\x48\x89\x5c\x24\x20\x56\x48\x83\xec\x20\x41\x80\x78\x06\x00\x48"), // 41 pixel_enqueue_b
    Site::new(0x18d21c0, b"\x48\x89\x5c\x24\x20\x56\x48\x83\xec\x20\x41\x80\x78\x06\x00\x48"), // 42 pixel_enqueue_c
    Site::new(0xf4c620, b"\x40\x53\x56\x57\x48\x83\xec\x30\x41\x80\x78\x06\x00\x4d\x8b\xd1"), // 43 pixel_enqueue_d
    Site::new(0x2225470, b"\x48\x83\xec\x08\x80\x3d\x15\x02\x69\x01\x00\x0f\x28\xd9\xf3\x0f"), // 44 sky_projection
    Site::new(0x222afa0, b"\x40\x55\x41\x54\x41\x55\x41\x57\x48\x8d\xac\x24\x38\xfc\xff\xff"), // 45 sky_render
    Site::new(0x10ecdd0, b"\x0f\xb6\x81\x1c\x0c\x00\x00\xc3\xcc\xcc\xcc\xcc\xcc\xcc\xcc\xcc"), // 46 temporal_sample_index
    Site::new(0x109f300, b"\x83\xb9\x58\x02\x00\x00\x02\x41\xba\x0a\x00\x00\x00\x45\x8b\xc2"), // 47 scene_colour_format
    Site::new(0x112ff00, b"\x4c\x8b\xdc\x49\x89\x5b\x08\x49\x89\x6b\x18\x56\x57\x41\x56\x48"), // 48 render_family
    Site::new(0x3a5a20, b"\x40\x53\x48\x83\xec\x20\xc7\x41\x08\xff\x00\x00\x00\x48\x8d\x05"), // 49 frame_task_construct
    Site::new(0x3a8b60, b"\x48\x89\x5c\x24\x10\x48\x89\x6c\x24\x18\x48\x89\x74\x24\x20\x57"), // 50 frame_task_execute
    Site::new(0x1207df0, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\x51\x30\x48\x8b"), // 51 rhi_frame_begin
    Site::new(0x120c540, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\x51\x30\x48\x8b"), // 52 rhi_frame_end
    Site::new(0x1ada010, b"\x48\x89\x5c\x24\x08\x48\x89\x74\x24\x10\x57\x48\x83\xec\x20\x48"), // 53 frame_sync
    Site::new(0xa88dc0, b"\x48\x83\xec\x68\x45\x33\xc9\x45\x33\xc0\x33\xd2\x84\xc9\x48\x8d"), // 54 pump_messages
    Site::new(0x1adbcb0, b"\x40\x55\x53\x57\x48\x8d\x6c\x24\xb9\x48\x81\xec\xc0\x00\x00\x00"), // 55 engine_pacing
    Site::new(0xe31e90, b"\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10\x48\x89\x74\x24\x18\x57"), // 56 create_pixel_shader
    Site::new(0xebf050, b"\x48\x89\x5c\x24\x10\x55\x56\x57\x41\x54\x41\x57\x48\x83\xec\x40"), // 57 base_pass
    Site::new(0x1158250, b"\x4c\x89\x4c\x24\x20\x4c\x89\x44\x24\x18\x48\x89\x54\x24\x10\x48"), // 58 translucency_mesh
    Site::new(0x188cfa0, b"\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10\x48\x89\x74\x24\x20\x57"), // 59 mesh_particle_parameters
    Site::new(0x190e740, b"\x4c\x8b\xdc\x55\x53\x56\x57\x41\x56\x41\x57\x49\x8d\xab\xd8\xfe"), // 60 beam_no_noise
    Site::new(0x1904d40, b"\x4c\x8b\xdc\x55\x49\x8d\xab\xd8\xfc\xff\xff\x48\x81\xec\x20\x04"), // 61 beam_noise
    Site::new(0x1907980, b"\x40\x55\x48\x8d\xac\x24\xc0\xfb\xff\xff\x48\x81\xec\x40\x05\x00"), // 62 beam_interpolated_noise
    Site::new(0x190d510, b"\x40\x55\x53\x57\x41\x54\x41\x57\x48\x8d\xac\x24\x50\xfe\xff\xff"), // 63 ribbon_fill
    Site::new(0x190ce60, b"\x48\x89\x4c\x24\x08\x55\x53\x57\x41\x57\x48\x8d\xac\x24\x38\xff"), // 64 anim_trail_fill
];

/// The pass role of each Process site: tonemap, AA, material, HUD, composite, output.
pub(crate) const PROCESS_ROLES: [u32; RSF_AC7_PROCESS_HOOKS as usize] = [
    RSF_AC7_ROLE_TONEMAP,
    RSF_AC7_ROLE_AA,
    RSF_AC7_ROLE_MATERIAL,
    RSF_AC7_ROLE_HUD,
    RSF_AC7_ROLE_COMPOSITE,
    RSF_AC7_ROLE_OUTPUT,
];

/// Engine functions the detours call or whose layout they read; never hooked.
#[rustfmt::skip]
pub(crate) static HELPERS: [Site; 25] = [
    Site::new(0xfb7a00, b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x40\x48\x8b\x01\x48\x8b\xda"),
    Site::new(0x1126090, b"\x83\xb9\xb0\x09\x00\x00\x02\x4c\x8b\x89\x10\x14\x00\x00\x75\x58"),
    Site::new(0x12183c0, b"\x40\x53\x48\x83\xec\x20\x8b\x1d\x94\x0c\xa6\x02\x85\xdb\x0f\x84"),
    Site::new(0x116ea00, b"\x4c\x8b\xdc\x55\x56\x57\x41\x56\x48\x81\xec\x78\x0d\x00\x00\x48"),
    Site::new(0x109e0d0, b"\x4c\x89\x44\x24\x18\x53\x56\x57\x48\x81\xec\xa0\x00\x00\x00\x48"),
    Site::new(0xee93f0, b"\x48\x89\x5c\x24\x08\x55\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57"),
    Site::new(0x923e60, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x0a\xe1\x15\x03\x48\x85"),
    Site::new(0x938fb0, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\xa2\xb6\x14\x03\x48\x85"),
    Site::new(0x925860, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\xc2\xc8\x15\x03\x48\x85"),
    Site::new(0x925d10, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x22\xc4\x15\x03\x48\x85"),
    Site::new(0x925e10, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x3a\xc3\x15\x03\x48\x85"),
    Site::new(0x925710, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\xea\xc8\x15\x03\x48\x85"),
    Site::new(0x8b3100, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x22\x16\x1c\x03\x48\x85"),
    Site::new(0x1b495f0, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\x52\x6b\x17\x02\x48\x85"),
    Site::new(0x8c1bd0, b"\x4c\x8b\xdc\x48\x83\xec\x78\x48\x8b\x05\xb2\x50\x1b\x03\x48\x85"),
    Site::new(0xfcf880, b"\x48\x8b\xc4\x55\x53\x57\x41\x55\x48\x8d\x68\xa1\x48\x81\xec\x88"),
    Site::new(0x1ab7dc0, b"\x48\x89\x5c\x24\x10\x57\x48\x83\xec\x40\x48\x83\x79\x70\x00\x0f"),
    Site::new(0xde5cf0, b"\x40\x53\x48\x83\xec\x20\x48\x8b\xd9\x48\x3b\xca\x0f\x84\xbe\x00"),
    Site::new(0x19b6fe0, b"\x48\x83\xec\x28\x8b\x41\x08\x45\x32\xc0\x0f\x57\xd2\x4c\x8b\xd9"),
    Site::new(0xe93670, b"\x48\x89\x5c\x24\x08\x48\x89\x74\x24\x10\x57\x48\x83\xec\x20\x48"),
    Site::new(0x10b4870, b"\x48\x89\x5c\x24\x10\x48\x89\x6c\x24\x18\x48\x89\x74\x24\x20\x57"),
    Site::new(0x109dc70, b"\x48\x8d\x05\x99\xcf\xb9\x02\xc3\xcc\xcc\xcc\xcc\xcc\xcc\xcc\xcc"),
    Site::new(0x1127a20, b"\x48\x89\x5c\x24\x20\x55\x56\x57\x48\x8d\x6c\x24\xb9\x48\x81\xec"),
    Site::new(0xff9af0, b"\x40\x53\x48\x83\xec\x60\xf3\x0f\x10\x41\x20\x48\x8b\xd9\xf3\x0f"),
    Site::new(0x11346d0, b"\x48\x8b\xc4\x55\x56\x57\x41\x56\x41\x57\x48\x8d\x68\xb9\x48\x81"),
];

const SHADER_DEPTH_NATIVE: &[u8; 9] = b"\x0f\x2f\x86\x20\x02\x00\x00\x76\x04";
const SHADER_DEPTH_PATCHED: &[u8; 9] = b"\x0f\x2f\x86\x20\x02\x00\x00\x74\x04";

/// Enlarged separate translucency needs the matching depth and shader-view paths.
#[rustfmt::skip]
pub(crate) static DEPTH_GATES: [Gate; 11] = [
    Gate::new(0x1168f6f, b"\x76\x0e", b"\x74\x0e", "the depth allocation and fill"),
    Gate::new(0x1097a0c, b"\x76\x17", b"\x74\x17", "the depth bind"),
    Gate::new(0x109d03a, b"\x44\x0f\x47\xf9", b"\x44\x0f\x45\xf9", "the snapshot resolve"),
    Gate::new(0x109d061, b"\x76\x17", b"\x74\x17", "the resolve"),
    Gate::new(0x11583a6, b"\x0f\x2f\x80\x20\x02\x00\x00\x76\x0e",
        b"\x0f\x2f\x80\x20\x02\x00\x00\x74\x0e", "the layer view selection"),
    Gate::new(0x1025b12, SHADER_DEPTH_NATIVE, SHADER_DEPTH_PATCHED, "shader depth selection 1"),
    Gate::new(0x10294d2, SHADER_DEPTH_NATIVE, SHADER_DEPTH_PATCHED, "shader depth selection 2"),
    Gate::new(0x102ada2, SHADER_DEPTH_NATIVE, SHADER_DEPTH_PATCHED, "shader depth selection 3"),
    Gate::new(0x102c672, SHADER_DEPTH_NATIVE, SHADER_DEPTH_PATCHED, "shader depth selection 4"),
    Gate::new(0x102e792, SHADER_DEPTH_NATIVE, SHADER_DEPTH_PATCHED, "shader depth selection 5"),
    Gate::new(0x1031902, SHADER_DEPTH_NATIVE, SHADER_DEPTH_PATCHED, "shader depth selection 6"),
];

/// Code at `rva` in the module at `base`. The address is only passed to the native leaves.
pub(crate) fn at(base: *const u8, rva: u32) -> *const u8 {
    base.wrapping_add(rva as usize)
}

/// Exact-build admission in the former order: hooked entry sites, enlarged UI depth/view gates,
/// helper sites. The first difference is logged and refuses the build.
pub(crate) fn admit(base: *const u8, code: &impl CodeView) -> bool {
    if let Some(site) = HOOK_SITES
        .iter()
        .find(|site| !code.matches(at(base, site.rva), &site.expected))
    {
        code.log(&format!(
            "AC7 native renderer refused: expected entry bytes differ at RVA 0x{:x}",
            site.rva
        ));
        return false;
    }
    if let Some(gate) = DEPTH_GATES
        .iter()
        .find(|gate| !code.matches(at(base, gate.rva), gate.expected))
    {
        code.log(&format!(
            "AC7 native renderer refused: enlarged UI depth/view gate missing at RVA 0x{:x}",
            gate.rva
        ));
        return false;
    }
    if let Some(helper) = HELPERS
        .iter()
        .find(|helper| !code.matches(at(base, helper.rva), &helper.expected))
    {
        code.log(&format!(
            "AC7 native renderer refused: expected helper bytes differ at RVA 0x{:x}",
            helper.rva
        ));
        return false;
    }
    true
}

/// Evidence hooks run for every pixel uniform bind of every draw. Their sites are still verified,
/// but they are hooked only when the pixel binding diagnostic is asked for.
pub(crate) fn wanted(hook: u32, diagnostics: bool) -> bool {
    hook < RSF_AC7_HOOK_SITE_COUNT
        && (diagnostics || !(RSF_AC7_EVIDENCE_FIRST..=RSF_AC7_EVIDENCE_LAST).contains(&hook))
}

/// # Safety
/// `leaves` is null or a table valid for the call. The leaves receive `base` plus each RVA.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_build_admit(
    base: *const u8,
    leaves: *const rsf_ac7_code_leaves,
) -> bool {
    match unsafe { leaves.as_ref() }.and_then(NativeCodeView::new) {
        Some(code) => admit(base, &code),
        None => false,
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_ac7_hook_rva(hook: u32) -> u32 {
    HOOK_SITES.get(hook as usize).map_or(0, |site| site.rva)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_ac7_hook_role(hook: u32) -> u32 {
    PROCESS_ROLES.get(hook as usize).copied().unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_hook_wanted(hook: u32, diagnostics: bool) -> bool {
    wanted(hook, diagnostics)
}

#[cfg(test)]
mod tests;
