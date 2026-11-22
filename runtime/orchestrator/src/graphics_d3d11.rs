// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::{same_device, texture_desc};
use crate::d3d11_output_platform::srv_format;
use crate::sys::rsf_backend_log_fn as LogFn;
use crate::sys::{rsf_d3d11_output_is_immediate, rsf_d3d11_state};
use rsf_shader_compiler::{Compiler, Request, compile};
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::MaybeUninit;
use std::ptr;
use windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::core::Interface;

pub(super) type Handle = *mut c_void;
pub(super) type MakeView = unsafe extern "C" fn(Handle, Handle, u32) -> Handle;
pub(super) const GROUP_EDGE: u32 = 8;
const CACHE_IDLE_MS: u64 = 2000;

unsafe extern "C" {
    pub(super) fn rsf_d3d11_state_save(context: Handle, state: *mut rsf_d3d11_state) -> u32;
    pub(super) fn rsf_d3d11_state_restore(context: Handle, state: *mut rsf_d3d11_state);
    pub(super) fn rsf_call_native_log(log: LogFn, user: Handle, text: *const c_char);
}

pub(super) fn shader_code(
    source: &[u8],
    name: &CStr,
    entry: &CStr,
    target: &CStr,
    flags: u32,
) -> Result<Vec<u8>, String> {
    let compiler = Compiler::system("d3dcompiler_47.dll").ok_or_else(|| {
        format!(
            "shader {}: d3dcompiler_47 is not available",
            name.to_string_lossy()
        )
    })?;
    let entry_point = compiler.entry().ok_or_else(String::new)?;
    let request = Request {
        source,
        name,
        entry,
        target,
        flags,
    };
    let output = unsafe { compile(entry_point, request, &[], ptr::null_mut()) };
    if output.status >= 0
        && let Some(code) = output.code
    {
        return Ok(code);
    }
    let errors = output.errors.as_deref().unwrap_or_default();
    let errors = errors.split(|&byte| byte == 0).next().unwrap_or_default();
    let status = if output.status < 0 {
        output.status as u32
    } else {
        0x8000_4005
    };
    Err(format!(
        "shader {} ({}): compile failed 0x{:08x} {}",
        name.to_string_lossy(),
        entry.to_string_lossy(),
        status,
        String::from_utf8_lossy(errors)
    ))
}

pub(super) fn compute_shader(device: &ID3D11Device, code: &[u8]) -> Option<ID3D11ComputeShader> {
    let mut shader = None;
    unsafe { device.CreateComputeShader(code, None, Some(&mut shader)) }.ok()?;
    shader
}

pub(super) fn vertex_shader(device: &ID3D11Device, code: &[u8]) -> Option<ID3D11VertexShader> {
    let mut shader = None;
    unsafe { device.CreateVertexShader(code, None, Some(&mut shader)) }.ok()?;
    shader
}

pub(super) fn pixel_shader(device: &ID3D11Device, code: &[u8]) -> Option<ID3D11PixelShader> {
    let mut shader = None;
    unsafe { device.CreatePixelShader(code, None, Some(&mut shader)) }.ok()?;
    shader
}

pub(super) fn shader_with_log(request: Request<'_>, log: LogFn, user: Handle) -> Option<Vec<u8>> {
    match shader_code(
        request.source,
        request.name,
        request.entry,
        request.target,
        request.flags,
    ) {
        Ok(code) => Some(code),
        Err(text) => {
            if !text.is_empty()
                && let Ok(text) = CString::new(text)
            {
                unsafe { rsf_call_native_log(log, user, text.as_ptr()) };
            }
            None
        }
    }
}

pub(super) fn rasterizer(device: &ID3D11Device) -> Option<ID3D11RasterizerState> {
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        DepthClipEnable: true.into(),
        ..Default::default()
    };
    let mut state = None;
    unsafe { device.CreateRasterizerState(&desc, Some(&mut state)) }.ok()?;
    state
}

