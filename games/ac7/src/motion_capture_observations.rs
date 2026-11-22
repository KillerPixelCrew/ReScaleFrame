// SPDX-License-Identifier: MIT
//! F9 shader and layout creation, constant-buffer contents and colour snapshots.

pub(crate) mod buffers;
pub(super) mod colour;
pub(super) mod geometry;
#[cfg(windows)]
mod shader;

use crate::motion_capture_session::{CaptureInterval, capture_enabled, rsf_ac7_capture_fail};
use crate::shader_hash::crc32c;
use crate::sys::rsf_observer_layout_element as LayoutElement;
use crate::velocity_container::borrowed;
use crate::velocity_pass_cache;
use buffers::ConstantBuffers;
use colour::ColourReadbacks;
use geometry::GeometryReads;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

const MAX_SHADER_BYTES: usize = 1024 * 1024;
const SHADER_BUDGET: usize = 64 * 1024 * 1024;
const MAX_SHADERS: usize = 4096;
const MAX_LAYOUTS: usize = 4096;
const MAX_LAYOUT_ELEMENTS: usize = 16;
const CONSTANT_SLOTS: u32 = 14;
const ALL_CONSTANTS: u32 = (1 << CONSTANT_SLOTS) - 1;

#[derive(Clone, Copy)]
pub(crate) struct ShaderFacts {
    pub(crate) hash: u32,
    pub(crate) constant_mask: u32,
}

#[derive(Clone)]
pub(super) struct Shader {
    identity: usize,
    pub(super) stage: u32,
    facts: ShaderFacts,
    pub(super) data: Arc<Vec<u8>>,
}

