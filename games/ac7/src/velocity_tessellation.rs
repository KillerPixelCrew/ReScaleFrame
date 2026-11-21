// SPDX-License-Identifier: MIT
//! AC7 velocity hull control-point relay and native domain evaluation.

use crate::velocity_container::{
    AppendSemantic, Resize, Semantic, SemanticView, borrowed, chunk_index, chunks, native_output,
};
use crate::velocity_stage::{
    CompileHelper, REGISTER_COUNT, RegisterMap, Stage, compile_bytecode, insert, mapped,
    next_register, signature, stage, vertex_outputs, write_stage,
};
use crate::velocity_tokens::{
    DCL_TEMPS, Instruction, MAX_TEMPS, OperandEdit, OperandView, append_words, rewrite_operands,
    temp_count,
};
use std::collections::BTreeMap;
use std::ffi::c_void;

const HULL_PROFILE: u32 = 0x30050;
const DOMAIN_PROFILE: u32 = 0x40050;
const VERTEX_PROFILE: u32 = 0x10050;
const TRIANGLE_POINTS: u32 = 3;
const POSITION: u32 = 1;
const FLOAT32: u32 = 3;

const TEMP: u32 = 0;
const INPUT: u32 = 1;
const OUTPUT: u32 = 2;
const OUTPUT_CONTROL_POINT_ID: u32 = 22;
const INPUT_CONTROL_POINT: u32 = 25;
const INPUT_DOMAIN_POINT: u32 = 28;

const CALL: u32 = 4;
const CALL_C: u32 = 5;
const CUSTOM_DATA: u32 = 53;
const RET: u32 = 62;
const RET_C: u32 = 63;
const DCL_RESOURCE: u32 = 88;
const DCL_CONSTANT_BUFFER: u32 = 89;
const DCL_INPUT: u32 = 95;
const DCL_INPUT_SIV: u32 = 97;
const DCL_OUTPUT: u32 = 101;
const DCL_OUTPUT_SIV: u32 = 103;
const DCL_GLOBAL_FLAGS: u32 = 106;
const HS_CONTROL_POINT_PHASE: u32 = 114;
const HS_FORK_PHASE: u32 = 115;
const HS_JOIN_PHASE: u32 = 116;
const DCL_INPUT_CONTROL_POINT_COUNT: u32 = 147;
const DCL_OUTPUT_CONTROL_POINT_COUNT: u32 = 148;
const DCL_TESS_DOMAIN: u32 = 149;
const DCL_TESS_PARTITIONING: u32 = 150;
const DCL_RESOURCE_RAW: u32 = 161;
const DCL_RESOURCE_STRUCTURED: u32 = 162;

const HULL_FIELDS: [&[u8]; 4] = [b"RSFKEY", b"RSFLOOKUP", b"RSFCENTRE", b"RSFOLD"];

fn declares_input(inst: &Instruction, kind: u32) -> bool {
    inst.opcode == DCL_INPUT && inst.words.len() == 2 && (inst.words[1] >> 12) & 255 == kind
}

fn control_points(inst: &Instruction) -> u32 {
    (inst.words[0] >> 11) & 63
}

struct HullRelay {
    index_temp: u32,
    declares_temps: bool,
    declarations: Vec<u32>,
    copy: Vec<u32>,
}

