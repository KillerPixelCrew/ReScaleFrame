// SPDX-License-Identifier: MIT
use super::*;
use crate::fg_inputs::d3d::create_texture;
use crate::sr_stage_abi::{
    rsf_anomaly_sr_camera as ViewCamera, rsf_anomaly_sr_pass_inputs as PassInputs,
};
use crate::sr_temporal::fg_fixture::Temporal;
use std::cell::Cell;
use windows::Win32::Foundation::{HMODULE as Module, HWND as Window, RECT as Rect};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{BOOL, Interface};
use windows_sys::Win32::Foundation::{HWND, MAX_PATH, RECT};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileAttributesA, GetTempPathA, INVALID_FILE_ATTRIBUTES,
};
use windows_sys::Win32::System::Threading::Sleep;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub(super) const WIDTH: u32 = 1280;
pub(super) const HEIGHT: u32 = 720;
const OUTPUT: [u32; 2] = [WIDTH, HEIGHT];
const DEPTH: u32 = D3D11_BIND_DEPTH_STENCIL.0 as u32;
const TARGET: u32 = (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32;

thread_local! {
    static DEPTH_VIEW: Cell<Raw> = const { Cell::new(ptr::null_mut()) };
    static MOTION: Cell<rsf_anomaly_motion_frame_target> = Cell::new(Default::default());
    static OVERLAY: Cell<Option<(Raw, Raw)>> = const { Cell::new(None) };
    static OVERLAY_DRAWS: Cell<u64> = const { Cell::new(0) };
}

pub(super) unsafe extern "C" fn depth_view(_: u32) -> Raw {
    DEPTH_VIEW.get()
}

pub(super) fn motion_target() -> rsf_anomaly_motion_frame_target {
    MOTION.get()
}

unsafe extern "C" fn overlay_sink(_: Raw, _: Raw) {
    if let Some((context, target)) = OVERLAY.get() {
        let context = unsafe { ID3D11DeviceContext1::from_raw_borrowed(&context) }.unwrap();
        let target = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&target) }.unwrap();
        clear_rect(context, target, &[0.1, 0.1, 0.1, 1.0], [900, 80, 320, 400]);
        OVERLAY_DRAWS.set(OVERLAY_DRAWS.get() + 1);
    }
}

fn clear_rect(
    context: &ID3D11DeviceContext1,
    target: &ID3D11RenderTargetView,
    color: &[f32; 4],
    [x, y, w, h]: [i32; 4],
) {
    let rect = Rect {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    unsafe { context.ClearView(target, color, Some(&[rect])) };
}

fn nvidia_adapter() -> windows::core::Result<Option<IDXGIAdapter1>> {
    let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }?;
    let mut nvidia = None;
    for i in 0.. {
        let candidate = match unsafe { factory.EnumAdapters1(i) } {
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            other => other?,
        };
        let desc = unsafe { candidate.GetDesc1() }?;
        let software = desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0;
        let name = wide_name(&desc.Description);
        println!(
            "adapter {i}: {name} vendor 0x{:04x}{}",
            desc.VendorId,
            if software { " (software)" } else { "" }
        );
        if nvidia.is_none() && desc.VendorId == 0x10de && !software {
            nvidia = Some(candidate);
        }
    }
    Ok(nvidia)
}

fn wide_name(text: &[u16]) -> String {
    String::from_utf16_lossy(&text[..text.iter().position(|&ch| ch == 0).unwrap_or(text.len())])
}

fn describe_adapter(role: &str, adapter: windows::core::Result<IDXGIAdapter>) {
    match adapter.and_then(|adapter| unsafe { adapter.GetDesc() }) {
        Ok(desc) => println!(
            "{role} adapter: {} (vendor 0x{:04x}, device 0x{:04x}, LUID {:08x}:{:08x})",
            wide_name(&desc.Description),
            desc.VendorId,
            desc.DeviceId,
            desc.AdapterLuid.HighPart as u32,
            desc.AdapterLuid.LowPart
        ),
        Err(_) => println!("{role} adapter: unavailable"),
    }
}

