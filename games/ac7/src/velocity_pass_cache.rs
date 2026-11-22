// SPDX-License-Identifier: MIT
//! The translucent velocity pass's observed shader sources and input layouts, and its variant
//! admission: stage-checked source lookup, the beam/trail layout decision, the history identity of
//! generated geometry, variant identity, reuse and the refusal cache. The device owner builds
//! and retains D3D11 variants at the slots recorded here.

use crate::contact_shadow::container_crc;
use crate::sys::rsf_observer_layout_element as LayoutElement;
use crate::velocity_container::borrowed;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

const SOURCE_BUDGET: usize = 64 * 1024 * 1024;
const MAX_LAYOUTS: usize = 4096;
const MAX_LAYOUT_ELEMENTS: usize = 16;
const NO_VARIANT: u32 = u32::MAX;

// D3D11 observer stages, also the order in which the builder receives their bytecode.
const VERTEX: usize = 0;
const PIXEL: usize = 1;
const GEOMETRY: usize = 2;
const HULL: usize = 3;
const DOMAIN: usize = 4;
const STAGES: usize = 5;

// Builder outcomes. Anything else leaves the key unrecorded, so a later draw builds again.
pub(crate) const REFUSED: i32 = 0;
pub(crate) const BUILT: i32 = 1;

// The beam/trail vertex factory declaration: semantics 0 to 4 once each in stream 0 of an
// 80-byte vertex, an optional semantic 5 at the start of stream 1, all per vertex, float4 except
// the float3 semantic 1.
const BEAM_STRIDE: u32 = 80;
const BEAM_OFFSETS: [u32; 6] = [0, 16, 32, 64, 48, 0];
const DXGI_FORMAT_R32G32B32A32_FLOAT: u32 = 2;
const DXGI_FORMAT_R32G32B32_FLOAT: u32 = 6;

struct Source {
    stage: u32,
    hash: u32,
    bytes: Arc<Vec<u8>>,
}

struct Layout {
    elements: Vec<LayoutElement>,
    serial: u64,
}

pub(crate) struct VariantRequest {
    pub(crate) shaders: [*mut c_void; 5],
    pub(crate) layout: *mut c_void,
    pub(crate) material_blend: u32,
    pub(crate) stride: u32,
}

pub(crate) struct VariantSources<'a> {
    pub(crate) bytes: [&'a [u8]; STAGES],
    pub(crate) elements: &'a [LayoutElement],
    pub(crate) geometry_hash: u32,
    pub(crate) beam_layout: bool,
}

/// Interface addresses alone are not identity: D3D11 reuses released addresses, so the
/// bytecode hashes and the layout's creation serial are part of the key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct VariantKey {
    shaders: [usize; STAGES],
    layout: usize,
    serial: u64,
    hashes: [u32; STAGES],
    material_blend: u32,
    beam_layout: bool,
}

/// Retains the original bytecode for the builder after the cache lock is released.
struct Lease {
    bytes: [Option<Arc<Vec<u8>>>; STAGES],
    elements: Vec<LayoutElement>,
    geometry_hash: u32,
    beam_layout: bool,
}

enum Admission {
    Unavailable,
    Known(Option<u32>),
    Build(VariantKey, Lease),
}

#[derive(Default)]
struct Cache {
    sources: HashMap<usize, Source>,
    source_bytes: usize,
    layouts: HashMap<usize, Layout>,
    // Never reset, so a layout observed after a release cannot match an older key.
    layout_serial: u64,
    // A slot in the native variant list, or None for a cached refusal.
    variants: HashMap<VariantKey, Option<u32>>,
    // Advanced whenever the native variant list is discarded.
    generation: u64,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

fn lock(state: &Mutex<Cache>) -> MutexGuard<'_, Cache> {
    state.lock().unwrap_or_else(|error| error.into_inner())
}

fn copy_of<T: Copy>(data: &[T]) -> Option<Vec<T>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(data.len()).ok()?;
    copy.extend_from_slice(data);
    Some(copy)
}

