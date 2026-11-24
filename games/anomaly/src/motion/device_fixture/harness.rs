// SPDX-License-Identifier: MIT
//! Device, readback, binding and deployed-shader helpers shared by the motion device fixtures.
use super::draw::{
    self, Bindings, Input, Output, Override, Targets as RawTargets,
    rsf_anomaly_motion_fixture_bind_core, rsf_anomaly_motion_fixture_bind_stage,
    rsf_anomaly_motion_fixture_bindings_drop, rsf_anomaly_motion_fixture_bindings_new,
    rsf_anomaly_motion_fixture_make_layout, rsf_anomaly_motion_fixture_make_targets,
    rsf_anomaly_motion_fixture_patch_core,
};
use crate::motion_device::Owner;
use crate::shader_loader::deployed::{
    Deployed, Prepared, REPLACED, REPLACED_FROM_CACHE, ShaderRequest, outcome_name, parse_manifest,
};
use crate::shader_loader::{Manifest, Snapshot};
use crate::sys::{RSF_MOTION_CREATED, rsf_anomaly_motion_device_view as View};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;

/// motion_profile: both executables import the June 2010 d3dcompiler_43, not _47.
pub(super) const ENGINE_COMPILER: &str = "d3dcompiler_43.dll";
pub(super) const SIZE: u32 = 64;
pub(super) const PIXELS: usize = (SIZE * SIZE) as usize;
pub(super) type Outcome<T = ()> = Result<T, String>;
pub(super) type Half4 = [u16; 4];
pub(super) type Defines = Vec<(String, String)>;
/// Engine constants by variable name. Every matrix is identity and everything else 0.1 unless named here.
pub(super) type Overrides = BTreeMap<&'static str, Vec<f32>>;

static CHECKS: AtomicU32 = AtomicU32::new(0);

