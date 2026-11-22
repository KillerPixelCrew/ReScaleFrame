// SPDX-License-Identifier: MIT
//! The public AC7 capture ABI, observed through WARP and a hidden swap chain.

use crate::d3d11_observer_layout_abi::rsf_observer_layout_element;
use crate::motion_capture_graphics_abi::*;
use rsf_game_sys::{RSF_GAME_RENDER_TRANSLUCENCY, rsf_game_render_pass};
use std::ffi::{CStr, CString, OsString, c_char, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Foundation::{HINSTANCE, HMODULE, HWND};
use windows::Win32::Globalization::{CP_ACP, MultiByteToWideChar, WideCharToMultiByte};
use windows::Win32::Graphics::Direct3D::{Fxc::D3DCompile, *};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::{Common::*, *};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, Interface, PCSTR, PCWSTR, s, w};

type Test<T> = Result<T, String>;
type Log = unsafe extern "C" fn(*mut c_void, *const c_char);

unsafe extern "C" {
    fn rsf_ac7_motion_capture_configure(
        directory: *const c_char,
        log: Option<Log>,
        user: *mut c_void,
    );
    fn rsf_ac7_motion_capture_install() -> i32;
    fn rsf_ac7_motion_capture_shutdown();
    fn rsf_ac7_motion_capture_request();
    fn rsf_ac7_motion_capture_present(
        chain: *mut c_void,
        prefix: *mut c_char,
        capacity: u32,
    ) -> i32;
    fn rsf_ac7_motion_capture_shader(
        shader: *mut c_void,
        stage: u32,
        bytes: *const c_void,
        size: u32,
    );
    fn rsf_ac7_motion_capture_buffer(
        user: *mut c_void,
        buffer: *mut c_void,
        bytes: *const c_void,
        size: u32,
        bind_flags: u32,
    );
    fn rsf_ac7_motion_capture_upload(buffer: *mut c_void, bytes: *const c_void, size: u32);
    fn rsf_ac7_motion_capture_layout(
        layout: *mut c_void,
        elements: *const rsf_observer_layout_element,
        copied: u32,
        count: u32,
    );
    fn rsf_ac7_motion_capture_geometry(draw: *const rsf_frame_tap_geometry);
    fn rsf_ac7_motion_capture_native_pass(pass: *const rsf_game_render_pass, begin: u32);
}

static STAGES: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn shader_seen(
    _: *mut c_void,
    shader: *mut c_void,
    stage: u32,
    bytes: *const c_void,
    size: u32,
) {
    if stage < u32::BITS {
        STAGES.fetch_or(1 << stage, Ordering::Relaxed);
    }
    unsafe { rsf_ac7_motion_capture_shader(shader, stage, bytes, size) };
}

unsafe extern "C" fn uploaded(_: *mut c_void, buffer: *mut c_void, bytes: *mut c_void, size: u32) {
    unsafe { rsf_ac7_motion_capture_upload(buffer, bytes, size) };
}

unsafe extern "C" fn layout_seen(
    _: *mut c_void,
    layout: *mut c_void,
    elements: *const rsf_observer_layout_element,
    copied: u32,
    count: u32,
) {
    unsafe { rsf_ac7_motion_capture_layout(layout, elements, copied, count) };
}

fn require(condition: bool, message: &str) -> Test<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn win<T>(result: windows::core::Result<T>) -> Test<T> {
    result.map_err(|error| error.to_string())
}

macro_rules! create {
    ($device:expr, $method:ident($($arg:expr),*)) => {{
        let mut result = None;
        win($device.$method($($arg,)* Some(&mut result)))?;
        result.ok_or(concat!(stringify!($method), " returned no object"))?
    }};
}

fn narrow(path: &Path) -> Test<CString> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let len = unsafe { WideCharToMultiByte(CP_ACP, 0, &wide, None, PCSTR::null(), None) };
    require(len > 0, "Encode the capture directory")?;
    let mut bytes = vec![0; len as usize];
    let written =
        unsafe { WideCharToMultiByte(CP_ACP, 0, &wide, Some(&mut bytes), PCSTR::null(), None) };
    require(written == len, "Encode the entire capture directory")?;
    CString::new(bytes).map_err(|error| error.to_string())
}

