// SPDX-License-Identifier: MIT
//! Hardware fixture for the bidirectional GL/D3D12 handoff on the current WGL context.
use super::{FixtureError, log, require};
use crate::gl_fixture_abi::*;
use std::ffi::{CStr, c_char, c_void};
use std::mem::{ManuallyDrop, transmute};
use std::ptr::null_mut;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, HDC, ReleaseDC};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::DXGI_SAMPLE_DESC, OpenGL::*};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, UnregisterClassW,
    WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
};
use windows::core::{Interface, PCWSTR, s, w};

const FRAMEBUFFER: u32 = 0x8d40;
const COLOR_ATTACHMENT0: u32 = 0x8ce0;
const FRAMEBUFFER_COMPLETE: u32 = 0x8cd5;
const OK: i32 = 0;

unsafe extern "system" fn debug_gl(
    _: u32,
    _: u32,
    _: u32,
    _: u32,
    _: i32,
    text: *const c_char,
    _: *const c_void,
) {
    eprintln!("GL: {}", unsafe { CStr::from_ptr(text) }.to_string_lossy());
}

fn enable_debug(gl_debug: bool) {
    type Callback = unsafe extern "system" fn(
        unsafe extern "system" fn(u32, u32, u32, u32, i32, *const c_char, *const c_void),
        *const c_void,
    );
    unsafe {
        if gl_debug && let Some(proc) = gl_proc(s!("glDebugMessageCallback")) {
            let callback = transmute::<_, Callback>(proc);
            glEnable(0x92e0); // GL_DEBUG_OUTPUT
            glEnable(0x8242); // GL_DEBUG_OUTPUT_SYNCHRONOUS
            callback(debug_gl, std::ptr::null());
        }
        let mut debug: Option<ID3D12Debug> = None;
        if D3D12GetDebugInterface(&mut debug).is_ok()
            && let Some(debug) = debug
        {
            debug.EnableDebugLayer();
        }
    }
}

fn gl_proc(name: windows::core::PCSTR) -> windows::Win32::Foundation::PROC {
    // WGL can return sentinel values for unsupported entry points.
    unsafe { wglGetProcAddress(name) }.filter(|proc| !matches!(*proc as usize, 1..=3 | usize::MAX))
}

pub(super) struct Framebuffer {
    name: u32,
    bind: unsafe extern "system" fn(u32, u32),
    delete: unsafe extern "system" fn(i32, *const u32),
    attach: unsafe extern "system" fn(u32, u32, u32, u32, i32),
    check: unsafe extern "system" fn(u32) -> u32,
}

impl Framebuffer {
    pub(super) fn new() -> Result<Self, FixtureError> {
        macro_rules! load {
            ($name:literal, $ty:ty) => {{
                let proc =
                    gl_proc(s!($name)).ok_or(FixtureError::Fail(concat!("missing ", $name)))?;
                unsafe { transmute::<_, $ty>(proc) }
            }};
        }
        let generate = load!(
            "glGenFramebuffers",
            unsafe extern "system" fn(i32, *mut u32)
        );
        let mut framebuffer = Self {
            name: 0,
            bind: load!("glBindFramebuffer", unsafe extern "system" fn(u32, u32)),
            delete: load!(
                "glDeleteFramebuffers",
                unsafe extern "system" fn(i32, *const u32)
            ),
            attach: load!(
                "glFramebufferTexture2D",
                unsafe extern "system" fn(u32, u32, u32, u32, i32)
            ),
            check: load!(
                "glCheckFramebufferStatus",
                unsafe extern "system" fn(u32) -> u32
            ),
        };
        unsafe { generate(1, &mut framebuffer.name) };
        require(framebuffer.name != 0, "allocate GL framebuffer")?;
        Ok(framebuffer)
    }

