// SPDX-License-Identifier: MIT
//! Windows implementations of the shader patch service's injectable leaves.

use super::leaf::*;
use crate::sys::{rsf_patch_bytes, rsf_patch_json_visit};
use rsf_shader_compiler::{Request, blob_bytes, compile as compile_shader};
use std::ffi::{CStr, c_void};
use std::ptr;
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3D_DISASM_ENABLE_INSTRUCTION_NUMBERING, D3DDisassemble, D3DReflect,
};
use windows::Win32::Graphics::Direct3D::{D3D_INCLUDE_TYPE, D3D_SHADER_MACRO, ID3DInclude_Vtbl};
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::{HRESULT, Interface, PCSTR};

const E_FAIL: HRESULT = HRESULT(0x8000_4005_u32 as i32);
const E_INVALIDARG: HRESULT = HRESULT(0x8007_0057_u32 as i32);

unsafe extern "C" {
    pub(super) fn rsf_patch_parse_json(
        text: rsf_patch_bytes,
        user: *mut c_void,
        visit: rsf_patch_json_visit,
    ) -> i32;
}

use rsf_minhook_sys::{MH_CreateHook, MH_DisableHook, MH_EnableHook, MH_Initialize, MH_RemoveHook};

// MinHook.h v1.3.4 enum values.
const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;
const MH_ERROR_ENABLED: i32 = 5;

#[repr(C)]
struct Includes<'a> {
    vtable: &'static ID3DInclude_Vtbl,
    call: &'a CompileCall,
}

static INCLUDE_VTABLE: ID3DInclude_Vtbl = ID3DInclude_Vtbl {
    Open: include_open,
    Close: include_close,
};

unsafe extern "system" fn include_open(
    this: *mut c_void,
    _: D3D_INCLUDE_TYPE,
    name: PCSTR,
    _: *const c_void,
    data: *mut *mut c_void,
    size: *mut u32,
) -> HRESULT {
    if name.is_null() || data.is_null() || size.is_null() {
        return E_INVALIDARG;
    }
    let call = unsafe { (*this.cast::<Includes<'_>>()).call };
    let (mut bytes, mut length) = (ptr::null(), 0);
    let status =
        unsafe { (call.include.unwrap())(call.user, name.0.cast(), &mut bytes, &mut length) };
    let Ok(length) = u32::try_from(length) else {
        return E_FAIL;
    };
    if status != 0 {
        return E_FAIL;
    }
    unsafe {
        *data = bytes.cast_mut();
        *size = length;
    }
    HRESULT(0)
}

unsafe extern "system" fn include_close(_: *mut c_void, _: *const c_void) -> HRESULT {
    HRESULT(0)
}

unsafe fn compile(call: *const CompileCall) -> i32 {
    let call = unsafe { &*call };
    let Ok(source) = super::bytes(call.source) else {
        return E_INVALIDARG.0;
    };
    let defines = if call.define_count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(call.defines, call.define_count) }
    };
    let mut macros: Vec<_> = defines
        .iter()
        .map(|item| D3D_SHADER_MACRO {
            Name: PCSTR(item.name.cast()),
            Definition: PCSTR(item.value.cast()),
        })
        .collect();
    macros.push(D3D_SHADER_MACRO::default());
    let mut includes = Includes {
        vtable: &INCLUDE_VTABLE,
        call,
    };
    let output = unsafe {
        compile_shader(
            call.compiler,
            Request {
                source,
                name: c"",
                entry: CStr::from_ptr(call.entry),
                target: CStr::from_ptr(call.target),
                flags: call.flags,
            },
            &macros,
            (&raw mut includes).cast(),
        )
    };
    if let Some(code) = output.code {
        unsafe { (call.code.unwrap())(call.user, code.as_ptr().cast(), code.len()) };
    }
    if let Some(errors) = output.errors {
        unsafe { (call.errors.unwrap())(call.user, errors.as_ptr().cast(), errors.len()) };
    }
    output.status
}

