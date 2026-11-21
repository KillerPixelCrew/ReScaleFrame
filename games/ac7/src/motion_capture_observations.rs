// SPDX-License-Identifier: MIT
//! Process-lifetime F9 observations of successfully created shaders and input layouts.

use crate::motion_capture_session::rsf_ac7_capture_enabled;
use crate::shader_hash::crc32c;
use crate::velocity_container::borrowed;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::offset_of;
use std::sync::{LazyLock, Mutex, MutexGuard};

const MAX_SHADER_BYTES: usize = 1024 * 1024;
const SHADER_BUDGET: usize = 64 * 1024 * 1024;
const MAX_SHADERS: usize = 4096;
const MAX_LAYOUTS: usize = 4096;
const MAX_LAYOUT_ELEMENTS: usize = 16;
const CONSTANT_SLOTS: u32 = 14;
const ALL_CONSTANTS: u32 = (1 << CONSTANT_SLOTS) - 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct ShaderFacts {
    hash: u32,
    constant_mask: u32,
}

pub(super) struct Shader {
    identity: usize,
    pub(super) stage: u32,
    facts: ShaderFacts,
    pub(super) data: Vec<u8>,
}

impl Shader {
    pub(super) fn hash(&self) -> u32 {
        self.facts.hash
    }
}

// The observer already translates the native D3D11 descriptors to this public C structure.
// The velocity pass cache keeps its own copies of the same observations.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct LayoutElement {
    pub(crate) semantic_index: u32,
    pub(crate) format: u32,
    pub(crate) input_slot: u32,
    pub(crate) byte_offset: u32,
    pub(crate) per_instance: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LayoutSnapshot {
    count: u32,
    copied: u32,
    elements: [LayoutElement; MAX_LAYOUT_ELEMENTS],
}

const _: () = {
    assert!(size_of::<ShaderFacts>() == 8 && offset_of!(ShaderFacts, constant_mask) == 4);
    assert!(size_of::<LayoutElement>() == 20 && offset_of!(LayoutElement, per_instance) == 16);
    assert!(size_of::<LayoutSnapshot>() == 328 && offset_of!(LayoutSnapshot, elements) == 8);
};

#[derive(Default)]
pub(super) struct Observations {
    pub(super) shaders: Vec<Shader>,
    shader_bytes: usize,
    pub(super) shaders_dropped: u32,
    layouts: HashMap<usize, LayoutSnapshot>,
}

static OBSERVATIONS: LazyLock<Mutex<Observations>> = LazyLock::new(Mutex::default);

pub(super) fn observations() -> MutexGuard<'static, Observations> {
    OBSERVATIONS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

impl Observations {
    fn observe_shader(
        &mut self,
        identity: usize,
        stage: u32,
        bytes: &[u8],
        constant_mask: u32,
    ) -> bool {
        if let Some(index) = self
            .shaders
            .iter()
            .position(|shader| shader.identity == identity && shader.stage == stage)
        {
            self.shader_bytes -= self.shaders.remove(index).data.len();
        }
        if self.shaders.len() >= MAX_SHADERS || self.shader_bytes + bytes.len() > SHADER_BUDGET {
            self.shaders_dropped = self.shaders_dropped.wrapping_add(1);
            return true;
        }

        let mut data = Vec::new();
        if data.try_reserve_exact(bytes.len()).is_err() || self.shaders.try_reserve(1).is_err() {
            return false;
        }
        data.extend_from_slice(bytes);
        let facts = ShaderFacts {
            hash: crc32c(bytes),
            constant_mask,
        };
        self.shader_bytes += data.len();
        // Replacement moves to the end because artifact ordinals are positions in this list.
        self.shaders.push(Shader {
            identity,
            stage,
            facts,
            data,
        });
        true
    }
}