    pub(super) fn attach_texture(&self, texture: u32) -> Result<(), FixtureError> {
        unsafe {
            (self.bind)(FRAMEBUFFER, self.name);
            (self.attach)(FRAMEBUFFER, COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture, 0);
            require(
                (self.check)(FRAMEBUFFER) == FRAMEBUFFER_COMPLETE,
                "shared GL framebuffer",
            )
        }
    }

    pub(super) fn unbind(&self) {
        unsafe { (self.bind)(FRAMEBUFFER, 0) };
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            (self.bind)(FRAMEBUFFER, 0);
            (self.delete)(1, &self.name);
        }
    }
}

unsafe fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    from: D3D12_RESOURCE_STATES,
    to: D3D12_RESOURCE_STATES,
) {
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(unsafe {
                    ID3D12Resource::from_raw(resource.as_raw())
                })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: from,
                StateAfter: to,
            }),
        },
        ..Default::default()
    };
    unsafe { list.ResourceBarrier(&[barrier]) };
}

unsafe fn readback(
    device: &ID3D12Device,
    resource: &ID3D12Resource,
) -> Result<(ID3D12Resource, D3D12_PLACED_SUBRESOURCE_FOOTPRINT, usize), FixtureError> {
    let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
    let mut bytes = 0;
    let mut readback = None;
    unsafe {
        device.GetCopyableFootprints(
            &resource.GetDesc(),
            0,
            1,
            0,
            Some(&mut footprint),
            None,
            None,
            Some(&mut bytes),
        );
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_READBACK,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: bytes,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            },
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut readback,
        )
    }
    .map_err(|_| FixtureError::Fail("create verification readback"))?;
    let readback = readback.ok_or(FixtureError::Fail("readback resource missing"))?;
    Ok((readback, footprint, bytes as usize))
}

unsafe fn verify_gl_clear(
    ctx: *mut rsf_gl_context,
    shared: *mut rsf_gl_texture,
    device: &ID3D12Device,
    texture: &ID3D12Resource,
) -> Result<(), FixtureError> {
    let (readback, footprint, bytes) = unsafe { readback(device, texture) }?;
    let mut commands = null_mut();
    let mut slot = 0;
    unsafe {
        glViewport(0, 0, 32, 32);
        glClearColor(0.25, 0.5, 0.75, 1.0);
        glClear(GL_COLOR_BUFFER_BIT);
        require(glGetError() == GL_NO_ERROR, "GL clear shared resource")?;
        require(
            rsf_gl_begin(ctx, 1, &shared, &mut commands, &mut slot) == OK,
            "GL release and D3D12 GPU wait",
        )?;
        let list = ID3D12GraphicsCommandList::from_raw_borrowed(&commands)
            .ok_or(FixtureError::Fail("GL release returned no graphics list"))?;
        transition(
            list,
            texture,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        let from = D3D12_TEXTURE_COPY_LOCATION {
            pResource: ManuallyDrop::new(Some(ID3D12Resource::from_raw(texture.as_raw()))),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        let to = D3D12_TEXTURE_COPY_LOCATION {
            pResource: ManuallyDrop::new(Some(ID3D12Resource::from_raw(readback.as_raw()))),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: footprint,
            },
        };
        list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
        transition(
            list,
            texture,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_COMMON,
        );
        require(
            rsf_gl_submit(ctx, 1, &shared) == OK && rsf_gl_drain(ctx) == OK,
            "D3D12 completion signal and GL acquire",
        )?;
        let mut mapped = null_mut();
        readback
            .Map(
                0,
                Some(&D3D12_RANGE {
                    Begin: 0,
                    End: bytes,
                }),
                Some(&mut mapped),
            )
            .map_err(|_| FixtureError::Fail("map verification readback"))?;
        let pixel = mapped.cast::<[u16; 4]>().read();
        readback.Unmap(0, Some(&D3D12_RANGE::default()));
        require(
            pixel == [0x3400, 0x3800, 0x3a00, 0x3c00],
            "GL clear must reach D3D12 unchanged",
        )
    }
}

unsafe fn verify_d3d12_clear(
    ctx: *mut rsf_gl_context,
    shared: *mut rsf_gl_texture,
    device: &ID3D12Device,
    texture: &ID3D12Resource,
) -> Result<(), FixtureError> {
    let color = [0.8, 0.2, 0.4, 1.0];
    let mut commands = null_mut();
    let mut slot = 0;
    unsafe {
        let rtv = device
            .CreateDescriptorHeap::<ID3D12DescriptorHeap>(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 1,
                ..Default::default()
            })
            .map_err(|_| FixtureError::Fail("RTV heap"))?;
        let descriptor = rtv.GetCPUDescriptorHandleForHeapStart();
        device.CreateRenderTargetView(texture, None, descriptor);
        require(
            rsf_gl_begin(ctx, 1, &shared, &mut commands, &mut slot) == OK,
            "second shared batch",
        )?;
        let list = ID3D12GraphicsCommandList::from_raw_borrowed(&commands)
            .ok_or(FixtureError::Fail("second batch returned no graphics list"))?;
        transition(
            list,
            texture,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        );
        list.ClearRenderTargetView(descriptor, &color, None);
        transition(
            list,
            texture,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_COMMON,
        );
        require(rsf_gl_submit(ctx, 1, &shared) == OK, "D3D12 clear release")?;
        let mut result = [0.0_f32; 4];
        glReadPixels(16, 16, 1, 1, GL_RGBA, GL_FLOAT, result.as_mut_ptr().cast());
        require(
            glGetError() == GL_NO_ERROR && rsf_gl_drain(ctx) == OK,
            "retire shared GPU work",
        )?;
        require(
            result
                .into_iter()
                .zip(color)
                .all(|(sample, expected)| (sample - expected).abs() < 0.001),
            "D3D12 clear must reach GL unchanged",
        )
    }
}

