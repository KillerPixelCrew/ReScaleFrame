// SPDX-License-Identifier: MIT
//! Steam RDR2.exe 1.0.1491.50 facts: fingerprints, RVAs, expected entry bytes and native layouts.
//! Sources: games/rdr2/engine.json, README.md and RESEARCH-HANDOFF.md. Every hook site names the
//! bytes it expects so a different build refuses instead of patching an unrelated instruction.

/// Executable recognition (PE timestamp 2024-03-11, see engine.json).
pub(crate) const EXECUTABLE_NAME: &std::ffi::CStr = c"RDR2.exe";
pub(crate) const EXECUTABLE: &[u8] = EXECUTABLE_NAME.to_bytes();
pub(crate) const SHA256: &std::ffi::CStr =
    c"b56c9548f670654a9b73bf25def3cd73af12e269f6e47dba28a34079adaf465e";
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

/// Exact Social Club 2.4.0.240 backbuffer renderer profile.
#[cfg(windows)]
pub(crate) mod social_club {
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const IMAGE_TIMESTAMP: u32 = 0x6a86_d560;
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const IMAGE_SIZE: u32 = 0x2e000;
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const DISPATCH_BYTES: [u8; 16] = [
        0x40, 0x53, 0x48, 0x83, 0xec, 0x30, 0x48, 0x83, 0x79, 0x08, 0x00, 0x48, 0x8b, 0xd9, 0x75,
        0x29,
    ];
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const ENTRY: usize = 0x4b70;
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const ENTRY_BYTES: [u8; 15] = [
        0x48, 0x89, 0x5c, 0x24, 0x18, 0x48, 0x89, 0x54, 0x24, 0x10, 0x55, 0x56, 0x57, 0x41, 0x54,
    ];
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const GATE: usize = 0x4b89;
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const GATE_BYTES: [u8; 15] = [
        0x80, 0xb9, 0x0c, 0x04, 0x00, 0x00, 0x00, 0x4d, 0x8b, 0xf1, 0x48, 0x8b, 0xf9, 0x74, 0x17,
    ];
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const AFTER_COMPARISON: usize = 0x4b90;
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const SWAPCHAIN: usize = 0x20;
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const QUERY_CURRENT_INDEX: usize = 0x40c;
    // With R9 set (external command list), set ZF and continue at the engine's own branch.
    // Without R9, execute its original comparison first. Resume addresses are filled at 14 and 35.
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const RELAY: [u8; 43] = [
        0x4d, 0x85, 0xc9, 0x74, 0x11, 0x4d, 0x39, 0xc9, 0xff, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0x80, 0xb9, 0x0c, 0x04, 0, 0, 0, 0xff, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
}

/// Camera probe entry bytes and the native worker fields copied by its Version 2 record.
#[cfg(all(windows, feature = "research"))]
pub(crate) mod camera_probe {
    use super::Site;

    pub(crate) const FSR: Site = Site {
        rva: 0x7b_a10c,
        expected: [
            0x48, 0x83, 0x3a, 0x00, 0x4c, 0x8b, 0xca, 0x74, 0x27, 0x48, 0x83, 0x7a, 0x18, 0x00,
            0x74, 0x20,
        ],
    };
    pub(crate) const DLSS: Site = Site {
        rva: 0x7b_b60c,
        expected: [
            0x8b, 0x81, 0xa8, 0x00, 0x00, 0x00, 0x4c, 0x8d, 0x0c, 0x80, 0x4a, 0x8d, 0x0c, 0xc9,
            0x48, 0x83,
        ],
    };
    pub(crate) const JOB: usize = 0x1ce8;
    pub(crate) const COLOUR_EXTENT: usize = 0x18;
}

/// Read-only settings row and value diagnostics for the same executable.
#[cfg(all(windows, feature = "research"))]
pub(crate) mod settings_probe {
    use super::Site;

    pub(crate) const BIND: Site = Site {
        rva: 0x2c6_26e0,
        expected: [
            0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x7c, 0x24, 0x18, 0x55, 0x48, 0x8d, 0x6c,
            0x24, 0xd0,
        ],
    };
    pub(crate) const READ: Site = Site {
        rva: 0x2c6_27b8,
        expected: [
            0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x50, 0x48, 0x8b, 0x02, 0x4c,
            0x8b, 0xd2,
        ],
    };
    pub(crate) const KEYS: [u32; 4] = [0x4de8_7d5b, 0x58e2_3159, 0x3420_e356, 0x0cd9_de94];
    pub(crate) const METADATA_KEY: usize = 8;
    pub(crate) const GETTER_SLOT: usize = 0x78;
}
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
    pub(crate) const MAX_EXTENT: u64 = 16384;
}

/// Facts used by `ReScaleFrame.RDR2.asi` during startup, before the game reads its settings.
/// Sources: docs/research/rdr2-renderer.md, engine.json and RESEARCH-HANDOFF.md.
pub(crate) mod asi {
    use super::Site;

