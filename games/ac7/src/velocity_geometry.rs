// SPDX-License-Identifier: MIT
//! AC7 velocity geometry shader linkage and native emission augmentation.

use crate::velocity_container::{Semantic, chunks};
use crate::velocity_stage::{
    HelperCache, REGISTER_COUNT, RegisterMap, Stage, compile_bytecode, insert, mapped,
    next_register, rewrite_helper, stage, write_stage,
};
use crate::velocity_tokens::{
    DCL_TEMPS, Instruction, MAX_TEMPS, OperandEdit, append_words, read_operands, rewrite_operands,
    temp_count,
};

const GEOMETRY_PROFILE: u32 = 0x20050;
const VERTEX_PROFILE: u32 = 0x10050;
const POSITION: u32 = 1;
const FLOAT32: u32 = 3;

const TEMP: u32 = 0;
const INPUT: u32 = 1;
const OUTPUT: u32 = 2;

const CUT: u32 = 9;
const EMIT: u32 = 19;
const EMIT_THEN_CUT: u32 = 20;
const CUSTOM_DATA: u32 = 53;
const MOV: u32 = 54;
const RET: u32 = 62;
const DCL_RESOURCE: u32 = 88;
const DCL_CONSTANT_BUFFER: u32 = 89;
const DCL_GS_INPUT_PRIMITIVE: u32 = 93;
const DCL_MAX_OUTPUT_VERTEX_COUNT: u32 = 94;
// DCL_INPUT..=DCL_OUTPUT_SIV spans every input and output declaration.
const DCL_INPUT: u32 = 95;
const DCL_OUTPUT_SIV: u32 = 103;
const EMIT_STREAM: u32 = 117;
const CUT_STREAM: u32 = 118;
const EMIT_THEN_CUT_STREAM: u32 = 119;
const DCL_STREAM: u32 = 143;
const DCL_RESOURCE_RAW: u32 = 161;
const DCL_RESOURCE_STRUCTURED: u32 = 162;
const DCL_GS_INSTANCE_COUNT: u32 = 206;

#[derive(Default)]
pub(crate) struct GeometryRegisters {
    pub(crate) previous: u32,
    pub(crate) current: u32,
    pub(crate) maximum_vertices: u32,
}

impl GeometryRegisters {
    fn history_clip(&mut self, next: u32) -> (u32, u32) {
        self.current = next;
        self.previous = next + 1;
        (self.current, self.previous)
    }

    fn report_clip(&mut self, semantic: &Semantic<'_>) {
        match semantic.name {
            b"RSFPREV" => self.previous = semantic.reg,
            b"RSFCUR" => self.current = semantic.reg,
            _ => {}
        }
    }
}
fn clip_semantic(name: &'static [u8], reg: u32) -> Semantic<'static> {
    Semantic {
        name,
        index: 0,
        system: 0,
        ty: FLOAT32,
        reg,
        mask: 15,
        used: 0,
    }
}

fn history_match<'a, 'b>(
    source: &Semantic<'_>,
    vertex: &'a [Semantic<'b>],
) -> Option<&'a Semantic<'b>> {
    vertex.iter().find(|native| {
        (source.system == POSITION && native.system == POSITION)
            || (source.index == native.index && source.name.eq_ignore_ascii_case(native.name))
    })
}