fn link_hull<'a>(
    native: &mut Stage<'a>,
    vertex: &[Semantic<'a>],
    points: u32,
    next: u32,
) -> Option<HullRelay> {
    let mut in_control = false;
    let mut native_temps = None;
    for inst in &native.code {
        match inst.opcode {
            HS_CONTROL_POINT_PHASE => in_control = true,
            HS_FORK_PHASE | HS_JOIN_PHASE => in_control = false,
            DCL_TEMPS if in_control => native_temps = Some(inst.words[1]),
            _ => {}
        }
    }
    let index_temp = native_temps.unwrap_or(0);
    if index_temp >= MAX_TEMPS {
        return None;
    }

    let mut declarations = Vec::new();
    let mut copy = Vec::new();
    append_words(&mut copy, &[0x04000036, 0x00100012, index_temp, 0x00016001])?;
    native.inputs.try_reserve(HULL_FIELDS.len()).ok()?;
    native.outputs.try_reserve(HULL_FIELDS.len()).ok()?;
    for (reg, name) in (next..).zip(HULL_FIELDS) {
        let source = vertex.iter().find(|s| s.name == name)?;
        if source.reg as usize >= REGISTER_COUNT
            || native.inputs.iter().any(|s| s.reg == source.reg)
        {
            return None;
        }
        let mut semantic = source.clone();
        semantic.used = semantic.mask;
        native.inputs.push(semantic.clone());
        semantic.reg = reg;
        native.outputs.push(semantic);
        append_words(
            &mut declarations,
            &[
                0x0400005f, 0x002010f2, points, source.reg, 0x03000065, 0x001020f2, reg,
            ],
        )?;
        // The relative first index is the native output control-point ID.
        append_words(
            &mut copy,
            &[
                0x07000036, 0x001020f2, reg, 0x00a01e46, 0x0010000a, index_temp, source.reg,
            ],
        )?;
    }
    Some(HullRelay {
        index_temp,
        declares_temps: native_temps.is_some(),
        declarations,
        copy,
    })
}

fn implicit_control(native: &Stage<'_>, points: u32) -> Option<Vec<u32>> {
    let mut control = Vec::new();
    let mut copy = Vec::new();
    append_words(
        &mut control,
        &[0x01000072, 0x0200005f, 0x00016000, 0x02000068, 1],
    )?;
    append_words(&mut copy, &[0x04000036, 0x00100012, 0, 0x00016001])?;
    // Optimized identity hull shaders omit this phase. Match by semantic and index so
    // the native fields and the four added fields survive different register packing.
    for output in &native.outputs {
        let input = native
            .inputs
            .iter()
            .find(|s| s.name == output.name && s.index == output.index)?;
        if input.mask != output.mask
            || !(1..=15).contains(&output.mask)
            || input.reg as usize >= REGISTER_COUNT
        {
            return None;
        }
        let mask = u32::from(output.mask) << 4;
        append_words(
            &mut control,
            &[
                0x0400005f,
                0x00201002 | mask,
                points,
                input.reg,
                0x03000065,
                0x00102002 | mask,
                output.reg,
            ],
        )?;
        append_words(
            &mut copy,
            &[
                0x07000036,
                0x00102002 | mask,
                output.reg,
                0x00a01e46,
                0x0010000a,
                0,
                input.reg,
            ],
        )?;
    }
    append_words(&mut control, &copy)?;
    append_words(&mut control, &[0x0100003e])?;
    Some(control)
}

