// SPDX-License-Identifier: MIT
//! Shared DXBC stage linkage for AC7 velocity shaders.

use crate::dxbc_chunks::Chunk;
use crate::velocity_container::{
    Semantic, chunk_index, read_signature, shader_index, write_container, write_signature,
};
#[cfg(windows)]
use crate::velocity_container::valid_container_size;
use crate::velocity_tokens::{
    DCL_TEMPS, Instruction, MAX_TEMPS, OperandEdit, append_words, instructions, rewrite_operands,
    write_instructions,
};
#[cfg(windows)]
use std::ffi::CStr;
#[cfg(windows)]
use std::sync::OnceLock;

pub(crate) const REGISTER_COUNT: usize = 32;

#[cfg(windows)]
pub(crate) type HelperCache = OnceLock<Option<Vec<u8>>>;

pub(crate) type RegisterMap<T> = [Option<T>; REGISTER_COUNT];

pub(crate) struct StageChunks {
    shader: usize,
    input: usize,
    output: usize,
}

pub(crate) struct Stage<'a> {
    pub(crate) chunks: StageChunks,
    pub(crate) code: Vec<Instruction>,
    pub(crate) inputs: Vec<Semantic<'a>>,
    pub(crate) outputs: Vec<Semantic<'a>>,
}

pub(crate) fn signature<'a>(chunk: &Chunk<'a>) -> Option<Vec<Semantic<'a>>> {
    let mut result = Vec::new();
    read_signature(chunk.data, chunk.tag == b"OSG5", |semantic| {
        result.try_reserve(1).ok()?;
        result.push(semantic);
        Some(())
    })?;
    Some(result)
}

pub(crate) fn stage<'a>(
    chunks: &[Chunk<'a>],
    profile: u32,
    output_tags: &[[u8; 4]],
) -> Option<Stage<'a>> {
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

