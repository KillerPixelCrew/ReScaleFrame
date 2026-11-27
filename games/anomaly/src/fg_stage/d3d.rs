// SPDX-License-Identifier: MIT
//! Borrowed engine/DXGI interfaces and owned references for one FG handoff.
use super::Raw;
#[cfg(test)]
use crate::lease::Lease;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::Interface;

pub(super) enum Resource<T> {
    Native(T),
    #[cfg(test)]
    Fixture(Lease),
}

impl<T: Interface> Resource<T> {
    pub(super) fn as_raw(&self) -> Raw {
        match self {
            Self::Native(resource) => resource.as_raw(),
            #[cfg(test)]
            Self::Fixture(resource) => resource.as_ptr(),
        }
    }
}

#[cfg(test)]
pub(super) struct Fixture {
    pub immediate_context: unsafe extern "C" fn(Raw) -> Raw,
    pub backbuffer: unsafe extern "C" fn(Raw) -> Raw,
    pub output_window: unsafe extern "C" fn(Raw, *mut usize) -> bool,
    pub release: unsafe extern "C" fn(Raw),
}

#[derive(Default)]
pub(super) struct Graphics {
    #[cfg(test)]
    pub fixture: Option<Fixture>,
}

impl Graphics {
    pub(super) unsafe fn immediate_context(
        &self,
        chain: Raw,
    ) -> Option<Resource<ID3D11DeviceContext>> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return Lease::new(
                unsafe { (fixture.immediate_context)(chain) },
                fixture.release,
            )
            .map(Resource::Fixture);
        }
        let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }?;
        let device = unsafe { chain.GetDevice::<ID3D11Device>() }.ok()?;
        unsafe { device.GetImmediateContext() }
            .ok()
            .map(Resource::Native)
    }

    pub(super) unsafe fn backbuffer(&self, chain: Raw) -> Option<Resource<ID3D11Texture2D>> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return Lease::new(unsafe { (fixture.backbuffer)(chain) }, fixture.release)
                .map(Resource::Fixture);
        }
        let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }?;
        unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }
            .ok()
            .map(Resource::Native)
    }

    pub(super) unsafe fn output_window(&self, chain: Raw) -> Option<usize> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            let mut window = 0;
            return unsafe { (fixture.output_window)(chain, &mut window) }.then_some(window);
        }
        let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }?;
        unsafe { chain.GetDesc() }
            .ok()
            .map(|desc| desc.OutputWindow.0 as usize)
    }
}

pub(super) unsafe fn depth_texture(view: Raw) -> Option<ID3D11Texture2D> {
    let view = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&view) }?;
    unsafe { view.GetResource() }.ok()?.cast().ok()
}
