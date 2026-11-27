// SPDX-License-Identifier: MIT
//! AC7 translucent velocity pixel shader admission and assembly.

use crate::dxbc_chunks::Chunk;
use crate::velocity_container::{Semantic, chunk_index, chunks, read_signature, shader_index};
use crate::velocity_stage::{HelperCache, compile_bytecode, write_stage_inputs};
use crate::velocity_tokens::{
    DCL_TEMPS, Instruction, MAX_TEMPS, OperandEdit, OperandView, append_words, instructions,
    read_operands, rewrite_operands, temp_count,
};
use std::borrow::Cow;

const PIXEL_PROFILE: u32 = 0x50;
const MAX_INPUTS: u32 = 32;
const UNASSIGNED: u32 = u32::MAX;
const VELOCITY_CONSTANTS: u32 = 13;
const ADDITIVE: u32 = 3;

const CALL: u32 = 4;
const CALLC: u32 = 5;
const CUSTOM_DATA: u32 = 53;
const MOV: u32 = 54;
const MUL: u32 = 56;
const RET: u32 = 62;
const RETC: u32 = 63;
const DCL_CONSTANT_BUFFER: u32 = 89;
const DCL_INPUT_PS: u32 = 98;
const DCL_INPUT_PS_SIV: u32 = 100;
const DCL_OUTPUT: u32 = 101;
const DCL_GLOBAL_FLAGS: u32 = 106;

const TEMP: u32 = 0;
const INPUT: u32 = 1;
const OUTPUT: u32 = 2;
const IMMEDIATE32: u32 = 4;

const HELPER_SOURCE: &str = r#"
cbuffer Velocity : register(b13) {
    float4 Rect; float4 Jitter; float Cutoff; uint HasHistory; float2 Padding;
};
float4 main(float4 current_clip:RSFCUR0,float4 previous:RSFPREV0,float4 colour:RSFCOLOR0):SV_Target0 {
    clip(colour.w-Cutoff);
    float2 current=current_clip.xy/current_clip.w-Jitter.xy;
    float2 old=previous.xy/previous.w-Jitter.zw;
    return float4((current-old)*0.2495+32767.0/65535.0,0,0);
}"#;

impl Instruction {
    fn operand_mask(&self, operand: &OperandView) -> u32 {
        (self.words[operand.token] >> 4) & 15
    }

    fn scalar_swizzle(&self, operand: &OperandView) -> u32 {
        (self.words[operand.token] >> 4) & 255
    }
}

fn native_inputs(
    bytes: &[u8],
    previous_register: u32,
    current_register: u32,
) -> Option<(Vec<Semantic<'_>>, u32, u32)> {
    let mut inputs = Vec::new();
    let mut next = 0;
    read_signature(bytes, false, |semantic| {
        if semantic.name == b"RSFPREV" {
            return None;
        }
        if semantic.reg != UNASSIGNED {
            // Unbound system values are outside the ordinary pixel input register file.
            if semantic.reg >= MAX_INPUTS {
                return None;
            }
            next = next.max(semantic.reg + 1);
        }
        inputs.try_reserve(1).ok()?;
        inputs.push(semantic);
        Some(())
    })?;
    let previous = if previous_register == UNASSIGNED {
        next
    } else {
        previous_register
    };
    if !(next..MAX_INPUTS).contains(&previous) {
        return None;
    }
    let current = if current_register == UNASSIGNED {
        previous + 1
    } else {
        current_register
    };
    if current >= MAX_INPUTS || current == previous || inputs.iter().any(|s| s.reg == current) {
        return None;
    }
    inputs.try_reserve(2).ok()?;
    for (name, reg) in [(b"RSFPREV".as_slice(), previous), (b"RSFCUR", current)] {
        inputs.push(Semantic {
            name,
            index: 0,
            system: 0,
            ty: 3,
            reg,
            mask: 15,
            used: 11,
        });
    }
    Some((inputs, previous, current))
}

fn native_target(bytes: &[u8]) -> Option<()> {
    let mut found = false;
    read_signature(bytes, false, |semantic| {
        if semantic.name.eq_ignore_ascii_case(b"SV_Target") {
            if semantic.index != 0 || semantic.reg != 0 {
                return None;
            }
            found = true;
        }
        Some(())
    })?;
    found.then_some(())
}

struct HelperInputs {
    current: u32,
    previous: u32,
    colour: u32,
}