fn from_narrow(path: &CStr) -> Test<PathBuf> {
    let bytes = path.to_bytes();
    let len = unsafe { MultiByteToWideChar(CP_ACP, Default::default(), bytes, None) };
    require(len > 0, "Decode the returned sample prefix")?;
    let mut wide = vec![0; len as usize];
    let written =
        unsafe { MultiByteToWideChar(CP_ACP, Default::default(), bytes, Some(&mut wide)) };
    require(written == len, "Decode the entire sample prefix")?;
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

struct Window {
    handle: HWND,
    instance: HINSTANCE,
}

const CLASS: PCWSTR = w!("RSF_MotionCaptureTest");

impl Window {
    fn create() -> Test<Self> {
        let instance = HINSTANCE(win(unsafe { GetModuleHandleW(PCWSTR::null()) })?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        };
        require(
            unsafe { RegisterClassW(&class) } != 0,
            "Register the capture fixture window",
        )?;
        let mut window = Self {
            handle: HWND::default(),
            instance,
        };
        window.handle = win(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                w!(""),
                WS_OVERLAPPED,
                0,
                0,
                32,
                32,
                None,
                None,
                Some(instance),
                None,
            )
        })?;
        Ok(window)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.0.is_null() {
                let _ = DestroyWindow(self.handle);
            }
            let _ = UnregisterClassW(CLASS, Some(self.instance));
        }
    }
}

#[derive(Default)]
struct Hooks {
    observer: bool,
    tap: bool,
}

impl Drop for Hooks {
    fn drop(&mut self) {
        unsafe {
            rsf_ac7_motion_capture_shutdown();
            if self.tap {
                rsf_frame_tap_uninstall();
            }
            if self.observer {
                rsf_observer_uninstall();
            }
        }
    }
}

fn warp(window: HWND) -> Option<(IDXGISwapChain, ID3D11Device, ID3D11DeviceContext)> {
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: 16,
            Height: 16,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 1,
        OutputWindow: window,
        Windowed: BOOL(1),
        ..Default::default()
    };
    let (mut chain, mut device, mut context) = (None, None, None);
    unsafe {
        D3D11CreateDeviceAndSwapChain(
            None::<&IDXGIAdapter>,
            D3D_DRIVER_TYPE_WARP,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&desc),
            Some(&mut chain),
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .ok()?;
    Some((chain?, device?, context?))
}

fn compile(source: &str, profile: PCSTR) -> Test<Vec<u8>> {
    let (mut code, mut errors) = (None, None);
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            s!("main"),
            profile,
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Some(errors) = errors {
        let data = unsafe {
            std::slice::from_raw_parts(errors.GetBufferPointer().cast(), errors.GetBufferSize())
        };
        eprintln!("{}", String::from_utf8_lossy(data));
    }
    win(result)?;
    let code = code.ok_or("Shader compile produced no bytecode")?;
    Ok(
        unsafe { std::slice::from_raw_parts(code.GetBufferPointer().cast(), code.GetBufferSize()) }
            .to_vec(),
    )
}

fn buffer<T>(
    device: &ID3D11Device,
    data: &[T],
    flags: D3D11_BIND_FLAG,
    usage: D3D11_USAGE,
) -> Test<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of_val(data) as u32,
        BindFlags: flags.0 as u32,
        Usage: usage,
        ..Default::default()
    };
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: data.as_ptr().cast(),
        ..Default::default()
    };
    Ok(unsafe { create!(device, CreateBuffer(&desc, Some(&initial))) })
}

fn texture_desc(format: DXGI_FORMAT, flags: D3D11_BIND_FLAG) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: 16,
        Height: 16,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: flags.0 as u32,
        ..Default::default()
    }
}

const PIXEL: [f32; 4] = [0.25, 0.5, 0.75, 1.0];
const PREVIOUS: [f32; 16] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
];

