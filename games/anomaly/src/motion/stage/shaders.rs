// SPDX-License-Identifier: MIT
use super::*;
use crate::motion_profile::SHADER_OBJECT_OFFSET;
use crate::shader_family::{FAMILY_COUNT, NO_FAMILY, diagnostic_family_name};
use crate::shader_loader::{
    DecodedFrame, Entry, Manifest, OUTCOME_NAMES, Reflection, RuntimePack, Snapshot, decode_frame,
    outcome_name,
};
use crate::shader_loader::{rsf_anomaly_shader_linkable, rsf_anomaly_shader_tess_linkable};
use rsf_game_sys::{RSF_PATCH_OK, rsf_patch_counters};
use rsf_windows::sha256_hex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;

const NOT_ALLOWLISTED: u8 = 0;
const CREATE_FAILED: u8 = 9;
const NAMES_LIMIT: usize = 16_384;
const FILENAME_CAPACITY: usize = 520;

pub(in crate::motion) struct Variant {
    stock: Lease,
    shader: Lease,
    reflection: Snapshot,
    request: Arc<Request>,
    device: Lease,
    stage: u8,
    pub family: u8,
    pub skin: i32,
    pub(super) foliage: FoliageLayout,
}

// Requests own a sealed copy of original DXBC and the guarded compiler frame.
// Their storage is immutable once published.
struct Request {
    hash: [u8; 64],
    target: [u8; 17],
    frame: DecodedFrame,
    bytecode: Vec<u8>,
}

impl Request {
    fn copy_stock(
        call: &Creation<'_>,
        frame: DecodedFrame,
        hash: [u8; 64],
        read: impl Fn(usize, &mut [u8]) -> bool,
    ) -> Option<Self> {
        let mut target = [0; 17];
        // Leave the final byte zero for the Runtime's C-string input.
        read_prefix(call.target as usize, &mut target[..16], &read);
        let size = call.size as usize;
        let address = call.bytecode as usize;
        if size == 0 || address == 0 || address.checked_add(size - 1).is_none() {
            return None;
        }
        let mut bytecode = Vec::new();
        bytecode.try_reserve_exact(size).ok()?;
        bytecode.resize(size, 0);
        read(address, &mut bytecode).then_some(Self {
            hash,
            target,
            frame,
            bytecode,
        })
    }

    fn target(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.target).expect("target has a reserved terminator")
    }
}

// D3D11 shader references and plain reflection data can be released after
// the draw on any caller thread.
unsafe impl Send for Variant {}
unsafe impl Sync for Variant {}

pub(in crate::motion) struct Chain {
    pub(super) vertex: Arc<Variant>,
    pixel: Arc<Variant>,
    tess: Option<(Arc<Variant>, Arc<Variant>)>,
}

impl Chain {
    pub(in crate::motion) fn tessellated(&self) -> bool {
        self.tess.is_some()
    }

    pub(super) fn shaders(&self) -> ShaderObjects {
        ShaderObjects {
            vs: self.vertex.shader.as_ptr(),
            ps: self.pixel.shader.as_ptr(),
            hs: self
                .tess
                .as_ref()
                .map_or(ptr::null_mut(), |(hs, _)| hs.shader.as_ptr()),
            ds: self
                .tess
                .as_ref()
                .map_or(ptr::null_mut(), |(_, ds)| ds.shader.as_ptr()),
            gs: ptr::null_mut(),
        }
    }
}

#[derive(Default)]
pub(super) struct Registry {
    shaders: HashMap<usize, Arc<Variant>>,
    links: HashMap<[usize; 4], bool>,
    names: HashMap<usize, String>,
}

pub(super) enum Refusal {
    Missing,
    TessMissing,
    TessUnlinked,
    Geometry,
}

impl Registry {
    pub fn get(&self, stock: Raw) -> Option<Arc<Variant>> {
        self.shaders.get(&(stock as usize)).cloned()
    }

    fn put(&mut self, variant: Arc<Variant>) -> Option<Arc<Variant>> {
        let old = self
            .shaders
            .insert(variant.stock.as_ptr() as usize, variant);
        if old.is_some() {
            self.links.clear();
        }
        old
    }

