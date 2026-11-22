// SPDX-License-Identifier: MIT
use super::*;
use windows::{
    Win32::Graphics::{Direct3D12::*, Dxgi::Common::*},
    core::Interface,
};

#[path = "xess_abi.rs"]
mod abi;
use abi::*;

type Create = unsafe extern "C" fn(Raw, *mut Raw) -> i32;
type Destroy = unsafe extern "C" fn(Raw) -> i32;
type Init = unsafe extern "C" fn(Raw, *const xess_d3d12_init_params_t) -> i32;
type Execute = unsafe extern "C" fn(Raw, Raw, *const xess_d3d12_execute_params_t) -> i32;
type Resolution = unsafe extern "C" fn(Raw, *const xess_2d_t, i32, *mut xess_2d_t) -> i32;
type Velocity = unsafe extern "C" fn(Raw, f32, f32) -> i32;
type GetVersion = unsafe extern "C" fn(*mut xess_version_t) -> i32;

struct Sdk {
    create: Create,
    destroy: Destroy,
    init: Init,
    execute: Execute,
    resolution: Resolution,
    velocity: Velocity,
    version: GetVersion,
    _module: SdkModule,
}

impl Sdk {
    fn open(desc: &Open) -> Result<Self, i32> {
        let module = load_runtime(desc, "libxess.dll").ok_or(LOAD_FAILED)?;
        macro_rules! entry {
            ($name:literal, $signature:ty) => {{
                let symbol = module.symbol($name).ok_or(MISSING_ENTRY)?;
                // The export is borrowed from the module retained in this Sdk.
                unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, $signature>(symbol)
                }
            }};
        }
        Ok(Self {
            create: entry!(c"xessD3D12CreateContext", Create),
            destroy: entry!(c"xessDestroyContext", Destroy),
            init: entry!(c"xessD3D12Init", Init),
            execute: entry!(c"xessD3D12Execute", Execute),
            resolution: entry!(c"xessGetInputResolution", Resolution),
            velocity: entry!(c"xessSetVelocityScale", Velocity),
            version: entry!(c"xessGetVersion", GetVersion),
            _module: module,
        })
    }
}

// Order defined by rsf_quality in backend.h.
const QUALITIES: [i32; 6] = [
    XESS_QUALITY_SETTING_AA,
    XESS_QUALITY_SETTING_QUALITY,
    XESS_QUALITY_SETTING_BALANCED,
    XESS_QUALITY_SETTING_PERFORMANCE,
    XESS_QUALITY_SETTING_ULTRA_PERFORMANCE,
    XESS_QUALITY_SETTING_ULTRA_QUALITY,
];

fn zero_mask(device: Raw, [width, height]: Extent) -> Result<ID3D12Resource, i32> {
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }.ok_or(INVALID)?;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: width.into(),
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Format: DXGI_FORMAT_R8_UNORM,
        ..Default::default()
    };
    let mut texture = None;
    // Committed resources are zero-filled unless created with CREATE_NOT_ZEROED.
    unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            None,
            &mut texture,
        )
    }
    .map_err(|_| INIT_FAILED)?;
    texture.ok_or(INIT_FAILED)
}

pub(super) struct Owner {
    sdk: Sdk,
    context: Raw,
    zero_mask: Option<ID3D12Resource>,
    auto_exposure: bool,
}

