// SPDX-License-Identifier: MIT
//! FSR1 for the D3D11 SR pipeline: the EASU kernel the D3D12 SR path runs (`sr12/easu.hlsl`),
//! compiled for cs_5_0 from the same source and dispatched from the scene colour into the
//! pipeline's private output at the output extent. Sharpening and reinsertion follow it as they
//! follow every backend.
use crate::d3d11_output::SavedBindings;
use crate::graphics_d3d11::{
    GROUP_EDGE, Handle, compute_shader, constant_buffer, describe_source, dispatch_compute,
    prepare_compute, rsf_d3d11_state_restore, rsf_d3d11_state_save, shader_code, source_view,
    target_view,
};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
    ID3D11Texture2D,
};
use windows::core::Interface;

const SOURCE: &[u8] = include_bytes!("sr12/easu.hlsl");

/// FSR1's published render scale per quality: native, quality, balanced, performance, ultra
/// performance, ultra quality.
const RATIOS: [f32; 6] = [1.0, 1.5, 1.7, 2.0, 3.0, 1.3];

/// The render extent FSR1 plans for `output` at `quality`.
pub(crate) fn plan(output: [u32; 2], quality: u32) -> [u32; 2] {
    let ratio = RATIOS.get(quality as usize).copied().unwrap_or(1.0);
    output.map(|extent| ((extent as f32 / ratio) as u32).max(1))
}

fn code() -> Result<Vec<u8>, String> {
    shader_code(
        SOURCE,
        c"sr12/easu.hlsl",
        c"main",
        c"cs_5_0",
        D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
    )
}

// Built and run only through the pipeline's device leaves, which unit tests replace.
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct Spatial {
    shader: ID3D11ComputeShader,
    constants: ID3D11Buffer,
}

#[cfg_attr(test, allow(dead_code))]
impl Spatial {
    pub(crate) fn new(device: &ID3D11Device) -> Option<Self> {
        let code = code().ok()?;
        Some(Self {
            shader: compute_shader(device, &code)?,
            // input, output and jitter: six dwords in one 16-byte-aligned register pair.
            constants: constant_buffer(device, 32, false)?,
        })
    }

    /// EASU from `source` (render extent) into `target` (output extent), sampling at the
    /// jitter-free position. The caller's bindings are restored.
    ///
    /// # Safety
    /// `context` is the immediate context of the device both textures belong to.
    pub(crate) unsafe fn run(
        &self,
        context: Handle,
        source: Handle,
        target: Handle,
        [input, output, jitter]: [[u32; 2]; 3],
    ) -> bool {
        let Some(immediate) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
            return false;
        };
        let Ok(device) = (unsafe { immediate.GetDevice() }) else {
            return false;
        };
        let Some(info) = (unsafe { describe_source(device.as_raw(), source) }) else {
            return false;
        };
        let view = unsafe { source_view(device.as_raw(), source, info.view_format) };
        if view.is_null() {
            return false;
        }
        // SAFETY: `source_view` returned an owned reference.
        let view = unsafe { ID3D11ShaderResourceView::from_raw(view) };
        let Some(texture) = (unsafe { ID3D11Texture2D::from_raw_borrowed(&target) }) else {
            return false;
        };
        let Some(write) = target_view(&device, texture, 0) else {
            return false;
        };
        let Some(_saved) =
            (unsafe { SavedBindings::new(context, rsf_d3d11_state_save, rsf_d3d11_state_restore) })
        else {
            return false;
        };
        prepare_compute(immediate);
        let words = [
            input[0], input[1], output[0], output[1], jitter[0], jitter[1], 0, 0,
        ];
        unsafe {
            immediate.UpdateSubresource(&self.constants, 0, None, words.as_ptr().cast(), 0, 0)
        };
        let target: [Handle; 1] = [write.as_raw()];
        dispatch_compute(
            immediate,
            &self.shader,
            &[view.as_raw()],
            &target,
            None,
            output.map(|extent| extent.div_ceil(GROUP_EDGE)),
            true,
            Some(self.constants.as_raw()),
        );
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shared_easu_kernel_compiles_for_d3d11() {
        let code = super::code().expect("cs_5_0 bytecode from sr12/easu.hlsl");
        assert_eq!(&code[..4], b"DXBC");
    }

    #[test]
    fn fsr1_plans_its_published_ratios() {
        assert_eq!(super::plan([3840, 2160], 0), [3840, 2160]);
        assert_eq!(super::plan([3840, 2160], 1), [2560, 1440]);
        assert_eq!(super::plan([3840, 2160], 3), [1920, 1080]);
        assert_eq!(super::plan([3840, 2160], 4), [1280, 720]);
        assert_eq!(super::plan([1, 1], 4), [1, 1]);
    }
}
