// SPDX-License-Identifier: MIT
use super::*;
use crate::ffx_version;
use std::ffi::{CStr, CString};
use std::ptr::NonNull;

use super::ffx_abi::*;

const FFX_OK: u32 = FFX_API_RETURN_OK as u32;

pub(super) type Acquire =
    unsafe extern "C" fn(Raw, Raw, crate::sys::rsf_backend_log_fn, Raw) -> *const c_void;
pub(super) type Release = unsafe extern "C" fn(*const c_void);

pub(super) struct CompatibilityOps {
    pub acquire: Acquire,
    pub release: Release,
}

struct Sdk {
    create: PfnFfxCreateContext,
    destroy: PfnFfxDestroyContext,
    query: PfnFfxQuery,
    dispatch: PfnFfxDispatch,
    _compatibility: Option<Compatibility>,
    _module: SdkModule,
}

impl Sdk {
    fn open(desc: &Open, ops: &CompatibilityOps) -> Result<Self, i32> {
        let module = load_runtime(desc, "amd_fidelityfx_upscaler_dx12.dll").ok_or(LOAD_FAILED)?;
        let [create, destroy, query, dispatch] =
            crate::sdk_module::FFX_CONTEXT_EXPORTS.map(|name| module.symbol(name));
        if [create, destroy, query, dispatch]
            .iter()
            .any(Option::is_none)
        {
            return Err(MISSING_ENTRY);
        }
        let compatibility = if desc.fsr_major == 4 {
            let handle =
                unsafe { (ops.acquire)(module.as_ptr(), desc.device, desc.log, desc.log_user) };
            NonNull::new(handle.cast_mut()).map(|handle| Compatibility {
                handle,
                release: ops.release,
            })
        } else {
            None
        };
        // The signatures come from AMD's FFX API header. The module outlives each call.
        Ok(Self {
            create: unsafe {
                std::mem::transmute::<
                    std::option::Option<unsafe extern "system" fn() -> isize>,
                    PfnFfxCreateContext,
                >(create)
            },
            destroy: unsafe {
                std::mem::transmute::<
                    std::option::Option<unsafe extern "system" fn() -> isize>,
                    PfnFfxDestroyContext,
                >(destroy)
            },
            query: unsafe {
                std::mem::transmute::<
                    std::option::Option<unsafe extern "system" fn() -> isize>,
                    PfnFfxQuery,
                >(query)
            },
            dispatch: unsafe {
                std::mem::transmute::<
                    std::option::Option<unsafe extern "system" fn() -> isize>,
                    PfnFfxDispatch,
                >(dispatch)
            },
            _compatibility: compatibility,
            _module: module,
        })
    }

    unsafe fn query(&self, context: *mut Raw, desc: *mut ffxApiHeader) -> u32 {
        unsafe { self.query.unwrap()(context, desc) }
    }
}

#[derive(Default)]
struct CreateDescription {
    upscale: ffxCreateContextDescUpscale,
    device: ffxCreateBackendDX12Desc,
    version: ffxOverrideVersion,
}

fn header(kind: u32) -> ffxApiHeader {
    ffxApiHeader {
        type_: u64::from(kind),
        pNext: null_mut(),
    }
}

struct Compatibility {
    handle: NonNull<c_void>,
    release: unsafe extern "C" fn(*const c_void),
}

impl Drop for Compatibility {
    fn drop(&mut self) {
        unsafe { (self.release)(self.handle.as_ptr()) };
    }
}

pub(super) struct Owner {
    sdk: Sdk,
    description: Box<CreateDescription>,
    context: Raw,
    major: u32,
}

