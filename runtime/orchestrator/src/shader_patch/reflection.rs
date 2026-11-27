// SPDX-License-Identifier: MIT
//! Variant admission: native resources, buffers and stage interfaces stay as compiled, and only
//! the pack's declared bindings and interpolants may be added.

use super::pack::{Json, text, uint};
use super::{Outcome, REFLECTION, check, refuse};
use std::ptr;
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::{Interface, PCSTR};

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

fn name(text: PCSTR) -> Vec<u8> {
    if text.is_null() {
        return Vec::new();
    }
    unsafe { text.as_bytes() }.to_vec()
}

fn parameter(p: &D3D11_SIGNATURE_PARAMETER_DESC) -> Parameter {
    Parameter {
        semantic: name(p.SemanticName),
        index: p.SemanticIndex,
        register: p.Register,
        system_value: p.SystemValueType.0 as u32,
        component_type: p.ComponentType.0 as u32,
        mask: u32::from(p.Mask),
    }
}

pub(crate) fn reflect(code: &[u8]) -> Option<Shader> {
    unsafe {
        let mut raw = ptr::null_mut();
        D3DReflect(
            code.as_ptr().cast(),
            code.len(),
            &ID3D11ShaderReflection::IID,
            &mut raw,
        )
        .ok()?;
        if raw.is_null() {
            return None;
        }
        let reflection = ID3D11ShaderReflection::from_raw(raw);
        let mut desc = D3D11_SHADER_DESC::default();
        reflection.GetDesc(&mut desc).ok()?;
        let mut shader = Shader {
            version: desc.Version,
            ..Default::default()
        };
        for i in 0..desc.BoundResources {
            let mut b = D3D11_SHADER_INPUT_BIND_DESC::default();
            reflection.GetResourceBindingDesc(i, &mut b).ok()?;
            shader.bindings.push(Binding {
                name: name(b.Name),
                kind: b.Type.0 as u32,
                point: b.BindPoint,
                count: b.BindCount,
                return_type: b.ReturnType.0 as u32,
                dimension: b.Dimension.0 as u32,
            });
        }
        for i in 0..desc.ConstantBuffers {
            let buffer = reflection.GetConstantBufferByIndex(i)?;
            let mut b = D3D11_SHADER_BUFFER_DESC::default();
            buffer.GetDesc(&mut b).ok()?;
            let mut variables = Vec::new();
            for j in 0..b.Variables {
                let mut v = D3D11_SHADER_VARIABLE_DESC::default();
                buffer.GetVariableByIndex(j)?.GetDesc(&mut v).ok()?;
                variables.push((name(v.Name), v.StartOffset, v.Size));
            }
            shader.buffers.push(Buffer {
                name: name(b.Name),
                size: b.Size,
                variables,
            });
        }
        let mut p = D3D11_SIGNATURE_PARAMETER_DESC::default();
        for i in 0..desc.InputParameters {
            reflection.GetInputParameterDesc(i, &mut p).ok()?;
            shader.inputs.push(parameter(&p));
        }
        for i in 0..desc.OutputParameters {
            reflection.GetOutputParameterDesc(i, &mut p).ok()?;
            shader.outputs.push(parameter(&p));
        }
        for i in 0..desc.PatchConstantParameters {
            reflection.GetPatchConstantParameterDesc(i, &mut p).ok()?;
            shader.patch_constants.push(parameter(&p));
        }
        Some(shader)
    }
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
    fn binding<'a>(s: &'a Shader, n: &[u8]) -> Option<&'a Binding> {
        s.bindings.iter().find(|x| x.name == n)
    }
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