unsafe fn reflect(code: rsf_patch_bytes, sink: *const ReflectionSink) -> i32 {
    let sink = unsafe { &*sink };
    let result = (|| unsafe {
        let mut raw = ptr::null_mut();
        D3DReflect(code.data, code.size, &ID3D11ShaderReflection::IID, &mut raw).ok()?;
        if raw.is_null() {
            return None;
        }
        let shader = ID3D11ShaderReflection::from_raw(raw);
        let mut desc = D3D11_SHADER_DESC::default();
        shader.GetDesc(&mut desc).ok()?;
        (sink.shader.unwrap())(sink.user, desc.Version);
        for i in 0..desc.BoundResources {
            let mut b = D3D11_SHADER_INPUT_BIND_DESC::default();
            shader.GetResourceBindingDesc(i, &mut b).ok()?;
            (sink.binding.unwrap())(
                sink.user,
                &BindingRecord {
                    name: b.Name.0.cast(),
                    type_: b.Type.0 as u32,
                    point: b.BindPoint,
                    count: b.BindCount,
                    return_type: b.ReturnType.0 as u32,
                    dimension: b.Dimension.0 as u32,
                },
            );
        }
        for i in 0..desc.ConstantBuffers {
            let buffer = shader.GetConstantBufferByIndex(i)?;
            let mut b = D3D11_SHADER_BUFFER_DESC::default();
            buffer.GetDesc(&mut b).ok()?;
            (sink.buffer.unwrap())(sink.user, b.Name.0.cast(), b.Size);
            for j in 0..b.Variables {
                let mut v = D3D11_SHADER_VARIABLE_DESC::default();
                buffer.GetVariableByIndex(j)?.GetDesc(&mut v).ok()?;
                (sink.variable.unwrap())(sink.user, v.Name.0.cast(), v.StartOffset, v.Size);
            }
        }
        type Read = unsafe fn(
            &ID3D11ShaderReflection,
            u32,
            *mut D3D11_SIGNATURE_PARAMETER_DESC,
        ) -> windows::core::Result<()>;
        let signatures: [(i32, u32, Read); 3] = [
            (
                RSF_PATCH_SIGNATURE_INPUT,
                desc.InputParameters,
                ID3D11ShaderReflection::GetInputParameterDesc,
            ),
            (
                RSF_PATCH_SIGNATURE_OUTPUT,
                desc.OutputParameters,
                ID3D11ShaderReflection::GetOutputParameterDesc,
            ),
            (
                RSF_PATCH_SIGNATURE_PATCH,
                desc.PatchConstantParameters,
                ID3D11ShaderReflection::GetPatchConstantParameterDesc,
            ),
        ];
        for (kind, count, read) in signatures {
            for i in 0..count {
                let mut p = D3D11_SIGNATURE_PARAMETER_DESC::default();
                read(&shader, i, &mut p).ok()?;
                (sink.parameter.unwrap())(
                    sink.user,
                    kind as u32,
                    &ParameterRecord {
                        semantic: p.SemanticName.0.cast(),
                        index: p.SemanticIndex,
                        reg: p.Register,
                        system_value: p.SystemValueType.0 as u32,
                        component_type: p.ComponentType.0 as u32,
                        mask: u32::from(p.Mask),
                    },
                );
            }
        }
        Some(())
    })();
    if result.is_some() { 0 } else { -1 }
}

unsafe fn disassemble(code: rsf_patch_bytes, user: *mut c_void, text: Sink) -> i32 {
    let result = unsafe {
        D3DDisassemble(
            code.data,
            code.size,
            D3D_DISASM_ENABLE_INSTRUCTION_NUMBERING,
            PCSTR::null(),
        )
    };
    let Ok(assembly) = result else { return -1 };
    let bytes = blob_bytes(&assembly);
    unsafe { (text.unwrap())(user, bytes.as_ptr().cast(), bytes.len()) };
    0
}