    pub fn clear_links(&mut self) {
        self.links.clear();
    }

    pub fn name(&self, address: usize) -> &str {
        if address == 0 {
            "none"
        } else {
            self.names.get(&address).map_or("unknown", String::as_str)
        }
    }

    pub fn remember(&mut self, stock: Raw, file: &[u8]) {
        if self.names.len() < NAMES_LIMIT || self.names.contains_key(&(stock as usize)) {
            let root = file
                .rsplit(|byte| matches!(byte, b'/' | b'\\'))
                .find(|part| {
                    part.len() > 3
                        && [b".vs", b".ps", b".hs", b".ds"]
                            .iter()
                            .any(|ext| part.ends_with(*ext))
                })
                .unwrap_or_else(|| &file[file.len().saturating_sub(64)..]);
            self.names
                .insert(stock as usize, String::from_utf8_lossy(root).into_owned());
        }
    }

    pub fn select(
        &mut self,
        stock: ShaderObjects,
        device: Raw,
        link: impl FnOnce(&Variant, Option<(&Variant, &Variant)>, &Variant) -> bool,
    ) -> Result<Chain, Refusal> {
        if !stock.gs.is_null() {
            return Err(Refusal::Geometry);
        }
        let tessellated = !stock.hs.is_null() || !stock.ds.is_null();
        let variant = |key, stage| {
            self.get(key)
                .filter(|v| v.device.as_ptr() == device && v.stage == stage)
        };
        let (Some(vertex), Some(pixel)) = (variant(stock.vs, b'v'), variant(stock.ps, b'p')) else {
            return Err(if tessellated {
                Refusal::TessMissing
            } else {
                Refusal::Missing
            });
        };
        let tess = if tessellated {
            if !matches!(vertex.family, CORE_STATIC | CORE_MODEL) {
                return Err(Refusal::TessMissing);
            }
            let (Some(hs), Some(ds)) = (variant(stock.hs, b'h'), variant(stock.ds, b'd')) else {
                return Err(Refusal::TessMissing);
            };
            Some((hs, ds))
        } else {
            None
        };
        let key = [
            stock.vs as usize,
            stock.hs as usize,
            stock.ds as usize,
            stock.ps as usize,
        ];
        let linked = *self.links.entry(key).or_insert_with(|| {
            link(
                &vertex,
                tess.as_ref().map(|(hs, ds)| (hs.as_ref(), ds.as_ref())),
                &pixel,
            )
        });
        if !linked {
            return Err(if tessellated {
                Refusal::TessUnlinked
            } else {
                Refusal::Missing
            });
        }
        Ok(Chain {
            vertex,
            pixel,
            tess,
        })
    }
}

pub(super) struct Pack(RuntimePack);

// The loader lock serializes access to the pack, including its engine includer
// and Runtime session scratch storage.
unsafe impl Send for Pack {}

impl Pack {
    pub fn open(platform: &Platform, build: Build, root: &Path) -> Result<Self, String> {
        let profile = build.motion.shader_profile();
        let pack = wide_path(&root.join("motion.rsfpatch"));
        let cache = wide_path(&root.join("cache"));
        unsafe {
            RuntimePack::open_engine(&pack, &cache, build.image, profile, &*platform.includes)
        }
        .map(Self)
    }

    pub(super) unsafe fn attach(&self, device: Raw) -> i32 {
        unsafe { self.0.attach(device) }
    }

    pub fn constant_ops(&self) -> ConstantOps {
        self.0.constant_ops()
    }

    fn manifest(&self) -> &Manifest {
        self.0.manifest()
    }

    fn entry(&self, hash: &[u8; 64]) -> Option<&Entry> {
        self.manifest().find(hash)
    }

    pub fn counters(&self) -> rsf_patch_counters {
        self.0.counters()
    }
}

