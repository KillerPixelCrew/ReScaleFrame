// SPDX-License-Identifier: MIT
//! Steam RDR2.exe 1.0.1491.50 facts: fingerprints, RVAs, expected entry bytes and native layouts.
//! Sources: games/rdr2/engine.json, README.md and RESEARCH-HANDOFF.md. Every hook site names the
//! bytes it expects so a different build refuses instead of patching an unrelated instruction.

/// Executable recognition (PE timestamp 2024-03-11, see engine.json).
pub(crate) const EXECUTABLE: &[u8] = b"RDR2.exe";
pub(crate) const SHA256: &[u8] =
    b"b56c9548f670654a9b73bf25def3cd73af12e269f6e47dba28a34079adaf465e";
pub(crate) const AMD64: u16 = 0x8664;

/// Loaded image identity checked before any hook is created.
pub(crate) const IMAGE_TIMESTAMP: u32 = 0x65ee_c92e;
pub(crate) const IMAGE_SIZE: u32 = 0x0739_7600;

/// A 16-byte entry guard at an image RVA.
pub(crate) struct Site {
    pub(crate) rva: usize,
    pub(crate) expected: [u8; 16],
}

/// DLSS direct-resource evaluation consumer; its descriptor is built by +26f3f10.
pub(crate) const EVALUATE: Site = Site {
    rva: 0x26f_3a3c,
    expected: [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18,
        0x57,
    ],
};
/// Command-queue ring pop; the game's own submit returns to `SUBMIT_RETURN`.
pub(crate) const POP: Site = Site {
    rva: 0x26c_97d4,
    expected: [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xec, 0x20,
        0x48,
    ],
};
pub(crate) const SUBMIT_RETURN: usize = 0x26d_f79b;

/// Completed-scene producer and native frame events.
pub(crate) const BLIT: Site = Site {
    rva: 0x5c_1254,
    expected: [
        0x4c, 0x8b, 0xdc, 0x49, 0x89, 0x5b, 0x08, 0x49, 0x89, 0x6b, 0x10, 0x49, 0x89, 0x73, 0x18,
        0x49,
    ],
};
pub(crate) const MARKER: Site = Site {
    rva: 0x26d_4674,
    expected: [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x54, 0x24, 0x10, 0x55, 0x48, 0x8d, 0x6c, 0x24,
        0xa9,
    ],
};
pub(crate) const OUTPUT: Site = Site {
    rva: 0x5f_1e08,
    expected: [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec, 0x50,
        0x41,
    ],
};
pub(crate) const SLEEP: Site = Site {
    rva: 0x268_7fb4,
    expected: [
        0x40, 0x53, 0x48, 0x83, 0xec, 0x50, 0x80, 0x3d, 0x06, 0x3d, 0x09, 0x03, 0x00, 0x0f, 0x84,
        0xc4,
    ],
};
pub(crate) const MODE: Site = Site {
    rva: 0x1_e800,
    expected: [
        0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0xfa, 0x48, 0x8b,
        0xf1,
    ],
};
/// The live DX12 renderer's vtable+248: `mov rax,[rdx+48]; ret`, the texture's native resource.
pub(crate) const NATIVE_TEXTURE_RVA: usize = 0x26b_4250;
pub(crate) const NATIVE_TEXTURE: [u8; 5] = [0x48, 0x8b, 0x42, 0x48, 0xc3];
/// `call +5f1e08` sites in the postfx output path.
pub(crate) const OUTPUT_CALLS: [usize; 3] = [0x5c_aafc, 0x5c_ad96, 0x5c_adec];
/// Return addresses of those calls; only they open an output scope.
pub(crate) const OUTPUT_RETURNS: [usize; 3] = [0x5c_ab01, 0x5c_ad9b, 0x5c_adf1];
/// The blit inside the output scope that writes the completed scene.
pub(crate) const BLIT_RETURN: usize = 0x5c_11b9;

/// Game globals.
pub(crate) const SWAPCHAIN: usize = 0x571_c7e8;
pub(crate) const TLS_INDEX: usize = 0x5b4_03e0;
pub(crate) const FRAME_COUNTER: usize = 0x542_904c;
pub(crate) const FRAME_COUNTER_POINTER: usize = 0x542_9058;
pub(crate) const FRAME_TIME: usize = 0x59b_393c;
pub(crate) const FSR_IMPORT: usize = 0x603_2a98;
/// The measured game wrapper call into ffxFsr2ContextDispatch.
pub(crate) const FSR_WRAPPER_RETURN: usize = 0x26f_050a;

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
    pub(crate) const TIMESTAMP: u32 = 0x6490_7ede;
    pub(crate) const IMAGE_SIZE: u32 = 0x1_0000;
    pub(crate) const DISPATCH: &CStr = c"ffxFsr2ContextDispatch";
    pub(crate) const DISPATCH_RVA: usize = 0x3c70;
    /// Null checks, then renderSize.width at description+0x520 against the context's maximum.
    pub(crate) const DISPATCH_BYTES: [u8; 16] = [
        0x48, 0x85, 0xc9, 0x74, 0x36, 0x48, 0x85, 0xd2, 0x74, 0x31, 0x8b, 0x41, 0x04, 0x39, 0x82,
        0x20,
    ];
    pub(crate) const CREATE: &CStr = c"ffxFsr2ContextCreate";
    pub(crate) const CREATE_RVA: usize = 0x3ab0;
    pub(crate) const CREATE_BYTES: [u8; 16] = [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0xda, 0x41, 0xb8,
        0x60,
    ];
    /// FSR 2.1.2 prefix of the game's FSR 2.2 dispatch description, checked against the pinned
    /// 2.1.2 headers by vendor_census_layouts. The 2.2 description extends past it.
    pub(crate) const DESCRIPTION_BYTES: usize = 1352;
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
    pub(crate) const MAX_EXTENT: u64 = 16384;
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
