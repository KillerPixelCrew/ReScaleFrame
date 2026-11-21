// SPDX-License-Identifier: MIT
use super::{Bytes, Span};
use std::ffi::{CStr, c_char, c_void};

#[derive(Clone, Copy)]
#[repr(u8)]
enum Signature {
    Input = 0,
    Output = 1,
    Patch = 2,
}

#[derive(PartialEq, Eq)]
#[repr(transparent)]
struct ReflectionStep(u8);

impl ReflectionStep {
    const COPIED: Self = Self(0);
    const UNAVAILABLE: Self = Self(1);
}

#[repr(C)]
struct ReflectionCounts {
    inputs: u32,
    outputs: u32,
    patch: u32,
    bindings: u32,
    buffers: u32,
}

#[repr(C)]
struct ReflectionOperations {
    parameter: unsafe extern "C" fn(*mut c_void, Signature, u32) -> ReflectionStep,
    binding: unsafe extern "C" fn(*mut c_void, u32) -> ReflectionStep,
    begin_buffer: unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> ReflectionStep,
    variable: unsafe extern "C" fn(*mut c_void, u32) -> ReflectionStep,
    finish_buffer: unsafe extern "C" fn(*mut c_void) -> ReflectionStep,
}

const _: () = {
    assert!(size_of::<ReflectionCounts>() == 20);
    assert!(size_of::<ReflectionOperations>() == 40);
};

// The native reflector, destination and current buffer are live for this synchronous traversal.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_enumerate_reflection(
    user: *mut c_void,
    counts: &ReflectionCounts,
    ops: &ReflectionOperations,
) -> bool {
    for (signature, count) in [
        (Signature::Input, counts.inputs),
        (Signature::Output, counts.outputs),
        (Signature::Patch, counts.patch),
    ] {
        for index in 0..count {
            let step = unsafe { (ops.parameter)(user, signature, index) };
            // An unreadable signature row is omitted. A native copy failure ends enumeration.
            if !matches!(step, ReflectionStep::COPIED | ReflectionStep::UNAVAILABLE) {
                return false;
            }
        }
    }
    for index in 0..counts.bindings {
        if unsafe { (ops.binding)(user, index) } != ReflectionStep::COPIED {
            return false;
        }
    }
    for index in 0..counts.buffers {
        let mut variables = 0;
        if unsafe { (ops.begin_buffer)(user, index, &mut variables) } != ReflectionStep::COPIED {
            return false;
        }
        for variable in 0..variables {
            if unsafe { (ops.variable)(user, variable) } != ReflectionStep::COPIED {
                return false;
            }
        }
        // Publish only complete buffers, retaining any rows already published if this one fails.
        if unsafe { (ops.finish_buffer)(user) } != ReflectionStep::COPIED {
            return false;
        }
    }
    true
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
extern "C" fn rsf_anomaly_shader_linkable(vertex: &Reflection, pixel: &Reflection) -> bool {
    covers(vertex.outputs(), pixel.inputs())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_tess_linkable(
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
