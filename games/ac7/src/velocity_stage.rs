// SPDX-License-Identifier: MIT
//! Shared DXBC stage linkage for AC7 velocity shaders.

use crate::dxbc_chunks::Chunk;
use crate::velocity_container::{
    Semantic, SemanticView, borrowed, chunk_index, read_signature, resize_bytes, shader_index,
    valid_container_size, write_container, write_signature,
};
use crate::velocity_tokens::{Instruction, instructions, write_instructions};
use std::ffi::c_char;
use std::ptr;

pub(crate) const REGISTER_COUNT: usize = 32;
pub(crate) type CompileHelper =
    unsafe extern "C" fn(*const c_char, usize, *mut *const u8, *mut usize) -> bool;
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

/// The leaf borrows the exact HLSL and lends immutable bytecode owned by its module cache.
pub(crate) unsafe fn compile_bytecode<'a>(
    compile: CompileHelper,
    source: &str,
) -> Option<&'a [u8]> {
    let mut bytes = ptr::null();
    let mut size = 0;
    if !unsafe { compile(source.as_ptr().cast(), source.len(), &mut bytes, &mut size) }
        || !valid_container_size(size)
    {
        return None;
    }
    unsafe { borrowed(bytes, size) }
}
