// SPDX-License-Identifier: MIT
//! AC7 velocity SHEX/SHDR framing, operand decoding and immediate register edits.
//! Used by Rust pixel assembly and the native transforms of the other stages.

use crate::dxbc_chunks::word;
use crate::velocity_container::{Resize, borrowed, native_output};
use std::borrow::Cow;
use std::ffi::c_void;

const CUSTOM_DATA: u32 = 53;
const DCL_TEMPS: u32 = 104;
pub(crate) const MAX_TEMPS: u32 = 4096;
const MAX_INSTRUCTION_WORDS: usize = 127;
const OPCODE_MASK: u32 = 0x7ff;
const LENGTH_MASK: u32 = 0x7f00_0000;
const EXTENDED: u32 = 0x8000_0000;
const OPERAND_TYPE_AND_DIMENSIONS: u32 = 0x003f_f000;

#[repr(C)]
struct InstructionView {
    words: *const u32,
    count: usize,
    opcode: u32,
    declaration: bool,
}

#[repr(C)]
pub(crate) struct OperandView {
    pub(crate) token: usize,
    pub(crate) end: usize,
    pub(crate) kind: u32,
    dimensions: u32,
    immediate32: u32,
    registers: [u32; 3],
}

#[repr(C)]
pub(crate) struct OperandEdit {
    // All old indices must be immediate 32-bit values. UINT32_MAX leaves the operand intact.
    old_count: u32,
    kind: u32,
    count: u32,
    registers: [u32; 2],
}

impl OperandView {
    pub(crate) fn direct_register(&self) -> Option<u32> {
        (self.dimensions == 1 && self.immediate32 == 1).then_some(self.registers[0])
    }
}

impl OperandEdit {
    pub(crate) const KEEP: Self = Self {
        old_count: u32::MAX,
        kind: 0,
        count: 0,
        registers: [0; 2],
    };

    pub(crate) fn register(kind: u32, register: u32) -> Self {
        Self {
            old_count: 1,
            kind,
            count: 1,
            registers: [register, 0],
        }
    }
}

const _: () = {
    assert!(size_of::<InstructionView>() == 24);
    assert!(size_of::<OperandView>() == 40);
    assert!(size_of::<OperandEdit>() == 20);
};

type AppendInstruction = unsafe extern "C" fn(*mut c_void, *const InstructionView) -> bool;
type AppendOperand = unsafe extern "C" fn(*mut c_void, *const OperandView) -> bool;
type SelectEdit = unsafe extern "C" fn(*mut c_void, *const OperandView, *mut OperandEdit) -> bool;
type StoreWords = unsafe extern "C" fn(*mut c_void, *const u32, usize) -> bool;

fn instruction(words: &[u32]) -> Option<(u32, &[u32])> {
    let token = *words.first()?;
    let opcode = token & OPCODE_MASK;
    let count = if opcode == CUSTOM_DATA {
        let count = *words.get(1)? as usize;
        if count < 2 {
            return None;
        }
        count
    } else {
        ((token & LENGTH_MASK) >> 24) as usize
    };
    let words = words.get(..count)?;
    if words.is_empty() || (opcode == DCL_TEMPS && (words.len() != 2 || words[1] > MAX_TEMPS)) {
        return None;
    }
    Some((opcode, words))
}

/// The caller lends the chunk data and copies each instruction before returning.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_read_instructions(
    bytes: *const u8,
    size: usize,
    profile: u32,
    user: *mut c_void,
    append: AppendInstruction,
) -> bool {
    let Some(bytes) = (unsafe { borrowed(bytes, size) }) else {
        return false;
    };
    read_instructions(bytes, profile, |opcode, words, declaration| {
        let view = InstructionView {
            words: words.as_ptr(),
            count: words.len(),
            opcode,
            declaration,
        };
        unsafe { append(user, &view) }.then_some(())
    })
    .is_some()
}