fn corner_factors(
    code: &mut Vec<u32>,
    constants: &[Semantic<'_>],
    written: &BTreeMap<u32, u32>,
) -> Option<()> {
    for s in constants.iter().filter(|s| (11..=16).contains(&s.system)) {
        let mask = u32::from(s.mask) & written.get(&s.reg).copied().unwrap_or(0);
        if mask != 0 {
            append_words(
                code,
                &[
                    0x05000036,
                    0x00102002 | (mask << 4),
                    s.reg,
                    0x00004001,
                    0x3f800000,
                ],
            )?;
        }
    }
    Some(())
}

struct HullPrograms {
    ordinary: Vec<u32>,
    corners: Vec<u32>,
}

impl HullPrograms {
    fn both(&mut self, words: &[u32]) -> Option<()> {
        append_words(&mut self.ordinary, words)?;
        append_words(&mut self.corners, words)
    }
}

fn hull_programs<'a>(
    native: &mut Stage<'a>,
    constants: &[Semantic<'_>],
    vertex: &[Semantic<'a>],
    report_points: impl FnOnce(u32),
) -> Option<HullPrograms> {
    if native.outputs.iter().any(|s| s.name.starts_with(b"RSF")) {
        return None;
    }
    let mut points = 0;
    let mut output_points = 0;
    for inst in &native.code {
        match inst.opcode {
            DCL_INPUT_CONTROL_POINT_COUNT => points = control_points(inst),
            DCL_OUTPUT_CONTROL_POINT_COUNT => output_points = control_points(inst),
            _ => {}
        }
    }
    report_points(points);
    if !(1..=32).contains(&points) || output_points != TRIANGLE_POINTS {
        return None;
    }
    let next = next_register(&native.outputs, HULL_FIELDS.len())?;
    let relay = link_hull(native, vertex, points, next)?;
    let control_id = native
        .code
        .iter()
        .any(|inst| declares_input(inst, OUTPUT_CONTROL_POINT_ID));
    let mut explicit_control = native
        .code
        .iter()
        .any(|inst| inst.opcode == HS_CONTROL_POINT_PHASE);
    let mut phase = 0;
    let mut written = BTreeMap::new();
    let mut programs = HullPrograms {
        ordinary: Vec::new(),
        corners: Vec::new(),
    };
    programs.both(&[HULL_PROFILE, 0])?;
    for mut inst in std::mem::take(&mut native.code) {
        let op = inst.opcode;
        if !explicit_control && matches!(op, HS_FORK_PHASE | HS_JOIN_PHASE) {
            if points != output_points {
                return None;
            }
            programs.both(&implicit_control(native, points)?)?;
            explicit_control = true;
        }
        if matches!(op, HS_CONTROL_POINT_PHASE | HS_FORK_PHASE | HS_JOIN_PHASE) {
            phase = op;
            written.clear();
        }
        if phase == HS_CONTROL_POINT_PHASE {
            if op == DCL_TEMPS {
                inst.words[1] = relay.index_temp + 1;
            }
            if op == RET {
                programs.both(&relay.copy)?;
            }
        } else {
            if (DCL_OUTPUT..=DCL_OUTPUT_SIV).contains(&op) && inst.words.len() >= 3 {
                *written.entry(inst.words[2]).or_default() |= (inst.words[1] >> 4) & 15;
            }
            if op == RET {
                corner_factors(&mut programs.corners, constants, &written)?;
            }
        }
        append_words(&mut programs.ordinary, &inst.words)?;
        if op == DCL_TESS_PARTITIONING {
            inst.words[0] = (inst.words[0] & !0x00003800) | 0x00000800;
        }
        append_words(&mut programs.corners, &inst.words)?;
        if op == HS_CONTROL_POINT_PHASE {
            if !relay.declares_temps {
                programs.both(&[0x02000068, 1])?;
            }
            if !control_id {
                programs.both(&[0x0200005f, 0x00016000])?;
            }
            programs.both(&relay.declarations)?;
        }
    }
    Some(programs)
}

struct DomainLayout {
    position: u32,
    next: u32,
    has_domain: bool,
}

fn admit_domain(native: &Stage<'_>) -> Option<DomainLayout> {
    if native
        .code
        .iter()
        .any(|inst| inst.opcode == DCL_CONSTANT_BUFFER && inst.words.get(2) == Some(&13))
    {
        return None;
    }
    let mut position = None;
    for s in &native.outputs {
        if s.system == POSITION && s.mask == 15 && s.ty == FLOAT32 {
            position = Some(s.reg);
        }
        if s.name.starts_with(b"RSF") {
            return None;
        }
    }
    let mut points = 0;
    let mut returns = 0;
    let mut triangle = false;
    let mut has_domain = false;
    for inst in &native.code {
        match inst.opcode {
            DCL_INPUT_CONTROL_POINT_COUNT => points = control_points(inst),
            DCL_TESS_DOMAIN => triangle = (inst.words[0] >> 11) & 3 == 2,
            RET => returns += 1,
            CALL | CALL_C | RET_C => return None,
            DCL_RESOURCE | DCL_RESOURCE_RAW | DCL_RESOURCE_STRUCTURED
                if inst.words.get(2) == Some(&127) =>
            {
                return None;
            }
            _ => {}
        }
        has_domain |= declares_input(inst, INPUT_DOMAIN_POINT);
    }
    if !triangle || points != TRIANGLE_POINTS || returns != 1 || native.code.last()?.opcode != RET {
        return None;
    }
    Some(DomainLayout {
        position: position?,
        next: next_register(&native.outputs, 5)?,
        has_domain,
    })
}

struct DomainTemps {
    base: u32,
    scratch: u32,
    corners: u32,
    domain: u32,
    current: u32,
    helper: u32,
    total: u32,
}

impl DomainTemps {
    fn new(native: &[Instruction], helper_code: &[Instruction]) -> Option<Self> {
        let base = temp_count(native);
        let scratch = base * 2;
        let corners = scratch + REGISTER_COUNT as u32;
        let domain = corners + TRIANGLE_POINTS;
        let current = domain + 1;
        let helper = current + 1;
        let total = helper + temp_count(helper_code);
        (total <= MAX_TEMPS).then_some(Self {
            base,
            scratch,
            corners,
            domain,
            current,
            helper,
            total,
        })
    }
}

#[derive(Clone, Copy)]
enum DomainInput {
    Domain,
    Corner(u32),
    ControlPoint { point: u32, reg: u32 },
}

fn link_domain_inputs<'a>(
    inputs: &mut Vec<Semantic<'a>>,
    helper: &[Semantic<'_>],
    hull: &[Semantic<'a>],
    temps: &DomainTemps,
    declarations: &mut Vec<u32>,
) -> Option<RegisterMap<DomainInput>> {
    let mut mapping = [None; REGISTER_COUNT];
    inputs.try_reserve(helper.len()).ok()?;
    for s in helper {
        let (name, point) = match s.name {
            b"RSFDOMAIN" => {
                insert(&mut mapping, s.reg, DomainInput::Domain)?;
                continue;
            }
            b"RSFCORNER" => {
                if s.index >= TRIANGLE_POINTS {
                    return None;
                }
                insert(
                    &mut mapping,
                    s.reg,
                    DomainInput::Corner(temps.corners + s.index),
                )?;
                continue;
            }
            b"RSFA" => (b"RSFKEY".as_slice(), s.index),
            b"RSFB" => (b"RSFLOOKUP".as_slice(), s.index),
            name => (name, 0),
        };
        let source = hull.iter().find(|v| v.name == name)?;
        if point >= TRIANGLE_POINTS || source.reg as usize >= REGISTER_COUNT {
            return None;
        }
        insert(
            &mut mapping,
            s.reg,
            DomainInput::ControlPoint {
                point,
                reg: source.reg,
            },
        )?;
        if !inputs.iter().any(|s| s.reg == source.reg) {
            let mut semantic = source.clone();
            semantic.used = semantic.mask;
            inputs.push(semantic);
            append_words(
                declarations,
                &[0x0400005f, 0x002190f2, TRIANGLE_POINTS, source.reg],
            )?;
        }
    }
    Some(mapping)
}

#[repr(C)]
struct DomainRegisters {
    previous: *mut u32,
    current: *mut u32,
}

const _: () = assert!(size_of::<DomainRegisters>() == 16);

fn link_domain_outputs<'a>(
    outputs: &mut Vec<Semantic<'a>>,
    helper: Vec<Semantic<'a>>,
    next: u32,
    registers: &DomainRegisters,
) -> Option<RegisterMap<u32>> {
    let mut mapping = [None; REGISTER_COUNT];
    outputs.try_reserve(helper.len()).ok()?;
    for (reg, mut semantic) in (next..).zip(helper) {
        if reg as usize >= REGISTER_COUNT {
            return None;
        }
        insert(&mut mapping, semantic.reg, reg)?;
        semantic.reg = reg;
        // Keep the report order even when caller scalars alias, or an assembly step refuses.
        match semantic.name {
            b"RSFPREV" => unsafe { registers.previous.write(reg) },
            b"RSFCUR" => unsafe { registers.current.write(reg) },
            _ => {}
        }
        outputs.push(semantic);
    }
    Some(mapping)
}

fn domain_declarations(
    code: Vec<Instruction>,
    total: u32,
    declarations: &mut Vec<u32>,
) -> Option<Vec<Instruction>> {
    let mut body = Vec::new();
    let mut declared = false;
    for inst in code {
        match inst.opcode {
            DCL_TEMPS => {
                append_words(declarations, &[inst.words[0], total])?;
                declared = true;
            }
            RET => {}
            DCL_INPUT_CONTROL_POINT_COUNT..=DCL_TESS_PARTITIONING
            | DCL_RESOURCE_RAW
            | DCL_RESOURCE_STRUCTURED => {
                append_words(declarations, &inst.words)?;
            }
            _ if inst.declaration => append_words(declarations, &inst.words)?,
            _ => {
                body.try_reserve(1).ok()?;
                body.push(inst);
            }
        }
    }
    if !declared {
        append_words(declarations, &[0x02000068, total])?;
    }
    Some(body)
}

fn rebase_temp(operand: &OperandView, base: u32) -> Option<OperandEdit> {
    let reg = base.checked_add(operand.direct_register()?)?;
    (reg < MAX_TEMPS).then_some(OperandEdit::register(TEMP, reg))
}

fn domain_injection(
    declarations: &mut Vec<u32>,
    helper: Vec<Instruction>,
    inputs: &RegisterMap<DomainInput>,
    outputs: &RegisterMap<u32>,
    base: u32,
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
                        TEMP => return rebase_temp(operand, base),
                        OUTPUT => OperandEdit::register(
                            OUTPUT,
                            mapped(outputs, operand.direct_register()?)?,
                        ),
                        INPUT => match mapped(inputs, operand.direct_register()?)? {
                            DomainInput::Domain => {
                                OperandEdit::without_register(INPUT_DOMAIN_POINT)
                            }
                            DomainInput::Corner(reg) => OperandEdit::register(TEMP, reg),
                            DomainInput::ControlPoint { point, reg } => {
                                OperandEdit::register_pair(1, INPUT_CONTROL_POINT, [point, reg])
                            }
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

fn evaluate_domain(
    code: &mut Vec<u32>,
    body: &[Instruction],
    position: u32,
    temps: &DomainTemps,
) -> Option<()> {
    // Preserve native material varyings at the real sample. The three further evaluations
    // redirect outputs and use private temporaries for each patch corner.
    for evaluation in 0..=TRIANGLE_POINTS {
        let corner = evaluation != 0;
        if corner {
            let mut tokens = [0x08000036, 0x001000f2, temps.domain, 0x00004002, 0, 0, 0, 0];
            tokens[3 + evaluation as usize] = 0x3f800000;
            append_words(code, &tokens)?;
        }
        for inst in body {
            if inst.opcode == CUSTOM_DATA {
                append_words(code, &inst.words)?;
                continue;
            }
            let edited = rewrite_operands(&inst.words, true, |operand| {
                let edit = match operand.kind {
                    TEMP if corner => return rebase_temp(operand, temps.base),
                    OUTPUT => {
                        let reg = operand.direct_register()?;
                        if reg as usize >= REGISTER_COUNT {
                            return None;
                        }
                        if corner {
                            OperandEdit::register(TEMP, temps.scratch + reg)
                        } else if reg == position {
                            OperandEdit::register(TEMP, temps.current)
                        } else {
                            OperandEdit::KEEP
                        }
                    }
                    INPUT_DOMAIN_POINT if corner => {
                        OperandEdit::register_from(0, TEMP, temps.domain)
                    }
                    _ => OperandEdit::KEEP,
                };
                Some(edit)
            })?;
            append_words(code, &edited)?;
        }
        if corner {
            append_words(
                code,
                &[
                    0x05000036,
                    0x001000f2,
                    temps.corners + evaluation - 1,
                    0x00100e46,
                    temps.scratch + position,
                ],
            )?;
        }
    }
    append_words(
        code,
        &[0x05000036, 0x001020f2, position, 0x00100e46, temps.current],
    )
}

fn domain_program<'a>(
    native: &mut Stage<'a>,
    hull: &[Semantic<'a>],
    registers: &DomainRegisters,
    compile: impl FnOnce() -> Option<&'a [u8]>,
) -> Option<Vec<u32>> {
    let layout = admit_domain(native)?;
    let helper = stage(&chunks(compile()?)?, VERTEX_PROFILE, &[*b"OSGN"])?;
    let temps = DomainTemps::new(&native.code, &helper.code)?;
    let mut code = Vec::new();
    append_words(&mut code, &[DOMAIN_PROFILE, 0])?;
    if !layout.has_domain {
        append_words(&mut code, &[0x0200005f, 0x0001c072])?;
    }
    let inputs = link_domain_inputs(&mut native.inputs, &helper.inputs, hull, &temps, &mut code)?;
    let outputs = link_domain_outputs(&mut native.outputs, helper.outputs, layout.next, registers)?;
    let body = domain_declarations(std::mem::take(&mut native.code), temps.total, &mut code)?;
    let injection = domain_injection(&mut code, helper.code, &inputs, &outputs, temps.helper)?;
    evaluate_domain(&mut code, &body, layout.position, &temps)?;
    append_words(&mut code, &injection)?;
    append_words(&mut code, &[0x0100003e])?;
    Some(code)
}

/// The wrapper lends native bytecode and upstream names through both container writes.
/// The two resize leaves provide separate storage and the semantic callback copies each name.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_hull_shader(
    bytes: *const u8,
    size: usize,
    vertex: *const SemanticView,
    count: usize,
    user: *mut c_void,
    resize_ordinary: Resize,
    resize_corners: Resize,
    append: AppendSemantic,
    input_points: *mut u32,
) -> bool {
    let assemble = || {
        let native_chunks = chunks(unsafe { borrowed(bytes, size) }?)?;
        let patch = chunk_index(&native_chunks, b"PCSG")?;
        let mut native = stage(&native_chunks, HULL_PROFILE, &[*b"OSGN"])?;
        let constants = signature(&native_chunks[patch])?;
        let vertex = vertex_outputs(unsafe { borrowed(vertex, count) }?)?;
        let programs = hull_programs(&mut native, &constants, &vertex, |points| unsafe {
            input_points.write(points);
        })?;
        write_stage(
            &native_chunks,
            &native.chunks,
            &native.inputs,
            &native.outputs,
            &programs.ordinary,
            |size| unsafe { native_output(user, resize_ordinary, size) },
        )?;
        write_stage(
            &native_chunks,
            &native.chunks,
            &native.inputs,
            &native.outputs,
            &programs.corners,
            |size| unsafe { native_output(user, resize_corners, size) },
        )?;
        native
            .outputs
            .iter()
            .all(|s| unsafe { append(user, &s.view()) })
            .then_some(())
    };
    !input_points.is_null() && assemble().is_some()
}

/// The wrapper lends native bytecode, hull names and scalar outputs through the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_domain_shader(
    bytes: *const u8,
    size: usize,
    hull: *const SemanticView,
    count: usize,
    compile: CompileHelper,
    user: *mut c_void,
    resize: Resize,
    append: AppendSemantic,
    registers: *const DomainRegisters,
) -> bool {
    let assemble = || {
        let registers = unsafe { registers.as_ref() }?;
        if registers.previous.is_null() || registers.current.is_null() {
            return None;
        }
        let native_chunks = chunks(unsafe { borrowed(bytes, size) }?)?;
        let mut native = stage(&native_chunks, DOMAIN_PROFILE, &[*b"OSGN"])?;
        let hull = vertex_outputs(unsafe { borrowed(hull, count) }?)?;
        let code = domain_program(&mut native, &hull, registers, || unsafe {
            compile_bytecode(compile, DOMAIN_SOURCE)
        })?;
        write_stage(
            &native_chunks,
            &native.chunks,
            &native.inputs,
            &native.outputs,
            &code,
            |size| unsafe { native_output(user, resize, size) },
        )?;
        native
            .outputs
            .iter()
            .all(|s| unsafe { append(user, &s.view()) })
            .then_some(())
    };
    assemble().is_some()
}

const DOMAIN_SOURCE: &str = r#"
ByteAddressBuffer History:register(t127);
cbuffer Parameters:register(b13) {
 float4 Rect;float4 Jitter;float Cutoff;uint HasHistory;uint HistoryMask;uint Padding;
 row_major float4x4 ClipToPrevious;
};
struct O {float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 current:RSFCUR0;float4 previous:RSFPREV0;};
uint3 patch_key(float4 a,float4 b,float4 c) {
 uint3 hash=uint3(2166136261,2246822519,3266489917);
 [unroll]for(uint v=0;v<3;v++) {
  uint4 bits=asuint(v==0 ? a : v==1 ? b : c);
  [unroll]for(uint j=0;j<4;j++)hash=(hash^bits[j])*uint3(16777619,3266489917,668265263);
 }
 return hash;
}
float4 previous_corner(float4 current,uint3 patch,uint corner,float3 old,bool sprite,bool valid) {
 float4 p=current;p.xy-=Jitter.xy*p.w;float4 fallback=mul(p,ClipToPrevious);fallback.xy+=Jitter.zw*fallback.w;
 if(HasHistory==0 || !valid)return fallback;
 uint4 key=uint4(patch,asuint(float(corner)));uint hash=2166136261;
 [unroll]for(uint j=0;j<4;j++)hash=(hash^key[j])*16777619;
 hash^=hash>>16;hash=max(1,hash&0x7fffffff);
 bool found=false,ambiguous=false;float distance=3.402823e38;float4 selected=fallback;
 [loop]for(uint probe=0;probe<32;probe++) {
  uint address=((hash+probe)&HistoryMask)*64;uint marker=History.Load(address);
  if(marker==0)break;if(marker!=hash || any(History.Load4(address+4)!=key))continue;
  float4 candidate=asfloat(History.Load4(address+20));if(!all(isfinite(candidate)) || candidate.w<=0)continue;
  float3 centre=asfloat(History.Load3(address+36));float d=sprite ? dot(centre-old,centre-old) : 0;
  if(!found || d<distance) {found=true;distance=d;selected=candidate;ambiguous=false;}
  else if(d==distance && any(asuint(candidate)!=asuint(selected)))ambiguous=true;
 }
 return found && !ambiguous ? selected : fallback;
}
O main(float3 domain:RSFDOMAIN0,float4 p0:RSFCORNER0,float4 p1:RSFCORNER1,float4 p2:RSFCORNER2,
 float4 a:RSFA0,float4 b:RSFA1,float4 c:RSFA2,float4 la:RSFB0,float4 lb:RSFB1,float4 lc:RSFB2,
 float4 centre:RSFCENTRE0,float4 old:RSFOLD0) {
 O o;uint3 now=patch_key(a,b,c),prior=patch_key(la,lb,lc);
 uint corner=domain.x==1 ? 1 : domain.y==1 ? 2 : domain.z==1 ? 3 : 0;
 o.key=asfloat(uint4(now,asuint(float(corner))));o.centre=centre;o.old=old;
 o.current=p0*domain.x+p1*domain.y+p2*domain.z;
 bool valid=la.w!=3 && lb.w!=3 && lc.w!=3;
 o.previous=previous_corner(p0,prior,1,old.xyz,la.w==1,valid)*domain.x+
  previous_corner(p1,prior,2,old.xyz,la.w==1,valid)*domain.y+
  previous_corner(p2,prior,3,old.xyz,la.w==1,valid)*domain.z;
 return o;
}"#;
