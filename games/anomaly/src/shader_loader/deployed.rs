// SPDX-License-Identifier: MIT
//! Deployed-tree shader preparation for the standalone motion device fixtures.
//! The fixtures compile the stock permutation from `deployment/original`, hand it in as the engine
//! bytecode and prepare its variant from `deployment/motion` through the production tree path.
use super::d3d::{Modules, compile_tree};
use super::reflection::{Snapshot, reflect, rsf_anomaly_shader_snapshot_keeps_buffer_sizes};
use super::{
    Bytes, Define, Manifest, Request, Span, manifest, prepare_tree,
    rsf_anomaly_shader_match_request, rsf_anomaly_shader_skin_mode,
};
use std::path::Path;

pub(crate) const NOT_ALLOWLISTED: u8 = 0;
pub(crate) const REPLACED: u8 = 1;
pub(crate) const REPLACED_FROM_CACHE: u8 = 2;
pub(crate) const ORIGINAL_MISMATCH: u8 = 6;

/// One permutation as `CResourceManager` asks for it: entry `main`, row-major packing.
pub(crate) struct ShaderRequest<'a> {
    pub(crate) target: &'a str,
    pub(crate) defines: &'a [(String, String)],
    pub(crate) source: &'a [u8],
}

pub(crate) struct Prepared {
    pub(crate) outcome: u8,
    pub(crate) family: Option<u8>,
    /// -1 SKIN_NONE, 0..4 SKIN_n, -2 none of the skin defines.
    pub(crate) skin: i32,
    pub(crate) variant: Vec<u8>,
    /// Reflection of the variant.
    pub(crate) reflection: Option<Snapshot>,
    /// Reflection of the engine bytecode.
    pub(crate) stock: Option<Snapshot>,
    pub(crate) detail: String,
}

impl Default for Prepared {
    fn default() -> Self {
        Self {
            outcome: NOT_ALLOWLISTED,
            family: None,
            skin: -2,
            variant: Vec::new(),
            reflection: None,
            stock: None,
            detail: String::new(),
        }
    }
}

pub(crate) fn outcome_name(outcome: u8) -> &'static str {
    super::outcome_name(outcome)
}

pub(crate) fn parse_manifest(text: &[u8]) -> Result<Manifest, String> {
    manifest::parse(text)
}

pub(crate) fn keeps_buffer_sizes(stock: &Snapshot, variant: &Snapshot) -> bool {
    rsf_anomaly_shader_snapshot_keeps_buffer_sizes(Some(stock), Some(variant))
}

/// D3D10_SHADER_PACK_MATRIX_ROW_MAJOR, what the engine passes for every stage.
const FLAGS: u32 = 8;

fn with_request<R>(
    input: &ShaderRequest<'_>,
    bytecode: &[u8],
    run: impl FnOnce(&Request) -> R,
) -> R {
    let defines: Vec<Define> = input
        .defines
        .iter()
        .map(|(name, value)| Define {
            name: Bytes::from(name.as_bytes()),
            value: Bytes::from(value.as_bytes()),
        })
        .collect();
    let request = Request {
        target: Bytes::from(input.target.as_bytes()),
        entry: Bytes::from(&b"main"[..]),
        flags: FLAGS,
        defines: Span::from(defines.as_slice()),
        source: Bytes::from(input.source),
        bytecode: Bytes::from(bytecode),
    };
    run(&request)
}

/// The engine's compiler and the system reflector, loaded once per fixture.
pub(crate) struct Deployed {
    modules: Modules,
}

impl Deployed {
    /// `compiler` is a NUL-terminated UTF-16 module name.
    pub(crate) fn load(compiler: &[u16]) -> Option<Self> {
        if compiler.last() != Some(&0) {
            return None;
        }
        let modules = unsafe { Modules::load(compiler.as_ptr(), true) };
        (modules.compile().is_some() && modules.reflect().is_some()).then_some(Self { modules })
    }

    pub(crate) fn compile(
        &self,
        tree: &Path,
        input: &ShaderRequest<'_>,
    ) -> Result<Vec<u8>, String> {
        with_request(input, &[], |request| {
            compile_tree(self.modules.compile(), tree, request, input.source)
        })
        .map_err(|error| String::from_utf8_lossy(&error).into_owned())
    }

    pub(crate) fn reflect(&self, code: &[u8]) -> Option<Snapshot> {
        reflect(self.modules.reflect(), code)
    }

    /// Prepares the motion variant for `bytecode`, the simulated engine bytecode of `input`.
    pub(crate) fn prepare(
        &self,
        manifest: &Manifest,
        deployment: &Path,
        input: &ShaderRequest<'_>,
        bytecode: &[u8],
    ) -> Prepared {
        with_request(input, bytecode, |request| {
            let Some(entry) = (unsafe {
                rsf_anomaly_shader_match_request(Some(manifest), request.source, request.target)
            }) else {
                return Prepared::default();
            };
            let id = entry.identity(manifest.namespace());
            let (original, motion) = (deployment.join("original"), deployment.join("motion"));
            let result = prepare_tree(
                request,
                &id,
                deployment,
                None,
                |_, code| reflect(self.modules.reflect(), code),
                Snapshot::view,
                |patched, code| {
                    let tree = if patched { &motion } else { &original };
                    compile_tree(self.modules.compile(), tree, request, code)
                },
            );
            Prepared {
                outcome: result.outcome as u8,
                family: Some(entry.family),
                skin: unsafe { rsf_anomaly_shader_skin_mode(request.defines) },
                variant: result.bytes,
                reflection: result.reflection,
                stock: result.stock,
                detail: String::from_utf8_lossy(&result.detail).into_owned(),
            }
        })
    }

    /// Two independent original-tree compiles: the first supplies the engine bytecode, the second
    /// (inside `prepare`) proves it before the variant is built.
    pub(crate) fn build(
        &self,
        manifest: &Manifest,
        deployment: &Path,
        input: &ShaderRequest<'_>,
    ) -> Result<(Vec<u8>, Prepared), String> {
        let stock = self.compile(&deployment.join("original"), input)?;
        let prepared = self.prepare(manifest, deployment, input, &stock);
        Ok((stock, prepared))
    }
}
