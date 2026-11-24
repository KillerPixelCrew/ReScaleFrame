// SPDX-License-Identifier: MIT
//! Variant admission: native resources, buffers and stage interfaces stay as compiled, and only
//! the pack's declared bindings and interpolants may be added.

use super::leaf::{
    BindingRecord, ParameterRecord, RSF_PATCH_SIGNATURE_INPUT, RSF_PATCH_SIGNATURE_OUTPUT,
    ReflectionSink,
};
use super::pack::{Json, text, uint};
use super::{Outcome, REFLECTION, check, raw, refuse};
use std::ffi::{CStr, c_char, c_void};

#[derive(Clone, Default)]
pub(crate) struct Binding {
    pub(crate) name: Vec<u8>,
    pub(crate) kind: u32,
    pub(crate) point: u32,
    pub(crate) count: u32,
    pub(crate) return_type: u32,
    pub(crate) dimension: u32,
}

#[derive(Clone, Default)]
pub(crate) struct Buffer {
    pub(crate) name: Vec<u8>,
    pub(crate) size: u32,
    // Name, start offset and size.
    pub(crate) variables: Vec<(Vec<u8>, u32, u32)>,
}

#[derive(Clone, Default)]
pub(crate) struct Parameter {
    pub(crate) semantic: Vec<u8>,
    pub(crate) index: u32,
    pub(crate) register: u32,
    pub(crate) system_value: u32,
    pub(crate) component_type: u32,
    pub(crate) mask: u32,
}

#[derive(Clone, Default)]
pub(crate) struct Shader {
    pub(crate) version: u32,
    pub(crate) bindings: Vec<Binding>,
    pub(crate) buffers: Vec<Buffer>,
    pub(crate) inputs: Vec<Parameter>,
    pub(crate) outputs: Vec<Parameter>,
    pub(crate) patch_constants: Vec<Parameter>,
}

fn name(text: *const c_char) -> Vec<u8> {
    if text.is_null() {
        return Vec::new();
    }
    unsafe { CStr::from_ptr(text) }.to_bytes().to_vec()
}

fn shader<'a>(user: *mut c_void) -> &'a mut Shader {
    unsafe { &mut *user.cast::<Shader>() }
}

unsafe fn on_shader(user: *mut c_void, version: u32) {
    shader(user).version = version;
}

unsafe fn on_binding(user: *mut c_void, b: *const BindingRecord) {
    let b = unsafe { &*b };
    shader(user).bindings.push(Binding {
        name: name(b.name),
        kind: b.type_,
        point: b.point,
        count: b.count,
        return_type: b.return_type,
        dimension: b.dimension,
    });
}

unsafe fn on_buffer(user: *mut c_void, text: *const c_char, size: u32) {
    shader(user).buffers.push(Buffer {
        name: name(text),
        size,
        variables: Vec::new(),
    });
}

unsafe fn on_variable(user: *mut c_void, text: *const c_char, offset: u32, size: u32) {
    if let Some(buffer) = shader(user).buffers.last_mut() {
        buffer.variables.push((name(text), offset, size));
    }
}

unsafe fn on_parameter(user: *mut c_void, signature: u32, p: *const ParameterRecord) {
    let p = unsafe { &*p };
    let parameter = Parameter {
        semantic: name(p.semantic),
        index: p.index,
        register: p.reg,
        system_value: p.system_value,
        component_type: p.component_type,
        mask: p.mask,
    };
    let s = shader(user);
    match signature {
        x if x == RSF_PATCH_SIGNATURE_INPUT as u32 => s.inputs.push(parameter),
        x if x == RSF_PATCH_SIGNATURE_OUTPUT as u32 => s.outputs.push(parameter),
        _ => s.patch_constants.push(parameter),
    }
}

