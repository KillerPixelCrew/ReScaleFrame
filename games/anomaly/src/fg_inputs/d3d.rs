// SPDX-License-Identifier: MIT
//! D3D11 allocations and recording for the SR-off FG input owner.
use super::*;
use rsf_shader_compiler::{Compiler, Request, compile};
use std::ffi::CString;
use windows::Win32::Graphics::Direct3D::D3D11_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{IUnknown, Interface};

type Raw = *mut c_void;

// The generic save and restore pair retains the entire immediate-context state.
struct SavedState<'a> {
    context: Raw,
    state: &'a mut rsf_d3d11_state,
    scope: StateScope,
}

impl Drop for SavedState<'_> {
    fn drop(&mut self) {
        unsafe { (self.scope.restore)(self.context, self.state) };
    }
}

pub(super) const BINDINGS: Bindings = Bindings {
    device,
    release_interface: release,
    describe,
    same_device,
    shader,
    constant_buffer,
    texture,
    depth_view,
    unordered_view,
    motion_view,
    dispatch,
};

unsafe extern "C" fn release(raw: Raw) {
    drop(unsafe { IUnknown::from_raw(raw) });
}

fn owned<T: Interface>(value: Option<T>) -> Raw {
    value.map_or(ptr::null_mut(), Interface::into_raw)
}

unsafe extern "C" fn device(raw: Raw) -> Raw {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    if unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return ptr::null_mut();
    }
    owned(unsafe { context.GetDevice() }.ok())
}

unsafe extern "C" fn describe(raw: Raw, out: *mut Description) {
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
        out.write(Description {
            format: desc.Format.0 as u32,
            width: desc.Width,
            height: desc.Height,
            samples: desc.SampleDesc.Count,
            layers: desc.ArraySize,
            mips: desc.MipLevels,
        });
    }
}

unsafe extern "C" fn same_device(texture: Raw, device: Raw) -> u32 {
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) }.unwrap();
    let Ok(owner) = (unsafe { texture.GetDevice() }) else {
        return 0;
    };
    if owner.as_raw() == device {
        return 1;
    }
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.unwrap();
    u32::from(
        matches!((owner.cast::<IUnknown>(), device.cast::<IUnknown>()),
        (Ok(actual), Ok(expected)) if actual.as_raw() == expected.as_raw()),
    )
}

fn shader_code(log: Log, user: Raw) -> Option<Vec<u8>> {
    let emit = |message: &std::ffi::CStr| {
        if let Some(log) = log {
            unsafe { log(user, message.as_ptr()) };
        }
    };
    let Some(compiler) = Compiler::system("d3dcompiler_47.dll") else {
        emit(c"shader rsf_fg_inputs.hlsl: d3dcompiler_47 is not available");
        return None;
    };
    let entry = compiler.entry()?;
    let result = unsafe {
        compile(
            entry,
            Request {
                source: include_bytes!("../../shaders/fg/rsf_fg_inputs.hlsl"),
                name: c"rsf_fg_inputs.hlsl",
                entry: c"rsf_fg_inputs_cs",
                target: c"cs_5_0",
                flags: 1 << 11, // D3DCOMPILE_ENABLE_STRICTNESS
            },
            &[],
            ptr::null_mut(),
        )
    };
    if result.status < 0 {
        let errors = result.errors.as_deref().unwrap_or_default();
        let end = errors
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(errors.len());
        if let Ok(message) = CString::new(format!(
            "shader rsf_fg_inputs.hlsl (rsf_fg_inputs_cs): compile failed 0x{:08x} {}",
            result.status as u32,
            String::from_utf8_lossy(&errors[..end])
        )) {
            emit(&message);
        }
        return None;
    }
    result.code
}

unsafe extern "C" fn shader(raw: Raw, log: Log, user: Raw) -> Raw {
    let Some(code) = shader_code(log, user) else {
        return ptr::null_mut();
    };
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let mut shader = None;
    if unsafe { device.CreateComputeShader(&code, None::<&ID3D11ClassLinkage>, Some(&mut shader)) }
        .is_err()
    {
        return ptr::null_mut();
    }
    owned(shader)
}

