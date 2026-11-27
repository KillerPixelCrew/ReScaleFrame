// SPDX-License-Identifier: MIT
//! AC7 velocity SHEX/SHDR framing, operand decoding and immediate register edits.

use crate::dxbc_chunks::word;
use std::borrow::Cow;

const CUSTOM_DATA: u32 = 53;
pub(crate) const DCL_TEMPS: u32 = 104;
pub(crate) const MAX_TEMPS: u32 = 4096;
const MAX_INSTRUCTION_WORDS: usize = 127;
const OPCODE_MASK: u32 = 0x7ff;
const LENGTH_MASK: u32 = 0x7f00_0000;
const EXTENDED: u32 = 0x8000_0000;
const OPERAND_TYPE_AND_DIMENSIONS: u32 = 0x003f_f000;

pub(crate) struct OperandView {
    pub(crate) token: usize,
    pub(crate) end: usize,
    pub(crate) kind: u32,
    dimensions: u32,
    immediate32: u32,
    registers: [u32; 3],
}

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

    pub(crate) fn direct_register_pair(&self) -> Option<[u32; 2]> {
        (self.dimensions == 2 && self.immediate32 == 3)
            .then_some([self.registers[0], self.registers[1]])
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
        Self::register_from(1, kind, register)
    }

    pub(crate) fn register_from(old_count: u32, kind: u32, register: u32) -> Self {
        Self {
            old_count,
            kind,
            count: 1,
            registers: [register, 0],
        }
    }

    pub(crate) fn without_register(kind: u32) -> Self {
        Self {
            old_count: 1,
            kind,
            count: 0,
            registers: [0; 2],
        }
    }

    pub(crate) fn register_pair(old_count: u32, kind: u32, registers: [u32; 2]) -> Self {
        Self {
            old_count,
            kind,
            count: 2,
            registers,
        }
    }
}

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
        // SM4 declarations, SM5 stream/function/tessellation/resource declarations,
        // and the later GS instance count. Hull phase markers are separate opcodes.
        visit(opcode, words, matches!(opcode, 88..=106 | 143..=162 | 206))?;
        remaining = &remaining[words.len()..];
    }
    Some(())
}

pub(crate) struct Instruction {
    pub(crate) words: Vec<u32>,
    pub(crate) opcode: u32,
    pub(crate) declaration: bool,
}

pub(crate) fn instructions(bytes: &[u8], profile: u32) -> Option<Vec<Instruction>> {
    let mut code = Vec::new();
    read_instructions(bytes, profile, |opcode, words, declaration| {
        let mut tokens = Vec::new();
        append_words(&mut tokens, words)?;
        code.try_reserve(1).ok()?;
        code.push(Instruction {
            words: tokens,
            opcode,
            declaration,
        });
        Some(())
    })?;
    Some(code)
}

#[cfg(test)]
#[test]
fn sm5_resource_declarations_are_not_executable_instructions() {
    let profile = 0x20050u32;
    let words = [
        profile,
        10,
        0x0300_00a1,
        0x0010_7000,
        126, // dcl_resource_raw
        0x0400_00a2,
        0x0010_7000,
        125,
        16,          // dcl_resource_structured
        0x0100_003e, // ret
    ];
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    let code = instructions(&bytes, profile).unwrap();
    assert!(code[0].declaration);
    assert!(code[1].declaration);
    assert!(!code[2].declaration);
}

pub(crate) fn append_words(code: &mut Vec<u32>, words: &[u32]) -> Option<()> {
    code.try_reserve(words.len()).ok()?;
    code.extend_from_slice(words);
    Some(())
}

pub(crate) fn temp_count(code: &[Instruction]) -> u32 {
    code.iter()
        .find(|inst| inst.opcode == DCL_TEMPS)
        .map_or(0, |inst| inst.words[1])
}

pub(crate) fn write_instructions(code: &[u32], destination: &mut Vec<u8>) -> Option<()> {
    let count = u32::try_from(code.len()).ok()?;
    let mut remaining = code.get(2..)?;
    while !remaining.is_empty() {
        let (_, words) = instruction(remaining)?;
        remaining = &remaining[words.len()..];
    }
    let size = code.len().checked_mul(4)?;
    let bytes = crate::velocity_container::resize_bytes(destination, size)?;
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

pub(crate) fn read_operands(words: &[u32]) -> Option<impl Iterator<Item = OperandView> + use<>> {
    Some(
        decode_operands(words)?
            .into_iter()
            .filter(|operand| !operand.nested)
            .map(|operand| operand.view),
    )
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
