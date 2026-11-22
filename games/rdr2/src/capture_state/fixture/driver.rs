// SPDX-License-Identifier: MIT
//! Real WARP capture with simulated NGX execution and a queue-drained teardown.
use super::verify;
use crate::asi::{RSF_CENSUS_ABI_VERSION, rsf_census_options};
use crate::capture_state::install::{arm_fixture, stop_fixture};
use rsf_game_sys::win32::typed_export;
use std::ffi::{CStr, CString, c_void};
use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows::Win32::Foundation::{HANDLE, HWND, RECT};
use windows::Win32::Graphics::{
    Direct3D::{
        D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCompile, ID3DBlob,
        ID3DInclude,
    },
    Direct3D12::*,
    Dxgi::{Common::*, *},
};
use windows::core::{BOOL, Interface, PCSTR};
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::{
    LibraryLoader::{GetModuleHandleW, LoadLibraryW},
    Threading::{CreateEventW, WaitForSingleObject},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW,
};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;
const DRAWS: u32 = 75;
const INPUT_FRAMES: u32 = 4;

// loader/census/include/rescaleframe/vendor_census.h
const CENSUS_NO_STATIC_SCAN: u32 = 0x1;
const CENSUS_NO_VULKAN: u32 = 0x4;

type Setup = unsafe extern "C" fn(*const u16) -> bool;
type CensusStart = unsafe extern "C" fn(*const rsf_census_options) -> i32;
type CensusStop = unsafe extern "C" fn() -> i32;

type NgxFlags = unsafe extern "C" fn(*mut c_void) -> bool;
type NgxInputs =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) -> bool;
#[derive(Clone, Copy)]
struct NgxLeaves {
    flags: NgxFlags,
    inputs: NgxInputs,
}

struct Driver {
    setup: Setup,
    start: CensusStart,
    stop: CensusStop,
    ngx: NgxLeaves,
}

type Result<T = ()> = std::result::Result<T, String>;

fn hr<T>(result: windows::core::Result<T>, operation: &str) -> Result<T> {
    result.map_err(|error| {
        format!(
            "{operation} failed with HRESULT 0x{:08x}",
            error.code().0 as u32
        )
    })
}

fn require(condition: bool, operation: &'static str) -> Result {
    condition.then_some(()).ok_or_else(|| operation.to_owned())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0.0) };
    }
}
struct Event(HANDLE);
impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0.0) };
    }
}
struct Session(Option<CensusStop>);
impl Drop for Session {
    fn drop(&mut self) {
        stop_fixture();
        if let Some(stop) = self.0 {
            unsafe { stop() };
        }
    }
}
impl Session {
    fn finish(mut self) -> Result {
        stop_fixture();
        let stop = self.0.take().expect("started fixture census");
        require(unsafe { stop() } == 0, "census shutdown")
    }
}

struct Inputs {
    color: ID3D12Resource,
    depth: ID3D12Resource,
    motion: ID3D12Resource,
    output: ID3D12Resource,
}

fn input_texture(
    device: &ID3D12Device,
    format: DXGI_FORMAT,
    flags: D3D12_RESOURCE_FLAGS,
    state: D3D12_RESOURCE_STATES,
) -> Result<ID3D12Resource> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: 32,
        Height: 24,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: flags,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let mut resource = None;
    hr(
        unsafe {
            device.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                state,
                None,
                &mut resource,
            )
        },
        "input texture",
    )?;
    resource.ok_or_else(|| "input texture returned no resource".to_owned())
}

fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    // The caller's resource lease covers recording, and the descriptor does not release it.
    let borrowed = unsafe { ID3D12Resource::from_raw(resource.as_raw()) };
    let transition = D3D12_RESOURCE_TRANSITION_BARRIER {
        pResource: ManuallyDrop::new(Some(borrowed)),
        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
        StateBefore: before,
        StateAfter: after,
    };
    unsafe {
        list.ResourceBarrier(&[D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(transition),
            },
        }])
    };
}

