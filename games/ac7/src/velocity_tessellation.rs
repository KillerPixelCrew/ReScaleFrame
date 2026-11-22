// SPDX-License-Identifier: MIT
//! AC7 velocity hull control-point relay and native domain evaluation.

use crate::velocity_container::{Semantic, chunk_index, chunks};
use crate::velocity_stage::{
    HelperCache, REGISTER_COUNT, RegisterMap, Stage, compile_bytecode, insert, mapped,
    next_register, rewrite_helper, signature, stage, write_stage,
};
use crate::velocity_tokens::{
    DCL_TEMPS, Instruction, MAX_TEMPS, OperandEdit, append_words, rewrite_operands, temp_count,
};
use std::collections::BTreeMap;

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
const DCL_OUTPUT: u32 = 101;
const DCL_OUTPUT_SIV: u32 = 103;
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

#[derive(Default)]
pub(crate) struct DomainRegisters {
    pub(crate) previous: u32,
    pub(crate) current: u32,
}
fn link_domain_outputs<'a>(
    outputs: &mut Vec<Semantic<'a>>,
    helper: Vec<Semantic<'a>>,
    next: u32,
    registers: &mut DomainRegisters,
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
            b"RSFPREV" => registers.previous = reg,
            b"RSFCUR" => registers.current = reg,
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

fn domain_injection(
    declarations: &mut Vec<u32>,
    helper: Vec<Instruction>,
    inputs: &RegisterMap<DomainInput>,
    outputs: &RegisterMap<u32>,
    base: u32,
) -> Option<Vec<u32>> {
    rewrite_helper(declarations, helper, outputs, base, |register| {
        Some(match mapped(inputs, register)? {
            DomainInput::Domain => OperandEdit::without_register(INPUT_DOMAIN_POINT),
            DomainInput::Corner(reg) => OperandEdit::register(TEMP, reg),
            DomainInput::ControlPoint { point, reg } => {
                OperandEdit::register_pair(1, INPUT_CONTROL_POINT, [point, reg])
            }
        })
    })
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
    registers: &mut DomainRegisters,
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

pub(crate) fn transform<'a>(
    hull_bytes: &'a [u8],
    domain_bytes: &'a [u8],
    vertex: &[Semantic<'a>],
    input_points: &mut u32,
    registers: &mut DomainRegisters,
) -> Option<([Vec<u8>; 3], Vec<Semantic<'a>>)> {
    let hull_chunks = chunks(hull_bytes)?;
    let patch = chunk_index(&hull_chunks, b"PCSG")?;
    let mut hull = stage(&hull_chunks, HULL_PROFILE, &[*b"OSGN"])?;
    let constants = signature(&hull_chunks[patch])?;
    let programs = hull_programs(&mut hull, &constants, vertex, |points| {
        *input_points = points
    })?;

    let domain_chunks = chunks(domain_bytes)?;
    let mut domain = stage(&domain_chunks, DOMAIN_PROFILE, &[*b"OSGN"])?;
    let code = domain_program(&mut domain, &hull.outputs, registers, || {
        static HELPER: HelperCache = HelperCache::new();
        compile_bytecode(
            &HELPER,
            DOMAIN_SOURCE,
            c"ac7-tessellated-control-motion",
            c"vs_5_0",
        )
    })?;
    let mut products = [Vec::new(), Vec::new(), Vec::new()];
    for (product, (chunks, stage, code)) in products.iter_mut().zip([
        (&hull_chunks, &hull, &programs.ordinary),
        (&hull_chunks, &hull, &programs.corners),
        (&domain_chunks, &domain, &code),
    ]) {
        write_stage(
            chunks,
            &stage.chunks,
            &stage.inputs,
            &stage.outputs,
            code,
            product,
        )?;
    }
    Some((products, domain.outputs))
}

const DOMAIN_SOURCE: &str = concat!(
    "#define RSF_HISTORY_SLOT t127\n",
    include_str!("velocity_history_table.hlsl"),
    include_str!("velocity_history_lookup.hlsl"),
    include_str!("velocity_domain.hlsl")
);