fn run() -> Test<i32> {
    let directory = std::env::temp_dir().join(format!("rsf-motion-capture-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = narrow(&directory)?;
    let observer = rsf_observer_options {
        struct_size: size_of::<rsf_observer_options>() as u32,
        abi_version: RSF_OBSERVER_ABI_VERSION,
        on_shader: Some(shader_seen),
        on_buffer: Some(rsf_ac7_motion_capture_buffer),
        on_layout: Some(layout_seen),
        ..Default::default()
    };
    let window = Window::create()?;
    // Keep the actual device and its bindings alive until the hooks retire.
    let device_keepalive;
    let mut hooks = Hooks::default();
    unsafe {
        rsf_ac7_motion_capture_configure(path.as_ptr(), None, null_mut());
        require(
            rsf_ac7_motion_capture_install() == 0,
            "Wrong executable must refuse the engine detours",
        )?;
        require(
            rsf_observer_install(&observer) == 0,
            "Observer must install",
        )?;
        hooks.observer = true;
        device_keepalive = warp(window.handle);
        let Some((chain, device, context)) = device_keepalive.as_ref() else {
            return Ok(77);
        };
        let captured = capture_frames(chain, device, context, &mut hooks)?;
        let (directory_of_capture, target) = captured;
        check_artifacts(&directory_of_capture, context.as_raw())?;
        check_textures(device, context, &directory, &target)?;
        println!("capture fixture: {}", directory_of_capture.display());
    }
    Ok(0)
}

unsafe fn capture_frames(
    chain: &IDXGISwapChain,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    hooks: &mut Hooks,
) -> Test<(PathBuf, ID3D11Texture2D)> {
    let vertex = compile(VS_SOURCE, s!("vs_5_0"))?;
    let pixel = compile(PS_SOURCE, s!("ps_5_0"))?;
    let compute = compile(CS_SOURCE, s!("cs_5_0"))?;
    let particle = compile(PARTICLE_SOURCE, s!("vs_5_0"))?;
    let geometry = compile(GS_SOURCE, s!("gs_5_0"))?;
    let hull = compile(HS_SOURCE, s!("hs_5_0"))?;
    let domain = compile(DS_SOURCE, s!("ds_5_0"))?;
    let none = None::<&ID3D11ClassLinkage>;
    unsafe {
        let vs = create!(device, CreateVertexShader(&vertex, none));
        let ps = create!(device, CreatePixelShader(&pixel, none));
        let cs = create!(device, CreateComputeShader(&compute, none));
        let _gs = create!(device, CreateGeometryShader(&geometry, none));
        let _hs = create!(device, CreateHullShader(&hull, none));
        let _ds = create!(device, CreateDomainShader(&domain, none));
        require(
            STAGES.load(Ordering::Relaxed) == 0x3f,
            "All six shader creation stages must reach capture",
        )?;

        let desc = texture_desc(
            DXGI_FORMAT_R8G8B8A8_UNORM,
            D3D11_BIND_RENDER_TARGET | D3D11_BIND_UNORDERED_ACCESS,
        );
        let target = create!(device, CreateTexture2D(&desc, None));
        let rtv = [Some(create!(device, CreateRenderTargetView(&target, None)))];
        let output = Some(create!(device, CreateUnorderedAccessView(&target, None)));
        let constants = [Some(buffer(
            device,
            &PIXEL,
            D3D11_BIND_CONSTANT_BUFFER,
            D3D11_USAGE_DEFAULT,
        )?)];
        let tap = rsf_frame_tap_options {
            struct_size: size_of::<rsf_frame_tap_options>() as u32,
            abi_version: RSF_FRAME_TAP_ABI_VERSION,
            ..Default::default()
        };
        require(
            rsf_frame_tap_install(context.as_raw(), &tap) == 0,
            "Install draw and compute tap",
        )?;
        hooks.tap = true;
        require(
            rsf_frame_tap_set_constant_watch(0, Some(uploaded), null_mut()) == 0,
            "Install constant upload watch",
        )?;
        rsf_ac7_motion_capture_request();
        let mut directory = None;
        for interval in 0..=60 {
            let mut prefix = [0 as c_char; 640];
            let sampled = rsf_ac7_motion_capture_present(
                chain.as_raw(),
                prefix.as_mut_ptr(),
                prefix.len() as u32,
            ) != 0;
            let expected = match interval {
                0 => Some("f000"),
                30 => Some("f030"),
                59 => Some("f059"),
                _ => None,
            };
            require(
                sampled == expected.is_some(),
                "Capture must sample only intervals 0, 30 and 59",
            )?;
            if let Some(expected) = expected {
                let path = from_narrow(CStr::from_ptr(prefix.as_ptr()))?;
                require(
                    path.file_name().and_then(|name| name.to_str()) == Some(expected),
                    "Sample prefix identifies the active interval",
                )?;
                let parent = path.parent().ok_or("Sample has no session directory")?;
                require(
                    parent
                        == directory
                            .get_or_insert_with(|| parent.to_path_buf())
                            .as_path(),
                    "All samples use the same session directory",
                )?;
            }
            context.OMSetRenderTargets(Some(&rtv), None::<&ID3D11DepthStencilView>);
            context.VSSetShader(&vs, None);
            context.PSSetShader(&ps, None);
            context.PSSetConstantBuffers(0, Some(&constants));
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: 16.0,
                Height: 16.0,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            context.Draw(3, 0);
            if interval == 0 {
                check_geometry(device, context, &particle)?;
            }
            context.OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>);
            context.CSSetUnorderedAccessViews(0, 1, Some(&output), None);
            context.CSSetShader(&cs, None);
            context.Dispatch(16, 16, 1);
            context.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
        }
        Ok((
            directory.ok_or("Capture did not produce a directory")?,
            target,
        ))
    }
}

unsafe fn check_geometry(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    particle: &[u8],
) -> Test<()> {
    unsafe {
        let element = |semantic_index, byte_offset| D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("ATTRIBUTE"),
            SemanticIndex: semantic_index,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: byte_offset,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            ..Default::default()
        };
        let layout = create!(
            device,
            CreateInputLayout(&[element(0, 0), element(1, 16)], particle)
        );
        let particle_data = [
            -1.0_f32, -1.0, 0.0, 1.0, -1.0, -1.0, 0.0, 0.0001, 3.0, -1.0, 0.0, 1.0, 3.0, -1.0, 0.0,
            0.0002, -1.0, 3.0, 0.0, 1.0, -1.0, 3.0, 0.0, 0.0003,
        ];
        let vertices = buffer(
            device,
            &particle_data,
            D3D11_BIND_VERTEX_BUFFER,
            D3D11_USAGE_IMMUTABLE,
        )?;
        let particle_vs = create!(
            device,
            CreateVertexShader(particle, None::<&ID3D11ClassLinkage>)
        );
        let previous = buffer(
            device,
            &PREVIOUS,
            D3D11_BIND_SHADER_RESOURCE,
            D3D11_USAGE_IMMUTABLE,
        )?;
        let srv = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            ViewDimension: D3D11_SRV_DIMENSION_BUFFER,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_SRV {
                    Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 1 },
                    Anonymous2: D3D11_BUFFER_SRV_1 { NumElements: 2 },
                },
            },
        };
        let previous_view = [Some(create!(
            device,
            CreateShaderResourceView(&previous, Some(&srv))
        ))];
        let query = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_OCCLUSION_PREDICATE,
            MiscFlags: 0,
        };
        let predicate = create!(device, CreatePredicate(&query));
        context.Begin(&predicate);
        context.End(&predicate);
        let velocity_depth = create!(
            device,
            CreateTexture2D(
                &texture_desc(DXGI_FORMAT_D32_FLOAT, D3D11_BIND_DEPTH_STENCIL),
                None
            )
        );
        let velocity_dsv = create!(device, CreateDepthStencilView(&velocity_depth, None));

        let mut pass = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            role: RSF_GAME_RENDER_TRANSLUCENCY,
            scope_id: 17,
            ..std::mem::zeroed()
        };
        rsf_ac7_motion_capture_native_pass(&pass, 1);
        let mut draw = rsf_frame_tap_geometry {
            context: context.as_raw(),
            input_layout: layout.as_raw(),
            count: 3,
            ..Default::default()
        };
        draw.vertex_buffers[0] = vertices.as_raw();
        draw.vertex_buffers[31] = vertices.as_raw(); // A stale IA stream absent from the layout.
        draw.strides[0] = 32;
        let format = DXGI_FORMAT_R32G32B32A32_FLOAT.0 as u32;
        let element = |semantic_index, byte_offset, per_instance| rsf_observer_layout_element {
            semantic_index,
            format,
            input_slot: 0,
            byte_offset,
            per_instance,
        };
        let generic = [element(0, 0, 0)];
        rsf_ac7_motion_capture_layout(layout.as_raw(), generic.as_ptr(), 1, 1);
        for _ in 0..12 {
            rsf_ac7_motion_capture_geometry(&draw);
        }
        let sprite = [element(0, 0, 0), element(1, 16, 0)];
        rsf_ac7_motion_capture_layout(layout.as_raw(), sprite.as_ptr(), 2, 2);
        draw.vertex_shader = particle_vs.as_raw();
        rsf_ac7_motion_capture_geometry(&draw);
        let mesh = [element(8, 0, 1), element(9, 16, 1), element(10, 32, 1)];
        rsf_ac7_motion_capture_layout(layout.as_raw(), mesh.as_ptr(), 3, 3);
        context.VSSetShaderResources(127, Some(&previous_view));
        context.SetPredication(&predicate, false);
        rsf_ac7_motion_capture_geometry(&draw);
        let (mut restored, mut value) = (None, BOOL(1));
        context.GetPredication(Some(&mut restored), Some(&mut value));
        context.SetPredication(None::<&ID3D11Predicate>, false);
        require(
            restored == Some(predicate) && !value.as_bool(),
            "Geometry capture must restore native predication",
        )?;
        pass.role = 0;
        rsf_ac7_motion_capture_native_pass(&pass, 1);
        draw.format = DXGI_FORMAT_R16G16_UNORM.0 as u32;
        draw.depth_view = velocity_dsv.as_raw();
        context.VSSetShaderResources(0, Some(&previous_view));
        context.VSSetShaderResources(1, Some(&previous_view));
        rsf_ac7_motion_capture_geometry(&draw);
        rsf_ac7_motion_capture_native_pass(null(), 0);
    }
    Ok(())
}

