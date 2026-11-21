// SPDX-License-Identifier: MIT
//! AC7 velocity geometry shader linkage and native emission augmentation.

use crate::dxbc_chunks::Chunk;
use crate::velocity_container::{
    AppendSemantic, Resize, Semantic, SemanticView, borrowed, chunk_index, chunks, native_output,
    read_signature, resize_bytes, shader_index, valid_container_size, write_container,
    write_signature,
};
use crate::velocity_tokens::{
    DCL_TEMPS, Instruction, MAX_TEMPS, OperandEdit, append_words, instructions, read_operands,
    rewrite_operands, temp_count, write_instructions,
};
use std::ffi::{c_char, c_void};
use std::ptr;

const GEOMETRY_PROFILE: u32 = 0x20050;
const VERTEX_PROFILE: u32 = 0x10050;
const REGISTER_COUNT: usize = 32;
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
const DCL_INPUT: u32 = 95;
const DCL_INPUT_SIV: u32 = 97;
const DCL_OUTPUT: u32 = 101;
const DCL_OUTPUT_SIV: u32 = 103;
const DCL_GLOBAL_FLAGS: u32 = 106;
const EMIT_STREAM: u32 = 117;
const CUT_STREAM: u32 = 118;
const EMIT_THEN_CUT_STREAM: u32 = 119;
const DCL_STREAM: u32 = 143;
const DCL_RESOURCE_RAW: u32 = 161;
const DCL_RESOURCE_STRUCTURED: u32 = 162;
const DCL_GS_INSTANCE_COUNT: u32 = 206;

type CompileHelper = unsafe extern "C" fn(*const c_char, usize, *mut *const u8, *mut usize) -> bool;
type RegisterMap<T> = [Option<T>; REGISTER_COUNT];

#[repr(C)]
struct GeometryRegisters {
    previous: *mut u32,
    current: *mut u32,
    maximum_vertices: *mut u32,
}

const _: () = assert!(size_of::<GeometryRegisters>() == 24);

impl GeometryRegisters {
    // These are caller-owned scalars. Raw writes keep their original order even when
    // the C++ references alias, and preserve values already reported before a refusal.
    fn history_clip(&self, next: u32) -> (u32, u32) {
        unsafe {
            self.current.write(next);
            self.previous.write(next + 1);
            (self.current.read(), self.previous.read())
        }
    }

    fn report_clip(&self, semantic: &Semantic<'_>) {
        match semantic.name {
            b"RSFPREV" => unsafe { self.previous.write(semantic.reg) },
            b"RSFCUR" => unsafe { self.current.write(semantic.reg) },
            _ => {}
        }
    }

    fn report_maximum(&self, maximum: u32) {
        unsafe { self.maximum_vertices.write(maximum) };
    }
}

struct StageChunks {
    shader: usize,
    input: usize,
    output: usize,
}

struct Stage<'a> {
    chunks: StageChunks,
    code: Vec<Instruction>,
    inputs: Vec<Semantic<'a>>,
    outputs: Vec<Semantic<'a>>,
}

fn signature<'a>(chunk: &Chunk<'a>) -> Option<Vec<Semantic<'a>>> {
    let mut result = Vec::new();
    read_signature(chunk.data, chunk.tag == b"OSG5", |semantic| {
        result.try_reserve(1).ok()?;
        result.push(semantic);
        Some(())
    })?;
    Some(result)
}

fn stage<'a>(chunks: &[Chunk<'a>], profile: u32, output_tags: &[[u8; 4]]) -> Option<Stage<'a>> {
    let shader = shader_index(chunks)?;
    let input = chunk_index(chunks, b"ISGN")?;
    let output = output_tags
        .iter()
        .find_map(|tag| chunk_index(chunks, tag))?;
    Some(Stage {
        chunks: StageChunks {
            shader,
            input,
            output,
        },
        code: instructions(chunks[shader].data, profile)?,
        inputs: signature(&chunks[input])?,
        outputs: signature(&chunks[output])?,
    })
}

