// SPDX-License-Identifier: MIT
//! AC7 translucent velocity vertex shader admission and assembly.

use crate::dxbc_chunks::Chunk;
use crate::velocity_container::{Semantic, chunks};
use crate::velocity_stage::{
    HelperCache, REGISTER_COUNT, Stage, compile_bytecode, stage, write_stage,
};
use crate::velocity_tokens::{
    DCL_TEMPS, Instruction, MAX_TEMPS, OperandEdit, append_words, read_operands, rewrite_operands,
    temp_count,
};

const VERTEX_PROFILE: u32 = 0x10050;
const POSITION: u32 = 1;
const FLOAT32: u32 = 3;

const CALL: u32 = 4;
const CALLC: u32 = 5;
const RET: u32 = 62;
const RETC: u32 = 63;
const DCL_INPUT: u32 = 95;
const DCL_INPUT_SIV: u32 = 97;
const DCL_OUTPUT: u32 = 101;
const DCL_OUTPUT_SIV: u32 = 103;
const DCL_GLOBAL_FLAGS: u32 = 106;

const TEMP: u32 = 0;
const INPUT: u32 = 1;
const OUTPUT: u32 = 2;

const GENERIC_SOURCE: &str = r#"
struct O {float4 p:SV_Position;float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 lookup:RSFLOOKUP0;};
O main(uint vertex:SV_VertexID,uint instance:SV_InstanceID) {O o;o.p=float4(0,0,0,1);
o.key=float4(float(vertex),float(instance),0,0);o.lookup=o.key;o.centre=0;o.old=0;return o;}"#;

const SPRITE_SOURCE: &str = r#"
struct O {float4 p:SV_Position;float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 lookup:RSFLOOKUP0;};
O main(float4 centre:ATTRIBUTE0,float4 old:ATTRIBUTE1,float2 corner:ATTRIBUTE4) {
O o;o.p=float4(0,0,0,1);o.key=float4(corner,old.w,1);o.centre=float4(centre.xyz,1);
o.old=float4(old.xyz,1);o.lookup=o.key;return o;}"#;

const MESH_SOURCE: &str = r#"
Buffer<float4> Previous:register(t127);
struct O {float4 p:SV_Position;float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 lookup:RSFLOOKUP0;};
uint2 pose_hash(float4 a,float4 b,float4 c) {
    uint2 value=uint2(2166136261,2246822519);
    [unroll]for(uint row=0;row<3;row++) {
        uint4 bits=asuint(row==0 ? a : row==1 ? b : c);
        [unroll]for(uint i=0;i<4;i++) {value=(value^bits[i])*uint2(16777619,3266489917);}
    }
    return value;
}
O main(float4 a:ATTRIBUTE8,float4 b:ATTRIBUTE9,float4 c:ATTRIBUTE10,
       uint vertex:SV_VertexID,uint instance:SV_InstanceID) {
    O o;o.p=float4(0,0,0,1);uint2 now=pose_hash(a,b,c);
    o.key=float4(float(vertex),asfloat(now.x),asfloat(now.y),2);o.centre=float4(a.w,b.w,c.w,1);
    uint count;Previous.GetDimensions(count);o.lookup=float4(float(vertex),0,0,3);o.old=o.centre;
    if(instance<count/3) {
        float4 pa=Previous[instance*3],pb=Previous[instance*3+1],pc=Previous[instance*3+2];
        if(any(pa.xyz!=0) && any(pb.xyz!=0) && any(pc.xyz!=0)) {
            uint2 old=pose_hash(pa,pb,pc);o.lookup=float4(float(vertex),asfloat(old.x),asfloat(old.y),2);
            o.old=float4(pa.w,pb.w,pc.w,1);
        }
    }
    return o;
}"#;

const BEAM_SOURCE: &str = r#"
struct O {float4 p:SV_Position;float4 key:RSFKEY0;float4 centre:RSFCENTRE0;float4 old:RSFOLD0;float4 lookup:RSFLOOKUP0;};
O main(float4 p:ATTRIBUTE0,float4 old:ATTRIBUTE1,float4 size:ATTRIBUTE2,float4 uv:ATTRIBUTE3) {
 O o;o.p=float4(0,0,0,1);o.key=0;o.lookup=float4(0,0,0,3);o.centre=0;o.old=0;
 uint3 bits=asuint(float3(p.w,old.w,size.w));
 uint3 words=(bits&0x1fffffff)|((bits>>2)&0x20000000);uint sample=words.z&4095,intervals=(words.z>>12)&4095;
 if(all((bits&0x60000000)==0x20000000) && words.x>>28<=2 && words.z>>24==0 && intervals!=0 && sample<=intervals && (uv.y==0 || uv.y==1)) {
  float u=float(sample)/float(intervals);o.key=float4(p.w,old.w,floor(u*128),uv.y+4);
  o.lookup=o.key;o.centre=float4(u,0,0,1);o.old=o.centre;
 }
 return o;
}"#;

#[repr(u32)]
enum Particle {
    Generic = 0,
    Sprite = 1,
    Mesh = 2,
    Beam = 3,
}