fn check_artifacts(directory: &Path, context: *mut c_void) -> Test<()> {
    let summary = std::fs::read_to_string(directory.join("session.json"))
        .map_err(|error| error.to_string())?;
    let draws = std::fs::read_to_string(directory.join("draws.jsonl"))
        .map_err(|error| error.to_string())?;
    require(
        summary.contains("\"files_written\":true"),
        "Session must finish and write successfully",
    )?;
    require(
        draws.contains("\"kind\":\"draw\"") && draws.contains("\"kind\":\"dispatch\""),
        "Draw and compute records must both exist",
    )?;
    require(
        draws.contains(&format!("\"context\":{}", context.addr())),
        "Draw rows retain the context used for phase matching",
    )?;
    require(
        !draws.contains("\"blob\":0"),
        "Initial constant buffer contents must be captured",
    )?;
    let bytes = std::fs::read(directory.join("cb_1.bin")).map_err(|error| error.to_string())?;
    require(
        bytes.len() == 16,
        "Exact constant buffer bytes must be saved",
    )?;
    require(
        draws.contains("\"kind\":\"geometry\"")
            && draws.contains("\"attribute\":1")
            && draws.contains("\"sample_bytes\":96"),
        "Capture must retain particle attributes and bounded vertex bytes",
    )?;
    require(
        draws.contains("\"geometry_bucket\":1") && draws.contains("\"geometry_bucket\":2"),
        "Early generic geometry must preserve sprite and mesh budgets",
    )?;
    require(
        !draws.contains("\"slot\":31"),
        "Unused IA slots must not consume geometry readbacks",
    )?;
    require(
        draws.contains("\"geometry_bucket\":4") && draws.contains("\"vs_buffers\":[{\"slot\":0"),
        "Native velocity draws retain bone buffer source views outside translucency scope",
    )?;
    let mesh = draws
        .lines()
        .find(|row| row.contains("\"vs_buffers\":[{\"slot\":127"))
        .ok_or("Mesh-particle previous-transform views must be sampled")?;
    let (_, view) = mesh
        .split_once("\"vs_buffers\":")
        .ok_or("Missing view data")?;
    let (_, id) = view
        .split_once("\"blob\":")
        .ok_or("Previous-transform view has no blob")?;
    let id = id
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .ok_or("Missing blob ID")?;
    let bytes =
        std::fs::read(directory.join(format!("cb_{id}.bin"))).map_err(|error| error.to_string())?;
    let expected: Vec<_> = PREVIOUS[4..12]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    require(
        bytes == expected,
        "Capture must copy the actual SRV slice under native predication",
    )
}

