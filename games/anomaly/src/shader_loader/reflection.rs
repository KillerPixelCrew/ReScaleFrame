// SPDX-License-Identifier: MIT
use super::{Bytes, Span};
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::{HRESULT, Interface, PCSTR};
use windows_sys::Win32::Foundation::FARPROC;

type Reflect = unsafe extern "system" fn(
    *const c_void,
    usize,
    *const windows::core::GUID,
    *mut *mut c_void,
) -> HRESULT;
// All spans point into the owned heap allocations, which remain fixed when the
// snapshot moves. The descriptors and their text are immutable after construction.
pub(crate) struct Snapshot {
    view: Reflection,
    _inputs: Vec<Parameter>,
    _outputs: Vec<Parameter>,
    _patch: Vec<Parameter>,
    _bindings: Vec<Binding>,
    _buffers: Vec<Buffer>,
    _variables: Vec<Vec<Variable>>,
    _names: Vec<Box<[u8]>>,
}
unsafe impl Send for Snapshot {}
unsafe impl Sync for Snapshot {}

impl Snapshot {
    pub(crate) fn view(&self) -> &Reflection {
        &self.view
    }
}

#[cfg(test)]
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            view: Reflection {
                version: 0,
                inputs: Span::from(&[][..]),
                outputs: Span::from(&[][..]),
                patch: Span::from(&[][..]),
                bindings: Span::from(&[][..]),
                buffers: Span::from(&[][..]),
            },
            _inputs: Vec::new(),
            _outputs: Vec::new(),
            _patch: Vec::new(),
            _bindings: Vec::new(),
            _buffers: Vec::new(),
            _variables: Vec::new(),
            _names: Vec::new(),
        }
    }
}

fn name(value: PCSTR, storage: &mut Vec<Box<[u8]>>) -> Bytes {
    if value.is_null() {
        return Bytes::from(&[][..]);
    }
    let owned: Box<[u8]> = unsafe { CStr::from_ptr(value.0.cast()) }.to_bytes().into();
    let span = Bytes::from(owned.as_ref());
    storage.push(owned);
    span
}

fn signature(
    count: u32,
    names: &mut Vec<Box<[u8]>>,
    mut read: impl FnMut(u32, &mut D3D11_SIGNATURE_PARAMETER_DESC) -> windows::core::Result<()>,
) -> Option<Vec<Parameter>> {
    (0..count)
        .map(|index| {
            let mut d = D3D11_SIGNATURE_PARAMETER_DESC::default();
            read(index, &mut d).ok()?;
            Some(Parameter {
                name: name(d.SemanticName, names),
                index: d.SemanticIndex,
                register: d.Register,
                system_value: d.SystemValueType.0 as u32,
                component: d.ComponentType.0 as u32,
                mask: u32::from(d.Mask),
            })
        })
        .collect()
}