pub(super) fn link_variants(
    vertex: &Variant,
    tess: Option<(&Variant, &Variant)>,
    pixel: &Variant,
) -> bool {
    let vs = vertex.reflection.view();
    let ps = pixel.reflection.view();
    if let Some((hs, ds)) = tess {
        rsf_anomaly_shader_tess_linkable(vs, hs.reflection.view(), ds.reflection.view(), ps)
    } else {
        rsf_anomaly_shader_linkable(vs, ps)
    }
}

#[derive(Default, Clone, Copy)]
pub(super) struct ShaderCounters {
    pub outcomes: [[u64; OUTCOME_NAMES.len()]; FAMILY_COUNT],
    pub frame_refusals: u64,
}

#[derive(Default)]
pub(super) struct Loader {
    pub pack: Option<Pack>,
    pub stats: ShaderCounters,
    refused_logs: u32,
    replacement_logs: u32,
}

// The staged tree transform (TreeRecord::xform) and the detail batch rows.
const TREE_XFORM_BYTES: u32 = size_of::<[f32; 12]>() as u32;
const DETAIL_ARRAY_BYTES: u32 = size_of::<foliage::Rows>() as u32;
// consts, wave and wind are each one float4 register.
const REGISTER_BYTES: u32 = size_of::<[f32; 4]>() as u32;

impl ConstantRef {
    fn find(stock: &Reflection, name: &[u8]) -> Self {
        stock.constant(name).map_or_else(Self::default, |c| Self {
            present: true,
            buffer: c.buffer,
            buffer_size: c.buffer_size,
            offset: c.offset,
            size: c.size,
        })
    }

    fn inside(&self) -> bool {
        !self.present
            || (self.size != 0
                && self.offset <= self.buffer_size
                && self.size <= self.buffer_size - self.offset)
    }
}

impl FoliageLayout {
    // Locates the engine's staged VS constants in the stock reflection, whose buffer indices are
    // the engine's CBackend::m_aVertexConstants indices. Names are the stock shader source's: tree
    // m_xform consts wave wind, detail consts wave dir2D array. The compiler may remove consts, wave
    // and wind (USE_TREEWAVE zeroes the sway, the still detail route has no wave set); a removed one
    // is absent and its history words stay zero.
    pub(crate) fn from_stock(family: u8, stock: &Reflection) -> Self {
        let find = |name: &[u8]| ConstantRef::find(stock, name);
        let mut layout = Self {
            family,
            ..Self::default()
        };
        let (required, sized) = match family {
            FOLIAGE_TREE => {
                layout.xform = find(b"m_xform");
                layout.wind = find(b"wind");
                (layout.xform, layout.xform.size >= TREE_XFORM_BYTES)
            }
            FOLIAGE_DETAIL => {
                layout.wind = find(b"dir2D");
                layout.array = find(b"array");
                (layout.array, layout.array.size == DETAIL_ARRAY_BYTES)
            }
            _ => return layout,
        };
        layout.consts = find(b"consts");
        layout.wave = find(b"wave");
        let registers = [layout.consts, layout.wave, layout.wind];
        layout.usable = required.present
            && sized
            && registers.iter().all(|r| r.size <= REGISTER_BYTES)
            && required.inside()
            && registers.iter().all(ConstantRef::inside);
        layout
    }
}

struct Replacement {
    shader: Lease,
    reflection: Snapshot,
    skin: i32,
    outcome: u8,
    foliage: FoliageLayout,
    stage: u8,
    family: u8,
}

impl Replacement {
    fn build(
        pack: &mut Pack,
        request: &Request,
        identity: (u8, u8),
        device: Raw,
        attach: bool,
    ) -> Result<Self, (u8, String)> {
        let (stage, family) = identity;
        let result = request.frame.with_request(
            request.target().to_bytes(),
            &request.bytecode,
            |view| unsafe { pack.0.prepare(view, stage) },
        )?;
        if device.is_null() {
            return Err((CREATE_FAILED, "engine shader device unreadable".into()));
        }
        let foliage = if stage == b'v' && matches!(family, FOLIAGE_TREE | FOLIAGE_DETAIL) {
            FoliageLayout::from_stock(family, result.stock.view())
        } else {
            FoliageLayout::default()
        };
        if attach && unsafe { pack.attach(device) } != RSF_PATCH_OK {
            return Err((CREATE_FAILED, "Runtime D3D11 interception refused".into()));
        }
        let mut shader = ptr::null_mut();
        let status = unsafe {
            pack.0
                .create(device, request.target(), &result.bytecode, &mut shader)
        };
        let shader = Lease::new(shader, crate::motion_device::release);
        let shader = shader
            .filter(|_| status == RSF_PATCH_OK)
            .ok_or((CREATE_FAILED, "Create*Shader failed".into()))?;
        Ok(Self {
            shader,
            reflection: result.reflection,
            skin: result.skin,
            outcome: result.outcome,
            foliage,
            stage,
            family,
        })
    }