fn dump(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    resource: &ID3D11Texture2D,
    directory: &Path,
    name: &str,
    preview: bool,
) -> Test<Vec<u8>> {
    let path = directory.join(name);
    // The texture dump ABI takes UTF-8. Present prefixes use the native narrow code page.
    let path = CString::new(path.to_str().ok_or("Dump path is not Unicode")?)
        .map_err(|error| error.to_string())?;
    let options = rsf_texture_dump_options {
        struct_size: size_of::<rsf_texture_dump_options>() as u32,
        abi_version: RSF_TEXTURE_DUMP_ABI_VERSION,
        output_prefix_utf8: path.as_ptr(),
        ..Default::default()
    };
    let (device, context, resource) = (device.as_raw(), context.as_raw(), resource.as_raw());
    require(
        unsafe { rsf_dump_texture_bytes(device, context, resource, &options) } == 0,
        "Lossless texture dump must succeed",
    )?;
    if preview {
        let mut report = rsf_texture_dump_report {
            struct_size: size_of::<rsf_texture_dump_report>() as u32,
            ..Default::default()
        };
        require(
            unsafe { rsf_dump_texture(device, context, resource, &options, &mut report) } == 0,
            "10-bit final preview must succeed",
        )?;
    }
    std::fs::read(directory.join(format!("{name}.bin"))).map_err(|error| error.to_string())
}