unsafe extern "C" fn constant_buffer(raw: Raw) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<Constants>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        ..Default::default()
    };
    let mut buffer = None;
    if unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }.is_err() {
        return ptr::null_mut();
    }
    owned(buffer)
}

pub(crate) fn create_texture(
    device: &ID3D11Device,
    extent: [u32; 2],
    format: DXGI_FORMAT,
    bind: u32,
) -> Option<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: extent[0],
        Height: extent[1],
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.ok()?;
    texture
}

unsafe extern "C" fn texture(raw: Raw, width: u32, height: u32, format: u32, bind: u32) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    owned(create_texture(
        device,
        [width, height],
        DXGI_FORMAT(format as i32),
        bind,
    ))
}

unsafe extern "C" fn depth_view(raw: Raw, texture: Raw) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) }.unwrap();
    let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
        ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    desc.Anonymous.Texture2D = D3D11_TEX2D_SRV {
        MostDetailedMip: 0,
        MipLevels: 1,
    };
    let mut view = None;
    if unsafe { device.CreateShaderResourceView(texture, Some(&desc), Some(&mut view)) }.is_err() {
        return ptr::null_mut();
    }
    owned(view)
}

unsafe extern "C" fn unordered_view(raw: Raw, texture: Raw) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) }.unwrap();
    let mut view = None;
    if unsafe { device.CreateUnorderedAccessView(texture, None, Some(&mut view)) }.is_err() {
        return ptr::null_mut();
    }
    owned(view)
}

unsafe extern "C" fn motion_view(raw: Raw, texture: Raw) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) }.unwrap();
    let mut view = None;
    if unsafe { device.CreateShaderResourceView(texture, None, Some(&mut view)) }.is_err() {
        return ptr::null_mut();
    }
    owned(view)
}

#[allow(clippy::too_many_arguments)]
unsafe fn dispatch(
    scope: StateScope,
    raw: Raw,
    pipeline: *const PipelineBindings,
    targets: *const Targets,
    depth: Raw,
    motion: Raw,
    values: *const Constants,
    groups_x: u32,
    groups_y: u32,
) -> u32 {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let pipeline = unsafe { &*pipeline };
    let targets = unsafe { &*targets };
    let constants = unsafe { ID3D11Buffer::from_raw_borrowed(&pipeline.constants) }.unwrap();
    let mut state = rsf_d3d11_state::default();
    if unsafe { (scope.save)(raw, &mut state) } == 0 {
        return 0;
    }
    let _saved = SavedState {
        context: raw,
        state: &mut state,
        scope,
    };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if unsafe { context.Map(constants, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)) }.is_err()
    {
        return 0;
    }
    if mapped.pData.is_null() {
        unsafe { context.Unmap(constants, 0) };
        return 0;
    }
    unsafe {
        ptr::copy_nonoverlapping(values, mapped.pData.cast(), 1);
        context.Unmap(constants, 0);
        context.SetPredication(None::<&ID3D11Predicate>, false);
        let copy = ID3D11Texture2D::from_raw_borrowed(&targets.depth_copy).unwrap();
        let depth = ID3D11Texture2D::from_raw_borrowed(&depth).unwrap();
        context.CopyResource(copy, depth);
        let views = [
            ID3D11ShaderResourceView::from_raw_borrowed(&targets.depth_view).cloned(),
            ID3D11ShaderResourceView::from_raw_borrowed(&motion).cloned(),
        ];
        let uavs = [
            ID3D11UnorderedAccessView::from_raw_borrowed(&targets.depth_uav).cloned(),
            ID3D11UnorderedAccessView::from_raw_borrowed(&targets.motion_uav).cloned(),
        ];
        let shader = ID3D11ComputeShader::from_raw_borrowed(&pipeline.shader).unwrap();
        context.CSSetShader(shader, None);
        context.CSSetShaderResources(0, Some(&views));
        context.CSSetUnorderedAccessViews(0, 2, Some(uavs.as_ptr()), None);
        context.CSSetConstantBuffers(0, Some(&[Some(constants.clone())]));
        context.Dispatch(groups_x, groups_y, 1);
        context.CSSetUnorderedAccessViews(0, 2, Some([None, None].as_ptr()), None);
        context.CSSetShaderResources(0, Some(&[None, None]));
    }
    1
}