    fn publish(self, stock: Raw, request: Arc<Request>, device: Raw) -> Arc<Variant> {
        let (release, retain) = (crate::motion_device::release, crate::motion_device::retain);
        let stock = Lease::new(stock, release).expect("created stock shader is non-null");
        let device = Lease::new(device, release).expect("shader creation requires a device");
        unsafe {
            retain(stock.as_ptr());
            retain(device.as_ptr());
        }
        Arc::new(Variant {
            stock,
            shader: self.shader,
            reflection: self.reflection,
            request,
            device,
            stage: self.stage,
            family: self.family,
            skin: self.skin,
            foliage: self.foliage,
        })
    }
}

fn read_prefix(address: usize, out: &mut [u8], read: impl Fn(usize, &mut [u8]) -> bool) -> usize {
    for (index, slot) in out.iter_mut().enumerate() {
        let Some(address) = address.checked_add(index).filter(|&address| address != 0) else {
            return index;
        };
        let mut byte = [0];
        if !read(address, &mut byte) || byte[0] == 0 {
            return index;
        }
        *slot = byte[0];
    }
    out.len()
}

struct Creation<'a> {
    stage: u8,
    target: *const c_char,
    bytecode: *const c_void,
    size: u32,
    file: &'a [u8],
    stock: Raw,
    return_address: *const c_void,
    stack: *const c_void,
}

impl<'a> Creation<'a> {
    fn read(
        native: &ShaderCreation,
        filename: &'a mut [u8; FILENAME_CAPACITY],
        read: impl Fn(usize, &mut [u8]) -> bool,
    ) -> Option<Self> {
        let mut stage = [0];
        if native.target.is_null()
            || !read(native.target.addr(), &mut stage)
            || !matches!(stage[0], b'v' | b'p' | b'h' | b'd')
            || native.bytecode.is_null()
            || native.size == 0
            || native.result.is_null()
        {
            return None;
        }
        let mut object = [0; size_of::<usize>()];
        if !read(native.result.addr(), &mut object) {
            return None;
        }
        let object = usize::from_ne_bytes(object);
        let address = object
            .checked_add(SHADER_OBJECT_OFFSET)
            .filter(|_| object != 0)?;
        let mut stock = [0; size_of::<usize>()];
        if !read(address, &mut stock) {
            return None;
        }
        let stock =
            ptr::with_exposed_provenance_mut::<std::ffi::c_void>(usize::from_ne_bytes(stock));
        if stock.is_null() {
            return None;
        }
        let length = read_prefix(native.file.addr(), filename, read);
        Some(Self {
            stage: stage[0],
            target: native.target,
            bytecode: native.bytecode,
            size: native.size,
            file: &filename[..length],
            stock,
            return_address: native.return_address,
            stack: native.stack,
        })
    }
}

impl Stage {
    pub(super) fn open_pack(&self, build: Build) -> bool {
        let mut loader = lock(&self.loader);
        if loader.pack.is_some() {
            return true;
        }
        let pack = match Pack::open(self.platform, build, &lock(&self.settings).shader_dir) {
            Ok(pack) => pack,
            Err(error) => {
                self.log()
                    .say(format!("Anomaly motion pack refused: {error}"));
                return false;
            }
        };
        self.log().say(format!(
            "Anomaly runtime shader pack: {} allowlisted roots, namespace {}",
            pack.manifest().entries().len(),
            String::from_utf8_lossy(&pack.manifest().namespace()[..16])
        ));
        loader.pack = Some(pack);
        self.pack_status.store(1, Relaxed);
        true
    }