    /// Engine string parameter read before the saved API setting; "d3d12" selects D3D12.
    pub(crate) const API_OVERRIDE: u32 = 0x57a_bf10;
    /// Every engine read of the parameter: `mov r64, [rip + disp32]` resolving to `API_OVERRIDE`.
    /// Settings load, adapter manager API selection, device settings adapter resolution and saved
    /// API level application.
    pub(crate) const API_OVERRIDE_READS: [(u32, [u8; 3]); 4] = [
        (0x2c6_5575, [0x48, 0x8b, 0x1d]),
        (0x26b_8ebb, [0x48, 0x8b, 0x05]),
        (0x22_a5f8, [0x48, 0x8b, 0x0d]),
        (0x2c6_62f6, [0x48, 0x8b, 0x05]),
    ];
    /// The D3D12 HRESULT check helper observed by the opt-in trace.
    #[cfg(feature = "research")]
    pub(crate) const RESULT_CHECK: Site = Site {
        rva: 0x268_b5c0,
        expected: [
            0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x8d, 0x81, 0xfb, 0xff, 0x85, 0x77, 0x8b, 0xd9,
            0x83, 0xf8,
        ],
    };
    /// Social Club backbuffer dispatcher armed by social_club.cpp; named in the startup report.
    pub(crate) const SOCIAL_CLUB_DISPATCHER: u32 = 0x27f_09e8;

    /// rage settings collection copy, settings-owner load and apply.
    pub(crate) const COPY: Site = Site {
        rva: 0x20_b7f8,
        expected: [
            0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x20, 0x0f, 0x10, 0x02, 0x48,
            0x8b, 0xd9,
        ],
    };
    pub(crate) const LOAD: Site = Site {
        rva: 0x2c7_c9a4,
        expected: [
            0x48, 0x8b, 0xc4, 0x48, 0x89, 0x58, 0x08, 0x48, 0x89, 0x70, 0x10, 0x48, 0x89, 0x78,
            0x18, 0x4c,
        ],
    };
    pub(crate) const APPLY: Site = Site {
        rva: 0x2c7_e6c0,
        expected: [
            0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0xd9, 0xe8, 0xca, 0xba, 0xff, 0xff,
            0x48, 0x8b,
        ],
    };
    /// DLSS optimal-settings table producer, used by allocations and NGX feature setup.
    pub(crate) const OPTIMAL: Site = Site {
        rva: 0x26f_5270,
        expected: [
            0x48, 0x89, 0x5c, 0x24, 0x08, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56,
            0x41, 0x57,
        ],
    };
    /// FSR2 render-dimension producer.
    pub(crate) const FSR_DIMENSIONS: Site = Site {
        rva: 0x26e_cc3c,
        expected: [
            0x48, 0x8b, 0xc4, 0x48, 0x89, 0x58, 0x08, 0x48, 0x89, 0x70, 0x10, 0x48, 0x89, 0x78,
            0x18, 0x55,
        ],
    };
    /// Renderer dimension refresh after a graphics reset.
    pub(crate) const REFRESH_DIMENSIONS: Site = Site {
        rva: 0x58_a908,
        expected: [
            0x48, 0x81, 0xec, 0x38, 0x01, 0x00, 0x00, 0x48, 0x8d, 0x4c, 0x24, 0x20, 0xe8, 0xcf,
            0xd7, 0xfc,
        ],
    };
    /// Settings manager graphics reconfiguration (state 9), also reached by stock Apply.
    pub(crate) const RECONFIGURE: Site = Site {
        rva: 0x2c7_e8f4,
        expected: [
            0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0xba, 0x09, 0x00, 0x00, 0x00, 0x48, 0x8b, 0xd9,
            0x39, 0x91,
        ],
    };

    /// Engine functions the quality request calls, each checked in full before any hook.
    pub(crate) struct Function {
        pub(crate) rva: usize,
        pub(crate) expected: &'static [u8],
    }
    /// Settings manager accessor: `mov rax, [rip + disp]; ret`.
    pub(crate) const MANAGER: Function = Function {
        rva: 0x2cf_31f8,
        expected: &[0x48, 0x8b, 0x05, 0xe9, 0xac, 0x1c, 0x01, 0xc3],
    };
    /// Active settings owner of a manager.
    pub(crate) const OWNER: Function = Function {
        rva: 0x2c7_6114,
        expected: &[
            0x4c, 0x8b, 0xdc, 0x53, 0x48, 0x81, 0xec, 0xb0, 0x00, 0x00, 0x00, 0x49, 0x8d, 0x43,
            0x08,
        ],
    };
    /// Pending settings owner lookup by id path.
    pub(crate) const FIND: Function = Function {
        rva: 0x2c7_36f4,
        expected: &[
            0x40, 0x53, 0x48, 0x83, 0xec, 0x30, 0x48, 0x83, 0x7a, 0x08, 0x00, 0x49, 0x8b, 0xc0,
        ],
    };

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

    /// Renderer dimension globals: input width/height and output width/height.
    pub(crate) const RENDER_INPUT: usize = 0x3f0_db38;
    pub(crate) const RENDER_OUTPUT: usize = 0x3f0_db30;
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