fn snapshot(shader: &ID3D11ShaderReflection) -> Option<Snapshot> {
    let mut names = Vec::new();
    let mut desc = D3D11_SHADER_DESC::default();
    unsafe { shader.GetDesc(&mut desc) }.ok()?;
    let inputs = signature(desc.InputParameters, &mut names, |index, d| unsafe {
        shader.GetInputParameterDesc(index, d)
    })?;
    let outputs = signature(desc.OutputParameters, &mut names, |index, d| unsafe {
        shader.GetOutputParameterDesc(index, d)
    })?;
    let patch = signature(
        desc.PatchConstantParameters,
        &mut names,
        |index, d| unsafe { shader.GetPatchConstantParameterDesc(index, d) },
    )?;

    let mut bindings = Vec::new();
    for index in 0..desc.BoundResources {
        let mut d = D3D11_SHADER_INPUT_BIND_DESC::default();
        unsafe { shader.GetResourceBindingDesc(index, &mut d) }.ok()?;
        bindings.push(Binding {
            name: name(d.Name, &mut names),
            kind: d.Type.0 as u32,
            point: d.BindPoint,
            count: d.BindCount,
            return_type: d.ReturnType.0 as u32,
            dimension: d.Dimension.0 as u32,
        });
    }

    let mut buffers = Vec::new();
    let mut variables = Vec::new();
    for index in 0..desc.ConstantBuffers {
        let buffer = unsafe { shader.GetConstantBufferByIndex(index) }?;
        let mut d = D3D11_SHADER_BUFFER_DESC::default();
        unsafe { buffer.GetDesc(&mut d) }.ok()?;
        let mut rows = Vec::new();
        for index in 0..d.Variables {
            let variable = unsafe { buffer.GetVariableByIndex(index) }?;
            let mut v = D3D11_SHADER_VARIABLE_DESC::default();
            unsafe { variable.GetDesc(&mut v) }.ok()?;
            rows.push(Variable {
                name: name(v.Name, &mut names),
                offset: v.StartOffset,
                size: v.Size,
            });
        }
        buffers.push(Buffer {
            name: name(d.Name, &mut names),
            size: d.Size,
            variables: Span::from(rows.as_slice()),
        });
        variables.push(rows);
    }
    Some(Snapshot {
        view: Reflection {
            version: desc.Version,
            inputs: Span::from(inputs.as_slice()),
            outputs: Span::from(outputs.as_slice()),
            patch: Span::from(patch.as_slice()),
            bindings: Span::from(bindings.as_slice()),
            buffers: Span::from(buffers.as_slice()),
        },
        _inputs: inputs,
        _outputs: outputs,
        _patch: patch,
        _bindings: bindings,
        _buffers: buffers,
        _variables: variables,
        _names: names,
    })
}

pub(super) fn reflect(entry: FARPROC, code: &[u8]) -> Option<Snapshot> {
    let entry = entry.filter(|_| !code.is_empty())?;
    let reflect: Reflect = unsafe { std::mem::transmute(entry) };
    let mut raw = ptr::null_mut();
    let status = unsafe {
        reflect(
            code.as_ptr().cast(),
            code.len(),
            &ID3D11ShaderReflection::IID,
            &mut raw,
        )
    };
    if status.is_err() || raw.is_null() {
        return None;
    }
    let shader = unsafe { ID3D11ShaderReflection::from_raw(raw) };
    snapshot(&shader)
}

#[repr(C)]
struct Parameter {
    name: Bytes,
    index: u32,
    register: u32,
    system_value: u32,
    component: u32,
    mask: u32,
}

impl Parameter {
    fn name(&self) -> &[u8] {
        unsafe { self.name.as_slice() }
    }

    fn same_element(&self, other: &Self, same_name: bool) -> bool {
        same_name
            && self.index == other.index
            && self.system_value == other.system_value
            && self.component == other.component
            && self.mask == other.mask
    }
}

impl PartialEq for Parameter {
    fn eq(&self, other: &Self) -> bool {
        self.same_element(other, self.name() == other.name()) && self.register == other.register
    }
}

#[repr(C)]
struct Variable {
    name: Bytes,
    offset: u32,
    size: u32,
}

#[repr(C)]
struct Buffer {
    name: Bytes,
    size: u32,
    variables: Span<Variable>,
}

#[repr(C)]
struct Binding {
    name: Bytes,
    kind: u32,
    point: u32,
    count: u32,
    return_type: u32,
    dimension: u32,
}

// The native reflection and its borrowed descriptors remain live for the entire check.
#[repr(C)]
pub(crate) struct Reflection {
    version: u32,
    inputs: Span<Parameter>,
    outputs: Span<Parameter>,
    patch: Span<Parameter>,
    bindings: Span<Binding>,
    buffers: Span<Buffer>,
}

const _: () = {
    assert!(size_of::<Reflection>() == 88);
    assert!(std::mem::offset_of!(Reflection, inputs) == 8);
    assert!(std::mem::offset_of!(Reflection, buffers) == 72);
    assert!(size_of::<Parameter>() == 40);
    assert!(size_of::<Variable>() == 24);
    assert!(size_of::<Buffer>() == 40);
    assert!(size_of::<Binding>() == 40);
};

// One constant-buffer variable. `buffer` is its buffer's index in reflection order.
pub(crate) struct Constant {
    pub buffer: u32,
    pub buffer_size: u32,
    pub offset: u32,
    pub size: u32,
}

