// SPDX-License-Identifier: MIT
//! RDR2.exe native layouts: struct offsets, the private FSR2 module's exports and descriptor, the
//! Social Club gate relay and NGX values. Code sites and globals are discovered by signature
//! (`discovery.rs`); researched on Steam 1.0.1491.50. Sources: games/rdr2/engine.json, README.md
//! and RESEARCH-HANDOFF.md.

/// Executable recognition by name; whether the build is usable is decided by site discovery.
pub(crate) const EXECUTABLE_NAME: &std::ffi::CStr = c"RDR2.exe";
pub(crate) const AMD64: u16 = 0x8664;

/// Social Club backbuffer renderer layout; its frame entry and gate are discovered.
pub(crate) mod social_club {
    /// The gate's `cmp byte [rcx+40C],0`; the relay resumes right after it.
    pub(crate) const GATE_LENGTH: usize = 7;
    pub(crate) const SWAPCHAIN: usize = 0x20;
    pub(crate) const QUERY_CURRENT_INDEX: usize = 0x40c;
    // With R9 set (external command list), set ZF and continue at the engine's own branch.
    // Without R9, execute its original comparison first. Resume addresses are filled at 14 and 35.
    pub(crate) const RELAY: [u8; 43] = [
        0x4d, 0x85, 0xc9, 0x74, 0x11, 0x4d, 0x39, 0xc9, 0xff, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0x80, 0xb9, 0x0c, 0x04, 0, 0, 0, 0xff, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
}

/// The native worker fields copied by the camera probe's Version 2 record.
#[cfg(all(windows, feature = "research"))]
pub(crate) mod camera_probe {
    pub(crate) const JOB: usize = 0x1ce8;
    pub(crate) const COLOUR_EXTENT: usize = 0x18;
}

/// Read-only settings row and value diagnostics.
#[cfg(all(windows, feature = "research"))]
pub(crate) mod settings_probe {
    pub(crate) const KEYS: [u32; 4] = [0x4de8_7d5b, 0x58e2_3159, 0x3420_e356, 0x0cd9_de94];
    pub(crate) const METADATA_KEY: usize = 8;
    pub(crate) const GETTER_SLOT: usize = 0x78;
}

/// Per-thread render block reached through the TLS slot array.
pub(crate) mod block {
    pub(crate) const TLS_LIMIT: u32 = 4096;
    pub(crate) const VIEW: usize = 0x14d8;
    pub(crate) const DRAW: usize = 0x14f0;
    pub(crate) const FLAGS: usize = 0x61c;
    /// Either flag selects the indirect frame counter.
    pub(crate) const INDIRECT_COUNTER: u32 = 6;
}

/// Draw context fields read at the completed-scene blit.
pub(crate) mod draw {
    /// 26d4964 writes the colour-view array and count.
    pub(crate) const COMMANDS: usize = 0xcc30;
    pub(crate) const COLOR_COUNT: usize = 0xca94;
    pub(crate) const COLOR_VIEWS: usize = 0xca48;
    pub(crate) const VIEW_SLICE: usize = 0x14;
    /// 26ddf80 reads view+8 as its texture.
    pub(crate) const VIEW_TEXTURE: usize = 8;
    pub(crate) const TEXTURE_RESOURCE: usize = 0x48;
}

/// RAGE worker view copied at the upscale boundary.
pub(crate) mod view {
    pub(crate) const BYTES: usize = 0x68c;
    pub(crate) const VIEW: usize = 0x40;
    pub(crate) const PROJECTION: usize = 0x80;
    pub(crate) const WORLD: usize = 0xc0;
    pub(crate) const PREVIOUS_VIEW: usize = 0x140;
    /// Rebuilt with the current jitter.
    pub(crate) const PREVIOUS_PROJECTION: usize = 0x180;
    pub(crate) const NEAR: usize = 0x630;
    pub(crate) const FAR: usize = 0x634;
    pub(crate) const FOV_DEGREES: usize = 0x640;
    pub(crate) const JITTER_X: usize = 0x664;
    pub(crate) const JITTER_Y: usize = 0x668;
    pub(crate) const ACTIVE: usize = 0x671;
    pub(crate) const VIEWPORT_WIDTH: usize = 0x67c;
    pub(crate) const VIEWPORT_HEIGHT: usize = 0x680;
}

/// NGX parameter interface: GetI is slot 11, also used by the bounded probe.
pub(crate) const NGX_GET_INT_SLOT: usize = 11;
pub(crate) const NGX_CREATE_FLAGS: &std::ffi::CStr = c"DLSS.Feature.Create.Flags";
/// NVSDK_NGX_DLSS_Feature_Flags_MVJittered and _DepthInverted.
pub(crate) const NGX_MV_JITTERED: u32 = 4;
pub(crate) const NGX_DEPTH_INVERTED: u32 = 8;
pub(crate) const NGX_SUCCESS: u32 = 1;
pub(crate) const NGX_FAIL_MASK: u32 = 0xfff0_0000;
pub(crate) const NGX_FAIL: u32 = 0xbad0_0000;

/// The game's private ffx_fsr2_api_x64.dll, statically imported and resolved before the ASI loads.
pub(crate) mod fsr {
    use std::ffi::CStr;
    pub(crate) const MODULE: &CStr = c"ffx_fsr2_api_x64.dll";
    /// Both entries are found by export name and checked by their entry bytes.
    pub(crate) const DISPATCH: &CStr = c"ffxFsr2ContextDispatch";
    /// Null checks, then renderSize.width at description+0x520 against the context's maximum.
    pub(crate) const DISPATCH_BYTES: [u8; 16] = [
        0x48, 0x85, 0xc9, 0x74, 0x36, 0x48, 0x85, 0xd2, 0x74, 0x31, 0x8b, 0x41, 0x04, 0x39, 0x82,
        0x20,
    ];
    pub(crate) const CREATE: &CStr = c"ffxFsr2ContextCreate";
    pub(crate) const CREATE_BYTES: [u8; 16] = [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0xda, 0x41, 0xb8,
        0x60,
    ];
    /// FSR 2.1.2 prefix of the game's FSR 2.2 dispatch description, checked against the pinned
    /// 2.1.2 headers by vendor_census_layouts. The 2.2 description extends past it.
    pub(crate) const DESCRIPTION_BYTES: usize = 1352;
    // GPUOpen FSR 2.2.0/2.2.1, Windows x64: factor at 1352, auto-reactive flag at
    // 1356, 184-byte FfxResource at 1360, then four floats at 1544..1560.
    pub(crate) const FULL_DESCRIPTION_BYTES: usize = 1560;
    pub(crate) const DISPLAY_MOTION: u32 = 1 << 1;
    pub(crate) const JITTERED_MOTION: u32 = 1 << 2;
    pub(crate) const INVERTED_DEPTH: u32 = 1 << 3;
    pub(crate) const INFINITE_DEPTH: u32 = 1 << 4;
    pub(crate) const KNOWN_FLAGS: u32 = (1 << 9) - 1;
    pub(crate) const COMMAND_LIST: usize = 0;
    pub(crate) const COLOR: usize = 8;
    pub(crate) const DEPTH: usize = 192;
    pub(crate) const MOTION: usize = 376;
    pub(crate) const EXPOSURE: usize = 560;
    pub(crate) const OUTPUT: usize = 1112;
    /// FfxResource.state within each resource record.
    pub(crate) const RESOURCE_STATE: usize = 164;
    pub(crate) const STATE_UNORDERED_ACCESS: u32 = 1;
    pub(crate) const STATE_COMPUTE_READ: u32 = 2;
    pub(crate) const JITTER: usize = 1296;
    pub(crate) const MOTION_SCALE: usize = 1304;
    pub(crate) const RENDER_SIZE: usize = 1312;
    pub(crate) const PRE_EXPOSURE: usize = 1332;
    pub(crate) const RESET: usize = 1336;
    pub(crate) const FFX_OK: i32 = 0;
}

/// D3D12 and DXGI values used by the SR transitions and output check.
pub(crate) mod d3d {
    pub(crate) const UNORDERED_ACCESS: u32 = 0x8;
    pub(crate) const DEPTH_READ: u32 = 0x20;
    pub(crate) const NON_PIXEL_SHADER_RESOURCE: u32 = 0x40;
    pub(crate) const PIXEL_SHADER_RESOURCE: u32 = 0x80;
    pub(crate) const COPY_DEST: u32 = 0x400;
    pub(crate) const R16G16B16A16_FLOAT: u32 = 10;
    /// The largest render or output extent any RDR2 path accepts.
    pub(crate) const MAX_EXTENT: u32 = 16384;
}

/// Facts used by `ReScaleFrame.RDR2.asi` during startup, before the game reads its settings.
/// Sources: docs/research/rdr2-renderer.md, engine.json and RESEARCH-HANDOFF.md.
pub(crate) mod asi {
    /// rage::fwuiSystemSettingsCollection offsets from its generated parser tables. Graphics group
    /// at +b4: dlssIndex +34, dlssQuality +38, fsr2Index +40. Video group at +124: refresh rate
    /// numerator +24 and denominator +28, ReflexSettings +38.
    pub(crate) mod collection {
        pub(crate) const BYTES: usize = 0x180;
        pub(crate) const DLSS: usize = 0xe8;
        pub(crate) const DLSS_QUALITY: usize = 0xec;
        pub(crate) const FSR: usize = 0xf4;
        pub(crate) const VIDEO_ADAPTER: usize = 0x124;
        pub(crate) const REFRESH_NUMERATOR: usize = 0x148;
        pub(crate) const REFRESH_DENOMINATOR: usize = 0x14c;
        pub(crate) const REFLEX: usize = 0x15c;
        /// Native defaults have version 0; a loaded 1491.50 collection has version 37. The two
        /// bytes after it are padding.
        pub(crate) const VERSION: usize = 0x178;
        pub(crate) const VERSIONS: [u16; 2] = [0, 37];
    }