fn fixture_window() -> Option<HWND> {
    let class = WNDCLASSW {
        hInstance: unsafe { GetModuleHandleW(ptr::null()) },
        lpfnWndProc: Some(DefWindowProcW),
        lpszClassName: w!("_XRAY_1.5"),
        ..Default::default()
    };
    let mut rect = RECT {
        right: WIDTH as i32,
        bottom: HEIGHT as i32,
        ..Default::default()
    };
    unsafe {
        if RegisterClassW(&class) == 0 {
            return None;
        }
        AdjustWindowRect(&mut rect, WS_OVERLAPPEDWINDOW, 0);
        let window = CreateWindowExW(
            0,
            class.lpszClassName,
            w!("Anomaly FG fixture"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            rect.right - rect.left,
            rect.bottom - rect.top,
            ptr::null_mut(),
            ptr::null_mut(),
            class.hInstance,
            ptr::null(),
        );
        if window.is_null() {
            return None;
        }
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
        Some(window)
    }
}

fn create_chain(
    adapter: &IDXGIAdapter1,
    window: HWND,
) -> Option<(IDXGISwapChain, ID3D11Device, ID3D11DeviceContext1)> {
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: WIDTH,
            Height: HEIGHT,
            RefreshRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        OutputWindow: Window(window),
        Windowed: BOOL(1),
        SwapEffect: DXGI_SWAP_EFFECT_SEQUENTIAL,
        Flags: 0,
    };
    let (mut chain, mut device, mut context) = (None, None, None);
    unsafe {
        D3D11CreateDeviceAndSwapChain(
            &**adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            Module(ptr::null_mut()),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&desc),
            Some(&mut chain),
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .ok()?;
    Some((chain?, device?, context?.cast().ok()?))
}

fn target(device: &ID3D11Device, texture: &ID3D11Texture2D) -> Option<ID3D11RenderTargetView> {
    let mut view = None;
    unsafe { device.CreateRenderTargetView(texture, None, Some(&mut view)) }.ok()?;
    view
}

fn depth_target(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
    desc: Option<&D3D11_DEPTH_STENCIL_VIEW_DESC>,
) -> Option<ID3D11DepthStencilView> {
    let mut view = None;
    unsafe { device.CreateDepthStencilView(texture, desc.map(ptr::from_ref), Some(&mut view)) }
        .ok()?;
    view
}

fn backbuffer(device: &ID3D11Device, chain: &IDXGISwapChain) -> Option<ID3D11RenderTargetView> {
    let buffer = unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.ok()?;
    target(device, &buffer)
}

fn camera_at(x: f32) -> ViewCamera {
    let (near, far, fov) = (0.1, 1000.0, 1.2_f32);
    let q = far / (far - near);
    let h = 1.0 / (fov / 2.0).tan();
    let w = h / (WIDTH as f32 / HEIGHT as f32);
    ViewCamera {
        view: [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -x, 0.0, 0.0, 1.0,
        ],
        projection: [
            w,
            0.0,
            0.0,
            0.0,
            0.0,
            h,
            0.0,
            0.0,
            0.0,
            0.0,
            q,
            1.0,
            0.0,
            0.0,
            -q * near,
            0.0,
        ],
        valid: true,
    }
}

struct SrScene {
    driver: Temporal,
    extent: [u32; 2],
    color: ID3D11Texture2D,
    twin: ID3D11Texture2D,
    motion: ID3D11Texture2D,
    output: ID3D11Texture2D,
    color_view: ID3D11RenderTargetView,
    motion_view: ID3D11RenderTargetView,
    twin_view: ID3D11DepthStencilView,
    previous: ViewCamera,
    frames: u64,
    evaluated: u64,
}

impl SrScene {
    fn create(driver: Temporal, device: &ID3D11Device) -> Option<Self> {
        driver.boundary(device.as_raw(), OUTPUT);
        let Some(extent) = driver.plan(OUTPUT) else {
            eprintln!(
                "FAIL the SR pipeline did not start on the shared host ({})",
                driver.reason()
            );
            return None;
        };
        println!(
            "SR stage 5: render {}x{} -> {WIDTH}x{HEIGHT}",
            extent[0], extent[1]
        );
        let color = create_texture(device, extent, DXGI_FORMAT_R16G16B16A16_FLOAT, TARGET)?;
        let twin = create_texture(
            device,
            extent,
            DXGI_FORMAT_R24G8_TYPELESS,
            DEPTH | D3D11_BIND_SHADER_RESOURCE.0 as u32,
        )?;
        let motion = create_texture(device, extent, DXGI_FORMAT_R16G16B16A16_FLOAT, TARGET)?;
        let output = create_texture(
            device,
            OUTPUT,
            DXGI_FORMAT(driver.output_format() as i32),
            TARGET,
        )?;
        let dsv = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        Some(Self {
            color_view: target(device, &color)?,
            motion_view: target(device, &motion)?,
            twin_view: depth_target(device, &twin, Some(&dsv))?,
            driver,
            extent,
            color,
            twin,
            motion,
            output,
            previous: ViewCamera::default(),
            frames: 0,
            evaluated: 0,
        })
    }

    fn evaluate(&mut self, context: &ID3D11DeviceContext1, camera: ViewCamera) {
        const JITTER: [[f32; 2]; 8] = [
            [0.0, -0.16],
            [-0.25, 0.16],
            [0.25, -0.38],
            [-0.37, 0.05],
            [0.12, -0.27],
            [-0.12, 0.27],
            [0.37, -0.05],
            [-0.43, 0.38],
        ];
        unsafe {
            context.ClearView(&self.color_view, &[0.5, 0.4, 0.3, 1.0], None);
            context.ClearView(&self.motion_view, &[0.0, 0.0, 1.0, 0.0], None);
            context.ClearDepthStencilView(&self.twin_view, D3D11_CLEAR_DEPTH.0, 0.5, 0);
        }
        let i = self.frames as usize % JITTER.len();
        self.frames += 1;
        let [rw, rh] = self.extent;
        let mut inputs = PassInputs {
            color: self.color.as_raw(),
            color_output: self.output.as_raw(),
            depth: self.twin.as_raw(),
            motion: self.motion.as_raw(),
            ..Default::default()
        };
        inputs.frame.native_frame = self.frames;
        inputs.frame.render_width = rw;
        inputs.frame.render_height = rh;
        inputs.frame.output_width = WIDTH;
        inputs.frame.output_height = HEIGHT;
        inputs.frame.current = camera;
        inputs.frame.previous = if self.previous.valid {
            self.previous
        } else {
            camera
        };
        inputs.frame.frame_seconds = 1.0 / 60.0;
        let jitter = &mut inputs.frame.jitter;
        jitter.frame = self.frames;
        jitter.applied = true;
        jitter.previous_applied = self.frames > 1;
        jitter.render_width = rw;
        jitter.render_height = rh;
        jitter.previous_render_width = rw;
        jitter.previous_render_height = rh;
        [jitter.current_x, jitter.current_y] = JITTER[i];
        [jitter.previous_x, jitter.previous_y] = JITTER[(i + 7) % 8];
        self.evaluated += u64::from(self.driver.evaluate(context.as_raw(), &mut inputs));
        self.previous = camera;
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Gameplay,
    Menu,
    Scope,
    Overlay,
}

struct Scene {
    device: ID3D11Device,
    context: ID3D11DeviceContext1,
    chain: IDXGISwapChain,
    backbuffer: Option<ID3D11RenderTargetView>,
    depth_view: ID3D11DepthStencilView,
    motion: ID3D11Texture2D,
    motion_view: ID3D11RenderTargetView,
    sr: Option<SrScene>,
    present: rsf_d3d11_present_api,
    fg_status: FgBindings,
    frame: u32,
    presents: u64,
    failed_presents: u64,
    active_by_backend: [u64; 8],
    generated_windows: u64,
}

impl Scene {
    fn frame(&mut self, kind: Kind, scope: bool) {
        self.frame += 1;
        let camera = camera_at(0.002 * self.frame as f32);
        unsafe { rsf_anomaly_fg_stage_camera(camera.view.as_ptr(), camera.projection.as_ptr()) };
        rsf_anomaly_fg_stage_input_begin(self.frame);
        rsf_anomaly_fg_stage_render_begin(self.frame, WIDTH, HEIGHT);
        let buffer = self.backbuffer.as_ref().expect("live backbuffer view");
        let context = &self.context;
        let t = (self.frame % 240) as f32 / 240.0;
        let box_rect = [(100 + (self.frame * 6) % 900) as i32, 300, 160, 160];
        unsafe {
            context.ClearView(buffer, &[0.15 + 0.3 * t, 0.25, 0.45, 1.0], None);
            clear_rect(context, buffer, &[0.9, 0.6, 0.1, 1.0], box_rect);
            context.ClearDepthStencilView(
                &self.depth_view,
                (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL).0,
                0.5,
                0,
            );
            context.ClearView(&self.motion_view, &[0.0; 4], None);
            clear_rect(
                context,
                &self.motion_view,
                &[6.0 / WIDTH as f32, 0.0, 1.0, 0.0],
                box_rect,
            );
        }
        if !scope && let Some(sr) = &mut self.sr {
            sr.evaluate(context, camera);
        }
        if scope {
            rsf_anomaly_fg_stage_scope();
        }
        if !matches!(kind, Kind::Menu) {
            DEPTH_VIEW.set(self.depth_view.as_raw());
            MOTION.set(rsf_anomaly_motion_frame_target {
                texture: self.motion.as_raw(),
                width: WIDTH,
                height: HEIGHT,
                frame: self.frame,
                written: true,
                main: true,
            });
            rsf_anomaly_fg_stage_ui_boundary();
            DEPTH_VIEW.set(ptr::null_mut());
            MOTION.set(Default::default());
            clear_rect(context, buffer, &[1.0; 4], [40, 620, 300, 60]);
        }
        if let Some(sr) = &self.sr {
            sr.driver.boundary(self.device.as_raw(), OUTPUT);
        }
        rsf_anomaly_fg_stage_end_before();
        if !scope {
            if matches!(kind, Kind::Overlay) {
                OVERLAY.set(Some((context.as_raw(), buffer.as_raw())));
            }
            let result = unsafe { self.chain.Present(0, DXGI_PRESENT(0)) };
            OVERLAY.set(None);
            self.presents += 1;
            if result.is_err() {
                self.failed_presents += 1;
                eprintln!("Present failed {:08x}", result.0 as u32);
            }
        }
        rsf_anomaly_fg_stage_end_after();
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let status = self.status();
        if status.vendor.active != 0 && status.vendor.effective_mode != RSF_FG_OFF {
            self.active_by_backend[(self.backend() & 7) as usize] += 1;
        }
        unsafe { Sleep(6) };
    }

    fn backend(&self) -> u32 {
        unsafe { self.present.backend.expect("backend query bound")() }
    }

    fn status(&self) -> rsf_native_fg_status {
        let mut status = rsf_native_fg_status {
            struct_size: size_of::<rsf_native_fg_status>() as u32,
            ..Default::default()
        };
        unsafe { (self.fg_status.status)(&mut status) };
        status
    }

    fn window(&mut self, name: &str, kind: Kind, count: u32, alternate_scope: bool) -> u64 {
        let start = self.status();
        let start_presents = self.presents;
        for i in 0..count {
            self.frame(kind, alternate_scope && i % 2 == 0);
        }
        let end = self.status();
        let tagged = end.tagged_frames.wrapping_sub(start.tagged_frames);
        let vendor = end
            .vendor
            .total_presented
            .wrapping_sub(start.vendor.total_presented);
        let presents = self.presents - start_presents;
        println!(
            "window {name:<10} frames={count} engine_presents={presents} runtime_tagged={tagged} vendor_presented={vendor} backend={}",
            self.backend()
        );
        self.generated_windows += u64::from(vendor > presents);
        tagged
    }

    fn scenarios(&mut self, window: HWND, backend: u32, switching: bool) -> i32 {
        let mut checks = Checks(0);
        self.window("warmup", Kind::Gameplay, 60, false);
        let gameplay = self.window("gameplay", Kind::Gameplay, 60, false);
        checks.require(
            self.window("menu", Kind::Menu, 20, false) == 0,
            "menu is never tagged",
        );
        let overlay = self.window("overlay", Kind::Overlay, 40, false);
        let scope = self.window("scope", Kind::Scope, 40, true);
        self.window("gameplay2", Kind::Gameplay, 60, false);
        rsf_anomaly_fg_stage_device_reset();
        self.backbuffer = None;
        checks.require(
            unsafe {
                self.chain.ResizeBuffers(
                    2,
                    WIDTH,
                    HEIGHT,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
            }
            .is_ok(),
            "ResizeBuffers",
        );
        self.backbuffer = backbuffer(&self.device, &self.chain);
        if self.backbuffer.is_none() {
            eprintln!("FAIL backbuffer view after resize");
            return 1;
        }
        let after_reset = self.window("reset", Kind::Gameplay, 60, false);
        if switching {
            let Some(request) = self.present.request else {
                return 1;
            };
            let Some(switch_result) = self.present.switch_result else {
                return 1;
            };
            for (next, label) in [
                (RSF_FG_BACKEND_FSR3, "fsr3"),
                (RSF_FG_BACKEND_XESS, "xess"),
                (0, "off"),
                (RSF_FG_BACKEND_DLSS, "dlss"),
            ] {
                println!("switch request backend={next} result={}", unsafe {
                    request(next)
                });
                for _ in 0..6 {
                    self.frame(Kind::Gameplay, false);
                }
                self.window(label, Kind::Gameplay, 80, false);
                println!(
                    "switch result={} effective backend={}",
                    unsafe { switch_result() },
                    self.backend()
                );
            }
        }
        let st = stage_status();
        let rs = self.status();
        let active = self.active_by_backend.iter().any(|&count| count > 0);
        let overlay_draws = OVERLAY_DRAWS.get();
        let (evaluated, refused) = self
            .sr
            .as_ref()
            .map_or((0, 0), |sr| (sr.evaluated, sr.frames - sr.evaluated));
        println!(
            "adapter counters: source={} level={} eligible_at_ui_boundary={} fed={} presented={} unpresented={} captures={}",
            st.source_frames,
            st.level_frames,
            st.eligible_at_ui_boundary,
            st.fed,
            st.presented,
            st.unpresented,
            st.captures
        );
        println!(
            "runtime: tagged={} vendor_presented={} effective_mode={} generated={} reason={} last_result={} vendor_status={}",
            rs.tagged_frames,
            rs.vendor.total_presented,
            rs.vendor.effective_mode,
            rs.vendor.effective_generated_frames,
            rs.reason,
            rs.last_result,
            rs.vendor.vendor_status
        );
        println!(
            "active samples: DLSS={} FSR3={} XeSS={}; overlay draws={overlay_draws}; SR evaluated={evaluated} refused={refused}",
            self.active_by_backend[1], self.active_by_backend[3], self.active_by_backend[5]
        );
        checks.require(self.failed_presents == 0, "all Presents succeed");
        checks.require(
            st.source_frames == u64::from(self.frame - 1000),
            "source count",
        );
        checks.require(
            st.presented + st.unpresented == st.source_frames,
            "presented and unpresented count",
        );
        checks.require(
            st.fed > 0 && st.eligible_at_ui_boundary >= st.fed,
            "fed count",
        );
        checks.require(st.captures == 1, "one FG capture");
        checks.require(overlay_draws > 0, "overlay rendered");
        if self.sr.is_some() {
            checks.require(evaluated > 0, "SR evaluated");
        }
        if backend == RSF_FG_BACKEND_DLSS && !active && unsafe { GetForegroundWindow() } != window {
            println!(
                "NOT RUN: DLSS-G activity needs the foreground; this fixture was denied foreground ownership"
            );
            return if checks.0 == 0 { 77 } else { 1 };
        }
        checks.require(active, "generation active");
        checks.require(
            gameplay > 0 && overlay > 0 && after_reset > 0,
            "gameplay, overlay and reset tagged",
        );
        checks.require(scope == 0, "scope and hold frames are never tagged");
        checks.require(
            self.generated_windows > 0,
            "vendor presents exceed engine presents",
        );
        if switching {
            checks.require(
                self.active_by_backend[3] > 0 && self.active_by_backend[5] > 0,
                "FSR3 and XeSS active after switching",
            );
        }
        if checks.0 == 0 {
            let stage = if self.sr.is_some() {
                "SR stage 5"
            } else {
                "SR off"
            };
            println!(
                "PASS: Anomaly FG ({stage}, backend {backend}) generated presented frames through the shared bridge"
            );
        } else {
            println!("{} failure(s)", checks.0);
        }
        checks.exit()
    }
}

fn setup_scene(
    device: ID3D11Device,
    context: ID3D11DeviceContext1,
    chain: IDXGISwapChain,
    present: rsf_d3d11_present_api,
    fg_status: FgBindings,
    driver: Option<Temporal>,
) -> Option<Scene> {
    let backbuffer = backbuffer(&device, &chain)?;
    let depth = create_texture(&device, OUTPUT, DXGI_FORMAT_D24_UNORM_S8_UINT, DEPTH)?;
    let motion = create_texture(&device, OUTPUT, DXGI_FORMAT_R16G16B16A16_FLOAT, TARGET)?;
    let depth_view = depth_target(&device, &depth, None)?;
    let motion_view = target(&device, &motion)?;
    let sr = match driver {
        Some(driver) => Some(SrScene::create(driver, &device)?),
        None => None,
    };
    Some(Scene {
        device,
        context,
        chain,
        backbuffer: Some(backbuffer),
        depth_view,
        motion,
        motion_view,
        sr,
        present,
        fg_status,
        frame: 1000,
        presents: 0,
        failed_presents: 0,
        active_by_backend: [0; 8],
        generated_windows: 0,
    })
}

pub(super) fn run(
    stage5: bool,
    backend: u32,
    root: &CStr,
    switching: bool,
    state_scope: StateScope,
) -> i32 {
    let vendor_path = |suffix: &[u8]| {
        let mut path = root.to_bytes().to_vec();
        path.extend_from_slice(suffix);
        CString::new(path).expect("argv and suffix contain no NUL")
    };
    let interposer = vendor_path(br"\streamline\sl.interposer.dll");
    if unsafe { GetFileAttributesA(interposer.as_ptr().cast()) } == INVALID_FILE_ATTRIBUTES {
        println!("SKIP: no vendor package at {}", root.to_string_lossy());
        return 77;
    }
    let adapter = match nvidia_adapter() {
        Ok(Some(adapter)) => adapter,
        Ok(None) => {
            println!("SKIP: no NVIDIA adapter");
            return 77;
        }
        Err(error) => {
            eprintln!("FAIL DXGI adapters: {error}");
            return 1;
        }
    };
    let Some(window) = fixture_window() else {
        eprintln!("FAIL fixture window");
        return 1;
    };
    let dirs = [br"\streamline".as_slice(), br"\fidelityfx", br"\xess"].map(vendor_path);
    // The old fixture widened each command-line byte for FG; SR consumed the original narrow path.
    let vendors = dirs
        .each_ref()
        .map(|dir| dir.to_bytes().iter().copied().map(u16::from).collect());
    let mut temp = [0u8; MAX_PATH as usize];
    let length = unsafe { GetTempPathA(MAX_PATH, temp.as_mut_ptr()) } as usize;
    if length == 0 || length >= temp.len() {
        eprintln!("FAIL temporary capture directory");
        return 1;
    }
    let capture_dir = temp[..length]
        .iter()
        .copied()
        .map(char::from)
        .collect::<String>()
        + "rsf-anomaly-fg-fixture";
    let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
    let Some(tables) = Tables::query(runtime, LOG) else {
        return 1;
    };
    let fg = tables.fg;
    let is_owner = tables.present.is_owner;
    if !Stage::install(
        options(Some(backend), capture_dir, vendors),
        LOG,
        tables,
        engine_bindings(state_scope),
        0,
    ) {
        eprintln!("FAIL the bridge did not install");
        return 1;
    }
    rsf_anomaly_fg_set_overlay_sink(Some(overlay_sink), ptr::null_mut());
    let driver = if stage5 {
        match Temporal::bind(
            runtime,
            dirs.each_ref().map(CString::as_c_str),
            Some(log_line),
        ) {
            Some(driver) => Some(driver),
            None => {
                eprintln!("FAIL rsf_get_d3d11_sr_api refused");
                return 1;
            }
        }
    } else {
        None
    };
    let Some((chain, device, context)) = create_chain(&adapter, window) else {
        eprintln!("FAIL D3D11CreateDeviceAndSwapChain");
        return 1;
    };
    if unsafe { is_owner(chain.as_raw()) } == 0 {
        println!(
            "SKIP: the bridge refused this chain (no provider for this device); presentation stayed stock"
        );
        return 77;
    }
    println!("PASS: D3D11CreateDeviceAndSwapChain returned the bridge's facade at cold start");
    describe_adapter(
        "device",
        device
            .cast::<IDXGIDevice>()
            .and_then(|dxgi| unsafe { dxgi.GetAdapter() }),
    );
    describe_adapter(
        "swap chain output",
        unsafe { chain.GetContainingOutput() }.and_then(|output| unsafe { output.GetParent() }),
    );
    let Some(query) = (unsafe {
        crate::runtime_module::export::<rsf_get_d3d11_present_api_fn>(
            runtime,
            RSF_D3D11_PRESENT_API_ENTRY_POINT,
        )
    })
    .flatten() else {
        return 1;
    };
    let mut present = rsf_d3d11_present_api {
        struct_size: size_of::<rsf_d3d11_present_api>() as u32,
        abi_version: RSF_D3D11_PRESENT_API_VERSION,
        ..Default::default()
    };
    if unsafe { query(&mut present) } != RSF_OK as i32 || present.backend.is_none() {
        return 1;
    }
    let Some(mut scene) = setup_scene(device, context, chain, present, fg, driver) else {
        eprintln!("FAIL fixture resources");
        return 1;
    };
    scene.scenarios(window, backend, switching)
}