fn vertex_stage<'a>(chunks: &[Chunk<'a>]) -> Option<Stage<'a>> {
    let stage = stage(chunks, VERTEX_PROFILE, &[*b"OSGN"])?;
    stage
        .inputs
        .iter()
        .chain(&stage.outputs)
        .all(|semantic| (semantic.reg as usize) < REGISTER_COUNT)
        .then_some(stage)
}

fn particle_mode(inputs: &[Semantic<'_>], beam: bool) -> Particle {
    let mut position = None;
    let mut old = false;
    let mut corner = false;
    let mut beam_old = false;
    let mut beam_size = false;
    let mut beam_uv = false;
    let mut mesh_rows = [false; 3];
    for s in inputs {
        if !s.name.eq_ignore_ascii_case(b"ATTRIBUTE") {
            continue;
        }
        match (s.index, s.mask) {
            (0, 15) => position = Some(s),
            (1, 15) => old = true,
            (1, 7) => beam_old = true,
            (2, 15) if s.used & 8 == 0 => beam_size = true,
            (3, 15) => beam_uv = true,
            (4, 3) => corner = true,
            (8..=10, 15) => mesh_rows[(s.index - 8) as usize] = true,
            _ => {}
        }
    }
    if mesh_rows == [true; 3] {
        Particle::Mesh
    } else if position.is_some() && old && corner {
        Particle::Sprite
    } else if beam && position.is_some_and(|s| s.used & 8 == 0) && beam_old && beam_size && beam_uv
    {
        Particle::Beam
    } else {
        Particle::Generic
    }
}

fn position_register(outputs: &[Semantic<'_>], tessellation: bool) -> Option<u32> {
    if outputs.iter().any(|s| s.name.starts_with(b"RSF")) {
        return None;
    }
    outputs
        .iter()
        .rfind(|s| {
            s.system == POSITION
                || (tessellation
                    && s.name.eq_ignore_ascii_case(b"VS_To_DS_Position")
                    && s.mask == 15
                    && s.ty == FLOAT32)
        })
        .map(|s| s.reg)
}

fn next_register(semantics: &[Semantic<'_>]) -> u32 {
    semantics.iter().map(|s| s.reg + 1).max().unwrap_or(0)
}

type RegisterMap = [Option<u32>; REGISTER_COUNT];

struct Link {
    inputs: RegisterMap,
    outputs: RegisterMap,
    dummy_position: u32,
}

fn link_signatures<'a>(
    native: &mut Stage<'a>,
    helper_inputs: Vec<Semantic<'a>>,
    helper_outputs: Vec<Semantic<'a>>,
) -> Option<Link> {
    let mut inputs = [None; REGISTER_COUNT];
    let mut outputs = [None; REGISTER_COUNT];
    let mut next_input = next_register(&native.inputs);
    let mut next_output = next_register(&native.outputs);
    let mut dummy_position = None;
    native.inputs.try_reserve(helper_inputs.len()).ok()?;
    native.outputs.try_reserve(helper_outputs.len()).ok()?;
    for mut s in helper_inputs {
        let source = s.reg as usize;
        if let Some(matched) = native
            .inputs
            .iter_mut()
            .find(|input| input.index == s.index && input.name.eq_ignore_ascii_case(s.name))
        {
            inputs[source] = Some(matched.reg);
            matched.used |= s.used;
            matched.mask |= s.mask;
        } else {
            if s.system == 0 || next_input as usize >= REGISTER_COUNT {
                return None;
            }
            s.reg = next_input;
            inputs[source] = Some(next_input);
            next_input += 1;
            native.inputs.push(s);
        }
    }
    for mut s in helper_outputs {
        if s.system == POSITION {
            dummy_position = Some(s.reg);
            continue;
        }
        if next_output as usize >= REGISTER_COUNT {
            return None;
        }
        outputs[s.reg as usize] = Some(next_output);
        s.reg = next_output;
        next_output += 1;
        native.outputs.push(s);
    }
    Some(Link {
        inputs,
        outputs,
        dummy_position: dummy_position?,
    })
}

fn mapped(registers: &RegisterMap, register: u32) -> Option<u32> {
    registers.get(register as usize).copied().flatten()
}

// VS declarations in these opcode ranges have one direct register and, for system
// values, a trailing name token. Validate the shape before editing the register or mask.
fn declaration_register(inst: &Instruction, kind: u32) -> Option<usize> {
    let count = if matches!(inst.opcode, DCL_INPUT | DCL_OUTPUT) {
        3
    } else {
        4
    };
    if inst.words.len() != count || inst.words[0] & 0x8000_0000 != 0 {
        return None;
    }
    let operand = inst.words[1];
    let register = inst.words[2] as usize;
    (operand & 0x81f0_0000 == 0x0010_0000
        && (operand >> 12) & 255 == kind
        && register < REGISTER_COUNT)
        .then_some(register)
}

fn assemble(native: &[Instruction], added: Vec<Instruction>, link: &Link) -> Option<Vec<u32>> {
    let base = temp_count(native);
    let helper_temps = temp_count(&added);
    let total = base + helper_temps;
    if total > MAX_TEMPS
        || !native.iter().any(|inst| inst.opcode == RET)
        || native
            .iter()
            .any(|inst| matches!(inst.opcode, CALL | CALLC | RETC))
    {
        return None;
    }

    let mut code = Vec::new();
    append_words(&mut code, &[VERTEX_PROFILE, 0])?;
    let mut input_declarations = [None; REGISTER_COUNT];
    let mut temps_declared = false;
    for inst in native.iter().filter(|inst| inst.declaration) {
        match inst.opcode {
            DCL_TEMPS => {
                append_words(&mut code, &[inst.words[0], total])?;
                temps_declared = true;
                continue;
            }
            DCL_INPUT..=DCL_INPUT_SIV => {
                let register = declaration_register(inst, INPUT)?;
                input_declarations[register] = Some(code.len() + 1);
            }
            _ => {}
        }
        append_words(&mut code, &inst.words)?;
    }

    let mut injection = Vec::new();
    for mut inst in added {
        match inst.opcode {
            DCL_TEMPS | DCL_GLOBAL_FLAGS | RET => continue,
            DCL_INPUT..=DCL_INPUT_SIV => {
                let source = declaration_register(&inst, INPUT)?;
                let register = link.inputs[source]?;
                if let Some(mask) = input_declarations[register as usize] {
                    code[mask] |= inst.words[1] & 0xf0;
                    continue;
                }
                inst.words[2] = register;
            }
            DCL_OUTPUT..=DCL_OUTPUT_SIV => {
                let source = declaration_register(&inst, OUTPUT)?;
                if source as u32 == link.dummy_position {
                    continue;
                }
                inst.words[2] = link.outputs[source]?;
            }
            _ => {}
        }
        if inst.declaration {
            append_words(&mut code, &inst.words)?;
            continue;
        }
        if let Some(dest) = read_operands(&inst.words)?.next()
            && dest.kind == OUTPUT
            && dest.direct_register() == Some(link.dummy_position)
        {
            continue;
        }
        let rewritten = rewrite_operands(&inst.words, false, |operand| {
            let register = match operand.kind {
                TEMP => {
                    let register = operand.direct_register()?;
                    if register >= helper_temps {
                        return None;
                    }
                    base + register
                }
                INPUT => mapped(&link.inputs, operand.direct_register()?)?,
                OUTPUT => mapped(&link.outputs, operand.direct_register()?)?,
                _ => return Some(OperandEdit::KEEP),
            };
            Some(OperandEdit::register(operand.kind, register))
        })?;
        append_words(&mut injection, &rewritten)?;
    }

    if !temps_declared {
        append_words(&mut code, &[(2 << 24) | DCL_TEMPS, total])?;
    }
    // The helper contributes keys and history inputs at each native return.
    for inst in native.iter().filter(|inst| !inst.declaration) {
        if inst.opcode == RET {
            append_words(&mut code, &injection)?;
        }
        append_words(&mut code, &inst.words)?;
    }
    Some(code)
}

fn patch<'a>(
    bytes: &'a [u8],
    tessellation: bool,
    beam: bool,
    destination: &mut Vec<u8>,
) -> Option<(u32, Vec<Semantic<'a>>)> {
    let native_chunks = chunks(bytes)?;
    let mut native = vertex_stage(&native_chunks)?;
    let particle = particle_mode(&native.inputs, beam) as u32;
    position_register(&native.outputs, tessellation)?;
    if next_register(&native.outputs) as usize + 4 > REGISTER_COUNT {
        return None;
    }

    let helper = vertex_stage(&chunks(vertex_helper(particle)?)?)?;
    let link = link_signatures(&mut native, helper.inputs, helper.outputs)?;
    let code = assemble(&native.code, helper.code, &link)?;
    write_stage(
        &native_chunks,
        &native.chunks,
        &native.inputs,
        &native.outputs,
        &code,
        destination,
    )?;
    Some((particle, native.outputs))
}

fn vertex_helper(particle: u32) -> Option<&'static [u8]> {
    static CACHE: [HelperCache; 4] = [const { HelperCache::new() }; 4];
    let sources = [
        (GENERIC_SOURCE, c"ac7-geometry-key"),
        (SPRITE_SOURCE, c"ac7-particle-key"),
        (MESH_SOURCE, c"ac7-mesh-particle-pose-key"),
        (BEAM_SOURCE, c"ac7-beam-sample-key"),
    ];
    // Keep the original initialization order, including for the first particle draw.
    let mut selected = None;
    for (index, (source, name)) in sources.into_iter().enumerate() {
        let code = compile_bytecode(&CACHE[index], source, name, c"vs_5_0");
        if index as u32 == particle {
            selected = code;
        }
    }
    selected
}

pub(crate) fn transform(
    bytes: &[u8],
    tessellation: bool,
    beam: bool,
) -> Option<(Vec<u8>, u32, Vec<Semantic<'_>>)> {
    let mut code = Vec::new();
    let (particle, semantics) = patch(bytes, tessellation, beam, &mut code)?;
    Some((code, particle, semantics))
}