    pub(super) fn check_pack_requests(&self, render: &mut Render) {
        let dump = self.dump_shaders.swap(false, Relaxed);
        let reload = self.reload.swap(false, Relaxed);
        if !dump && !reload {
            return;
        }
        let mut loader = lock(&self.loader);
        let Some(old_pack) = &loader.pack else { return };
        let root = lock(&self.settings).shader_dir.clone();
        if dump {
            let path = wide_path(&root.join("cache").join("shader-dump"));
            let result = old_pack.0.dump(&path);
            self.log()
                .say(format!("Runtime shader dump result={result}"));
        }
        if !reload {
            return;
        }
        let Some(build) = *lock(&self.build) else {
            return;
        };
        match self.reload_pack(build, &root) {
            Ok((pack, next)) => {
                let previous = {
                    let mut registry = lock(&self.registry);
                    registry.clear_links();
                    std::mem::replace(&mut registry.shaders, next)
                };
                loader.pack = Some(pack);
                drop(previous);
                render.reset_history();
                self.pack_status.store(1, Relaxed);
                self.log()
                    .text(c"Motion shader pack reloaded from runtime source; history reset.");
            }
            Err(error) => {
                self.pack_status.store(2, Relaxed);
                self.log().say(format!(
                    "Motion pack reload kept previous generation: {error}"
                ));
            }
        }
    }

    fn reload_pack(
        &self,
        build: Build,
        root: &Path,
    ) -> Result<(Pack, HashMap<usize, Arc<Variant>>), String> {
        let p = self.platform;
        let mut pack = Pack::open(p, build, root)?;
        let previous: Vec<_> = lock(&self.registry).shaders.values().cloned().collect();
        let mut next = HashMap::new();
        for old in previous {
            let Some(entry) = pack.entry(&old.request.hash) else {
                continue;
            };
            if entry.stage != old.stage {
                return Err("stage changed for a registered root".into());
            }
            let device = old.device.as_ptr();
            let identity = (entry.stage, entry.family);
            let replacement = Replacement::build(&mut pack, &old.request, identity, device, false)
                .map_err(|(outcome, detail)| format!("{}: {detail}", outcome_name(outcome)))?;
            let key = old.stock.as_ptr();
            next.insert(
                key as usize,
                replacement.publish(key, old.request.clone(), device),
            );
        }
        Ok((pack, next))
    }

    pub(super) fn admit_shader_creation(&self, native: &ShaderCreation) {
        let mut filename = [0; FILENAME_CAPACITY];
        let call = Creation::read(native, &mut filename, |address, bytes| unsafe {
            (self.platform.read_engine.unwrap())(
                ptr::null(),
                address,
                bytes.as_mut_ptr(),
                bytes.len(),
            )
        });
        if let Some(call) = call
            && self.active.load(Acquire)
        {
            self.shader_created(call);
        }
    }