unsafe fn verify() -> Result<(), FixtureError> {
    let setup = rsf_gl_setup {
        struct_size: size_of::<rsf_gl_setup>() as u32,
        abi_version: 1,
        log: Some(log),
        ..Default::default()
    };
    let mut ctx = null_mut();
    unsafe {
        require(
            rsf_gl_open(&setup, &mut ctx) == OK,
            "open same-adapter GL/D3D12 sharing",
        )?;
        let (mut device, mut queue) = (null_mut(), null_mut());
        require(
            rsf_gl_graphics(ctx, &mut device, &mut queue) == OK,
            "borrow D3D12 device",
        )?;
        let device = ID3D12Device::from_raw_borrowed(&device)
            .ok_or(FixtureError::Fail("GL sharing returned no device"))?;
        for size in [32, 854, 1920, 2560, 32] {
            for format in [RSF_GL_RGBA16F, RSF_GL_R32F, RSF_GL_RG16F, RSF_GL_RGBA8] {
                let mut probe = null_mut();
                require(
                    rsf_gl_texture_create(ctx, size, size * 9 / 16, format, &mut probe) == OK,
                    "repeated import of scene/depth/motion/output formats",
                )?;
                require(rsf_gl_drain(ctx) == OK, "retire format probe")?;
                rsf_gl_texture_destroy(probe);
            }
        }
        let mut shared = null_mut();
        require(
            rsf_gl_texture_create(ctx, 32, 32, RSF_GL_RGBA16F, &mut shared) == OK,
            "create shared GL/D3D12 texture",
        )?;
        let mut info = rsf_gl_texture_info {
            struct_size: size_of::<rsf_gl_texture_info>() as u32,
            ..Default::default()
        };
        require(
            rsf_gl_texture_get(shared, &mut info) == OK,
            "shared texture identity",
        )?;
        let framebuffer = Framebuffer::new()?;
        framebuffer.attach_texture(info.texture)?;
        let texture = ID3D12Resource::from_raw_borrowed(&info.resource)
            .ok_or(FixtureError::Fail("shared texture returned no resource"))?;
        verify_gl_clear(ctx, shared, device, texture)?;
        verify_d3d12_clear(ctx, shared, device, texture)?;
        drop(framebuffer);
        rsf_gl_texture_destroy(shared);
        require(rsf_gl_close(ctx) == OK, "close shared graphics")
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub(super) struct Window {
    instance: HINSTANCE,
    pub(super) hwnd: HWND,
    pub(super) dc: HDC,
    pub(super) context: HGLRC,
}

impl Window {
    const CLASS: windows::core::PCWSTR = w!("RSFHiddenGLTest");

    fn create() -> Result<Self, FixtureError> {
        unsafe {
            let instance = GetModuleHandleW(PCWSTR::null())
                .map_err(|_| FixtureError::Fail("get fixture module"))?;
            let instance = HINSTANCE(instance.0);
            require(
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: Self::CLASS,
                    ..Default::default()
                }) != 0,
                "register hidden GL window",
            )?;
            let mut window = Self {
                instance,
                hwnd: HWND::default(),
                dc: HDC::default(),
                context: HGLRC::default(),
            };
            window.hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                Self::CLASS,
                w!("RSF hidden GPU test"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|_| FixtureError::Fail("create hidden GL window"))?;
            window.dc = GetDC(Some(window.hwnd));
            require(!window.dc.is_invalid(), "get GL window DC")?;
            let format = PIXELFORMATDESCRIPTOR {
                nSize: size_of::<PIXELFORMATDESCRIPTOR>() as u16,
                nVersion: 1,
                dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
                iPixelType: PFD_TYPE_RGBA,
                cColorBits: 32,
                cDepthBits: 24,
                ..Default::default()
            };
            let pixel = ChoosePixelFormat(window.dc, &format);
            require(pixel != 0, "choose GL pixel format")?;
            SetPixelFormat(window.dc, pixel, &format)
                .map_err(|_| FixtureError::Fail("set GL pixel format"))?;
            window.context = wglCreateContext(window.dc)
                .map_err(|_| FixtureError::Fail("create WGL context"))?;
            wglMakeCurrent(window.dc, window.context)
                .map_err(|_| FixtureError::Fail("bind WGL context"))?;
            Ok(window)
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            if !self.context.is_invalid() {
                let _ = wglMakeCurrent(HDC::default(), HGLRC::default());
                let _ = wglDeleteContext(self.context);
            }
            if !self.dc.is_invalid() {
                ReleaseDC(Some(self.hwnd), self.dc);
            }
            if !self.hwnd.0.is_null() {
                let _ = DestroyWindow(self.hwnd);
            }
            let _ = UnregisterClassW(Self::CLASS, Some(self.instance));
        }
    }
}

fn gl_string(name: u32) -> String {
    let value = unsafe { glGetString(name) };
    if value.is_null() {
        "unknown".to_owned()
    } else {
        // The driver owns the string for the current WGL context.
        unsafe { CStr::from_ptr(value.cast()) }
            .to_string_lossy()
            .into_owned()
    }
}

fn run() -> Result<(), FixtureError> {
    let window = Window::create()?;
    println!(
        "OpenGL interop GPU: {} / {}",
        gl_string(GL_RENDERER),
        gl_string(GL_VERSION)
    );
    enable_debug(std::env::var_os("RSF_GL_NO_DEBUG").is_none());
    unsafe { verify() }?;
    println!(
        "PASS: OpenGL -> D3D12 and D3D12 -> OpenGL pixels, shared memory, GPU fence ordering."
    );
    crate::gl_pipeline_fixture::run(&window)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_gl_shared_surface_fixture() -> i32 {
    if !std::env::var_os("RSF_RUN_GL_INTEROP_TESTS")
        .is_some_and(|value| value.to_string_lossy().starts_with('1'))
    {
        return 77;
    }
    match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("FAIL: {error:?}");
            1
        }
    }
}