pub(crate) fn read_instructions(
    bytes: &[u8],
    profile: u32,
    mut visit: impl FnMut(u32, &[u32], bool) -> Option<()>,
) -> Option<()> {
    let (data, trailing) = bytes.as_chunks::<4>();
    if !trailing.is_empty()
        || word(bytes, 0) != Some(profile)
        || word(bytes, 4).map(|count| count as usize) != Some(data.len())
    {
        return None;
    }
    let mut code = Vec::new();
    code.try_reserve_exact(data.len() - 2).ok()?;
    code.extend(data[2..].iter().copied().map(u32::from_le_bytes));
    let mut remaining = code.as_slice();
    while !remaining.is_empty() {
        let (opcode, words) = instruction(remaining)?;
        visit(opcode, words, (88..=106).contains(&opcode))?;
        remaining = &remaining[words.len()..];
    }
    Some(())
}

/// The caller lends a stream beginning with its shader profile and a count placeholder.
/// The resize callback returns a separate writable buffer.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_write_instructions(
    code: *const u32,
    count: usize,
    user: *mut c_void,
    resize: Resize,
) -> bool {
    let Some(code) = (unsafe { borrowed(code, count) }) else {
        return false;
    };
    write_instructions(code, |size| unsafe { native_output(user, resize, size) }).is_some()
}

pub(crate) fn write_instructions<'a>(
    code: &[u32],
    allocate: impl FnOnce(usize) -> Option<&'a mut [u8]>,
) -> Option<()> {
    let count = u32::try_from(code.len()).ok()?;
    let mut remaining = code.get(2..)?;
    while !remaining.is_empty() {
        let (_, words) = instruction(remaining)?;
        remaining = &remaining[words.len()..];
    }
    let size = code.len().checked_mul(4)?;
    let bytes = allocate(size)?.get_mut(..size)?;
    for (dst, &value) in bytes.chunks_exact_mut(4).zip(code) {
        dst.copy_from_slice(&value.to_le_bytes());
    }
    bytes[4..8].copy_from_slice(&count.to_le_bytes());
    Some(())
}

struct Operand {
    view: OperandView,
    index_start: usize,
    nested: bool,
}

fn take(code: &[u32], cursor: &mut usize) -> Option<u32> {
    let value = *code.get(*cursor)?;
    *cursor += 1;
    Some(value)
}

fn skip_extensions(code: &[u32], mut token: u32, cursor: &mut usize) -> Option<()> {
    while token & EXTENDED != 0 {
        token = take(code, cursor)?;
    }
    Some(())
}

fn operand(
    code: &[u32],
    cursor: &mut usize,
    operands: &mut Vec<Operand>,
    nested: bool,
) -> Option<()> {
    let start = *cursor;
    let token = take(code, cursor)?;
    skip_extensions(code, token, cursor)?;
    let kind = (token >> 12) & 255;
    let dimensions = (token >> 20) & 3;
    let slot = operands.len();
    operands.push(Operand {
        view: OperandView {
            token: start,
            end: 0,
            kind,
            dimensions,
            immediate32: 0,
            registers: [0; 3],
        },
        index_start: *cursor,
        nested,
    });
    if kind == 4 || kind == 5 {
        let components = match token & 3 {
            1 => 1,
            2 => 4,
            _ => return None,
        };
        *cursor += components * if kind == 5 { 2 } else { 1 };
        code.get(..*cursor)?;
    } else {
        for i in 0..dimensions {
            let representation = (token >> (22 + i * 3)) & 7;
            match representation {
                0 => {
                    let register = take(code, cursor)?;
                    operands[slot].view.immediate32 |= 1 << i;
                    operands[slot].view.registers[i as usize] = register;
                }
                1 | 4 => {
                    take(code, cursor)?;
                    take(code, cursor)?;
                }
                2 => {}
                3 => {
                    take(code, cursor)?;
                }
                _ => return None,
            }
            if representation >= 2 {
                operand(code, cursor, operands, true)?;
            }
        }
    }
    operands[slot].view.end = *cursor;
    Some(())
}