fn helper_inputs(bytes: &[u8]) -> Option<HelperInputs> {
    let mut inputs = HelperInputs {
        current: UNASSIGNED,
        previous: UNASSIGNED,
        colour: UNASSIGNED,
    };
    read_signature(bytes, false, |semantic| {
        match semantic.name {
            b"RSFCUR" => inputs.current = semantic.reg,
            b"RSFPREV" => inputs.previous = semantic.reg,
            b"RSFCOLOR" => inputs.colour = semantic.reg,
            _ => {}
        }
        Some(())
    })?;
    (![inputs.current, inputs.previous, inputs.colour].contains(&UNASSIGNED)).then_some(inputs)
}

// AC7 additive shaders finish with RGB multiplied by opacity, an explicit zero alpha, and RET.
// Recover the original scalar only if its component survives until the injected helper runs.
fn additive_opacity(code: &[Instruction]) -> Option<&[u32]> {
    let (ret, body) = code.split_last()?;
    if ret.opcode != RET || body.len() < 2 {
        return None;
    }
    let mut multiply_index = None;
    let mut zero_index = None;
    for (index, inst) in body.iter().enumerate().rev() {
        if inst.declaration || inst.opcode == CUSTOM_DATA {
            continue;
        }
        let Some(dest) = read_operands(&inst.words)?.next() else {
            continue;
        };
        if dest.kind == OUTPUT && dest.direct_register() == Some(0) {
            let mask = inst.operand_mask(&dest);
            if mask & 7 != 0 {
                multiply_index.get_or_insert(index);
            }
            if mask & 8 != 0 {
                zero_index.get_or_insert(index);
            }
        }
    }

    let multiply_index = multiply_index?;
    let multiply = &body[multiply_index];
    let zero = &body[zero_index?];
    if multiply.opcode != MUL || zero.opcode != MOV {
        return None;
    }
    let mut mul_operands = read_operands(&multiply.words)?;
    let mut zero_operands = read_operands(&zero.words)?;
    let (rgb, lhs, rhs) = (
        mul_operands.next()?,
        mul_operands.next()?,
        mul_operands.next()?,
    );
    let (alpha, literal) = (zero_operands.next()?, zero_operands.next()?);
    if mul_operands.next().is_some()
        || zero_operands.next().is_some()
        || rgb.kind != OUTPUT
        || rgb.direct_register() != Some(0)
        || multiply.operand_mask(&rgb) != 7
        || alpha.kind != OUTPUT
        || alpha.direct_register() != Some(0)
        || zero.operand_mask(&alpha) != 8
        || literal.kind != IMMEDIATE32
        || zero.words[literal.end - 1] != 0
    {
        return None;
    }

    let mut scalars = [lhs, rhs].into_iter().filter(|operand| {
        let swizzle = multiply.scalar_swizzle(operand);
        operand.kind == TEMP && operand.direct_register().is_some() && swizzle == (swizzle & 3) * 85
    });
    let scalar = scalars.next()?;
    if scalars.next().is_some() {
        return None;
    }
    let register = scalar.direct_register()?;
    let component = 1 << (multiply.scalar_swizzle(&scalar) & 3);
    for inst in &body[multiply_index + 1..] {
        if inst.declaration || inst.opcode == CUSTOM_DATA {
            continue;
        }
        if let Some(dest) = read_operands(&inst.words)?.next()
            && dest.kind == TEMP
            && dest.direct_register() == Some(register)
            && inst.operand_mask(&dest) & component != 0
        {
            return None;
        }
    }
    Some(&multiply.words[scalar.token..scalar.end])
}