const SHADER: &CStr = c"float4 vs(uint id:SV_VertexID):SV_Position { float2 p=float2((id<<1)&2,id&2); return float4(p*float2(2,-2)+float2(-1,1),0,1); } float4 ps():SV_Target { return float4(0.25,0.5,0.75,1); }";

fn shader(entry: &CStr, profile: &CStr) -> Result<ID3DBlob> {
    let mut blob = None;
    let result = unsafe {
        D3DCompile(
            SHADER.as_ptr().cast(),
            SHADER.to_bytes_with_nul().len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(profile.as_ptr().cast()),
            0,
            0,
            &mut blob,
            None,
        )
    };
    hr(result, "shader compilation")?;
    blob.ok_or_else(|| "shader compilation returned no blob".to_owned())
}

fn bytecode(blob: &ID3DBlob) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: unsafe { blob.GetBufferPointer() },
        BytecodeLength: unsafe { blob.GetBufferSize() },
    }
}

// Match the native SDK stream ABI: tag and naturally aligned payload, padded to pointer alignment.
#[repr(C, align(8))]
struct Item<T> {
    tag: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE,
    payload: T,
}
fn item<T>(tag: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE, payload: T) -> Item<T> {
    Item { tag, payload }
}
#[repr(C)]
struct Stream {
    root: Item<*mut c_void>,
    vs: Item<D3D12_SHADER_BYTECODE>,
    ps: Item<D3D12_SHADER_BYTECODE>,
    blend: Item<D3D12_BLEND_DESC>,
    formats: Item<D3D12_RT_FORMAT_ARRAY>,
    raster: Item<D3D12_RASTERIZER_DESC>,
    depth: Item<D3D12_DEPTH_STENCIL_DESC1>,
    topology: Item<D3D12_PRIMITIVE_TOPOLOGY_TYPE>,
}
const _: () = {
    assert!(std::mem::offset_of!(Item<*mut c_void>, payload) == 8);
    assert!(size_of::<Item<D3D12_SHADER_BYTECODE>>() == 24);
};

fn pipeline(
    device: &ID3D12Device,
    format: DXGI_FORMAT,
) -> Result<(ID3D12RootSignature, ID3D12PipelineState)> {
    let vs = shader(c"vs", c"vs_5_0")?;
    let ps = shader(c"ps", c"ps_5_0")?;
    let mut root_blob = None;
    hr(
        unsafe {
            D3D12SerializeRootSignature(
                &D3D12_ROOT_SIGNATURE_DESC::default(),
                D3D_ROOT_SIGNATURE_VERSION_1,
                &mut root_blob,
                None,
            )
        },
        "root serialize",
    )?;
    let root_blob = root_blob.ok_or("root serialize returned no blob")?;
    let bytes = unsafe {
        std::slice::from_raw_parts(
            root_blob.GetBufferPointer().cast(),
            root_blob.GetBufferSize(),
        )
    };
    let signature: ID3D12RootSignature = hr(
        unsafe { device.CreateRootSignature(0, bytes) },
        "root signature",
    )?;
    let mut blend = D3D12_BLEND_DESC::default();
    blend.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
    let mut formats = D3D12_RT_FORMAT_ARRAY {
        NumRenderTargets: 1,
        ..Default::default()
    };
    formats.RTFormats[0] = format;
    let raster = D3D12_RASTERIZER_DESC {
        FillMode: D3D12_FILL_MODE_SOLID,
        CullMode: D3D12_CULL_MODE_NONE,
        DepthClipEnable: BOOL(1),
        ..Default::default()
    };
    let mut stream = Stream {
        root: item(
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE,
            signature.as_raw(),
        ),
        vs: item(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS, bytecode(&vs)),
        ps: item(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS, bytecode(&ps)),
        blend: item(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND, blend),
        formats: item(
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS,
            formats,
        ),
        raster: item(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER, raster),
        depth: item(
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1,
            D3D12_DEPTH_STENCIL_DESC1::default(),
        ),
        topology: item(
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY,
            D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        ),
    };
    let device2: ID3D12Device2 = hr(device.cast(), "device2")?;
    let desc = D3D12_PIPELINE_STATE_STREAM_DESC {
        SizeInBytes: size_of::<Stream>(),
        pPipelineStateSubobjectStream: (&mut stream as *mut Stream).cast(),
    };
    let pso = hr(unsafe { device2.CreatePipelineState(&desc) }, "stream PSO")?;
    Ok((signature, pso))
}