pub(crate) fn reflect(code: &[u8]) -> Option<Shader> {
    let mut shader = Shader::default();
    let sink = ReflectionSink {
        user: (&raw mut shader).cast(),
        shader: Some(on_shader),
        binding: Some(on_binding),
        buffer: Some(on_buffer),
        variable: Some(on_variable),
        parameter: Some(on_parameter),
    };
    let reflect = super::native::reflect;
    (unsafe { reflect(raw(code), &sink) } == 0).then_some(shader)
}

/// Checks a compiled variant against the stock shader it replaces.
pub(crate) fn admit(rules: &Json, stock: &[u8], variant: &[u8]) -> Outcome<()> {
    let (Some(a), Some(b)) = (reflect(stock), reflect(variant)) else {
        return Err(refuse(REFLECTION, "D3DReflect"));
    };
    compare(rules, &a, &b)
}

pub(crate) fn compare(rules: &Json, a: &Shader, b: &Shader) -> Outcome<()> {
    check(a.version == b.version, REFLECTION, "shader model")?;
    let binding = |s: &'_ Shader, n: &[u8]| s.bindings.iter().find(|x| x.name == n).cloned();
    for x in &a.bindings {
        let same = binding(b, &x.name).is_some_and(|y| {
            (x.kind, x.point, x.count, x.return_type, x.dimension)
                == (y.kind, y.point, y.count, y.return_type, y.dimension)
        });
        check(same, REFLECTION, "native resource changed")?;
    }
    for x in &b.bindings {
        if binding(a, &x.name).is_some() {
            continue;
        }
        let mut allowed = false;
        // Every declaration's name is read, as a malformed list is a pack error.
        for v in Json::items(rules.get(b"allowed_bindings")) {
            let named = text(Some(v), b"name")? == x.name;
            allowed |= named
                && uint(v.get(b"point")) == u64::from(x.point)
                && uint(v.get(b"type")) == u64::from(x.kind)
                && uint(v.get(b"count")) == u64::from(x.count);
        }
        check(allowed, REFLECTION, "undeclared resource addition")?;
    }
    for x in &a.buffers {
        let y = b.buffers.iter().find(|y| y.name == x.name);
        let y = y.filter(|y| y.size == x.size);
        let y = y.ok_or_else(|| refuse(REFLECTION, "native buffer layout"))?;
        for (n, offset, size) in &x.variables {
            let same = y.variables.iter().find(|(m, ..)| m == n);
            let same = same.is_some_and(|(_, o, s)| o == offset && s == size);
            check(same, REFLECTION, "native variable layout")?;
        }
    }
    let added = rules.get(b"added_semantics");
    let interface =
        signature(&a.inputs, &b.inputs, added) && signature(&a.outputs, &b.outputs, added);
    check(interface, REFLECTION, "stage interface")?;
    let patches = a.patch_constants.len() == b.patch_constants.len();
    check(patches, REFLECTION, "patch constants")?;
    for (x, y) in a.patch_constants.iter().zip(&b.patch_constants) {
        let same = (&x.semantic, x.index, x.register, x.mask, x.component_type)
            == (&y.semantic, y.index, y.register, y.mask, y.component_type);
        check(same, REFLECTION, "patch signature")?;
    }
    Ok(())
}

fn signature(first: &[Parameter], second: &[Parameter], additions: Option<&Json>) -> bool {
    let equal = |x: &Parameter, y: &Parameter| {
        x.semantic.eq_ignore_ascii_case(&y.semantic)
            && (x.index, x.system_value, x.component_type, x.mask)
                == (y.index, y.system_value, y.component_type, y.mask)
    };
    if !first.iter().all(|p| second.iter().any(|q| equal(p, q))) {
        return false;
    }
    second
        .iter()
        .filter(|p| !first.iter().any(|q| equal(p, q)))
        .all(|p| {
            let semantic = p.semantic.to_ascii_uppercase();
            let indices = Json::items(additions.and_then(|a| a.get(&semantic)));
            indices.iter().any(|v| uint(Some(v)) == u64::from(p.index))
        })
}