    /// Settings owner and manager layout.
    pub(crate) mod manager {
        /// Collections inside a settings owner.
        pub(crate) const OWNER_COLLECTION: usize = 0xb0;
        pub(crate) const PENDING_SECOND_COLLECTION: usize = 0x240;
        /// Scratch argument of the pending lookup; Apply copies the pending values from here.
        pub(crate) const PENDING_SCRATCH: usize = 0xb8;
        pub(crate) const GRAPHICS_ID: u32 = 0x8c46_3422;
        pub(crate) const STAGE: usize = 0x2c8;
        /// Active capability list of DLSS modes and its count.
        pub(crate) const DLSS_CHOICES: usize = 0x410;
        pub(crate) const DLSS_CHOICE_COUNT: usize = 0x42c;
        pub(crate) const DLSS_CHOICE_LIMIT: usize = 7;
    }
}

/// Direct-resource descriptor constructed by +26f3f10 and consumed by +26f3a3c. Its full native size
/// is kept so a fallback can reset history without changing the caller's stack descriptor.
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Evaluation {
    pub(crate) color: *mut std::ffi::c_void,
    pub(crate) output: *mut std::ffi::c_void,
    _sharpness: f32,
    _padding14: u32,
    pub(crate) depth: *mut std::ffi::c_void,
    pub(crate) motion: *mut std::ffi::c_void,
    pub(crate) jitter: [f32; 2],
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) reset: u32,
    pub(crate) motion_scale: [f32; 2],
    _padding44: u32,
    _transparency: *mut std::ffi::c_void,
    pub(crate) exposure: *mut std::ffi::c_void,
    _bias: *mut std::ffi::c_void,
    pub(crate) origins: [u32; 12],
    pub(crate) pre_exposure: f32,
    pub(crate) invert_x: u32,
    pub(crate) invert_y: u32,
    _tail: [u8; 0x168 - 0x9c],
}