impl Owner {
    fn open(desc: &Open, ops: &CompatibilityOps) -> Result<(Backend, Version), i32> {
        // The SDK may retain the chained descriptions until context destruction.
        let description = crate::allocate_handle(CreateDescription::default());
        if description.is_null() {
            return Err(INIT_FAILED);
        }
        let description = unsafe { Box::from_raw(description) };
        let sdk = Sdk::open(desc, ops)?;
        let mut owner = Self {
            sdk,
            description,
            context: null_mut(),
            major: desc.fsr_major,
        };
        let version = owner.select_version(desc)?;
        owner.create_context(desc, version.id)?;
        let mut actual = ffxQueryGetProviderVersion {
            header: header(FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION),
            ..Default::default()
        };
        if unsafe {
            owner
                .sdk
                .query(&mut owner.context, (&raw mut actual).cast())
        } != FFX_OK
            || actual.versionId != version.id
        {
            return Err(NOT_SUPPORTED);
        }
        Ok((Backend::Fsr(owner), version))
    }

    fn create_context(&mut self, desc: &Open, version: u64) -> Result<(), i32> {
        let data = &mut *self.description;
        data.version.header = header(FFX_API_DESC_TYPE_OVERRIDE_VERSION);
        data.version.versionId = version;
        data.device.header = header(RSF_FFX_CREATE_BACKEND_DX12 as u32);
        data.device.device = desc.device;
        data.device.header.pNext = (&raw mut data.version).cast();
        data.upscale.header = header(RSF_FFX_CREATE_UPSCALE as u32);
        data.upscale.header.pNext = (&raw mut data.device).cast();
        let output = FfxApiDimensions2D {
            width: desc.output_width,
            height: desc.output_height,
        };
        data.upscale.maxRenderSize = output;
        data.upscale.maxUpscaleSize = output;
        for (condition, flag) in [
            (desc.hdr, FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE),
            (desc.inverted_depth, FFX_UPSCALE_ENABLE_DEPTH_INVERTED),
            (desc.depth_infinite, FFX_UPSCALE_ENABLE_DEPTH_INFINITE),
            (
                desc.dynamic_resolution,
                FFX_UPSCALE_ENABLE_DYNAMIC_RESOLUTION,
            ),
            (
                desc.motion_jittered,
                FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION,
            ),
            (desc.auto_exposure, FFX_UPSCALE_ENABLE_AUTO_EXPOSURE),
        ] {
            if condition != 0 {
                data.upscale.flags |= flag as u32;
            }
        }
        let result = unsafe {
            self.sdk.create.unwrap()(
                &mut self.context,
                (&raw mut data.upscale).cast(),
                std::ptr::null(),
            )
        };
        if result != FFX_OK {
            return Err(INIT_FAILED);
        }
        if self.context.is_null() {
            return Err(NOT_SUPPORTED);
        }
        Ok(())
    }

    fn select_version(&self, desc: &Open) -> Result<Version, i32> {
        let mut versions = [ProviderVersion::default(); MAX_VERSIONS];
        let versions = unsafe {
            query_d3d12(
                self.sdk.query,
                desc.device,
                u64::from(RSF_FFX_CREATE_UPSCALE as u32),
                &mut versions,
            )
        }
        .ok_or(NOT_SUPPORTED)?;
        let count = versions.len();
        if let Some(best) = ffx_version::admitted(versions, self.major, desc.version_id) {
            let name = unsafe { CStr::from_ptr(best.name) };
            return Ok(Version::new(best.id, name.to_bytes()));
        }
        if let Some(log) = desc.log {
            if let Ok(message) = CString::new(format!(
                "FSR{} unavailable: runtime exposes no matching provider on this device ({count} available)",
                self.major
            )) {
                unsafe { log(desc.log_user, message.as_ptr()) };
            }
            const PREFIX: &[u8] = b"FSR available provider: ";
            for version in versions {
                let name = if version.name.is_null() {
                    c"unnamed"
                } else {
                    unsafe { CStr::from_ptr(version.name) }
                };
                let mut message = [0; 192];
                copy_bytes(&mut message[..PREFIX.len()], PREFIX);
                copy_bytes(&mut message[PREFIX.len()..191], name.to_bytes());
                unsafe { log(desc.log_user, message.as_ptr()) };
            }
        }
        Err(NOT_SUPPORTED)
    }