fn history_declarations(
    code: &mut Vec<u32>,
    inputs: &[Semantic<'_>],
    outputs: &[Semantic<'_>],
) -> Option<()> {
    for s in inputs {
        if !(1..=15).contains(&s.mask) {
            return None;
        }
        let token = if s.system == 0 {
            0x0400005f
        } else {
            0x05000061
        };
        append_words(
            code,
            &[token, 0x00201002 | (u32::from(s.mask) << 4), 3, s.reg],
        )?;
        if s.system != 0 {
            append_words(code, &[s.system])?;
        }
    }
    for s in outputs {
        let token = if s.system == 0 {
            0x03000065
        } else {
            0x04000067
        };
        append_words(code, &[token, 0x00102002 | (u32::from(s.mask) << 4), s.reg])?;
        if s.system != 0 {
            append_words(code, &[s.system])?;
        }
    }
    Some(())
}

fn history_body(
    stage: &Stage<'_>,
    vertex: &[Semantic<'_>],
    current: u32,
    previous: u32,
    declarations: &mut Vec<u32>,
) -> Option<Vec<u32>> {
    let mut inputs = [None; REGISTER_COUNT];
    let mut outputs = [None; REGISTER_COUNT];
    for s in &stage.inputs {
        let native = history_match(s, vertex)?;
        if native.mask != 15 || native.ty != FLOAT32 {
            return None;
        }
        insert(&mut inputs, s.reg, native.reg)?;
    }
    for s in &stage.outputs {
        let reg = match s.name {
            b"RSFCUR" => current,
            b"RSFPREV" => previous,
            _ => history_match(s, vertex)?.reg,
        };
        insert(&mut outputs, s.reg, reg)?;
    }

    let position = stage.outputs.iter().find(|s| s.system == POSITION)?.reg;
    let mut vertex_index = u32::MAX;
    let mut emissions = 0;
    let mut body = Vec::new();
    for inst in &stage.code {
        match inst.opcode {
            DCL_INPUT..=DCL_OUTPUT_SIV => continue,
            _ if inst.declaration => {
                append_words(declarations, &inst.words)?;
                continue;
            }
            CUSTOM_DATA => {
                append_words(&mut body, &inst.words)?;
                continue;
            }
            MOV => {
                let mut operands = read_operands(&inst.words)?;
                if let (Some(dest), Some(source), None) =
                    (operands.next(), operands.next(), operands.next())
                    && dest.kind == OUTPUT
                    && dest.direct_register() == Some(position)
                    && source.kind == INPUT
                    && let Some([point, _]) = source.direct_register_pair()
                {
                    vertex_index = point;
                }
            }
            EMIT | EMIT_STREAM => {
                if vertex_index > 2 {
                    return None;
                }
                emissions += 1;
                // The fixed algorithm ignores some packed varyings. Forward them at each emit.
                for s in vertex {
                    append_words(
                        &mut body,
                        &[
                            0x06000036,
                            0x00102002 | (u32::from(s.mask) << 4),
                            s.reg,
                            0x00201e46,
                            vertex_index,
                            s.reg,
                        ],
                    )?;
                }
            }
            _ => {}
        }
        let edited = rewrite_operands(&inst.words, false, |operand| match operand.kind {
            OUTPUT => Some(OperandEdit::register(
                OUTPUT,
                mapped(&outputs, operand.direct_register()?)?,
            )),
            INPUT => {
                let [point, reg] = operand.direct_register_pair()?;
                if point > 2 {
                    return None;
                }
                Some(OperandEdit::register_pair(
                    2,
                    INPUT,
                    [point, mapped(&inputs, reg)?],
                ))
            }
            _ => Some(OperandEdit::KEEP),
        })?;
        append_words(&mut body, &edited)?;
    }
    (emissions == 3).then_some(body)
}

fn link_history(
    prototype: &[u8],
    mut vertex: Vec<Semantic<'_>>,
    registers: &mut GeometryRegisters,
    destination: &mut Vec<u8>,
) -> Option<()> {
    let chunks = chunks(prototype)?;
    let stage = stage(&chunks, GEOMETRY_PROFILE, &[*b"OSG5", *b"OSGN"])?;
    let (current, previous) = registers.history_clip(next_register(&vertex, 2)?);
    vertex.try_reserve(2).ok()?;
    let input_count = vertex.len();
    vertex.push(clip_semantic(b"RSFCUR", current));
    vertex.push(clip_semantic(b"RSFPREV", previous));
    let inputs = &vertex[..input_count];
    let mut code = Vec::new();
    append_words(&mut code, &[GEOMETRY_PROFILE, 0])?;
    history_declarations(&mut code, inputs, &vertex)?;
    let body = history_body(&stage, inputs, current, previous, &mut code)?;
    append_words(&mut code, &body)?;
    write_stage(&chunks, &stage.chunks, inputs, &vertex, &code, destination)
}

struct NativeGeometry {
    position: u32,
    next_output: u32,
}

fn admit_native(stage: &Stage<'_>, registers: &mut GeometryRegisters) -> Option<NativeGeometry> {
    if stage
        .code
        .iter()
        .any(|inst| inst.opcode == DCL_CONSTANT_BUFFER && inst.words.get(2) == Some(&13))
    {
        return None;
    }
    let mut position = None;
    for s in &stage.outputs {
        if s.system == POSITION && s.mask == 15 && s.ty == FLOAT32 {
            position = Some(s.reg);
        }
        if s.name.starts_with(b"RSF") {
            return None;
        }
    }
    let mut triangle = false;
    let mut maximum_vertices = 0;
    for inst in &stage.code {
        match inst.opcode {
            DCL_GS_INPUT_PRIMITIVE => triangle = (inst.words[0] >> 11) & 63 == 3,
            DCL_MAX_OUTPUT_VERTEX_COUNT if inst.words.len() == 2 => {
                maximum_vertices = inst.words[1];
                registers.maximum_vertices = maximum_vertices;
            }
            DCL_GS_INSTANCE_COUNT if inst.words.len() != 2 || inst.words[1] != 1 => return None,
            DCL_RESOURCE | DCL_RESOURCE_RAW | DCL_RESOURCE_STRUCTURED
                if inst.words.get(2) == Some(&127) =>
            {
                return None;
            }
            DCL_STREAM if inst.words.len() != 3 || inst.words[2] != 0 => return None,
            _ => {}
        }
    }
    if !triangle || !(1..=1024).contains(&maximum_vertices) {
        return None;
    }
    Some(NativeGeometry {
        position: position?,
        next_output: next_register(&stage.outputs, 5)?,
    })
}

struct NativeTemps {
    base: u32,
    ordinal: u32,
    position: u32,
    total: u32,
}

impl NativeTemps {
    fn new(native: &[Instruction], helper: &[Instruction]) -> Option<Self> {
        let base = temp_count(native);
        let ordinal = base + temp_count(helper);
        let position = ordinal + 1;
        let total = position + 1;
        (total <= MAX_TEMPS).then_some(Self {
            base,
            ordinal,
            position,
            total,
        })
    }
}

// The helper's inputs are either values computed in this GS or one of the three upstream vertices.
#[derive(Clone, Copy)]
enum HelperInput {
    Temp(u32),
    Vertex { point: u32, reg: u32 },
}

fn link_native_inputs<'a>(
    inputs: &mut Vec<Semantic<'a>>,
    helper: &[Semantic<'_>],
    vertex: &[Semantic<'a>],
    temps: &NativeTemps,
    declarations: &mut Vec<u32>,
) -> Option<RegisterMap<HelperInput>> {
    let mut mapping = [None; REGISTER_COUNT];
    inputs.try_reserve(helper.len()).ok()?;
    for s in helper {
        let (name, point) = match s.name {
            b"RSFCLIP" => {
                insert(&mut mapping, s.reg, HelperInput::Temp(temps.position))?;
                continue;
            }
            b"RSFORDINAL" => {
                insert(&mut mapping, s.reg, HelperInput::Temp(temps.ordinal))?;
                continue;
            }
            b"RSFA" => (b"RSFKEY".as_slice(), s.index),
            b"RSFB" => (b"RSFLOOKUP".as_slice(), s.index),
            name => (name, 0),
        };
        let found = vertex.iter().find(|v| v.name == name)?;
        if point > 2 || found.reg as usize >= REGISTER_COUNT {
            return None;
        }
        insert(
            &mut mapping,
            s.reg,
            HelperInput::Vertex {
                point,
                reg: found.reg,
            },
        )?;
        if !inputs.iter().any(|input| input.reg == found.reg) {
            let mut semantic = found.clone();
            semantic.used = semantic.mask;
            inputs.push(semantic);
            append_words(declarations, &[0x0400005f, 0x002010f2, 3, found.reg])?;
        }
    }
    Some(mapping)
}

fn link_native_outputs<'a>(
    outputs: &mut Vec<Semantic<'a>>,
    helper: Vec<Semantic<'a>>,
    next: u32,
    registers: &mut GeometryRegisters,
) -> Option<RegisterMap<u32>> {
    if next as usize + helper.len() > REGISTER_COUNT {
        return None;
    }
    let mut mapping = [None; REGISTER_COUNT];
    outputs.try_reserve(helper.len()).ok()?;
    for (reg, mut semantic) in (next..).zip(helper) {
        insert(&mut mapping, semantic.reg, reg)?;
        semantic.reg = reg;
        registers.report_clip(&semantic);
        outputs.push(semantic);
    }
    Some(mapping)
}

fn native_declarations(
    code: &mut Vec<u32>,
    native: &[Instruction],
    temps: &NativeTemps,
) -> Option<()> {
    let mut declared = false;
    for inst in native.iter().filter(|inst| inst.declaration) {
        if inst.opcode == DCL_TEMPS {
            append_words(code, &[inst.words[0], temps.total])?;
            declared = true;
        } else {
            append_words(code, &inst.words)?;
        }
    }
    if !declared {
        append_words(code, &[0x02000068, temps.total])?;
    }
    Some(())
}

fn native_injection(
    declarations: &mut Vec<u32>,
    helper: Vec<Instruction>,
    inputs: &RegisterMap<HelperInput>,
    outputs: &RegisterMap<u32>,
    temps: &NativeTemps,
) -> Option<Vec<u32>> {
    rewrite_helper(declarations, helper, outputs, temps.base, |register| {
        Some(match mapped(inputs, register)? {
            HelperInput::Vertex { point, reg } => {
                OperandEdit::register_pair(1, INPUT, [point, reg])
            }
            HelperInput::Temp(reg) => OperandEdit::register(TEMP, reg),
        })
    })
}

fn native_body(
    code: &mut Vec<u32>,
    native: &[Instruction],
    injection: &[u32],
    position: u32,
    temps: &NativeTemps,
) -> Option<()> {
    let ordinal = temps.ordinal;
    append_words(code, &[0x05000036, 0x00100012, ordinal, 0x00004001, 0])?;
    for inst in native.iter().filter(|inst| !inst.declaration) {
        match inst.opcode {
            EMIT | EMIT_THEN_CUT | EMIT_STREAM | EMIT_THEN_CUT_STREAM => {
                // Position is held in a temporary throughout the original body. Restore
                // the real output and record its history immediately before each emission.
                append_words(
                    code,
                    &[0x05000036, 0x001020f2, position, 0x00100e46, temps.position],
                )?;
                append_words(code, injection)?;
                append_words(
                    code,
                    &[
                        0x0700001e, 0x00100012, ordinal, 0x0010000a, ordinal, 0x00004001, 1,
                    ],
                )?;
                append_words(code, &inst.words)?;
            }
            CUSTOM_DATA | CUT | CUT_STREAM | RET => append_words(code, &inst.words)?,
            _ => {
                let edited = rewrite_operands(&inst.words, false, |operand| {
                    if operand.kind == OUTPUT && operand.direct_register()? == position {
                        Some(OperandEdit::register(TEMP, temps.position))
                    } else {
                        Some(OperandEdit::KEEP)
                    }
                })?;
                append_words(code, &edited)?;
            }
        }
    }
    Some(())
}

fn patch_native<'a>(
    bytes: &'a [u8],
    vertex: &[Semantic<'a>],
    registers: &mut GeometryRegisters,
    compile: impl FnOnce() -> Option<&'a [u8]>,
    destination: &mut Vec<u8>,
) -> Option<Vec<Semantic<'a>>> {
    let native_chunks = chunks(bytes)?;
    let mut native = stage(&native_chunks, GEOMETRY_PROFILE, &[*b"OSGN", *b"OSG5"])?;
    let binding = admit_native(&native, registers)?;
    let helper = stage(&chunks(compile()?)?, VERTEX_PROFILE, &[*b"OSGN"])?;
    let temps = NativeTemps::new(&native.code, &helper.code)?;

    let mut code = Vec::new();
    append_words(&mut code, &[GEOMETRY_PROFILE, 0])?;
    let inputs = link_native_inputs(
        &mut native.inputs,
        &helper.inputs,
        vertex,
        &temps,
        &mut code,
    )?;
    let outputs = link_native_outputs(
        &mut native.outputs,
        helper.outputs,
        binding.next_output,
        registers,
    )?;
    native_declarations(&mut code, &native.code, &temps)?;
    let injection = native_injection(&mut code, helper.code, &inputs, &outputs, &temps)?;
    native_body(
        &mut code,
        &native.code,
        &injection,
        binding.position,
        &temps,
    )?;

    write_stage(
        &native_chunks,
        &native.chunks,
        &native.inputs,
        &native.outputs,
        &code,
        destination,
    )?;
    Some(native.outputs)
}

pub(crate) fn history(
    vertex: &[Semantic<'_>],
    registers: &mut GeometryRegisters,
) -> Option<Vec<u8>> {
    let prototype = compile_bytecode(
        &HISTORY,
        HISTORY_SOURCE,
        c"ac7-previous-geometry",
        c"gs_5_0",
    )?;
    let mut code = Vec::new();
    let mut inputs = Vec::new();
    inputs.try_reserve_exact(vertex.len()).ok()?;
    inputs.extend_from_slice(vertex);
    link_history(prototype, inputs, registers, &mut code)?;
    Some(code)
}

pub(crate) fn native<'a>(
    bytes: &'a [u8],
    vertex: &[Semantic<'a>],
    registers: &mut GeometryRegisters,
) -> Option<(Vec<u8>, Vec<Semantic<'a>>)> {
    let mut code = Vec::new();
    let outputs = patch_native(
        bytes,
        vertex,
        registers,
        || {
            static HELPER: HelperCache = HelperCache::new();
            compile_bytecode(
                &HELPER,
                NATIVE_HELPER_SOURCE,
                c"ac7-native-generated-geometry",
                c"vs_5_0",
            )
        },
        &mut code,
    )?;
    Some((code, outputs))
}
static HISTORY: HelperCache = HelperCache::new();

#[cfg(feature = "warp-fixture")]
pub(crate) fn history_compile_count() -> u32 {
    u32::from(HISTORY.get().is_some())
}

const HISTORY_SOURCE: &str = concat!(
    "#define RSF_HISTORY_SLOT t0\n",
    include_str!("velocity_history_table.hlsl"),
    include_str!("velocity_history_lookup.hlsl"),
    include_str!("velocity_history_geometry.hlsl")
);

const NATIVE_HELPER_SOURCE: &str = concat!(
    "#define RSF_HISTORY_SLOT t127\n",
    include_str!("velocity_history_table.hlsl"),
    include_str!("velocity_history_lookup.hlsl"),
    include_str!("velocity_native_geometry.hlsl")
);