type DisassembleConstants = unsafe extern "C" fn(*const c_void, usize) -> u32;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_observe_shader(
    shader: *mut c_void,
    stage: u32,
    bytes: *const c_void,
    size: u32,
    disassemble: DisassembleConstants,
) -> bool {
    let size = size as usize;
    if !rsf_ac7_capture_enabled() || shader.is_null() || !(1..=MAX_SHADER_BYTES).contains(&size) {
        return true;
    }
    // SAFETY: The creation callback lends its original bytecode until this call returns.
    let Some(data) = (unsafe { borrowed(bytes.cast::<u8>(), size) }) else {
        return true;
    };
    // Disassembly can load the compiler DLL. Keep it outside the cache lock.
    let constant_mask = unsafe { disassemble(bytes, size) };
    observations().observe_shader(shader.addr(), stage, data, constant_mask)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_shader_facts(shader: *mut c_void, stage: u32) -> ShaderFacts {
    let identity = shader.addr();
    observations()
        .shaders
        .iter()
        .find(|item| item.identity == identity && item.stage == stage)
        .map_or(
            ShaderFacts {
                hash: 0,
                constant_mask: ALL_CONSTANTS,
            },
            |item| item.facts,
        )
}

// Match the old base-ten Windows strtoul scan, including its zero result without digits
// and its saturating overflow value, which is outside the captured slot range.
fn declaration_slot(text: &[u8]) -> Option<u32> {
    let start = text
        .iter()
        .position(|&byte| !matches!(byte, b' ' | b'\t'..=b'\r'))
        .unwrap_or(text.len());
    let (negative, digits) = match &text[start..] {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        rest => (false, rest),
    };
    let slot = digits
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .try_fold(0u32, |value, digit| {
            value.checked_mul(10)?.checked_add(u32::from(digit - b'0'))
        })?;
    Some(if negative { slot.wrapping_neg() } else { slot })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_declared_constants(text: *const u8, size: usize) -> u32 {
    if text.is_null() {
        return ALL_CONSTANTS;
    }
    // SAFETY: The native disassembly blob remains live and readable for this call.
    let Some(text) = (unsafe { borrowed(text, size) }) else {
        return ALL_CONSTANTS;
    };
    const DECLARATION: &[u8] = b"dcl_constantbuffer CB";
    let mut mask = 0;
    for (offset, window) in text.windows(DECLARATION.len()).enumerate() {
        if window == DECLARATION
            && let Some(slot) = declaration_slot(&text[offset + DECLARATION.len()..])
            && slot < CONSTANT_SLOTS
        {
            mask |= 1 << slot;
        }
    }
    mask
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_observe_layout(
    layout: *mut c_void,
    elements: *const LayoutElement,
    copied: u32,
    count: u32,
) -> bool {
    let len = copied as usize;
    if !rsf_ac7_capture_enabled() || layout.is_null() || !(1..=MAX_LAYOUT_ELEMENTS).contains(&len) {
        return true;
    }
    // SAFETY: The creation callback lends the translated descriptors for this call.
    let Some(elements) = (unsafe { borrowed(elements, len) }) else {
        return true;
    };
    let mut cache = observations();
    let identity = layout.addr();
    cache.layouts.remove(&identity); // A newly created object may reuse a retired address.
    if cache.layouts.len() >= MAX_LAYOUTS {
        return true;
    }
    if cache.layouts.try_reserve(1).is_err() {
        return false;
    }
    let mut snapshot = LayoutSnapshot {
        count,
        copied,
        ..Default::default()
    };
    snapshot.elements[..len].copy_from_slice(elements);
    cache.layouts.insert(identity, snapshot);
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_find_layout(
    layout: *mut c_void,
    out: *mut LayoutSnapshot,
) -> bool {
    let Some(&snapshot) = observations().layouts.get(&layout.addr()) else {
        return false;
    };
    // SAFETY: The native caller supplies its own writable snapshot storage.
    unsafe { out.write(snapshot) };
    true
}