pub(crate) fn write_stage(
    chunks: &[Chunk<'_>],
    stage: &StageChunks,
    inputs: &[Semantic<'_>],
    outputs: &[Semantic<'_>],
    code: &[u32],
    destination: &mut Vec<u8>,
) -> Option<()> {
    let mut output = Vec::new();
    write_signature(outputs, chunks[stage.output].tag == b"OSG5", &mut output)?;

    write_stage_inputs(
        chunks,
        stage.shader,
        stage.input,
        inputs,
        code,
        Some((stage.output, &output)),
        destination,
    )
}

pub(crate) fn write_stage_inputs(
    chunks: &[Chunk<'_>],
    shader_index: usize,
    input_index: usize,
    inputs: &[Semantic<'_>],
    code: &[u32],
    output: Option<(usize, &[u8])>,
    destination: &mut Vec<u8>,
) -> Option<()> {
    let mut shader = Vec::new();
    let mut input = Vec::new();
    write_instructions(code, &mut shader)?;
    write_signature(inputs, false, &mut input)?;
    let mut linked = Vec::new();
    linked.try_reserve_exact(chunks.len()).ok()?;
    for (index, chunk) in chunks.iter().enumerate() {
        let data = if index == shader_index {
            &shader[..]
        } else if index == input_index {
            &input[..]
        } else {
            output
                .filter(|(slot, _)| *slot == index)
                .map_or(chunk.data, |(_, bytes)| bytes)
        };
        linked.push(Chunk {
            tag: chunk.tag,
            data,
        });
    }
    write_container(&linked, destination)
}

/// Rebase a compiled VS helper for injection into a different shader stage.
/// The caller declares its replacement inputs and places the returned body.
pub(crate) fn rewrite_helper(
    declarations: &mut Vec<u32>,
    helper: Vec<Instruction>,
    outputs: &RegisterMap<u32>,
    base: u32,
    remap_input: impl Fn(u32) -> Option<OperandEdit>,
) -> Option<Vec<u32>> {
    const TEMP: u32 = 0;
    const INPUT: u32 = 1;
    const OUTPUT: u32 = 2;
    const RET: u32 = 62;
    const DCL_INPUT: u32 = 95;
    const DCL_INPUT_SIV: u32 = 97;
    const DCL_OUTPUT: u32 = 101;
    const DCL_OUTPUT_SIV: u32 = 103;
    const DCL_GLOBAL_FLAGS: u32 = 106;

    let mut injection = Vec::new();
    for mut inst in helper {
        match inst.opcode {
            DCL_TEMPS | DCL_GLOBAL_FLAGS | RET | DCL_INPUT..=DCL_INPUT_SIV => continue,
            DCL_OUTPUT..=DCL_OUTPUT_SIV => {
                let reg = inst.words.get_mut(2)?;
                *reg = mapped(outputs, *reg)?;
                append_words(declarations, &inst.words)?;
            }
            _ if inst.declaration => append_words(declarations, &inst.words)?,
            _ => {
                let edited = rewrite_operands(&inst.words, false, |operand| {
                    Some(match operand.kind {
                        TEMP => {
                            let reg = base.checked_add(operand.direct_register()?)?;
                            if reg >= MAX_TEMPS {
                                return None;
                            }
                            OperandEdit::register(TEMP, reg)
                        }
                        INPUT => remap_input(operand.direct_register()?)?,
                        OUTPUT => OperandEdit::register(
                            OUTPUT,
                            mapped(outputs, operand.direct_register()?)?,
                        ),
                        _ => OperandEdit::KEEP,
                    })
                })?;
                append_words(&mut injection, &edited)?;
            }
        }
    }
    Some(injection)
}

pub(crate) fn next_register(semantics: &[Semantic<'_>], added: usize) -> Option<u32> {
    let next = semantics.iter().map(|semantic| semantic.reg).max();
    let next = next.map_or(Some(0), |reg| reg.checked_add(1))?;
    (next as usize <= REGISTER_COUNT.checked_sub(added)?).then_some(next)
}

pub(crate) fn insert<T>(map: &mut RegisterMap<T>, register: u32, value: T) -> Option<()> {
    *map.get_mut(register as usize)? = Some(value);
    Some(())
}

pub(crate) fn mapped<T: Copy>(map: &RegisterMap<T>, register: u32) -> Option<T> {
    map.get(register as usize).copied().flatten()
}

/// The shared System32 compiler lends its blob during the sink call. Copy once into
/// the Rust cache and keep the immutable bytecode alive through all later link calls.
#[cfg(windows)]
pub(crate) fn compile_bytecode<'a>(
    cache: &'a HelperCache,
    source: &str,
    name: &CStr,
    target: &CStr,
) -> Option<&'a [u8]> {
    cache
        .get_or_init(|| compile_source(source, name, target))
        .as_deref()
}

#[cfg(windows)]
pub(crate) fn compile_source(source: &str, name: &CStr, target: &CStr) -> Option<Vec<u8>> {
    let bytecode = rsf_shader_compiler::compile_system(rsf_shader_compiler::Request {
        source: source.as_bytes(),
        name,
        entry: c"main",
        target,
        flags: 1 << 11, // D3DCOMPILE_ENABLE_STRICTNESS
    })?;
    valid_container_size(bytecode.len()).then_some(bytecode)
}

#[cfg(windows)]
pub(crate) const HISTORY_WRITER: &str = concat!(
    include_str!("velocity_history_table.hlsl"),
    include_str!("velocity_history_writer.hlsl"),
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::velocity_container::chunks;

    #[test]
    fn pixel_rewrite_preserves_other_chunk_payloads() {
        let native = [
            Chunk {
                tag: b"SHEX",
                data: &[1; 8],
            },
            Chunk {
                tag: b"ISGN",
                data: &[2; 8],
            },
            Chunk {
                tag: b"OSGN",
                data: &[3; 8],
            },
            Chunk {
                tag: b"STAT",
                data: &[4; 8],
            },
        ];
        let mut output = Vec::new();
        write_stage_inputs(
            &native,
            0,
            1,
            &[],
            &[0x50, 0, 0x0100_003e],
            None,
            &mut output,
        )
        .unwrap();
        let linked = chunks(&output).unwrap();
        assert_ne!(linked[0].data, native[0].data);
        assert_ne!(linked[1].data, native[1].data);
        for index in 2..native.len() {
            assert_eq!(linked[index].tag, native[index].tag);
            assert_eq!(linked[index].data, native[index].data);
        }
    }
}