fn assemble(
    original: Vec<Instruction>,
    helper: Vec<Instruction>,
    inputs: HelperInputs,
    previous: u32,
    current: u32,
    blend: u32,
) -> Option<Vec<u32>> {
    let colour = temp_count(&original);
    let helper_base = colour + 1;
    let total_temps = helper_base + temp_count(&helper);
    // A framed stream without RET has no site at which to write velocity.
    if total_temps > MAX_TEMPS
        || !original.iter().any(|inst| inst.opcode == RET)
        || original.iter().any(|inst| {
            matches!(inst.opcode, CALL | CALLC | RETC)
                || (inst.opcode == DCL_CONSTANT_BUFFER
                    && inst.words.get(2) == Some(&VELOCITY_CONSTANTS))
        })
    {
        return None;
    }

    let mut opacity = Vec::new();
    if blend == ADDITIVE {
        let scalar = additive_opacity(&original)?;
        let mov_length = u32::try_from(3 + scalar.len()).ok()?;
        append_words(
            &mut opacity,
            &[(mov_length << 24) | MOV, 0x0010_0082, colour],
        )?;
        append_words(&mut opacity, scalar)?;
    }

    let mut code = Vec::new();
    append_words(&mut code, &[PIXEL_PROFILE, 0])?;
    let mut body = Vec::new();
    body.try_reserve_exact(original.len()).ok()?;
    let mut temps_declared = false;
    for mut inst in original {
        if inst.opcode == DCL_TEMPS {
            inst.words[1] = total_temps;
            temps_declared = true;
        }
        if inst.declaration {
            append_words(&mut code, &inst.words)?;
            continue;
        }
        if inst.opcode != CUSTOM_DATA
            && let Cow::Owned(rewritten) = rewrite_operands(&inst.words, false, |operand| {
                if operand.kind == OUTPUT && operand.direct_register()? == 0 {
                    return Some(OperandEdit::register(TEMP, colour));
                }
                Some(OperandEdit::KEEP)
            })?
        {
            inst.words = rewritten;
        }
        body.push(inst);
    }
    if !temps_declared {
        append_words(&mut code, &[(2 << 24) | DCL_TEMPS, total_temps])?;
    }

    let mut injection = opacity;
    for mut inst in helper {
        match inst.opcode {
            DCL_TEMPS | DCL_GLOBAL_FLAGS | DCL_OUTPUT | RET => continue,
            DCL_INPUT_PS..=DCL_INPUT_PS_SIV => {
                let register = inst.words.get_mut(2)?;
                if *register == inputs.colour {
                    continue;
                }
                *register = if *register == inputs.current {
                    current
                } else if *register == inputs.previous {
                    previous
                } else {
                    return None;
                };
            }
            _ => {}
        }
        if inst.declaration {
            append_words(&mut code, &inst.words)?;
            continue;
        }
        let rewritten = rewrite_operands(&inst.words, false, |operand| {
            let edit = match operand.kind {
                TEMP => {
                    let register = operand.direct_register()?.checked_add(helper_base)?;
                    if register >= MAX_TEMPS {
                        return None;
                    }
                    OperandEdit::register(TEMP, register)
                }
                INPUT => match operand.direct_register()? {
                    reg if reg == inputs.colour => OperandEdit::register(TEMP, colour),
                    reg if reg == inputs.current => OperandEdit::register(INPUT, current),
                    reg if reg == inputs.previous => OperandEdit::register(INPUT, previous),
                    _ => return None,
                },
                _ => OperandEdit::KEEP,
            };
            Some(edit)
        })?;
        append_words(&mut injection, &rewritten)?;
    }

    // Preserve native material control flow and discards by inlining at each unconditional RET.
    for inst in body {
        if inst.opcode == RET {
            append_words(&mut code, &injection)?;
        }
        append_words(&mut code, &inst.words)?;
    }
    Some(code)
}

fn patch(
    native: &[Chunk<'_>],
    helper: &[Chunk<'_>],
    blend: u32,
    previous: u32,
    current: u32,
    destination: &mut Vec<u8>,
) -> Option<()> {
    let shader = shader_index(native)?;
    let input = chunk_index(native, b"ISGN")?;
    let target = chunk_index(native, b"OSGN")?;
    let extra_shader = shader_index(helper)?;
    let extra_input = chunk_index(helper, b"ISGN")?;
    let original = instructions(native[shader].data, PIXEL_PROFILE)?;
    let added = instructions(helper[extra_shader].data, PIXEL_PROFILE)?;
    let (inputs, previous, current) = native_inputs(native[input].data, previous, current)?;
    native_target(native[target].data)?;
    let extra_inputs = helper_inputs(helper[extra_input].data)?;
    let code = assemble(original, added, extra_inputs, previous, current, blend)?;

    write_stage_inputs(native, shader, input, &inputs, &code, None, destination)
}

pub(crate) fn transform(bytes: &[u8], blend: u32, previous: u32, current: u32) -> Option<Vec<u8>> {
    if !(2..=5).contains(&blend) {
        return None;
    }
    let native = chunks(bytes)?;
    static HELPER: HelperCache = HelperCache::new();
    let helper = chunks(compile_bytecode(
        &HELPER,
        HELPER_SOURCE,
        c"ac7-translucent-velocity",
        c"ps_5_0",
    )?)?;
    let mut code = Vec::new();
    patch(&native, &helper, blend, previous, current, &mut code)?;
    Some(code)
}