fn decode_operands(words: &[u32]) -> Option<Vec<Operand>> {
    // The SM5 instruction length bounds both the number of operands and relative-index depth.
    let (opcode, instruction) = instruction(words)?;
    if opcode == CUSTOM_DATA || instruction.len() != words.len() {
        return None;
    }
    let mut operands = Vec::new();
    operands
        .try_reserve_exact(words.len().saturating_sub(1))
        .ok()?;
    let mut cursor = 1;
    skip_extensions(words, words[0], &mut cursor)?;
    while cursor < words.len() {
        operand(words, &mut cursor, &mut operands, false)?;
    }
    Some(operands)
}

/// The caller lends a single instruction and copies the requested operand views.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_read_operands(
    words: *const u32,
    count: usize,
    user: *mut c_void,
    append: AppendOperand,
) -> bool {
    let Some(mut operands) = (unsafe { borrowed(words, count) }).and_then(read_operands) else {
        return false;
    };
    operands.all(|operand| unsafe { append(user, &operand) })
}

pub(crate) fn read_operands(words: &[u32]) -> Option<impl Iterator<Item = OperandView> + use<>> {
    Some(
        decode_operands(words)?
            .into_iter()
            .filter(|operand| !operand.nested)
            .map(|operand| operand.view),
    )
}

/// The caller lends one instruction. SelectEdit supplies register mappings and StoreWords
/// copies updated words after every edit has been checked. No store is needed for an unchanged
/// instruction.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_ac7_velocity_rewrite_operands(
    words: *const u32,
    count: usize,
    nested: bool,
    user: *mut c_void,
    select: SelectEdit,
    store: StoreWords,
) -> bool {
    let Some(words) = (unsafe { borrowed(words, count) }) else {
        return false;
    };
    let edited = rewrite_operands(words, nested, |view| {
        let mut edit = OperandEdit::KEEP;
        unsafe { select(user, view, &mut edit) }.then_some(edit)
    });
    match edited {
        Some(Cow::Owned(edited)) => unsafe { store(user, edited.as_ptr(), edited.len()) },
        Some(Cow::Borrowed(_)) => true,
        None => false,
    }
}

/// An unchanged instruction borrows its input words.
pub(crate) fn rewrite_operands(
    words: &[u32],
    nested: bool,
    mut select: impl FnMut(&OperandView) -> Option<OperandEdit>,
) -> Option<Cow<'_, [u32]>> {
    let operands = decode_operands(words)?;
    let mut edited = Vec::new();
    for operand in operands
        .iter()
        .rev()
        .filter(|operand| nested || !operand.nested)
    {
        let view = &operand.view;
        let edit = select(view)?;
        if edit.old_count == u32::MAX {
            continue;
        }
        if edit.old_count > 2
            || edit.count > 2
            || edit.kind > 255
            || matches!(view.kind, 4 | 5)
            || matches!(edit.kind, 4 | 5)
            || view.dimensions != edit.old_count
            || view.immediate32 != (1 << edit.old_count) - 1
        {
            return None;
        }
        if edited.is_empty() {
            // At most two indices can be added per operand.
            edited
                .try_reserve_exact(words.len() + 2 * operands.len())
                .ok()?;
            edited.extend_from_slice(words);
        }
        let new_representations = ((1 << (edit.count * 3)) - 1) << 22;
        edited[view.token] = (edited[view.token]
            & !(OPERAND_TYPE_AND_DIMENSIONS | new_representations))
            | (edit.kind << 12)
            | (edit.count << 20);
        let registers = &edit.registers[..edit.count as usize];
        let indices = operand.index_start..view.end;
        if indices.len() == registers.len() {
            edited[indices].copy_from_slice(registers);
        } else {
            drop(edited.splice(indices, registers.iter().copied()));
        }
    }
    if edited.is_empty() {
        return Some(Cow::Borrowed(words));
    }
    if edited.len() > MAX_INSTRUCTION_WORDS {
        return None;
    }
    edited[0] = (edited[0] & !LENGTH_MASK) | ((edited.len() as u32) << 24);
    Some(Cow::Owned(edited))
}