const _: () = {
    assert!(size_of::<Evaluation>() == 0x168);
    assert!(std::mem::offset_of!(Evaluation, depth) == 0x18);
    assert!(std::mem::offset_of!(Evaluation, jitter) == 0x28);
    assert!(std::mem::offset_of!(Evaluation, reset) == 0x38);
    assert!(std::mem::offset_of!(Evaluation, exposure) == 0x50);
    assert!(std::mem::offset_of!(Evaluation, origins) == 0x60);
    assert!(std::mem::offset_of!(Evaluation, pre_exposure) == 0x90);
    assert!(std::mem::offset_of!(Evaluation, _tail) == 0x9c);
};

impl Default for Evaluation {
    fn default() -> Self {
        // Null resource pointers and zero numbers.
        unsafe { std::mem::zeroed() }
    }
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    let mut value = [0; N];
    value.copy_from_slice(&bytes[offset..offset + N]);
    value
}

fn pointer(bytes: &[u8], offset: usize) -> *mut std::ffi::c_void {
    usize::from_le_bytes(field(bytes, offset)) as *mut std::ffi::c_void
}

fn float(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(field(bytes, offset))
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(field(bytes, offset))
}

/// The FSR dispatch normalized to the DLSS direct-resource descriptor, with engine states.
pub(crate) struct FsrDispatch {
    pub(crate) command_list: *mut std::ffi::c_void,
    pub(crate) descriptor: Evaluation,
    /// Engine states for color, depth, motion, output and exposure, in that order.
    pub(crate) states: [u32; 5],
    pub(crate) states_valid: bool,
}