    pub(super) fn plan(
        &mut self,
        output: &Extent,
        quality: u32,
        width: *mut u32,
        height: *mut u32,
    ) -> i32 {
        if quality > ULTRA_PERFORMANCE {
            return INVALID;
        }
        // FSR2 can dispatch 1:1 but its quality query does not accept Native AA.
        if quality == 0 && self.major == 2 {
            unsafe {
                width.write(output[0]);
                height.write(output[1]);
            }
            return OK;
        }
        let mut query = ffxQueryDescUpscaleGetRenderResolutionFromQualityMode {
            header: header(RSF_FFX_QUERY_UPSCALE_SIZE as u32),
            displayWidth: output[0],
            displayHeight: output[1],
            qualityMode: quality,
            pOutRenderWidth: width,
            pOutRenderHeight: height,
        };
        if unsafe { self.sdk.query(&mut self.context, (&raw mut query).cast()) } == FFX_OK {
            OK
        } else {
            FEATURE_FAILED
        }
    }

    pub(super) fn evaluate(&mut self, commands: Raw, frame: &Frame) -> i32 {
        let record = unsafe { &*frame.record };
        let far = record.camera.far_plane;
        let far = if far > 0.0 { far } else { f32::MAX };
        let (enable_sharpening, sharpness) =
            crate::sharpening_policy::ffx_sharpening(frame.sharpness);
        let read = FFX_API_RESOURCE_STATE_COMPUTE_READ;
        let resource =
            |texture, state: i32| unsafe { super::ffx_resource::describe(texture, state as u32) };
        let data = ffxDispatchDescUpscale {
            header: header(RSF_FFX_DISPATCH_UPSCALE as u32),
            commandList: commands,
            color: resource(frame.color.resource, read),
            depth: resource(frame.depth.resource, read),
            motionVectors: resource(frame.motion.resource, read),
            exposure: resource(frame.exposure.resource, read),
            reactive: resource(frame.reactive.resource, read),
            transparencyAndComposition: resource(frame.transparency.resource, read),
            output: resource(
                frame.output.resource,
                FFX_API_RESOURCE_STATE_UNORDERED_ACCESS,
            ),
            jitterOffset: FfxApiFloatCoords2D {
                x: frame.jitter_x,
                y: frame.jitter_y,
            },
            motionVectorScale: FfxApiFloatCoords2D {
                x: frame.motion_scale_x,
                y: frame.motion_scale_y,
            },
            renderSize: FfxApiDimensions2D {
                width: record.render_width,
                height: record.render_height,
            },
            upscaleSize: FfxApiDimensions2D {
                width: record.output_width,
                height: record.output_height,
            },
            frameTimeDelta: record.frame_time_ms,
            preExposure: frame.pre_exposure,
            reset: frame.reset != 0,
            enableSharpening: enable_sharpening,
            sharpness,
            cameraNear: record.camera.near_plane,
            cameraFar: far,
            cameraFovAngleVertical: record.camera.vertical_fov_radians,
            viewSpaceToMetersFactor: frame.view_space_to_meters,
            flags: 0,
        };
        if unsafe { self.sdk.dispatch.unwrap()(&mut self.context, (&raw const data).cast()) }
            == FFX_OK
        {
            OK
        } else {
            FEATURE_FAILED
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe { self.sdk.destroy.unwrap()(&mut self.context, std::ptr::null()) };
        }
        // Sdk drops the INT8 compatibility lease before unloading the DLL.
    }
}

pub(super) fn open(desc: &Open, out: *mut Raw, ops: &CompatibilityOps) -> i32 {
    if !(2..=4).contains(&desc.fsr_major) {
        return INVALID;
    }
    open_session(desc, out, |_| Owner::open(desc, ops))
}