pub(super) fn count() {
    CHECKS.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn checks() -> u32 {
    CHECKS.load(Ordering::Relaxed)
}

/// Counts one check and returns the formatted failure when it does not hold.
macro_rules! ensure {
    ($ok:expr, $($message:tt)+) => {{
        $crate::motion::device_fixture::harness::count();
        let ok: bool = $ok;
        if !ok {
            return Err(format!($($message)+));
        }
    }};
}
pub(super) use ensure;

fn message(text: *const c_char) -> String {
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

fn refusal(error: *const c_char) -> Outcome {
    if error.is_null() {
        Ok(())
    } else {
        Err(message(error))
    }
}

pub(super) fn define(name: impl Into<String>, value: impl Into<String>) -> (String, String) {
    (name.into(), value.into())
}

/// The defines CResourceManager adds to every G-buffer permutation.
pub(super) fn engine_defines() -> Defines {
    vec![
        define("SMAP_size", "2048"),
        define("USE_BRANCHING", "1"),
        define("USE_VTF", "1"),
        define("SM_5", "1"),
    ]
}

pub(super) fn target_of(stage: u8) -> &'static str {
    match stage {
        b'v' => "vs_5_0",
        b'h' => "hs_5_0",
        b'd' => "ds_5_0",
        _ => "ps_5_0",
    }
}

pub(super) use rsf_test_support::device::half_to_f32 as half;

pub(super) fn zero(value: &Half4) -> bool {
    value.iter().all(|&lane| lane == 0)
}

/// The position target's clear value: the pixel was not covered.
pub(super) fn uncovered(position: &Half4) -> bool {
    half(position[0]) == -999.
}

pub(super) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// RSF_ANOMALY_MOTION_DEPLOY, else the first `.local/anomaly/<candidate>` with a manifest, else the last candidate.
pub(super) fn deployment(candidates: &[&str]) -> PathBuf {
    if let Some(path) =
        std::env::var_os("RSF_ANOMALY_MOTION_DEPLOY").filter(|path| !path.is_empty())
    {
        return PathBuf::from(path);
    }
    let local = repo_root().join(".local/anomaly");
    candidates
        .iter()
        .map(|name| local.join(name))
        .find(|path| path.join("manifest.txt").exists())
        .unwrap_or_else(|| local.join(candidates.last().copied().unwrap_or("motion-deploy")))
}

pub(super) fn file_bytes(path: &Path) -> Outcome<Vec<u8>> {
    count();
    std::fs::read(path).map_err(|_| format!("missing {}", path.display()))
}

pub(super) struct Gpu {
    pub(super) device: ID3D11Device,
    pub(super) context: ID3D11DeviceContext,
}

impl Gpu {
    pub(super) fn context_raw(&self) -> *mut c_void {
        self.context.as_raw()
    }

    /// Error and corruption messages since the last call, with the first description.
    pub(super) fn debug_errors(&self) -> (u64, String) {
        let Ok(queue) = self.device.cast::<ID3D11InfoQueue>() else {
            return (0, String::new());
        };
        let mut errors = 0;
        let mut first = String::new();
        for index in 0..unsafe { queue.GetNumStoredMessages() } {
            let mut size = 0;
            if unsafe { queue.GetMessage(index, None, &mut size) }.is_err()
                || size < size_of::<D3D11_MESSAGE>()
            {
                continue;
            }
            // The SDK writes the header and its description in one allocation.
            let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
            let message = storage.as_mut_ptr().cast::<D3D11_MESSAGE>();
            if unsafe { queue.GetMessage(index, Some(message), &mut size) }.is_err() {
                continue;
            }
            let message = unsafe { &*message };
            if !matches!(
                message.Severity,
                D3D11_MESSAGE_SEVERITY_ERROR | D3D11_MESSAGE_SEVERITY_CORRUPTION
            ) {
                continue;
            }
            if errors == 0 && !message.pDescription.is_null() {
                let text = unsafe {
                    std::slice::from_raw_parts(
                        message.pDescription.cast::<u8>(),
                        message.DescriptionByteLength,
                    )
                };
                first = String::from_utf8_lossy(text).trim_end_matches('\0').into();
            }
            errors += 1;
        }
        unsafe { queue.ClearStoredMessages() };
        (errors, first)
    }

    pub(super) fn require_clean_debug_layer(&self, what: &str) -> Outcome {
        let (errors, first) = self.debug_errors();
        ensure!(errors == 0, "{what}: debug layer error: {first}");
        Ok(())
    }

    /// The fixture texture as `SIZE * SIZE` elements of `T`.
    pub(super) fn read<T: Copy + Default>(&self, texture: *mut c_void) -> Outcome<Vec<T>> {
        let mut out = vec![T::default(); PIXELS];
        let ok = unsafe {
            draw::rsf_anomaly_motion_fixture_read(
                self.device.as_raw(),
                self.context.as_raw(),
                texture,
                out.as_mut_ptr().cast(),
                size_of_val(out.as_slice()),
            )
        };
        if ok {
            Ok(out)
        } else {
            Err("staging readback failed".into())
        }
    }

    pub(super) fn clear(&self, output: &Output, stencil: bool) {
        unsafe { draw::rsf_anomaly_motion_fixture_clear(self.context_raw(), output, stencil) };
    }

    pub(super) fn bind_input(&self, input: &Input) {
        unsafe { draw::rsf_anomaly_motion_fixture_input_bind(self.context_raw(), input) };
    }

    pub(super) fn bind_output(&self, output: &Output) {
        unsafe { draw::rsf_anomaly_motion_fixture_output_bind(self.context_raw(), output) };
    }

    pub(super) fn draw(&self, count: u32) {
        unsafe { draw::rsf_anomaly_motion_fixture_draw(self.context_raw(), count) };
    }

    pub(super) fn draw_quad(&self, buffer: &ID3D11Buffer, stride: u32) {
        unsafe {
            draw::rsf_anomaly_motion_fixture_draw_quad(self.context_raw(), buffer.as_raw(), stride)
        };
    }

    pub(super) fn vertex_buffer<T: Copy>(&self, data: &[T]) -> Outcome<ID3D11Buffer> {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(size_of_val(data)).map_err(|_| "vertex buffer too large")?,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            ..Default::default()
        };
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr().cast(),
            ..Default::default()
        };
        let mut buffer = None;
        unsafe {
            self.device
                .CreateBuffer(&desc, Some(&init), Some(&mut buffer))
        }
        .map_err(|_| "vertex buffer failed")?;
        buffer.ok_or_else(|| "vertex buffer failed".into())
    }

    pub(super) fn layout(&self, signature: &Signature, stock: &[u8]) -> Outcome<ID3D11InputLayout> {
        let mut raw = null_mut();
        refusal(unsafe {
            rsf_anomaly_motion_fixture_make_layout(
                self.device.as_raw(),
                signature.inputs.as_ptr(),
                signature.inputs.len() as u32,
                stock.as_ptr(),
                stock.len(),
                &mut raw,
            )
        })?;
        Ok(unsafe { ID3D11InputLayout::from_raw(raw) })
    }
}