fn run_ngx(list: &ID3D12GraphicsCommandList, inputs: &Inputs, leaves: NgxLeaves) -> Result {
    type Get = unsafe extern "C" fn(*mut *mut c_void) -> i32;
    type Create = unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *mut *mut c_void) -> i32;
    type Evaluate = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> i32;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let path = wide(&exe.with_file_name("_nvngx.dll"));
    let ngx = unsafe { LoadLibraryW(path.as_ptr()) };
    require(!ngx.is_null(), "NGX fixture load")?;
    let get: Get = unsafe { typed_export(ngx, c"NVSDK_NGX_D3D12_GetParameters") }
        .ok_or("NGX GetParameters")?;
    let create: Create = unsafe { typed_export(ngx, c"NVSDK_NGX_D3D12_CreateFeature") }
        .ok_or("NGX CreateFeature")?;
    let evaluate: Evaluate = unsafe { typed_export(ngx, c"NVSDK_NGX_D3D12_EvaluateFeature") }
        .ok_or("NGX EvaluateFeature")?;
    let mut parameters = null_mut();
    require(
        unsafe { get(&mut parameters) } == 1 && !parameters.is_null(),
        "NGX params",
    )?;
    require(unsafe { (leaves.flags)(parameters) }, "NGX feature flags")?;
    let mut handle = null_mut();
    require(
        unsafe { create(list.as_raw(), 1, parameters, &mut handle) } == 1 && !handle.is_null(),
        "NGX create",
    )?;
    require(
        unsafe {
            (leaves.inputs)(
                parameters,
                inputs.color.as_raw(),
                inputs.depth.as_raw(),
                inputs.motion.as_raw(),
                inputs.output.as_raw(),
            )
        },
        "NGX inputs",
    )?;
    for _ in 0..INPUT_FRAMES {
        require(
            unsafe { evaluate(list.as_raw(), handle, parameters, null_mut()) } == 1,
            "NGX evaluate",
        )?;
    }
    Ok(())
}

fn drain(device: &ID3D12Device, queue: &ID3D12CommandQueue) -> Result {
    // The probe's copy fence precedes DXGI's later Present work.
    let fence: ID3D12Fence = hr(
        unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) },
        "teardown fence",
    )?;
    let event = unsafe { CreateEventW(null(), 0, 0, null()) };
    require(!event.is_null(), "teardown event")?;
    let event = Event(HANDLE(event));
    hr(unsafe { queue.Signal(&fence, 1) }, "teardown Signal")?;
    hr(
        unsafe { fence.SetEventOnCompletion(1, event.0) },
        "teardown completion",
    )?;
    require(
        unsafe { WaitForSingleObject(event.0.0, 5000) } == WAIT_OBJECT_0,
        "presentation work drained",
    )
}

fn debug_messages(info: &ID3D12InfoQueue) -> Result {
    println!("Checking D3D12 debug messages.");
    for index in 0..unsafe { info.GetNumStoredMessages() } {
        let mut bytes = 0;
        hr(
            unsafe { info.GetMessage(index, None, &mut bytes) },
            "debug message size",
        )?;
        require(
            bytes >= size_of::<D3D12_MESSAGE>(),
            "debug message is short",
        )?;
        let mut storage = vec![0u64; bytes.div_ceil(size_of::<u64>())];
        let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        hr(
            unsafe { info.GetMessage(index, Some(message), &mut bytes) },
            "debug message",
        )?;
        let message = unsafe { &*message };
        if message.Severity.0 <= D3D12_MESSAGE_SEVERITY_ERROR.0 {
            let summary = if message.pDescription.is_null() {
                c"unknown D3D12 error"
            } else {
                unsafe { CStr::from_ptr(message.pDescription.0.cast()) }
            };
            return Err(format!(
                "D3D12 debug validation: {}",
                summary.to_string_lossy()
            ));
        }
    }
    Ok(())
}