pub(super) fn depth_state(
    device: &ID3D11Device,
    desc: &D3D11_DEPTH_STENCIL_DESC,
) -> Option<ID3D11DepthStencilState> {
    let mut state = None;
    unsafe { device.CreateDepthStencilState(desc, Some(&mut state)) }.ok()?;
    state
}

pub(super) fn constant_buffer(
    device: &ID3D11Device,
    bytes: u32,
    dynamic: bool,
) -> Option<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes,
        Usage: if dynamic {
            D3D11_USAGE_DYNAMIC
        } else {
            D3D11_USAGE_DEFAULT
        },
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: if dynamic {
            D3D11_CPU_ACCESS_WRITE.0 as u32
        } else {
            0
        },
        ..Default::default()
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }.ok()?;
    buffer
}

pub(super) fn compute_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
) -> Option<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS).0 as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.ok()?;
    texture
}

pub(super) fn target_view(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
    format: u32,
) -> Option<ID3D11UnorderedAccessView> {
    let desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
        Format: DXGI_FORMAT(format as i32),
        ViewDimension: D3D11_UAV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let mut view = None;
    unsafe {
        device.CreateUnorderedAccessView(
            texture,
            (format != 0).then_some(&desc as *const _),
            Some(&mut view),
        )
    }
    .ok()?;
    view
}

pub(super) unsafe extern "C" fn source_view(device: Handle, source: Handle, format: u32) -> Handle {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.expect("live device");
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&source) }.expect("live texture");
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT(format as i32),
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: 1,
            },
        },
    };
    let mut view = None;
    if unsafe {
        device.CreateShaderResourceView(
            texture,
            (format != 0).then_some(&desc as *const _),
            Some(&mut view),
        )
    }
    .is_err()
    {
        return ptr::null_mut();
    }
    view.map_or(ptr::null_mut(), Interface::into_raw)
}

pub(super) unsafe fn context_matches(device: Handle, context: Handle) -> i32 {
    i32::from(unsafe {
        rsf_d3d11_output_is_immediate(context) != 0 && same_device(device, context)
    })
}

pub(super) unsafe fn describe_source(device: Handle, source: Handle, out: &mut SourceInfo) -> i32 {
    if !unsafe { same_device(device, source) } {
        return 0;
    }
    let desc = unsafe { texture_desc(source) };
    *out = SourceInfo {
        width: desc.width,
        height: desc.height,
        slices: desc.layers,
        samples: desc.samples,
        format: desc.format,
        view_format: srv_format(DXGI_FORMAT(desc.format as i32)).0 as u32,
    };
    1
}

pub(super) fn prepare_compute(context: &ID3D11DeviceContext) {
    unsafe {
        context.SetPredication(None, false);
        context.OMSetRenderTargets(None, None);
    }
}