fn created<T>(result: windows::core::Result<()>, value: Option<T>, what: &str) -> Outcome<T> {
    result
        .ok()
        .and(value)
        .ok_or_else(|| format!("{what} failed"))
}

/// The four stage objects of one shader set.
pub(super) fn vertex_shader(gpu: &Gpu, code: &[u8], what: &str) -> Outcome<ID3D11VertexShader> {
    let mut shader = None;
    let result = unsafe { gpu.device.CreateVertexShader(code, None, Some(&mut shader)) };
    created(result, shader, what)
}

pub(super) fn hull_shader(gpu: &Gpu, code: &[u8], what: &str) -> Outcome<ID3D11HullShader> {
    let mut shader = None;
    let result = unsafe { gpu.device.CreateHullShader(code, None, Some(&mut shader)) };
    created(result, shader, what)
}

pub(super) fn domain_shader(gpu: &Gpu, code: &[u8], what: &str) -> Outcome<ID3D11DomainShader> {
    let mut shader = None;
    let result = unsafe { gpu.device.CreateDomainShader(code, None, Some(&mut shader)) };
    created(result, shader, what)
}

pub(super) fn pixel_shader(gpu: &Gpu, code: &[u8], what: &str) -> Outcome<ID3D11PixelShader> {
    let mut shader = None;
    let result = unsafe { gpu.device.CreatePixelShader(code, None, Some(&mut shader)) };
    created(result, shader, what)
}

pub(super) fn raw<T: Interface>(value: Option<&T>) -> *mut c_void {
    value.map_or(null_mut(), Interface::as_raw)
}

/// SDK reflection of bytecode, kept alive with its input signature rows (their names borrow it).
pub(super) struct Signature {
    pub(super) reflection: ID3D11ShaderReflection,
    pub(super) inputs: Vec<D3D11_SIGNATURE_PARAMETER_DESC>,
}

impl Signature {
    pub(super) fn new(code: &[u8]) -> Outcome<Self> {
        let mut raw = null_mut();
        let status = unsafe {
            D3DReflect(
                code.as_ptr().cast(),
                code.len(),
                &ID3D11ShaderReflection::IID,
                &mut raw,
            )
        };
        if status.is_err() || raw.is_null() {
            return Err("reflect failed".into());
        }
        let reflection = unsafe { ID3D11ShaderReflection::from_raw(raw) };
        let mut desc = D3D11_SHADER_DESC::default();
        unsafe { reflection.GetDesc(&mut desc) }.map_err(|_| "reflect VS")?;
        let mut inputs = Vec::with_capacity(desc.InputParameters as usize);
        for index in 0..desc.InputParameters {
            let mut row = D3D11_SIGNATURE_PARAMETER_DESC::default();
            unsafe { reflection.GetInputParameterDesc(index, &mut row) }
                .map_err(|_| "reflect VS input")?;
            inputs.push(row);
        }
        Ok(Self { reflection, inputs })
    }

    pub(super) fn semantic(&self, index: usize) -> &[u8] {
        let name = self.inputs[index].SemanticName;
        if name.is_null() {
            &[]
        } else {
            unsafe { CStr::from_ptr(name.0.cast()) }.to_bytes()
        }
    }

    /// One float4 per input element.
    pub(super) fn stride(&self) -> u32 {
        self.inputs.len() as u32 * 16
    }
}

pub(super) struct FixtureTargets {
    pub(super) position: ID3D11Texture2D,
    pub(super) position_rtv: ID3D11RenderTargetView,
    pub(super) albedo: ID3D11Texture2D,
    pub(super) albedo_rtv: ID3D11RenderTargetView,
    pub(super) depth: ID3D11Texture2D,
    pub(super) dsv: ID3D11DepthStencilView,
    pub(super) less: ID3D11DepthStencilState,
    pub(super) raster: ID3D11RasterizerState,
}