fn first_pixel(bytes: &[u8]) -> Test<u32> {
    let bytes: [u8; 4] = bytes
        .get(..4)
        .ok_or("Texture dump has no first pixel")?
        .try_into()
        .map_err(|_| "Bad pixel size")?;
    Ok(u32::from_le_bytes(bytes))
}

fn check_textures(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    directory: &Path,
    target: &ID3D11Texture2D,
) -> Test<()> {
    let rgba = dump(device, context, target, directory, "colour", false)?;
    require(
        rgba.len() == 1024 && rgba[..4] == [64, 128, 191, 255],
        "Capture must preserve rendered pixels",
    )?;
    unsafe {
        let ten_bit = texture_desc(DXGI_FORMAT_R10G10B10A2_UNORM, D3D11_BIND_RENDER_TARGET);
        let image = create!(device, CreateTexture2D(&ten_bit, None));
        let view = create!(device, CreateRenderTargetView(&image, None));
        context.ClearRenderTargetView(&view, &PIXEL);
        let raw = dump(device, context, &image, directory, "present", true)?;
        let packed = first_pixel(&raw)?;
        require(
            raw.len() == 1024
                && packed & 1023 == 256
                && (packed >> 10) & 1023 == 512
                && (packed >> 20) & 1023 == 767
                && packed >> 30 == 3,
            "Final capture must preserve ten-bit channels and alpha",
        )?;

        let depth_desc = texture_desc(DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL);
        let depth = create!(device, CreateTexture2D(&depth_desc, None));
        let view = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        let dsv = create!(device, CreateDepthStencilView(&depth, Some(&view)));
        context.ClearDepthStencilView(&dsv, (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL).0, 0.25, 7);
        let raw = dump(device, context, &depth, directory, "depth", false)?;
        let packed = first_pixel(&raw)?;
        require(
            raw.len() == 1024
                && packed >> 24 == 7
                && (0x3fffff..=0x400001).contains(&(packed & 0xffffff)),
            "Depth and stencil bits must survive",
        )?;
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_ac7_capture_io() -> i32 {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

const VS_SOURCE: &str =
    r#"float4 main(uint id:SV_VertexID):SV_Position{return float4(id==2?3:-1,id==1?3:-1,0,1);}"#;

const PS_SOURCE: &str =
    r#"cbuffer P:register(b0){float4 colour;} float4 main():SV_Target{return colour;}"#;

const CS_SOURCE: &str = r#"RWTexture2D<float4> result:register(u0); [numthreads(1,1,1)]void main(uint3 id:SV_DispatchThreadID){result[id.xy]=float4(.25,.5,.75,1);}"#;

const PARTICLE_SOURCE: &str =
    r#"float4 main(float4 p:ATTRIBUTE0,float4 old:ATTRIBUTE1):SV_Position{return p+old*.01;}"#;

const GS_SOURCE: &str = r#"struct V{float4 p:SV_Position;};[maxvertexcount(3)]void main(triangle V v[3],inout TriangleStream<V> stream){stream.Append(v[0]);stream.Append(v[1]);stream.Append(v[2]);}"#;

const HS_SOURCE: &str = r#"struct V{float4 p:SV_Position;};struct F{float edge[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};F patch(InputPatch<V,3> v){F f;f.edge[0]=f.edge[1]=f.edge[2]=f.inside=1;return f;}[domain("tri")][partitioning("integer")][outputtopology("triangle_cw")][outputcontrolpoints(3)][patchconstantfunc("patch")]V main(InputPatch<V,3> v,uint id:SV_OutputControlPointID){return v[id];}"#;

const DS_SOURCE: &str = r#"struct V{float4 p:SV_Position;};struct F{float edge[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};[domain("tri")] V main(F f,float3 uv:SV_DomainLocation,const OutputPatch<V,3> v){V o;o.p=v[0].p*uv.x+v[1].p*uv.y+v[2].p*uv.z;return o;}"#;