fn write_stage<'a>(
    chunks: &[Chunk<'_>],
    stage: &StageChunks,
    inputs: &[Semantic<'_>],
    outputs: &[Semantic<'_>],
    code: &[u32],
    allocate: impl FnOnce(usize) -> Option<&'a mut [u8]>,
) -> Option<()> {
    let mut shader = Vec::new();
    let mut input = Vec::new();
    let mut output = Vec::new();
    let shader_allocation = &mut shader;
    write_instructions(code, move |size| resize_bytes(shader_allocation, size))?;
    let input_allocation = &mut input;
    write_signature(inputs, false, move |size| {
        resize_bytes(input_allocation, size)
    })?;
    let output_allocation = &mut output;
    write_signature(outputs, chunks[stage.output].tag == b"OSG5", move |size| {
        resize_bytes(output_allocation, size)
    })?;

    let mut linked = Vec::new();
    linked.try_reserve_exact(chunks.len()).ok()?;
    for (index, chunk) in chunks.iter().enumerate() {
        let data = if index == stage.shader {
            &shader[..]
        } else if index == stage.input {
            &input[..]
        } else if index == stage.output {
            &output[..]
        } else {
            chunk.data
        };
        linked.push(Chunk {
            tag: chunk.tag,
            data,
        });
    }
    write_container(&linked, allocate)
}

fn next_register(semantics: &[Semantic<'_>], added: usize) -> Option<u32> {
    let next = semantics.iter().map(|semantic| semantic.reg).max();
    let next = next.map_or(Some(0), |reg| reg.checked_add(1))?;
    (next as usize <= REGISTER_COUNT.checked_sub(added)?).then_some(next)
}

fn insert<T>(map: &mut RegisterMap<T>, register: u32, value: T) -> Option<()> {
    *map.get_mut(register as usize)? = Some(value);
    Some(())
}

fn mapped<T: Copy>(map: &RegisterMap<T>, register: u32) -> Option<T> {
    map.get(register as usize).copied().flatten()
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
            _ if inst.declaration
                || matches!(
                    inst.opcode,
                    DCL_STREAM | DCL_GS_INSTANCE_COUNT | DCL_RESOURCE_RAW
                ) =>
            {
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

fn link_history<'a>(
    prototype: &[u8],
    mut vertex: Vec<Semantic<'_>>,
    registers: &GeometryRegisters,
    allocate: impl FnOnce(usize) -> Option<&'a mut [u8]>,
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
    write_stage(&chunks, &stage.chunks, inputs, &vertex, &code, allocate)
}

struct NativeGeometry {
    position: u32,
    next_output: u32,
}

fn admit_native(stage: &Stage<'_>, registers: &GeometryRegisters) -> Option<NativeGeometry> {
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
                registers.report_maximum(maximum_vertices);
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
    registers: &GeometryRegisters,
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

fn native_declaration(inst: &Instruction) -> bool {
    inst.declaration || matches!(inst.opcode, DCL_STREAM | DCL_GS_INSTANCE_COUNT)
}

fn native_declarations(
    code: &mut Vec<u32>,
    native: &[Instruction],
    temps: &NativeTemps,
) -> Option<()> {
    let mut declared = false;
    for inst in native.iter().filter(|inst| native_declaration(inst)) {
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
    let mut injection = Vec::new();
    for mut inst in helper {
        match inst.opcode {
            DCL_TEMPS | DCL_GLOBAL_FLAGS | RET | DCL_INPUT..=DCL_INPUT_SIV => continue,
            DCL_OUTPUT..=DCL_OUTPUT_SIV => {
                let reg = inst.words.get_mut(2)?;
                *reg = mapped(outputs, *reg)?;
                append_words(declarations, &inst.words)?;
            }
            _ if inst.declaration || inst.opcode == DCL_RESOURCE_RAW => {
                append_words(declarations, &inst.words)?;
            }
            _ => {
                let edited = rewrite_operands(&inst.words, false, |operand| {
                    let edit = match operand.kind {
                        TEMP => {
                            let reg = temps.base.checked_add(operand.direct_register()?)?;
                            if reg >= MAX_TEMPS {
                                return None;
                            }
                            OperandEdit::register(TEMP, reg)
                        }
                        OUTPUT => OperandEdit::register(
                            OUTPUT,
                            mapped(outputs, operand.direct_register()?)?,
                        ),
                        INPUT => match mapped(inputs, operand.direct_register()?)? {
                            HelperInput::Vertex { point, reg } => {
                                OperandEdit::register_pair(1, INPUT, [point, reg])
                            }
                            HelperInput::Temp(reg) => OperandEdit::register(TEMP, reg),
                        },
                        _ => OperandEdit::KEEP,
                    };
                    Some(edit)
                })?;
                append_words(&mut injection, &edited)?;
            }
        }
    }
    Some(injection)
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
    for inst in native.iter().filter(|inst| !native_declaration(inst)) {
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

fn patch_native<'a, 'out>(
    bytes: &'a [u8],
    vertex: &[Semantic<'a>],
    registers: &GeometryRegisters,
    compile: impl FnOnce() -> Option<&'a [u8]>,
    allocate: impl FnOnce(usize) -> Option<&'out mut [u8]>,
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
        allocate,
    )?;
    Some(native.outputs)
}

fn vertex_outputs(views: &[SemanticView]) -> Option<Vec<Semantic<'_>>> {
    let mut outputs = Vec::new();
    outputs.try_reserve_exact(views.len()).ok()?;
    for view in views {
        outputs.push(unsafe { view.borrow() }?);
    }
    Some(outputs)
}

/// The leaf borrows the exact HLSL and lends immutable bytecode owned by its module cache.
unsafe fn compile_bytecode<'a>(compile: CompileHelper, source: &str) -> Option<&'a [u8]> {
    let mut bytes = ptr::null();
    let mut size = 0;
    if !unsafe { compile(source.as_ptr().cast(), source.len(), &mut bytes, &mut size) }
        || !valid_container_size(size)
    {
        return None;
    }
    unsafe { borrowed(bytes, size) }
}

/// The wrapper keeps all input views and names alive, lends scalar outputs, and resizes a
/// separate native byte vector.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_geometry_shader(
    vertex: *const SemanticView,
    count: usize,
    compile: CompileHelper,
    user: *mut c_void,
    resize: Resize,
    registers: *const GeometryRegisters,
) -> bool {
    let Some(registers) = (unsafe { registers.as_ref() }) else {
        return false;
    };
    if registers.previous.is_null() || registers.current.is_null() {
        return false;
    }
    let Some(vertex) = (unsafe { borrowed(vertex, count) }).and_then(vertex_outputs) else {
        return false;
    };
    let Some(prototype) = (unsafe { compile_bytecode(compile, HISTORY_SOURCE) }) else {
        return false;
    };
    link_history(prototype, vertex, registers, |size| unsafe {
        native_output(user, resize, size)
    })
    .is_some()
}

/// The wrapper also lends the original GS and copies each output semantic into a separate
/// destination before returning. Scalar outputs can alias one another.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_native_geometry_shader(
    bytes: *const u8,
    size: usize,
    vertex: *const SemanticView,
    count: usize,
    compile: CompileHelper,
    user: *mut c_void,
    resize: Resize,
    append: AppendSemantic,
    registers: *const GeometryRegisters,
) -> bool {
    let Some(registers) = (unsafe { registers.as_ref() }) else {
        return false;
    };
    if !valid_container_size(size)
        || registers.previous.is_null()
        || registers.current.is_null()
        || registers.maximum_vertices.is_null()
    {
        return false;
    }
    let Some(bytes) = (unsafe { borrowed(bytes, size) }) else {
        return false;
    };
    let Some(vertex) = (unsafe { borrowed(vertex, count) }).and_then(vertex_outputs) else {
        return false;
    };
    let Some(outputs) = patch_native(
        bytes,
        &vertex,
        registers,
        || unsafe { compile_bytecode(compile, NATIVE_HELPER_SOURCE) },
        |size| unsafe { native_output(user, resize, size) },
    ) else {
        return false;
    };
    outputs
        .iter()
        .all(|semantic| unsafe { append(user, &semantic.view()) })
}

const HISTORY_SOURCE: &str = r#"struct V{float4 f0:SV_Position0;float4 f1:RSFKEY0;float4 f2:RSFCENTRE0;float4 f3:RSFOLD0;float4 f4:RSFLOOKUP0;};struct O{float4 f0:SV_Position0;float4 f1:RSFKEY0;float4 f2:RSFCENTRE0;float4 f3:RSFOLD0;float4 f4:RSFLOOKUP0;float4 current:RSFCUR0;float4 previous:RSFPREV0;};

ByteAddressBuffer History:register(t0);
cbuffer Parameters:register(b13) {
    float4 Rect;float4 Jitter;float Cutoff;uint HasHistory;uint HistoryMask;uint Padding;
    row_major float4x4 ClipToPrevious;
};
uint key_hash(uint4 key) {
    uint value=2166136261;
    [unroll]for(uint i=0;i<4;i++) {value=(value^key[i])*16777619;}
    value^=value>>16;return max(1,value&0x7fffffff);
}
uint decode_tag(float value) {uint bits=asuint(value);return (bits&0x1fffffff)|((bits>>2)&0x20000000);}
float encode_tag(uint value) {return asfloat(0x20000000|(value&0x1fffffff)|((value&0x20000000)<<2));}
bool endpoint(float4 key,out float4 position,out bool ambiguous) {
    uint4 bits=asuint(key);uint hash=key_hash(bits);bool found=false;position=0;ambiguous=false;
    [loop]for(uint probe=0;probe<32;probe++) {
        uint address=((hash+probe)&HistoryMask)*64;uint marker=History.Load(address);
        if(marker==0)break;if(marker!=hash || any(History.Load4(address+4)!=bits))continue;
        float4 candidate=asfloat(History.Load4(address+20));if(!all(isfinite(candidate)) || candidate.w<=0)continue;
        if(!found) {position=candidate;found=true;}
        else if(any(asuint(position)!=asuint(candidate)))ambiguous=true;
    }
    return found;
}
float4 previous_position(float4 current,float4 key,float3 old) {
    float4 unjittered=current;unjittered.xy-=Jitter.xy*current.w;
    float4 fallback=mul(unjittered,ClipToPrevious);fallback.xy+=Jitter.zw*fallback.w;
    if(HasHistory==0 || key.w==3)return fallback;
    if(key.w==4 || key.w==5) {
        bool left=false,right=false,bad_left=false,bad_right=false;float low=-1,high=2;float4 a=fallback,b=fallback;
        uint first=decode_tag(key.x),second=decode_tag(key.y);
        if(first>>28!=0) {
            float4 start=key;start.y=encode_tag((second&~0x3ffffff)|(first&0x3ffffff));start.z=0;
            left=endpoint(start,a,bad_left);if(left)low=0;
            float4 end=key;end.x=encode_tag((first&~0x3ffffff)|(second&0x3ffffff));end.z=0;
            right=endpoint(end,b,bad_right);if(right)high=1;
            if(old.x==0)return left && !bad_left ? a : fallback;
            if(old.x==1)return right && !bad_right ? b : fallback;
            // A head point has no prior segment. Its own rendered endpoint still has history.
            if((first&0x3ffffff)==(second&0x3ffffff))return left && !bad_left ? a : fallback;
        }
        uint bin=uint(key.z);
        [loop]for(uint distance=0;distance<=128;distance++) {
            if(left && right && (first>>28==0 || (low>=float(int(bin)-int(distance))/128 && high<=float(bin+distance)/128)))break;
            [unroll]for(uint side=0;side<2;side++) {
                int target=int(bin)+(side==0 ? -int(distance) : int(distance));
                if(target<0 || target>128 || (side==0 && left && low>=float(target+1)/128) || (side==1 && right && high<float(target)/128))continue;
                float4 query=key;query.z=float(target);uint4 bits=asuint(query);uint hash=key_hash(bits);
                [loop]for(uint probe=0;probe<32;probe++) {
                    uint address=((hash+probe)&HistoryMask)*64;uint marker=History.Load(address);
                    if(marker==0)break;if(marker!=hash || any(History.Load4(address+4)!=bits))continue;
                    float4 clip=asfloat(History.Load4(address+20));float u=asfloat(History.Load(address+36));
                    if(!all(isfinite(clip)) || clip.w<=0 || !isfinite(u))continue;
                    if(u<=old.x && (!left || u>low)) {left=true;low=u;a=clip;bad_left=false;}
                    else if(left && u==low && any(asuint(a)!=asuint(clip)))bad_left=true;
                    if(u>=old.x && (!right || u<high)) {right=true;high=u;b=clip;bad_right=false;}
                    else if(right && u==high && any(asuint(b)!=asuint(clip)))bad_right=true;
                }
            }
        }
        if(!left || !right || bad_left || bad_right)return fallback;
        return high==low ? a : lerp(a,b,(old.x-low)/(high-low));
    }
    uint4 bits=asuint(key);uint hash=key_hash(bits);
    bool found=false,ambiguous=false;float distance=3.402823e38;float4 selected=fallback;
    [loop]for(uint probe=0;probe<32;probe++) {
        uint address=((hash+probe)&HistoryMask)*64;
        uint marker=History.Load(address);
        if(marker==0)break;
        if(marker!=hash || any(History.Load4(address+4)!=bits))continue;
        float4 candidate=asfloat(History.Load4(address+20));
        if(!all(isfinite(candidate)) || candidate.w<=0)continue;
        float3 centre=asfloat(History.Load3(address+36));
        float d=key.w==1 ? dot(centre-old,centre-old) : 0;
        if(!found || d<distance) {found=true;distance=d;selected=candidate;ambiguous=false;}
        else if(d==distance && any(asuint(candidate)!=asuint(selected)))ambiguous=true;
    }
    return found && !ambiguous ? selected : fallback;
}
[maxvertexcount(3)]void main(triangle V input[3],inout TriangleStream<O> stream) {
    [unroll]for(uint j=0;j<3;j++) {O output;
output.f0=input[j].f0;output.f1=input[j].f1;output.f2=input[j].f2;output.f3=input[j].f3;output.f4=input[j].f4;output.current=input[j].f0;output.previous=previous_position(input[j].f0,input[j].f4,input[j].f3.xyz);stream.Append(output);}stream.RestartStrip();}"#;

const NATIVE_HELPER_SOURCE: &str = r#"
ByteAddressBuffer History:register(t127);
cbuffer Parameters:register(b13) {
 float4 Rect;float4 Jitter;float Cutoff;uint HasHistory;uint HistoryMask;uint Padding;
 row_major float4x4 ClipToPrevious;
};
struct O {float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 previous:RSFPREV0;float4 current:RSFCUR0;};
uint4 geometry_key(float4 a,float4 b,float4 c,uint ordinal) {
 uint3 hash=uint3(2166136261,2246822519,3266489917);
 [unroll]for(uint v=0;v<3;v++) {
  uint4 bits=asuint(v==0 ? a : v==1 ? b : c);
  [unroll]for(uint j=0;j<4;j++)hash=(hash^bits[j])*uint3(16777619,3266489917,668265263);
 }
 return uint4(hash,ordinal);
}
O main(float4 p:RSFCLIP0,float4 a:RSFA0,float4 b:RSFA1,float4 c:RSFA2,
       float4 la:RSFB0,float4 lb:RSFB1,float4 lc:RSFB2,
       float4 centre:RSFCENTRE0,float4 old:RSFOLD0,uint ordinal:RSFORDINAL0) {
 O o;o.key=asfloat(geometry_key(a,b,c,ordinal));o.centre=centre;o.old=old;o.current=p;
 float4 unjittered=p;unjittered.xy-=Jitter.xy*p.w;
 o.previous=mul(unjittered,ClipToPrevious);o.previous.xy+=Jitter.zw*o.previous.w;
 uint4 lookup=geometry_key(la,lb,lc,ordinal);uint hash=2166136261;
 [unroll]for(uint j=0;j<4;j++)hash=(hash^lookup[j])*16777619;
 hash^=hash>>16;hash=max(1,hash&0x7fffffff);
 bool found=false,ambiguous=false;float distance=3.402823e38;float4 selected=o.previous;
 if(HasHistory!=0 && la.w!=3 && lb.w!=3 && lc.w!=3) {
  [loop]for(uint probe=0;probe<32;probe++) {
   uint address=((hash+probe)&HistoryMask)*64;uint marker=History.Load(address);
   if(marker==0)break;
   if(marker!=hash || any(History.Load4(address+4)!=lookup))continue;
   float4 candidate=asfloat(History.Load4(address+20));
   if(!all(isfinite(candidate)) || candidate.w<=0)continue;
   float3 centre=asfloat(History.Load3(address+36));
   float d=la.w==1 ? dot(centre-old.xyz,centre-old.xyz) : 0;
   if(!found || d<distance) {found=true;distance=d;selected=candidate;ambiguous=false;}
   else if(d==distance && any(asuint(candidate)!=asuint(selected)))ambiguous=true;
  }
 }
 if(found && !ambiguous)o.previous=selected;return o;
}"#;