pub(super) fn bind_compute_parameters(
    context: &ID3D11DeviceContext,
    constants: Handle,
    sampler: Handle,
) {
    unsafe {
        let table = context.vtable();
        (table.CSSetConstantBuffers)(context.as_raw(), 0, 1, &constants);
        (table.CSSetSamplers)(context.as_raw(), 0, 1, &sampler);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_compute<const N: usize>(
    context: &ID3D11DeviceContext,
    shader: &ID3D11ComputeShader,
    sources: &[Handle],
    targets: &[Handle; N],
    counts: Option<&[u32; N]>,
    [width, height]: [u32; 2],
    clear_sources: bool,
    constants: Option<Handle>,
) {
    let table = context.vtable();
    let raw = context.as_raw();
    let counts = counts.map_or(ptr::null(), |counts| counts.as_ptr());
    unsafe {
        context.CSSetShader(shader, None);
        (table.CSSetShaderResources)(raw, 0, sources.len() as u32, sources.as_ptr());
        (table.CSSetUnorderedAccessViews)(raw, 0, N as u32, targets.as_ptr(), counts);
        if let Some(constants) = constants {
            (table.CSSetConstantBuffers)(raw, 0, 1, &constants);
        }
        context.Dispatch(width, height, 1);
        (table.CSSetUnorderedAccessViews)(raw, 0, N as u32, [ptr::null_mut(); N].as_ptr(), counts);
        if clear_sources {
            (table.CSSetShaderResources)(
                raw,
                0,
                sources.len() as u32,
                [ptr::null_mut(); 4].as_ptr(),
            );
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub(super) struct SourceInfo {
    pub width: u32,
    pub height: u32,
    pub slices: u32,
    pub samples: u32,
    pub format: u32,
    pub view_format: u32,
}

const _: () = assert!(size_of::<SourceInfo>() == 24);

impl SourceInfo {
    pub(super) fn single_surface(&self) -> bool {
        self.slices == 1 && self.samples == 1
    }
}

struct Entry {
    view: ComRef,
    texture: ComRef,
    format: u32,
    used: u64,
}

pub(super) struct SourceViews<const N: usize> {
    entries: [Option<Entry>; N],
}

impl<const N: usize> SourceViews<N> {
    pub(super) const fn new() -> Self {
        Self {
            entries: [const { None }; N],
        }
    }

    // The device and texture must be live native interfaces for the call.
    pub(super) unsafe fn get(
        &mut self,
        device: Handle,
        texture: Handle,
        format: u32,
        now: u64,
        make: MakeView,
    ) -> Option<&ComRef> {
        for entry in &mut self.entries {
            if entry
                .as_ref()
                .is_some_and(|entry| now.wrapping_sub(entry.used) > CACHE_IDLE_MS)
            {
                *entry = None;
            }
        }
        if let Some(index) = self.entries.iter().position(|entry| {
            entry
                .as_ref()
                .is_some_and(|entry| entry.texture.as_ptr() == texture && entry.format == format)
        }) {
            let entry = self.entries[index].as_mut()?;
            entry.used = now;
            return Some(&entry.view);
        }
        let index = self.entries.iter().position(Option::is_none).or_else(|| {
            self.entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| Some((index, entry.as_ref()?.used)))
                .min_by_key(|&(_, used)| used)
                .map(|(index, _)| index)
        })?;
        let view = unsafe { ComRef::from_owned(make(device, texture, format)) }?;
        self.entries[index] = Some(Entry {
            view,
            texture: unsafe { ComRef::retain(texture) },
            format,
            used: now,
        });
        self.entries[index].as_ref().map(|entry| &entry.view)
    }
}

// The ABI starts with struct_size. The caller guarantees that the indicated
// prefix is readable and that zeroed extension fields are valid for T.
pub(super) unsafe fn copy_params<T>(input: *const T, prefix: usize) -> Option<T> {
    if input.is_null() || prefix > size_of::<T>() {
        return None;
    }
    let size = unsafe { input.cast::<u32>().read() } as usize;
    if size < prefix {
        return None;
    }
    let bytes = if size >= size_of::<T>() {
        size_of::<T>()
    } else {
        prefix
    };
    let mut result = MaybeUninit::<T>::zeroed();
    unsafe {
        ptr::copy_nonoverlapping(input.cast::<u8>(), result.as_mut_ptr().cast::<u8>(), bytes);
        Some(result.assume_init())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{fake, raw};

    unsafe extern "C" fn make_view(_: Handle, texture: Handle, _: u32) -> Handle {
        unsafe { ComRef::retain(texture) }.into_raw()
    }

    #[test]
    fn a_selected_view_outlives_its_cache_slot() {
        let (device, first, second) = (fake(), fake(), fake());
        let mut cache = SourceViews::<1>::new();
        let held = unsafe { cache.get(raw(&device), raw(&first), 0, 1, make_view) }
            .cloned()
            .unwrap();
        assert!(unsafe { cache.get(raw(&device), raw(&second), 0, 1, make_view) }.is_some());
        assert_eq!(held.as_ptr(), raw(&first));
        assert_eq!(first.refs.get(), 2);
        drop(held);
        assert_eq!(first.refs.get(), 1);
    }
}