impl Owner {
    fn open(desc: &Open, output: Extent) -> Result<(Backend, Version), i32> {
        let sdk = Sdk::open(desc)?;
        let mut owner = Self {
            sdk,
            context: null_mut(),
            zero_mask: None,
            auto_exposure: desc.auto_exposure != 0,
        };
        let mut version = xess_version_t::default();
        if unsafe { (owner.sdk.version)(&mut version) } != XESS_RESULT_SUCCESS {
            return Err(INIT_FAILED);
        }
        let xess_version_t {
            major,
            minor,
            patch,
            reserved,
        } = version;
        let id = (u64::from(major) << 48)
            | (u64::from(minor) << 32)
            | (u64::from(patch) << 16)
            | u64::from(reserved);
        if desc.version_id != 0 && desc.version_id != id {
            return Err(NOT_SUPPORTED);
        }
        if unsafe { (owner.sdk.create)(desc.device, &mut owner.context) } != XESS_RESULT_SUCCESS {
            return Err(NOT_SUPPORTED);
        }
        if owner.context.is_null() {
            return Err(INIT_FAILED);
        }
        // Responsive-mask mode requires a texture at every evaluation.
        owner.zero_mask = Some(zero_mask(desc.device, output)?);
        let mut flags = XESS_INIT_FLAG_RESPONSIVE_PIXEL_MASK;
        flags |= if desc.auto_exposure != 0 {
            XESS_INIT_FLAG_ENABLE_AUTOEXPOSURE
        } else {
            XESS_INIT_FLAG_EXPOSURE_SCALE_TEXTURE
        };
        for (condition, flag) in [
            (desc.inverted_depth != 0, XESS_INIT_FLAG_INVERTED_DEPTH),
            (desc.hdr == 0, XESS_INIT_FLAG_LDR_INPUT_COLOR),
            (desc.motion_jittered != 0, XESS_INIT_FLAG_JITTERED_MV),
        ] {
            if condition {
                flags |= flag;
            }
        }
        let init = xess_d3d12_init_params_t {
            outputResolution: xess_2d_t {
                x: output[0],
                y: output[1],
            },
            qualitySetting: QUALITIES[desc.quality as usize],
            initFlags: flags as u32,
            ..Default::default()
        };
        if unsafe { (owner.sdk.init)(owner.context, &init) } != XESS_RESULT_SUCCESS {
            return Err(INIT_FAILED);
        }
        let version = Version::new(id, format!("XeSS {major}.{minor}.{patch}").as_bytes());
        Ok((Backend::Xess(owner), version))
    }

    pub(super) fn plan(
        &self,
        output: &Extent,
        quality: u32,
        width: *mut u32,
        height: *mut u32,
    ) -> i32 {
        let Some(&quality) = QUALITIES.get(quality as usize) else {
            return INVALID;
        };
        let output = xess_2d_t {
            x: output[0],
            y: output[1],
        };
        let mut input = xess_2d_t::default();
        if unsafe { (self.sdk.resolution)(self.context, &output, quality, &mut input) }
            != XESS_RESULT_SUCCESS
        {
            return FEATURE_FAILED;
        }
        unsafe {
            width.write(input.x);
            height.write(input.y);
        }
        OK
    }

    pub(super) fn evaluate(&self, commands: Raw, frame: &Frame) -> i32 {
        if !self.auto_exposure && frame.exposure.resource.is_null() {
            return INVALID;
        }
        let result = unsafe {
            (self.sdk.velocity)(self.context, frame.motion_scale_x, frame.motion_scale_y)
        };
        if result != XESS_RESULT_SUCCESS {
            return FEATURE_FAILED;
        }
        let mask = if frame.reactive.resource.is_null() {
            self.zero_mask.as_ref().unwrap().as_raw()
        } else {
            frame.reactive.resource
        };
        let record = unsafe { &*frame.record };
        let params = xess_d3d12_execute_params_t {
            pColorTexture: frame.color.resource,
            pDepthTexture: frame.depth.resource,
            pVelocityTexture: frame.motion.resource,
            pExposureScaleTexture: frame.exposure.resource,
            pResponsivePixelMaskTexture: mask,
            pOutputTexture: frame.output.resource,
            inputWidth: record.render_width,
            inputHeight: record.render_height,
            jitterOffsetX: frame.jitter_x,
            jitterOffsetY: frame.jitter_y,
            exposureScale: 1.0 / frame.pre_exposure,
            resetHistory: frame.reset,
            ..Default::default()
        };
        if unsafe { (self.sdk.execute)(self.context, commands, &params) } == XESS_RESULT_SUCCESS {
            OK
        } else {
            FEATURE_FAILED
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe { (self.sdk.destroy)(self.context) };
        }
        drop(self.zero_mask.take());
    }
}

pub(super) fn open(desc: &Open, out: *mut Raw) -> i32 {
    open_session(desc, out, |output| Owner::open(desc, output))
}