fn beam_layout(stride: u32, elements: &[LayoutElement]) -> bool {
    if stride != BEAM_STRIDE || !(5..=6).contains(&elements.len()) {
        return false;
    }
    let mut seen = [false; 5];
    for element in elements {
        let index = element.semantic_index as usize;
        let format = if index == 1 {
            DXGI_FORMAT_R32G32B32_FLOAT
        } else {
            DXGI_FORMAT_R32G32B32A32_FLOAT
        };
        if index > 5
            || element.per_instance != 0
            || element.input_slot != u32::from(index == 5)
            || element.byte_offset != BEAM_OFFSETS[index]
            || element.format != format
        {
            return false;
        }
        if index < 5 {
            if seen[index] {
                return false;
            }
            seen[index] = true;
        }
    }
    seen.iter().all(|&field| field)
}

impl Cache {
    // A replacement always forgets the old bytecode first, even when the new one is refused.
    fn observe_shader(&mut self, identity: usize, stage: u32, bytes: Option<Arc<Vec<u8>>>) {
        if let Some(old) = self.sources.remove(&identity) {
            self.source_bytes -= old.bytes.len();
        }
        let Some(bytes) = bytes else {
            return;
        };
        if self.source_bytes + bytes.len() > SOURCE_BUDGET || self.sources.try_reserve(1).is_err() {
            return;
        }
        self.source_bytes += bytes.len();
        let hash = container_crc(&bytes);
        self.sources.insert(identity, Source { stage, hash, bytes });
    }

    // None is a refused descriptor set, which still forgets the address.
    fn observe_layout(&mut self, identity: usize, elements: Option<&[LayoutElement]>) {
        self.layouts.remove(&identity);
        let Some(elements) = elements else {
            return;
        };
        if self.layouts.len() >= MAX_LAYOUTS || self.layouts.try_reserve(1).is_err() {
            return;
        }
        let Some(elements) = copy_of(elements) else {
            return;
        };
        self.layout_serial += 1;
        let serial = self.layout_serial;
        self.layouts.insert(identity, Layout { elements, serial });
    }

    fn source(&self, shader: *mut c_void, stage: usize) -> Option<&Source> {
        self.sources
            .get(&shader.addr())
            .filter(|source| source.stage as usize == stage)
    }

    fn prepare(&self, request: &VariantRequest, vertex_hash: &mut u32) -> Admission {
        // A bound native geometry shader, or either tessellation stage, must have its own
        // observed source. The draw callback already refuses a partial tessellation pipeline.
        let tessellated = !request.shaders[HULL].is_null() || !request.shaders[DOMAIN].is_null();
        let mut sources: [Option<&Source>; STAGES] = [None; STAGES];
        for (stage, slot) in sources.iter_mut().enumerate() {
            let shader = request.shaders[stage];
            let required = match stage {
                VERTEX | PIXEL => true,
                GEOMETRY => !shader.is_null(),
                _ => tessellated,
            };
            if required {
                let Some(source) = self.source(shader, stage) else {
                    return Admission::Unavailable;
                };
                *slot = Some(source);
            }
        }
        let hashes = sources.map(|source| source.map_or(0, |source| source.hash));
        *vertex_hash = hashes[VERTEX];

        let layout = self.layouts.get(&request.layout.addr());
        let elements = layout.map_or(&[][..], |layout| layout.elements.as_slice());
        let beam_layout = beam_layout(request.stride, elements);
        let key = VariantKey {
            shaders: request.shaders.map(|shader| shader.addr()),
            layout: request.layout.addr(),
            serial: layout.map_or(0, |layout| layout.serial),
            hashes,
            material_blend: request.material_blend,
            beam_layout,
        };
        if let Some(&slot) = self.variants.get(&key) {
            return Admission::Known(slot);
        }

        let bytes = sources.map(|source| source.map(|source| Arc::clone(&source.bytes)));
        let Some(leased_elements) = copy_of(elements) else {
            return Admission::Unavailable;
        };
        // Generated geometry keeps separate history per native producer.
        let geometry_hash = match (sources[HULL], sources[DOMAIN], sources[GEOMETRY]) {
            (Some(hull), Some(domain), _) => hull.hash ^ (domain.hash << 1) ^ (domain.hash >> 31),
            (_, _, Some(geometry)) => geometry.hash,
            _ => 0,
        };
        Admission::Build(
            key,
            Lease {
                bytes,
                elements: leased_elements,
                geometry_hash,
                beam_layout,
            },
        )
    }