    fn shader_created(&self, call: Creation<'_>) {
        let p = self.platform;
        // The installed build; a creation before install has no caller frame to decode.
        let Some(build) = *lock(&self.build) else {
            return;
        };
        lock(&self.registry).remember(call.stock, call.file);
        let mut guard = lock(&self.loader);
        // Split the guard so pack preparation and counter updates can borrow separate fields.
        let loader = &mut *guard;
        let Some(pack) = &mut loader.pack else { return };
        let layout = build.motion.frame_layout();
        let module = build.image;
        let read = |address: usize, bytes: &mut [u8]| unsafe {
            (p.read_engine.unwrap())(ptr::null(), address, bytes.as_mut_ptr(), bytes.len())
        };
        let file = String::from_utf8_lossy(call.file);
        let frame = match decode_frame(
            &layout,
            module,
            call.return_address as usize,
            call.stack as usize,
            read,
        ) {
            Ok(frame) => frame,
            Err(status) => {
                loader.stats.frame_refusals += 1;
                if loader.refused_logs < 32 {
                    loader.refused_logs += 1;
                    self.log().say(format!(
                        "Anomaly motion shader frame refused (status {}): {file}",
                        status as u8
                    ));
                }
                return;
            }
        };
        let Some(hash) = sha256_hex(&frame.source) else {
            return;
        };
        let Some(entry) = pack.entry(&hash) else {
            loader.stats.outcomes[NO_FAMILY as usize][NOT_ALLOWLISTED as usize] += 1;
            return;
        };
        let family_id = entry.family;
        let stage = entry.stage;
        let family = family_id as usize;
        let name = String::from_utf8_lossy(entry.name()).into_owned();
        let result = if stage == call.stage {
            Request::copy_stock(&call, frame, hash, read)
                .ok_or_else(|| {
                    (
                        CREATE_FAILED,
                        "original bytecode could not be retained".into(),
                    )
                })
                .and_then(|request| {
                    let device = unsafe { (p.shader_device.unwrap())() };
                    Replacement::build(pack, &request, (stage, family_id), device, true)
                        .map(|replacement| (replacement, request, device))
                })
        } else {
            Err((NOT_ALLOWLISTED, String::new()))
        };
        let outcome = match result {
            Ok((replacement, request, device)) => {
                if matches!(family_id, FOLIAGE_TREE | FOLIAGE_DETAIL)
                    && !replacement.foliage.usable
                    && loader.refused_logs < 32
                {
                    loader.refused_logs += 1;
                    self.log().say(format!("Anomaly motion foliage layout unusable for {name}; draws keep the all-invalid foliage buffer."));
                }
                let outcome = replacement.outcome;
                if loader.replacement_logs < 24 {
                    loader.replacement_logs += 1;
                    self.log().say(format!(
                        "Anomaly motion variant {name} {} skin={} ({}) for {file}",
                        diagnostic_family_name(family_id),
                        replacement.skin,
                        outcome_name(outcome)
                    ));
                }
                let variant = replacement.publish(call.stock, Arc::new(request), device);
                let previous = lock(&self.registry).put(variant);
                drop(previous);
                outcome
            }
            Err((outcome, detail)) => {
                if loader.refused_logs < 32 {
                    loader.refused_logs += 1;
                    let detail: String = detail.chars().take(300).collect();
                    self.log().say(format!(
                        "Anomaly motion kept stock {name} ({}): {detail}; {file}",
                        outcome_name(outcome)
                    ));
                }
                outcome
            }
        };
        loader.stats.outcomes[family][outcome as usize] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn raw(value: usize) -> Raw {
        value as Raw
    }

    #[test]
    fn native_shader_creation_borrows_verified_stock_and_keeps_readable_filename_prefix() {
        let native = ShaderCreation {
            target: raw(1).cast(),
            bytecode: raw(2),
            file: raw(0x200).cast(),
            result: raw(3).cast(),
            return_address: raw(4),
            stack: raw(5),
            status: 0,
            size: 12,
        };
        let reads = RefCell::new(Vec::new());
        let read = |address, out: &mut [u8]| {
            reads.borrow_mut().push(address);
            match address {
                1 => out.copy_from_slice(b"h"),
                3 => out.copy_from_slice(&0x100usize.to_ne_bytes()),
                0x110 => out.copy_from_slice(&0x150usize.to_ne_bytes()),
                0x200 => out.copy_from_slice(b"a"),
                0x201 => out.copy_from_slice(b"b"),
                _ => return false,
            }
            true
        };
        let mut name = [0; FILENAME_CAPACITY];
        let call = Creation::read(&native, &mut name, read).unwrap();
        assert_eq!(call.stage, b'h');
        assert_eq!(call.stock.addr(), 0x150);
        assert_eq!(call.file, b"ab");
        assert_eq!(call.return_address, native.return_address);
        assert_eq!(call.stack, native.stack);
        assert_eq!(
            reads.borrow().as_slice(),
            &[1, 3, 0x110, 0x200, 0x201, 0x202]
        );

        let read = |address, out: &mut [u8]| {
            match address {
                1 => out.copy_from_slice(b"d"),
                3 => out.copy_from_slice(&usize::MAX.to_ne_bytes()),
                _ => panic!("an overflowing object must refuse before the stock read"),
            }
            true
        };
        assert!(Creation::read(&native, &mut name, read).is_none());
    }

    unsafe extern "C" fn release(_: Raw) {}

    fn variant(
        stock: usize,
        replacement: usize,
        device: usize,
        family: u8,
        stage: u8,
    ) -> Arc<Variant> {
        let lease = |value| Lease::new(raw(value), release).unwrap();
        Arc::new(Variant {
            stock: lease(stock),
            shader: lease(replacement),
            reflection: Snapshot::default(),
            request: Arc::new(Request {
                hash: [0; 64],
                target: [0; 17],
                frame: DecodedFrame::default(),
                bytecode: Vec::new(),
            }),
            device: lease(device),
            stage,
            family,
            skin: -1,
            foliage: FoliageLayout::default(),
        })
    }

    #[test]
    fn linkage_cache_follows_published_variants_and_pending_chain_pins_its_objects() {
        let mut registry = Registry::default();
        let first = variant(1, 11, 42, CORE_MODEL, b'v');
        let weak = Arc::downgrade(&first);
        registry.put(first);
        registry.put(variant(2, 21, 42, 3, b'p'));
        let pair = ShaderObjects {
            vs: raw(1),
            ps: raw(2),
            ..Default::default()
        };
        let chain = registry
            .select(pair, raw(42), |vs, tess, ps| {
                vs.shader.as_ptr() == raw(11) && tess.is_none() && ps.shader.as_ptr() == raw(21)
            })
            .ok()
            .unwrap();
        assert!(!chain.tessellated());
        assert!(
            registry
                .select(pair, raw(42), |_, _, _| panic!("cached link"))
                .is_ok()
        );
        assert!(matches!(
            registry.select(pair, raw(43), |_, _, _| panic!("foreign device")),
            Err(Refusal::Missing)
        ));
        drop(registry.put(variant(1, 12, 42, CORE_MODEL, b'v')));
        assert!(weak.upgrade().is_some());
        assert_eq!(chain.shaders().vs, raw(11));
        assert!(matches!(
            registry.select(pair, raw(42), |_, _, _| false),
            Err(Refusal::Missing)
        ));
        drop(chain);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn tessellation_needs_both_stages_and_refuses_geometry_or_an_unlinked_chain() {
        let mut registry = Registry::default();
        for (stock, family, stage) in [
            (1, CORE_MODEL, b'v'),
            (2, 3, b'p'),
            (3, 6, b'h'),
            (4, 7, b'd'),
        ] {
            registry.put(variant(stock, stock + 10, 42, family, stage));
        }
        let chain = ShaderObjects {
            vs: raw(1),
            ps: raw(2),
            hs: raw(3),
            ds: raw(4),
            gs: ptr::null_mut(),
        };
        let partial = ShaderObjects {
            ds: ptr::null_mut(),
            ..chain
        };
        let with_gs = ShaderObjects {
            gs: raw(5),
            ..chain
        };
        fn unavailable(_: &Variant, _: Option<(&Variant, &Variant)>, _: &Variant) -> bool {
            panic!("an incomplete or unsafe chain cannot be linked");
        }
        assert!(matches!(
            registry.select(partial, raw(42), unavailable),
            Err(Refusal::TessMissing)
        ));
        assert!(matches!(
            registry.select(with_gs, raw(42), unavailable),
            Err(Refusal::Geometry)
        ));
        assert!(matches!(
            registry.select(chain, raw(42), |_, _, _| false),
            Err(Refusal::TessUnlinked)
        ));
        registry.clear_links();
        let selected = registry
            .select(chain, raw(42), |vs, tess, ps| {
                let (hs, ds) = tess.expect("tessellated");
                [vs, hs, ds, ps].map(|s| s.shader.as_ptr() as usize) == [11, 13, 14, 12]
            })
            .ok()
            .unwrap();
        assert!(selected.tessellated());
        assert_eq!(selected.shaders().ds, raw(14));
    }
}
