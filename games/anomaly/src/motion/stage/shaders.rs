// SPDX-License-Identifier: MIT
use super::*;
use crate::digest::sha256;
use crate::shader_loader::{Bytes, Entry, Manifest};
use std::collections::HashMap;
use std::sync::Arc;

const NOT_ALLOWLISTED: u8 = 0;
const CREATE_FAILED: u8 = 9;
const NAMES_LIMIT: usize = 16_384;

pub(super) const OUTCOME_NAMES: [&str; 10] = [
    "not_allowlisted",
    "replaced",
    "replaced_from_cache",
    "unknown_caller",
    "unreadable",
    "original_compile_failed",
    "original_mismatch",
    "motion_compile_failed",
    "reflection_mismatch",
    "create_failed",
];
pub(super) const FAMILY_NAMES: [&str; 8] = [
    "other",
    "core_static",
    "core_model",
    "core_pixel",
    "foliage_tree",
    "foliage_detail",
    "tess_hull",
    "tess_domain",
];

pub(in crate::motion) struct Variant {
    stock: Lease,
    shader: Lease,
    reflection: Lease,
    request: Arc<Request>,
    device: Lease,
    stage: u8,
    pub family: u8,
    pub skin: i32,
    pub(super) foliage: FoliageLayout,
}

// Requests own a sealed copy of original DXBC and the guarded compiler frame.
// Their storage is immutable once published. D3D11 shader references and
// plain reflection data can be released after the draw on any caller thread.
struct Request {
    data: Lease,
    hash: [u8; 64],
}
unsafe impl Send for Request {}
unsafe impl Sync for Request {}
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
        link: impl FnOnce(Raw, Raw, Raw, Raw) -> bool,
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
            let (hs, ds) = tess
                .as_ref()
                .map_or((ptr::null_mut(), ptr::null_mut()), |(hs, ds)| {
                    (hs.reflection.as_ptr(), ds.reflection.as_ptr())
                });
            link(
                vertex.reflection.as_ptr(),
                hs,
                ds,
                pixel.reflection.as_ptr(),
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

pub(super) struct Pack {
    lease: Lease,
    // Admitted by Rust and owned by the native pack, which frees it only after
    // `lease` closes the Runtime session. Rows are immutable until then.
    manifest: ptr::NonNull<Manifest>,
}

// The native pack and its compiler/include scratch storage are serialized by
// the stage's loader lock. Its Runtime service remains borrowed. The manifest
// has no interior mutability.
unsafe impl Send for Pack {}

impl Pack {
    pub fn open(platform: &Platform, build: Build, root: &Path) -> Result<Self, String> {
        let pack = wide_path(&root.join("motion.rsfpatch"));
        let cache = wide_path(&root.join("cache"));
        let mut info = MaybeUninit::<PackInfo>::uninit();
        let mut error = [0; 512];
        let handle = unsafe {
            (platform.open_pack)(
                build.image(),
                build.profile(),
                pack.as_ptr(),
                cache.as_ptr(),
                info.as_mut_ptr(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        let lease = Lease::new(handle, platform.close_pack).ok_or_else(|| {
            unsafe { CStr::from_ptr(error.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        })?;
        let info = unsafe { info.assume_init() };
        let manifest = ptr::NonNull::new(info.manifest.cast_mut())
            .ok_or_else(|| String::from("pack manifest unavailable"))?;
        Ok(Self { lease, manifest })
    }

    pub fn ptr(&self) -> Raw {
        self.lease.as_ptr()
    }

    fn manifest(&self) -> &Manifest {
        // The native pack keeps the manifest alive while `self.lease` is open.
        unsafe { self.manifest.as_ref() }
    }

    fn entry(&self, hash: &[u8; 64]) -> Option<&Entry> {
        self.manifest().find(hash)
    }

    pub fn counters(&self, platform: &Platform) -> PatchCounters {
        let mut counters = PatchCounters::default();
        unsafe { (platform.patch_counters)(self.ptr(), &mut counters) };
        counters
    }
}

#[derive(Default, Clone, Copy)]
pub(super) struct ShaderCounters {
    pub outcomes: [[u64; 10]; 8],
    pub frame_refusals: u64,
}

#[derive(Default)]
pub(super) struct Loader {
    pub pack: Option<Pack>,
    pub stats: ShaderCounters,
    refused_logs: u32,
    replacement_logs: u32,
}

struct Replacement {
    shader: Lease,
    reflection: Lease,
    skin: i32,
    outcome: u8,
    foliage: FoliageLayout,
}

impl Replacement {
    fn build(
        platform: &Platform,
        pack: &Pack,
        request: &Request,
        entry: &Entry,
        device: Raw,
        attach: bool,
    ) -> Result<Self, (u8, String)> {
        let mut result = ShaderResult::default();
        unsafe { (platform.prepare_shader)(pack.ptr(), request.data.as_ptr(), entry, &mut result) };
        let reflection = Lease::new(result.reflection, platform.drop_reflection);
        if !matches!(result.outcome, 1 | 2) {
            return Err((
                result.outcome,
                String::from_utf8_lossy(unsafe { result.detail.as_slice() }).into_owned(),
            ));
        }
        let Some(reflection) = reflection else {
            return Err((CREATE_FAILED, "reflection storage unavailable".into()));
        };
        if device.is_null() {
            return Err((CREATE_FAILED, "engine shader device unreadable".into()));
        }
        let mut foliage = FoliageLayout::default();
        if entry.stage == b'v' && matches!(entry.family, FOLIAGE_TREE | FOLIAGE_DETAIL) {
            unsafe { (platform.foliage_layout)(reflection.as_ptr(), entry.family, &mut foliage) };
        }
        let mut refused = false;
        let shader = unsafe {
            (platform.create_shader)(
                pack.ptr(),
                request.data.as_ptr(),
                device,
                reflection.as_ptr(),
                attach,
                &mut refused,
            )
        };
        let shader = Lease::new(shader, platform.device.release.unwrap()).ok_or_else(|| {
            (
                CREATE_FAILED,
                if refused {
                    "Runtime D3D11 interception refused"
                } else {
                    "Create*Shader failed"
                }
                .into(),
            )
        })?;
        Ok(Self {
            shader,
            reflection,
            skin: result.skin,
            outcome: result.outcome,
            foliage,
        })
    }

    fn publish(
        self,
        platform: &Platform,
        stock: Raw,
        request: Arc<Request>,
        entry: &Entry,
        device: Raw,
    ) -> Arc<Variant> {
        let release = platform.device.release.unwrap();
        let retain = platform.device.retain.unwrap();
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
            stage: entry.stage,
            family: entry.family,
            skin: self.skin,
            foliage: self.foliage,
        })
    }
}

pub(super) struct Creation<'a> {
    pub stage: u8,
    pub target: *const c_char,
    pub bytecode: Raw,
    pub size: u32,
    pub file: &'a [u8],
    pub stock: Raw,
    pub return_address: Raw,
    pub stack: Raw,
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
        let p = self.platform;
        unsafe { (p.activate_pack)(pack.ptr()) };
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
        let p = self.platform;
        if dump {
            let path = wide_path(&root.join("cache").join("shader-dump"));
            let result = unsafe { (p.dump_shaders)(old_pack.ptr(), path.as_ptr()) };
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
        let pack = Pack::open(p, build, root)?;
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
            let replacement = Replacement::build(p, &pack, &old.request, entry, device, false)
                .map_err(|(outcome, detail)| {
                    format!("{}: {detail}", OUTCOME_NAMES[outcome as usize])
                })?;
            let key = old.stock.as_ptr();
            next.insert(
                key as usize,
                replacement.publish(p, key, old.request.clone(), entry, device),
            );
        }
        Ok((pack, next))
    }

    pub(super) fn shader_created(&self, call: Creation<'_>) {
        let p = self.platform;
        lock(&self.registry).remember(call.stock, call.file);
        let mut guard = lock(&self.loader);
        // Split the guard so a borrowed manifest row can outlive counter updates.
        let loader = &mut *guard;
        let Some(pack) = &loader.pack else { return };
        let mut status = 0;
        let mut source = Bytes::from(&[][..]);
        let request = unsafe {
            (p.decode_request)(
                call.target,
                call.bytecode,
                call.size,
                call.return_address,
                call.stack,
                &mut status,
                &mut source,
            )
        };
        let Some(data) = Lease::new(request, p.drop_request) else {
            return;
        };
        let file = String::from_utf8_lossy(call.file);
        if status != 0 {
            loader.stats.frame_refusals += 1;
            if loader.refused_logs < 32 {
                loader.refused_logs += 1;
                self.log().say(format!(
                    "Anomaly motion shader frame refused (status {status}): {file}"
                ));
            }
            return;
        }
        let Some(hash) = sha256(unsafe { source.as_slice() }) else {
            return;
        };
        let Some(entry) = pack.entry(&hash) else {
            loader.stats.outcomes[0][NOT_ALLOWLISTED as usize] += 1;
            return;
        };
        let family = entry.family as usize;
        let name = String::from_utf8_lossy(entry.name()).into_owned();
        let request = Request { data, hash };
        let device = unsafe { (p.shader_device)() };
        let result = if entry.stage == call.stage {
            Replacement::build(p, pack, &request, entry, device, true)
        } else {
            Err((NOT_ALLOWLISTED, String::new()))
        };
        let result = result.and_then(|replacement| {
            if unsafe { (p.copy_bytecode)(request.data.as_ptr()) } {
                Ok(replacement)
            } else {
                Err((
                    CREATE_FAILED,
                    "original bytecode could not be retained".into(),
                ))
            }
        });
        let outcome = match result {
            Ok(replacement) => {
                if matches!(entry.family, FOLIAGE_TREE | FOLIAGE_DETAIL)
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
                        FAMILY_NAMES[family], replacement.skin, OUTCOME_NAMES[outcome as usize]
                    ));
                }
                let variant = replacement.publish(p, call.stock, Arc::new(request), entry, device);
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
                        OUTCOME_NAMES[outcome as usize]
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

    fn raw(value: usize) -> Raw {
        value as Raw
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
            reflection: lease(replacement),
            request: Arc::new(Request {
                data: lease(stock),
                hash: [0; 64],
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
            .select(pair, raw(42), |vs, hs, ds, ps| {
                [vs as usize, hs as usize, ds as usize, ps as usize] == [11, 0, 0, 21]
            })
            .ok()
            .unwrap();
        assert!(!chain.tessellated());
        assert!(
            registry
                .select(pair, raw(42), |_, _, _, _| panic!("cached link"))
                .is_ok()
        );
        assert!(matches!(
            registry.select(pair, raw(43), |_, _, _, _| panic!("foreign device")),
            Err(Refusal::Missing)
        ));
        drop(registry.put(variant(1, 12, 42, CORE_MODEL, b'v')));
        assert!(weak.upgrade().is_some());
        assert_eq!(chain.shaders().vs, raw(11));
        assert!(matches!(
            registry.select(pair, raw(42), |_, _, _, _| false),
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
        let unavailable = |_, _, _, _| panic!("an incomplete or unsafe chain cannot be linked");
        assert!(matches!(
            registry.select(partial, raw(42), unavailable),
            Err(Refusal::TessMissing)
        ));
        assert!(matches!(
            registry.select(with_gs, raw(42), unavailable),
            Err(Refusal::Geometry)
        ));
        assert!(matches!(
            registry.select(chain, raw(42), |_, _, _, _| false),
            Err(Refusal::TessUnlinked)
        ));
        registry.clear_links();
        let selected = registry
            .select(chain, raw(42), |vs, hs, ds, ps| {
                [vs as usize, hs as usize, ds as usize, ps as usize] == [11, 13, 14, 12]
            })
            .ok()
            .unwrap();
        assert!(selected.tessellated());
        assert_eq!(selected.shaders().ds, raw(14));
    }
}