pub(crate) fn fsr_dispatch(bytes: &[u8; fsr::DESCRIPTION_BYTES]) -> FsrDispatch {
    let descriptor = Evaluation {
        color: pointer(bytes, fsr::COLOR),
        depth: pointer(bytes, fsr::DEPTH),
        motion: pointer(bytes, fsr::MOTION),
        exposure: pointer(bytes, fsr::EXPOSURE),
        output: pointer(bytes, fsr::OUTPUT),
        jitter: [float(bytes, fsr::JITTER), float(bytes, fsr::JITTER + 4)],
        motion_scale: [
            float(bytes, fsr::MOTION_SCALE),
            float(bytes, fsr::MOTION_SCALE + 4),
        ],
        width: word(bytes, fsr::RENDER_SIZE),
        height: word(bytes, fsr::RENDER_SIZE + 4),
        pre_exposure: float(bytes, fsr::PRE_EXPOSURE),
        reset: u32::from(bytes[fsr::RESET] != 0),
        ..Evaluation::default()
    };
    let mut states = [0; 5];
    let mut states_valid = true;
    let records = [
        fsr::COLOR,
        fsr::DEPTH,
        fsr::MOTION,
        fsr::OUTPUT,
        fsr::EXPOSURE,
    ];
    for (state, record) in states.iter_mut().zip(records) {
        if pointer(bytes, record).is_null() {
            continue;
        }
        match word(bytes, record + fsr::RESOURCE_STATE) {
            fsr::STATE_UNORDERED_ACCESS => *state = d3d::UNORDERED_ACCESS,
            fsr::STATE_COMPUTE_READ => *state = d3d::NON_PIXEL_SHADER_RESOURCE,
            _ => states_valid = false,
        }
    }
    FsrDispatch {
        command_list: pointer(bytes, fsr::COMMAND_LIST),
        descriptor,
        states,
        states_valid,
    }
}

/// `rel32` call target of a five-byte `call` at `site`, or `None` for another instruction.
pub(crate) fn call_target(site: usize, bytes: &[u8; 5]) -> Option<usize> {
    if bytes[0] != 0xe8 {
        return None;
    }
    let displacement = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    usize::try_from(site as i64 + 5 + i64::from(displacement)).ok()
}

#[cfg(test)]
mod tests;