impl FixtureTargets {
    /// RGBA16F position, RGBA8 albedo and a D24S8 (`stencil`) or D32 depth target.
    pub(super) fn new(gpu: &Gpu, stencil: bool) -> Outcome<Self> {
        let mut raw = MaybeUninit::<RawTargets>::uninit();
        refusal(unsafe {
            rsf_anomaly_motion_fixture_make_targets(gpu.device.as_raw(), SIZE, stencil, &mut raw)
        })?;
        // All eight references transfer together on success.
        let raw = unsafe { raw.assume_init() };
        unsafe {
            Ok(Self {
                position: ID3D11Texture2D::from_raw(raw.position_texture),
                position_rtv: ID3D11RenderTargetView::from_raw(raw.position_rtv),
                albedo: ID3D11Texture2D::from_raw(raw.albedo_texture),
                albedo_rtv: ID3D11RenderTargetView::from_raw(raw.albedo_rtv),
                depth: ID3D11Texture2D::from_raw(raw.depth_texture),
                dsv: ID3D11DepthStencilView::from_raw(raw.depth_view),
                less: ID3D11DepthStencilState::from_raw(raw.depth_state),
                raster: ID3D11RasterizerState::from_raw(raw.raster),
            })
        }
    }

    pub(super) fn output(&self, motion: *mut c_void) -> Output {
        Output {
            position: self.position_rtv.as_raw(),
            albedo: self.albedo_rtv.as_raw(),
            motion,
            depth: self.dsv.as_raw(),
            depth_state: self.less.as_raw(),
            raster: self.raster.as_raw(),
            size: SIZE,
        }
    }

    pub(super) fn position_desc(&self) -> D3D11_TEXTURE2D_DESC {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { self.position.GetDesc(&mut desc) };
        desc
    }
}

/// Engine constant buffers, textures and samplers bound for one draw.
pub(super) struct Keep(*mut Bindings);

impl Keep {
    pub(super) fn new() -> Self {
        Self(rsf_anomaly_motion_fixture_bindings_new())
    }

    fn bindings(&mut self) -> &mut Bindings {
        unsafe { &mut *self.0 }
    }

    /// Binds the engine resources `code` reflects for `stage` ('v', 'h', 'd' or 'p').
    pub(super) fn bind_stage(
        &mut self,
        gpu: &Gpu,
        stage: u8,
        tessellated: bool,
        code: &[u8],
        overrides: &Overrides,
    ) -> Outcome {
        let (_names, rows) = override_rows(overrides);
        refusal(unsafe {
            rsf_anomaly_motion_fixture_bind_stage(
                self.bindings(),
                gpu.device.as_raw(),
                gpu.context.as_raw(),
                stage,
                tessellated,
                code.as_ptr().cast(),
                code.len(),
                rows.as_ptr(),
                rows.len(),
            )
        })
    }

    pub(super) fn bind_core(
        &mut self,
        gpu: &Gpu,
        reflection: &ID3D11ShaderReflection,
        vertex: bool,
    ) -> Outcome {
        refusal(unsafe {
            rsf_anomaly_motion_fixture_bind_core(
                self.bindings(),
                gpu.device.as_raw(),
                gpu.context.as_raw(),
                reflection.as_raw(),
                vertex,
            )
        })
    }

    /// Rewrites the named variables of the bound VS constant buffers.
    pub(super) fn patch_core(
        &mut self,
        gpu: &Gpu,
        reflection: &ID3D11ShaderReflection,
        overrides: &Overrides,
    ) -> Outcome {
        let (_names, rows) = override_rows(overrides);
        refusal(unsafe {
            rsf_anomaly_motion_fixture_patch_core(
                self.bindings(),
                gpu.device.as_raw(),
                gpu.context.as_raw(),
                reflection.as_raw(),
                rows.as_ptr(),
                rows.len(),
            )
        })
    }
}

impl Drop for Keep {
    fn drop(&mut self) {
        unsafe { rsf_anomaly_motion_fixture_bindings_drop(self.0) };
    }
}

fn override_rows(overrides: &Overrides) -> (Vec<CString>, Vec<Override>) {
    let names: Vec<CString> = overrides
        .keys()
        .map(|name| CString::new(*name).expect("override names have no NUL"))
        .collect();
    let rows = names
        .iter()
        .zip(overrides.values())
        .map(|(name, values)| Override {
            name: name.as_ptr(),
            values: values.as_ptr(),
            count: values.len(),
        })
        .collect();
    (names, rows)
}