impl Reflection {
    // The first variable named `name`, searching buffers and then their variables in reflection order.
    pub(crate) fn constant(&self, name: &[u8]) -> Option<Constant> {
        let buffers = unsafe { self.buffers.as_slice() };
        buffers.iter().zip(0u32..).find_map(|(buffer, index)| {
            unsafe { buffer.variables.as_slice() }
                .iter()
                .find(|variable| unsafe { variable.name.as_slice() } == name)
                .map(|variable| Constant {
                    buffer: index,
                    buffer_size: buffer.size,
                    offset: variable.offset,
                    size: variable.size,
                })
        })
    }

    fn inputs(&self) -> &[Parameter] {
        unsafe { self.inputs.as_slice() }
    }

    fn outputs(&self) -> &[Parameter] {
        unsafe { self.outputs.as_slice() }
    }

    fn patch(&self) -> &[Parameter] {
        unsafe { self.patch.as_slice() }
    }
}

#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
pub(super) enum Compatibility {
    Ok,
    Version,
    MissingBinding,
    ExtraBinding,
    Buffer,
    Inputs,
    Outputs,
    PatchConstants,
}

const COMPATIBILITY_NAMES: [&CStr; 8] = [
    c"ok",
    c"version",
    c"missing_binding",
    c"extra_binding",
    c"buffer",
    c"inputs",
    c"outputs",
    c"patch_constants",
];