fn run(root: &Path, rgb10: bool, driver: Driver) -> Result {
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let directory = wide(root);
    require(
        unsafe { (driver.setup)(directory.as_ptr()) },
        "probe install",
    )?;
    let mut session = Session(None);
    let mut debug: Option<ID3D12Debug> = None;
    if unsafe { D3D12GetDebugInterface(&mut debug) }.is_ok()
        && let Some(debug) = debug
    {
        unsafe { debug.EnableDebugLayer() };
    }
    let census_dir = CString::new(root.join("census").to_string_lossy().as_bytes())
        .map_err(|error| error.to_string())?;
    let options = rsf_census_options {
        struct_size: size_of::<rsf_census_options>() as u32,
        abi_version: RSF_CENSUS_ABI_VERSION,
        output_dir_utf8: census_dir.as_ptr(),
        flags: CENSUS_NO_STATIC_SCAN | CENSUS_NO_VULKAN,
        ..Default::default()
    };
    require(unsafe { (driver.start)(&options) } == 0, "census install")?;
    session.0 = Some(driver.stop);
    let factory: IDXGIFactory4 = hr(unsafe { CreateDXGIFactory1() }, "factory")?;
    let adapter: IDXGIAdapter = hr(unsafe { factory.EnumWarpAdapter() }, "WARP")?;
    let mut device: Option<ID3D12Device> = None;
    hr(
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) },
        "device",
    )?;
    let device = device.ok_or("device returned no object")?;
    let info = device.cast::<ID3D12InfoQueue>().ok();
    let queue: ID3D12CommandQueue = hr(
        unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default()) },
        "queue",
    )?;
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            windows::core::w!("STATIC").0,
            windows::core::w!("RSF capture fixture").0,
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    require(!hwnd.is_null(), "hidden window")?;
    let window = Window(HWND(hwnd));
    let format = if rgb10 {
        DXGI_FORMAT_R10G10B10A2_UNORM
    } else {
        DXGI_FORMAT_B8G8R8A8_UNORM
    };
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: WIDTH,
        Height: HEIGHT,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferCount: 2,
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        ..Default::default()
    };
    let chain1 = hr(
        unsafe {
            factory.CreateSwapChainForHwnd(&queue, window.0, &desc, None, None::<&IDXGIOutput>)
        },
        "swapchain",
    )?;
    let chain: IDXGISwapChain3 = hr(chain1.cast(), "swapchain3")?;
    hr(
        unsafe { chain.SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709) },
        "SDR color space",
    )?;
    arm_fixture(chain1.as_raw());
    hr(
        unsafe { chain.Present(0, DXGI_PRESENT(0)).ok() },
        "arm Present",
    )?;
    let back: ID3D12Resource = hr(
        unsafe { chain.GetBuffer(chain.GetCurrentBackBufferIndex()) },
        "backbuffer",
    )?;
    let rtv_heap: ID3D12DescriptorHeap = hr(
        unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 4,
                ..Default::default()
            })
        },
        "RTV heap",
    )?;
    let first_rtv = unsafe { rtv_heap.GetCPUDescriptorHandleForHeapStart() }.ptr;
    let rtv_step =
        unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) } as usize;
    let rtv = |index| D3D12_CPU_DESCRIPTOR_HANDLE {
        ptr: first_rtv + index * rtv_step,
    };
    let inputs = Inputs {
        color: input_texture(
            &device,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        )?,
        motion: input_texture(
            &device,
            DXGI_FORMAT_R16G16_FLOAT,
            D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        )?,
        output: input_texture(
            &device,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET | D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        )?,
        depth: input_texture(
            &device,
            DXGI_FORMAT_R32G8X24_TYPELESS,
            D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
        )?,
    };
    unsafe {
        for (index, target) in [&back, &inputs.color, &inputs.motion, &inputs.output]
            .into_iter()
            .enumerate()
        {
            device.CreateRenderTargetView(target, None, rtv(index));
        }
    }
    let dsv_heap: ID3D12DescriptorHeap = hr(
        unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
                NumDescriptors: 1,
                ..Default::default()
            })
        },
        "DSV heap",
    )?;
    let dsv = unsafe { dsv_heap.GetCPUDescriptorHandleForHeapStart() };
    unsafe {
        device.CreateDepthStencilView(
            &inputs.depth,
            Some(&D3D12_DEPTH_STENCIL_VIEW_DESC {
                Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
                ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
                ..Default::default()
            }),
            dsv,
        )
    };
    let (signature, pso) = pipeline(&device, format)?;
    let allocator: ID3D12CommandAllocator = hr(
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) },
        "allocator",
    )?;
    let list: ID3D12GraphicsCommandList = hr(
        unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, &pso) },
        "list",
    )?;
    let color = [0.25, 0.5, 0.75, 1.0];
    unsafe {
        list.ClearRenderTargetView(rtv(1), &color, None);
        list.ClearRenderTargetView(rtv(2), &[0.125, -0.25, 0.0, 0.0], None);
        list.ClearRenderTargetView(rtv(3), &color, None);
        list.ClearDepthStencilView(
            dsv,
            D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL,
            0.25,
            0,
            None,
        );
    }
    for (resource, from, to) in [
        (
            &inputs.color,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        (
            &inputs.motion,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        (
            &inputs.depth,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        (
            &inputs.output,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        ),
    ] {
        transition(&list, resource, from, to);
    }
    run_ngx(&list, &inputs, driver.ngx)?;
    transition(
        &list,
        &back,
        D3D12_RESOURCE_STATE_PRESENT,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
    );
    unsafe {
        list.OMSetRenderTargets(1, Some(&rtv(0)), false, None);
        list.SetGraphicsRootSignature(&signature);
        list.RSSetViewports(&[D3D12_VIEWPORT {
            Width: WIDTH as f32,
            Height: HEIGHT as f32,
            MaxDepth: 1.0,
            ..Default::default()
        }]);
        list.RSSetScissorRects(&[RECT {
            left: 0,
            top: 0,
            right: WIDTH as i32,
            bottom: HEIGHT as i32,
        }]);
        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        for _ in 0..DRAWS {
            list.DrawInstanced(3, 1, 0, 0);
        }
    }
    transition(
        &list,
        &back,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
        D3D12_RESOURCE_STATE_PRESENT,
    );
    hr(
        unsafe { list.Close() },
        "Close with restored resource states",
    )?;
    // Submit the same interface address observed by the list's recording hooks.
    let submitted = ID3D12CommandList::clone(&list);
    unsafe { queue.ExecuteCommandLists(&[Some(submitted)]) };
    let reused: ID3D12CommandAllocator = hr(
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) },
        "reuse allocator",
    )?;
    hr(
        unsafe { list.Reset(&reused, &pso) },
        "command-list reuse after submission",
    )?;
    hr(unsafe { list.Close() }, "reused list close")?;
    hr(
        unsafe { chain.Present(0, DXGI_PRESENT(0)).ok() },
        "captured Present",
    )?;
    drain(&device, &queue)?;
    println!("Checking capture manifest and raw pixels.");
    verify(root, rgb10).map_err(|message| message.to_string_lossy().into_owned())?;
    if let Some(info) = info {
        debug_messages(&info)?;
    }
    session.finish()?;
    println!(
        "RDR2 capture fixture passed: 75 draws, tail readback, stream PSO, 4 depth/MV/color/output groups; no game-camera claim."
    );
    Ok(())
}

fn entry(driver: Driver) -> Result {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    require((1..=2).contains(&args.len()), "output directory required")?;
    let rgb10 = args.get(1).is_some_and(|arg| arg == "rgb10");
    let root = std::path::absolute(&args[0]).map_err(|error| error.to_string())?;
    run(&root, rgb10, driver)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_fixture_main(
    setup: Setup,
    start: CensusStart,
    stop: CensusStop,
    flags: NgxFlags,
    inputs: NgxInputs,
) -> i32 {
    match entry(Driver {
        setup,
        start,
        stop,
        ngx: NgxLeaves { flags, inputs },
    }) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("RDR2 capture fixture failed: {error}");
            1
        }
    }
}