    fn record(&mut self, generation: u64, key: VariantKey, slot: Option<u32>) -> u32 {
        if self.generation != generation {
            return NO_VARIANT;
        }
        if self.variants.try_reserve(1).is_ok() {
            self.variants.insert(key, slot);
        }
        slot.unwrap_or(NO_VARIANT)
    }

    fn reset_variants(&mut self) {
        self.variants = HashMap::new();
        self.generation = self.generation.wrapping_add(1);
    }

    fn release(&mut self) {
        self.reset_variants();
        self.sources = HashMap::new();
        self.source_bytes = 0;
        self.layouts = HashMap::new();
    }
}

fn admit(
    state: &Mutex<Cache>,
    request: &VariantRequest,
    vertex_hash: &mut u32,
    build: impl FnOnce(&VariantSources<'_>, &mut u32) -> i32,
) -> u32 {
    let (generation, key, lease) = {
        let cache = lock(state);
        match cache.prepare(request, vertex_hash) {
            Admission::Unavailable => return NO_VARIANT,
            Admission::Known(slot) => return slot.unwrap_or(NO_VARIANT),
            Admission::Build(key, lease) => (cache.generation, key, lease),
        }
    };
    let bytes = lease.bytes.each_ref().map(|bytes| {
        // The stage interfaces accept an empty span backed by a non-null pointer.
        bytes.as_deref().map_or(&[][..], Vec::as_slice)
    });
    let sources = VariantSources {
        bytes,
        elements: &lease.elements,
        geometry_hash: lease.geometry_hash,
        beam_layout: lease.beam_layout,
    };
    // The builder creates D3D11 shaders, whose creation callbacks report back into this cache.
    let mut slot = NO_VARIANT;
    let entry = match build(&sources, &mut slot) {
        BUILT if slot != NO_VARIANT => Some(slot),
        REFUSED => None,
        _ => return NO_VARIANT,
    };
    lock(state).record(generation, key, entry)
}

pub(crate) fn observes_stage(stage: u32) -> bool {
    stage as usize <= DOMAIN
}

pub(crate) fn observe_shader(identity: usize, stage: u32, bytes: Option<Arc<Vec<u8>>>) {
    if observes_stage(stage) {
        lock(&CACHE).observe_shader(identity, stage, bytes);
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_velocity_layout_seen(
    layout: *mut c_void,
    elements: *const LayoutElement,
    copied: u32,
    count: u32,
) {
    if layout.is_null() {
        return;
    }
    let len = count as usize;
    let elements = if !elements.is_null() && copied == count && len <= MAX_LAYOUT_ELEMENTS {
        // SAFETY: The creation callback lends `copied` translated descriptors for this call.
        unsafe { borrowed(elements, len) }
    } else {
        None
    };
    lock(&CACHE).observe_layout(layout.addr(), elements);
}

#[cfg(windows)]
pub(crate) fn admit_variant(
    request: &VariantRequest,
    vertex_hash: &mut u32,
    build: impl FnOnce(&VariantSources<'_>, &mut u32) -> i32,
) -> u32 {
    admit(&CACHE, request, vertex_hash, build)
}

#[cfg(windows)]
pub(crate) fn variant_count() -> usize {
    lock(&CACHE).variants.len()
}

/// The native variant list was discarded with its renderer (`velocity_renderer.rs`).
pub(crate) fn reset_variants() {
    lock(&CACHE).reset_variants();
}

/// The pass was released (`velocity_renderer.rs`).
pub(crate) fn release() {
    lock(&CACHE).release();
}

#[cfg(test)]
mod tests;