/// The Rust motion owner with its RGBA16F target sized to the position target.
pub(super) struct Motion(Box<Owner>);

impl Motion {
    pub(super) fn new(gpu: &Gpu, targets: &FixtureTargets) -> Outcome<Self> {
        let mut owner = Box::new(Owner::new());
        ensure!(
            owner.create(gpu.device.as_raw(), None, None, null_mut()),
            "Rust motion owner create"
        );
        let mut motion = Self(owner);
        let desc = targets.position_desc();
        ensure!(
            motion.target(
                desc.Width,
                desc.Height,
                desc.SampleDesc.Count,
                desc.ArraySize
            ) == RSF_MOTION_CREATED,
            "motion target created"
        );
        Ok(motion)
    }

    pub(super) fn target(&mut self, width: u32, height: u32, samples: u32, layers: u32) -> i32 {
        self.0.target(width, height, samples, layers)
    }

    pub(super) fn view(&self) -> View {
        self.0.view()
    }

    pub(super) fn upload(&mut self, gpu: &Gpu, kind: i32, bytes: &[u8]) -> bool {
        self.0.upload(gpu.context_raw(), kind, bytes)
    }

    pub(super) fn owner(&mut self) -> &mut Owner {
        &mut self.0
    }
}

pub(super) struct Built {
    pub(super) stage: u8,
    pub(super) stock: Vec<u8>,
    pub(super) prepared: Prepared,
}

impl Built {
    pub(super) fn reflection(&self) -> Outcome<&Snapshot> {
        self.prepared
            .reflection
            .as_ref()
            .ok_or_else(|| "variant reflection missing".into())
    }

    pub(super) fn stock_reflection(&self) -> Outcome<&Snapshot> {
        self.prepared
            .stock
            .as_ref()
            .ok_or_else(|| "stock reflection missing".into())
    }
}

/// The deployed original/motion trees, their admitted manifest and the engine's compiler.
pub(super) struct Shaders {
    pub(super) api: Deployed,
    pub(super) manifest: Manifest,
    pub(super) deployment: PathBuf,
}

impl Shaders {
    pub(super) fn manifest(deployment: &Path) -> Outcome<Manifest> {
        let text = file_bytes(&deployment.join("manifest.txt"))?;
        let manifest = parse_manifest(&text);
        ensure!(
            manifest.is_ok(),
            "manifest: {}",
            manifest.as_ref().err().unwrap()
        );
        manifest
    }

    pub(super) fn source(&self, root: &str) -> Outcome<Vec<u8>> {
        file_bytes(&self.deployment.join("original").join("r3").join(root))
    }

    /// Stock and prepared variant of `root`, without judging the outcome.
    pub(super) fn try_build(
        &self,
        stage: u8,
        root: &str,
        defines: &[(String, String)],
    ) -> Outcome<Built> {
        let source = self.source(root)?;
        let request = ShaderRequest {
            target: target_of(stage),
            defines,
            source: &source,
        };
        let built = self.api.build(&self.manifest, &self.deployment, &request);
        ensure!(
            built.is_ok(),
            "{root} stock: {}",
            built.as_ref().err().unwrap()
        );
        let (stock, prepared) = built?;
        Ok(Built {
            stage,
            stock,
            prepared,
        })
    }

    /// As `try_build`, requiring a replaced (or cached) variant.
    pub(super) fn build(
        &self,
        stage: u8,
        root: &str,
        defines: &[(String, String)],
    ) -> Outcome<Built> {
        let built = self.try_build(stage, root, defines)?;
        let outcome = built.prepared.outcome;
        ensure!(
            outcome == REPLACED || outcome == REPLACED_FROM_CACHE,
            "{root} {}: {} {}",
            target_of(stage),
            outcome_name(outcome),
            built.prepared.detail
        );
        Ok(built)
    }

    pub(super) fn reflect(&self, code: &[u8]) -> Outcome<Snapshot> {
        let snapshot = self.api.reflect(code);
        ensure!(snapshot.is_some(), "reflect");
        Ok(snapshot.unwrap())
    }
}

/// The engine's own compiler, or `None` when it is not installed.
pub(super) fn engine_compiler(name: &str) -> Option<Deployed> {
    Deployed::load(&rsf_game_sys::ffi_util::wide(name))
}

pub(super) fn report(result: Outcome) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            eprintln!("FAILED {error}");
            false
        }
    }
}