impl Shader {
    pub(super) fn hash(&self) -> u32 {
        self.facts.hash
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct LayoutSnapshot {
    pub(super) count: u32,
    pub(super) copied: u32,
    pub(super) elements: [LayoutElement; MAX_LAYOUT_ELEMENTS],
}

#[derive(Default)]
pub(super) struct Observations {
    pub(super) shaders: Vec<Shader>,
    shader_bytes: usize,
    pub(super) shaders_dropped: u32,
    layouts: HashMap<usize, LayoutSnapshot>,
    constants: ConstantBuffers,
    colour: ColourReadbacks,
    geometry: GeometryReads,
}

static OBSERVATIONS: LazyLock<Mutex<Observations>> = LazyLock::new(Mutex::default);

pub(super) fn observations() -> MutexGuard<'static, Observations> {
    OBSERVATIONS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub(super) fn reset_session_observations() {
    let mut observations = observations();
    observations.constants.reset();
    observations.colour = ColourReadbacks::default();
    observations.geometry = GeometryReads::default();
}

pub(super) struct PublicationObservations {
    pub(super) shaders: Vec<Shader>,
    pub(super) shaders_dropped: u32,
    pub(super) constant_readbacks: (u32, u32),
    pub(super) colour_readbacks: (u32, u32),
}

impl PublicationObservations {
    pub(super) fn snapshot() -> Result<Self, std::collections::TryReserveError> {
        let observations = observations();
        let mut shaders = Vec::new();
        shaders.try_reserve_exact(observations.shaders.len())?;
        shaders.extend(observations.shaders.iter().cloned());
        Ok(Self {
            shaders,
            shaders_dropped: observations.shaders_dropped,
            constant_readbacks: observations.constants.readback_counts(),
            colour_readbacks: observations.colour.counts(),
        })
    }
}

impl Observations {
    fn shader_facts(&self, identity: usize, stage: u32) -> ShaderFacts {
        self.shaders
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

    fn forget_shader(&mut self, identity: usize, stage: u32) {
        if let Some(index) = self
            .shaders
            .iter()
            .position(|shader| shader.identity == identity && shader.stage == stage)
        {
            self.shader_bytes -= self.shaders.remove(index).data.len();
        }
    }

    fn retain_shader(
        &mut self,
        identity: usize,
        stage: u32,
        data: Arc<Vec<u8>>,
        constant_mask: u32,
    ) -> bool {
        if self.shaders.len() >= MAX_SHADERS || self.shader_bytes + data.len() > SHADER_BUDGET {
            self.shaders_dropped = self.shaders_dropped.wrapping_add(1);
            return true;
        }

        if self.shaders.try_reserve(1).is_err() {
            return false;
        }
        let facts = ShaderFacts {
            hash: crc32c(&data),
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

/// The constant-buffer slot mask the shader's disassembly declares.
type DisassembleConstants = fn(&[u8]) -> u32;

fn shader_bytes(bytes: &[u8]) -> Option<Arc<Vec<u8>>> {
    let mut data = Vec::new();
    data.try_reserve_exact(bytes.len()).ok()?;
    data.extend_from_slice(bytes);
    Some(Arc::new(data))
}

/// False when an allocation failed.
///
/// # Safety
/// `bytes` is null or `size` readable bytes for the call.
unsafe fn observe_shader(
    shader: *mut c_void,
    stage: u32,
    bytes: *const c_void,
    size: u32,
    disassemble: Option<DisassembleConstants>,
) -> bool {
    let size = size as usize;
    if shader.is_null() {
        return true;
    }
    let identity = shader.addr();
    // Creation ends the old identity even when no consumer can retain the replacement.
    if disassemble.is_some() {
        observations().forget_shader(identity, stage);
    }
    let capture = disassemble.filter(|_| capture_enabled());
    if !(1..=MAX_SHADER_BYTES).contains(&size) {
        velocity_pass_cache::observe_shader(identity, stage, None);
        return true;
    }
    if capture.is_none() && !velocity_pass_cache::observes_stage(stage) {
        return true;
    }
    // SAFETY: The creation callback lends its original bytecode until this call returns.
    let Some(bytes) = (unsafe { borrowed(bytes.cast::<u8>(), size) }) else {
        velocity_pass_cache::observe_shader(identity, stage, None);
        return true;
    };
    let data = shader_bytes(bytes);
    velocity_pass_cache::observe_shader(identity, stage, data.clone());
    let Some(disassemble) = capture else {
        return true;
    };
    let Some(data) = data else { return false };
    // Disassembly can load the compiler DLL. Keep it outside the cache lock.
    let constant_mask = disassemble(bytes);
    observations().retain_shader(identity, stage, data, constant_mask)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_shader_seen(
    shader: *mut c_void,
    stage: u32,
    bytes: *const c_void,
    size: u32,
) {
    // SAFETY: The velocity-only fixture lends the same creation callback byte span.
    unsafe { observe_shader(shader, stage, bytes, size, None) };
}

pub(crate) fn shader_facts(shader: *mut c_void, stage: u32) -> ShaderFacts {
    observations().shader_facts(shader.addr(), stage)
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

fn declared_constants(text: &[u8]) -> u32 {
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
unsafe extern "C" fn rsf_ac7_motion_capture_layout(
    layout: *mut c_void,
    elements: *const LayoutElement,
    copied: u32,
    count: u32,
) {
    // SAFETY: The public callback lends the observer's translated descriptors.
    if !unsafe { observe_layout(layout, elements, copied, count) } {
        rsf_ac7_capture_fail(CaptureInterval::load());
    }
}

unsafe fn observe_layout(
    layout: *mut c_void,
    elements: *const LayoutElement,
    copied: u32,
    count: u32,
) -> bool {
    let len = copied as usize;
    if !capture_enabled() || layout.is_null() || !(1..=MAX_LAYOUT_ELEMENTS).contains(&len) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_retains_the_shared_payload_and_forgets_replaced_facts() {
        let data = shader_bytes(&[3, 1, 4]).unwrap();
        let mut cache = Observations::default();
        assert!(cache.retain_shader(1, 0, Arc::clone(&data), 1));
        assert!(cache.retain_shader(2, 0, Arc::clone(&data), 2));
        assert!(Arc::ptr_eq(&cache.shaders[0].data, &data));
        assert_eq!(cache.shader_facts(1, 0).hash, crc32c(&data));
        cache.forget_shader(1, 0);
        assert_eq!(cache.shader_facts(1, 0).hash, 0);
        assert_eq!(cache.shader_bytes, data.len());
        assert!(cache.retain_shader(1, 0, data, 4));
        assert_eq!(
            cache
                .shaders
                .iter()
                .map(|shader| shader.identity)
                .collect::<Vec<_>>(),
            [2, 1]
        );
    }
}