impl Compatibility {
    pub(super) fn name(self) -> &'static [u8] {
        COMPATIBILITY_NAMES[self as usize].to_bytes()
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_compatibility_name(value: u8) -> *const c_char {
    COMPATIBILITY_NAMES
        .get(value as usize)
        .copied()
        .unwrap_or(c"unknown")
        .as_ptr()
}

// Previous clip, validity, previous object-space position and normal.
const TESS_CONTROL: [(u32, u32); 4] = [(12, 0xf), (13, 0x1), (14, 0xf), (15, 0x7)];
const TESS_DOMAIN: [(u32, u32); 2] = [(12, 0xf), (13, 0x1)];

fn extends<const N: usize>(
    stock: &[Parameter],
    variant: &[Parameter],
    fields: &[(u32, u32); N],
) -> bool {
    let same =
        |a: &Parameter, b: &Parameter| a.same_element(b, a.name().eq_ignore_ascii_case(b.name()));
    if !stock.iter().all(|a| variant.iter().any(|b| same(a, b))) {
        return false;
    }
    let mut seen = [false; N];
    for element in variant {
        if stock.iter().any(|p| same(p, element)) {
            continue;
        }
        // D3D_NAME_UNDEFINED = 0, D3D_REGISTER_COMPONENT_FLOAT32 = 3.
        if !element.name().eq_ignore_ascii_case(b"TEXCOORD")
            || element.system_value != 0
            || element.component != 3
        {
            return false;
        }
        let Some(i) = fields
            .iter()
            .position(|&field| field == (element.index, element.mask))
        else {
            return false;
        };
        if seen[i] {
            return false;
        }
        seen[i] = true;
    }
    seen.into_iter().all(|present| present)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_shader_compatible(
    stage: u8,
    stock: &Reflection,
    variant: &Reflection,
) -> Compatibility {
    use Compatibility as Check;
    if stock.version != variant.version {
        return Check::Version;
    }
    let stock_bindings = unsafe { stock.bindings.as_slice() };
    let variant_bindings = unsafe { variant.bindings.as_slice() };
    for binding in stock_bindings {
        let name = unsafe { binding.name.as_slice() };
        let found = variant_bindings
            .iter()
            .find(|other| unsafe { other.name.as_slice() } == name);
        if !found.is_some_and(|other| {
            (
                binding.kind,
                binding.point,
                binding.count,
                binding.return_type,
                binding.dimension,
            ) == (
                other.kind,
                other.point,
                other.count,
                other.return_type,
                other.dimension,
            )
        }) {
            return Check::MissingBinding;
        }
    }
    for binding in variant_bindings {
        let name = unsafe { binding.name.as_slice() };
        if stock_bindings
            .iter()
            .any(|b| unsafe { b.name.as_slice() } == name)
        {
            continue;
        }
        let name_ok = match stage {
            b'v' => matches!(name, b"RSFCorePrevious" | b"RSFFoliagePrevious"),
            b'p' => name == b"RSFCorePixel",
            b'd' => name == b"RSFTessPrevious",
            _ => false,
        };
        // D3D_SIT_CBUFFER = 0. Hull shaders only forward control points.
        if !name_ok || (binding.kind, binding.point, binding.count) != (0, 13, 1) {
            return Check::ExtraBinding;
        }
    }
    let stock_buffers = unsafe { stock.buffers.as_slice() };
    let variant_buffers = unsafe { variant.buffers.as_slice() };
    for buffer in stock_buffers {
        let name = unsafe { buffer.name.as_slice() };
        let Some(found) = variant_buffers
            .iter()
            .find(|other| unsafe { other.name.as_slice() } == name)
            .filter(|other| other.size == buffer.size)
        else {
            return Check::Buffer;
        };
        let stock_variables = unsafe { buffer.variables.as_slice() };
        let variant_variables = unsafe { found.variables.as_slice() };
        for variable in stock_variables {
            let name = unsafe { variable.name.as_slice() };
            let matched = variant_variables
                .iter()
                .find(|other| unsafe { other.name.as_slice() } == name)
                .is_some_and(|other| {
                    (variable.offset, variable.size) == (other.offset, other.size)
                });
            if !matched {
                return Check::Buffer;
            }
        }
    }
    let old = stock.outputs();
    let new = variant.outputs();
    match stage {
        b'h' | b'd' => {
            if stock.patch() != variant.patch() {
                return Check::PatchConstants;
            }
            if !extends(stock.inputs(), variant.inputs(), &TESS_CONTROL) {
                return Check::Inputs;
            }
            let valid = if stage == b'h' {
                extends(old, new, &TESS_CONTROL)
            } else {
                extends(old, new, &TESS_DOMAIN)
            };
            if !valid {
                return Check::Outputs;
            }
        }
        b'v' => {
            if stock.inputs() != variant.inputs() {
                return Check::Inputs;
            }
            if !old
                .iter()
                .all(|a| new.iter().any(|b| a.same_element(b, a.name() == b.name())))
            {
                return Check::Outputs;
            }
        }
        b'p' => {
            let mut extra = new.iter().filter(|output| !old.contains(output));
            if !old.iter().all(|output| new.contains(output))
                || !extra.next().is_some_and(|output| {
                    (output.index, output.register, output.mask) == (2, 2, 0xf)
                        && matches!(output.name(), b"SV_Target" | b"SV_TARGET")
                })
                || extra.next().is_some()
            {
                return Check::Outputs;
            }
        }
        _ => return Check::Version,
    }
    Check::Ok
}

fn covers(outputs: &[Parameter], inputs: &[Parameter]) -> bool {
    inputs.iter().all(|input| {
        outputs.iter().any(|output| {
            output.name().eq_ignore_ascii_case(input.name())
                && output.index == input.index
                && output.register == input.register
                && output.mask & input.mask == input.mask
        })
    })
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_shader_linkable(
    vertex: &Reflection,
    pixel: &Reflection,
) -> bool {
    covers(vertex.outputs(), pixel.inputs())
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_shader_tess_linkable(
    vertex: &Reflection,
    hull: &Reflection,
    domain: &Reflection,
    pixel: &Reflection,
) -> bool {
    covers(vertex.outputs(), hull.inputs())
        && covers(hull.outputs(), domain.inputs())
        && covers(domain.outputs(), pixel.inputs())
        && !domain.patch().is_empty()
        && covers(hull.patch(), domain.patch())
}

#[cfg(test)]
#[test]
fn unreadable_signature_row_refuses_the_entire_signature() {
    let mut visited = Vec::new();
    let rows = signature(3, &mut Vec::new(), |index, _| {
        visited.push(index);
        if index == 1 {
            HRESULT(0x8000_4005_u32 as i32).ok()
        } else {
            Ok(())
        }
    });
    assert!(rows.is_none());
    assert_eq!(visited, [0, 1]);
}