unsafe fn create_shader(
    device: *mut c_void,
    stage: u32,
    code: rsf_patch_bytes,
    shader: *mut *mut c_void,
) -> i32 {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.unwrap();
    let Ok(code) = super::bytes(code) else {
        return E_INVALIDARG.0;
    };
    let linkage = None::<&ID3D11ClassLinkage>;
    let result = unsafe {
        match stage as u8 {
            b'v' => device.CreateVertexShader(code, linkage, Some(shader.cast())),
            b'p' => device.CreatePixelShader(code, linkage, Some(shader.cast())),
            b'g' => device.CreateGeometryShader(code, linkage, Some(shader.cast())),
            b'h' => device.CreateHullShader(code, linkage, Some(shader.cast())),
            b'd' => device.CreateDomainShader(code, linkage, Some(shader.cast())),
            b'c' => device.CreateComputeShader(code, linkage, Some(shader.cast())),
            _ => return E_INVALIDARG.0,
        }
    };
    result.map_or_else(|error| error.code().0, |()| 0)
}

unsafe fn create_constant_buffer(device: *mut c_void, size: u32, buffer: *mut *mut c_void) -> i32 {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.unwrap();
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        ..Default::default()
    };
    unsafe { device.CreateBuffer(&desc, None, Some(buffer.cast())) }
        .map_or_else(|error| error.code().0, |()| 0)
}

unsafe fn buffer_desc(buffer: *mut c_void, width: *mut u32, usage: *mut u32, access: *mut u32) {
    let buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&buffer) }.unwrap();
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe {
        buffer.GetDesc(&mut desc);
        *width = desc.ByteWidth;
        *usage = desc.Usage.0 as u32;
        *access = desc.CPUAccessFlags;
    }
}

unsafe fn upload(context: *mut c_void, buffer: *mut c_void, data: rsf_patch_bytes) -> i32 {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.unwrap();
    let buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&buffer) }.unwrap();
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if unsafe { context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)) }.is_err() {
        return -1;
    }
    if !mapped.pData.is_null() {
        unsafe { ptr::copy_nonoverlapping(data.data.cast::<u8>(), mapped.pData.cast(), data.size) };
    }
    unsafe { context.Unmap(buffer, 0) };
    if mapped.pData.is_null() { -1 } else { 0 }
}

unsafe fn bind_constant(context: *mut c_void, stage: u32, slot: u32, buffer: *mut c_void) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.unwrap();
    // Option<ID3D11Buffer> is pointer-sized with null for None. Borrow the raw
    // pointer slot so the incoming COM reference is neither acquired nor released.
    let buffers = Some(unsafe { std::slice::from_raw_parts((&raw const buffer).cast(), 1) });
    unsafe {
        match stage as u8 {
            b'v' => context.VSSetConstantBuffers(slot, buffers),
            b'p' => context.PSSetConstantBuffers(slot, buffers),
            b'g' => context.GSSetConstantBuffers(slot, buffers),
            b'h' => context.HSSetConstantBuffers(slot, buffers),
            b'd' => context.DSSetConstantBuffers(slot, buffers),
            b'c' => context.CSSetConstantBuffers(slot, buffers),
            _ => {}
        }
    }
}

unsafe fn hooks_ready() -> i32 {
    i32::from(matches!(
        unsafe { MH_Initialize() },
        MH_OK | MH_ERROR_ALREADY_INITIALIZED
    ))
}
unsafe fn hook_create(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32 {
    i32::from(unsafe { MH_CreateHook(target, detour, original) } == MH_OK)
}
unsafe fn hook_enable(target: *mut c_void) -> i32 {
    i32::from(matches!(
        unsafe { MH_EnableHook(target) },
        MH_OK | MH_ERROR_ENABLED
    ))
}
unsafe fn hook_disable(target: *mut c_void) {
    unsafe { MH_DisableHook(target) };
}
unsafe fn hook_remove(target: *mut c_void) {
    unsafe { MH_RemoveHook(target) };
}

pub(super) const DYNAMIC_USAGE: u32 = D3D11_USAGE_DYNAMIC.0 as u32;
pub(super) const CPU_WRITE: u32 = D3D11_CPU_ACCESS_WRITE.0 as u32;
pub(super) const CONSTANT_SLOTS: u32 = D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT;

pub(super) static PLATFORM: Platform = Platform {
    parse_json: rsf_patch_parse_json,
    compile,
    reflect,
    disassemble,
    create_shader,
    create_constant_buffer,
    buffer_desc,
    upload,
    bind_constant,
    hooks_ready,
    hook_create,
    hook_enable,
    hook_disable,
    hook_remove,
};
