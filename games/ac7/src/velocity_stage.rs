// SPDX-License-Identifier: MIT
//! Shared DXBC stage linkage for AC7 velocity shaders.

use crate::dxbc_chunks::Chunk;
use crate::velocity_container::{
    Semantic, SemanticView, borrowed, chunk_index, read_signature, resize_bytes, shader_index,
    valid_container_size, write_container, write_signature,
};
use crate::velocity_tokens::{
    DCL_TEMPS, Instruction, MAX_TEMPS, OperandEdit, append_words, instructions, rewrite_operands,
    write_instructions,
};
use std::ffi::{CStr, c_char, c_void};
use std::sync::OnceLock;

pub(crate) const REGISTER_COUNT: usize = 32;
type CompileSink = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> i32;
pub(crate) type CompileHelper = unsafe extern "C" fn(
    *const u8,
    usize,
    *const c_char,
    *const c_char,
    *const c_char,
    u32,
    *mut c_void,
    CompileSink,
    CompileSink,
) -> i32;
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

pub(crate) fn write_stage<'a>(
    chunks: &[Chunk<'_>],
    stage: &StageChunks,
    inputs: &[Semantic<'_>],
    outputs: &[Semantic<'_>],
    code: &[u32],
    allocate: impl FnOnce(usize) -> Option<&'a mut [u8]>,
) -> Option<()> {
    let mut output = Vec::new();
    let output_allocation = &mut output;
    write_signature(outputs, chunks[stage.output].tag == b"OSG5", move |size| {
        resize_bytes(output_allocation, size)
    })?;

    write_stage_inputs(
        chunks,
        stage.shader,
        stage.input,
        inputs,
        code,
        Some((stage.output, &output)),
        allocate,
    )
}

pub(crate) fn write_stage_inputs<'a>(
    chunks: &[Chunk<'_>],
    shader_index: usize,
    input_index: usize,
    inputs: &[Semantic<'_>],
    code: &[u32],
    output: Option<(usize, &[u8])>,
    allocate: impl FnOnce(usize) -> Option<&'a mut [u8]>,
) -> Option<()> {
    let mut shader = Vec::new();
    let mut input = Vec::new();
    let shader_allocation = &mut shader;
    write_instructions(code, move |size| resize_bytes(shader_allocation, size))?;
    let input_allocation = &mut input;
    write_signature(inputs, false, move |size| {
        resize_bytes(input_allocation, size)
    })?;
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
    write_container(&linked, allocate)
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

pub(crate) fn vertex_outputs(views: &[SemanticView]) -> Option<Vec<Semantic<'_>>> {
    let mut outputs = Vec::new();
    outputs.try_reserve_exact(views.len()).ok()?;
    for view in views {
        outputs.push(unsafe { view.borrow() }?);
    }
    Some(outputs)
}

/// The shared System32 compiler lends its blob during the sink call. Copy once into
/// the Rust cache and keep the immutable bytecode alive through all later link calls.
pub(crate) fn compile_bytecode<'a>(
    cache: &'a HelperCache,
    compile: CompileHelper,
    source: &str,
    name: &CStr,
    target: &CStr,
) -> Option<&'a [u8]> {
    cache
        .get_or_init(|| {
            let mut bytecode = Vec::new();
            let status = unsafe {
                compile(
                    source.as_ptr(),
                    source.len(),
                    name.as_ptr(),
                    c"main".as_ptr(),
                    target.as_ptr(),
                    1 << 11, // D3DCOMPILE_ENABLE_STRICTNESS
                    (&mut bytecode as *mut Vec<u8>).cast(),
                    copy_code,
                    ignore_errors,
                )
            };
            (status == 0 && valid_container_size(bytecode.len())).then_some(bytecode)
        })
        .as_deref()
}

unsafe extern "C" fn copy_code(user: *mut c_void, bytes: *const u8, size: usize) -> i32 {
    if !valid_container_size(size) {
        return -1;
    }
    let output = unsafe { &mut *user.cast::<Vec<u8>>() };
    let Some(bytes) = (unsafe { borrowed(bytes, size) }) else {
        return -1;
    };
    let Some(output) = resize_bytes(output, size) else {
        return -1;
    };
    output.copy_from_slice(bytes);
    0
}

unsafe extern "C" fn ignore_errors(_: *mut c_void, _: *const u8, _: usize) -> i32 {
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_ac7_velocity_history_source() -> *const c_char {
    concat!(
        include_str!("velocity_history_table.hlsl"),
        include_str!("velocity_history_writer.hlsl"),
        "\0",
    )
    .as_ptr()
    .cast()
}

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
        write_stage_inputs(&native, 0, 1, &[], &[0x50, 0, 0x0100_003e], None, |size| {
            resize_bytes(&mut output, size)
        })
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
