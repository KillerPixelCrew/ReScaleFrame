// SPDX-License-Identifier: MIT
use super::{bundles, contents, entry, root, utf8};
use crate::sys::*;
use rsf_game_sys::{
    RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_NO_SR, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::ffi::{CStr, c_char};
use std::mem::ManuallyDrop;
use std::{fs, path::Path, ptr};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory4};
use windows::core::Interface;
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::Environment::SetEnvironmentVariableW;
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

const PIXELS: [u32; 8] = [
    0x00ff0000, 0x8000ff00, 0xffff0000, 0xffffffff, 0, 0xff112233, 0xff445566, 0xff778899,
];

fn warp() -> Option<ID3D12Device> {
    unsafe {
        let mut debug: Option<ID3D12Debug> = None;
        if D3D12GetDebugInterface(&mut debug).is_ok()
            && let Some(debug) = debug
        {
            debug.EnableDebugLayer();
        }
        let factory: IDXGIFactory4 = CreateDXGIFactory1().ok()?;
        let warp: IDXGIAdapter = factory.EnumWarpAdapter().ok()?;
        let mut device = None;
        D3D12CreateDevice(&warp, D3D_FEATURE_LEVEL_11_0, &mut device).ok()?;
        device
    }
}

fn resource(
    device: &ID3D12Device,
    heap: D3D12_HEAP_TYPE,
    desc: &D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
) -> ID3D12Resource {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: heap,
        ..Default::default()
    };
    let mut resource = None;
    unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            desc,
            state,
            None,
            &mut resource,
        )
    }
    .expect("allocate fixture resource");
    resource.unwrap()
}

fn texture(device: &ID3D12Device) -> ID3D12Resource {
    resource(
        device,
        D3D12_HEAP_TYPE_DEFAULT,
        &D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 4,
            Height: 2,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        },
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
    )
}

fn borrow(resource: &ID3D12Resource) -> ManuallyDrop<Option<ID3D12Resource>> {
    ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource.as_raw()) }))
}

fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    unsafe {
        list.ResourceBarrier(&[D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: borrow(resource),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: before,
                    StateAfter: after,
                }),
            },
        }]);
    }
}

fn upload(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    source: &ID3D12Resource,
) -> ID3D12Resource {
    let (mut footprint, mut bytes) = (D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(), 0);
    unsafe {
        device.GetCopyableFootprints(
            &source.GetDesc(),
            0,
            1,
            0,
            Some(&mut footprint),
            None,
            None,
            Some(&mut bytes),
        )
    };
    let upload = resource(
        device,
        D3D12_HEAP_TYPE_UPLOAD,
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
        D3D12_RESOURCE_STATE_GENERIC_READ,
    );
    let mut mapped = ptr::null_mut();
    unsafe { upload.Map(0, None, Some(&mut mapped)) }.expect("map padded source upload");
    assert!(!mapped.is_null());
    for (y, row) in PIXELS.chunks_exact(4).enumerate() {
        unsafe {
            ptr::copy_nonoverlapping(
                row.as_ptr().cast::<u8>(),
                mapped
                    .cast::<u8>()
                    .add(footprint.Offset as usize + y * footprint.Footprint.RowPitch as usize),
                16,
            )
        };
    }
    unsafe { upload.Unmap(0, None) };
    transition(
        list,
        source,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        D3D12_RESOURCE_STATE_COPY_DEST,
    );
    let from = D3D12_TEXTURE_COPY_LOCATION {
        pResource: borrow(&upload),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: footprint,
        },
    };
    let to = D3D12_TEXTURE_COPY_LOCATION {
        pResource: borrow(source),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    unsafe { list.CopyTextureRegion(&to, 0, 0, 0, &from, None) };
    transition(
        list,
        source,
        D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
    );
    upload
}

fn debug_messages(device: &ID3D12Device) {
    let Ok(info) = device.cast::<ID3D12InfoQueue>() else {
        return;
    };
    for index in 0..unsafe { info.GetNumStoredMessages() } {
        let mut length = 0;
        unsafe { info.GetMessage(index, None, &mut length) }.unwrap();
        let mut storage = vec![0usize; length.div_ceil(size_of::<usize>())];
        let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        unsafe { info.GetMessage(index, Some(message), &mut length) }.unwrap();
        let message = unsafe { &*message };
        assert!(
            message.Severity != D3D12_MESSAGE_SEVERITY_ERROR
                && message.Severity != D3D12_MESSAGE_SEVERITY_CORRUPTION,
            "{}",
            unsafe { CStr::from_ptr(message.pDescription.cast()) }.to_string_lossy()
        );
    }
}

fn run(path: &Path) -> i32 {
    unsafe {
        SetEnvironmentVariableW(
            windows::core::w!("RSF_FRAME_CAPTURE_DIR").as_ptr(),
            ptr::null(),
        )
    };
    let Some(device) = warp() else { return 77 };
    let kind = D3D12_COMMAND_LIST_TYPE_DIRECT;
    let queue: ID3D12CommandQueue = unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: kind,
            ..Default::default()
        })
    }
    .unwrap();
    let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(kind) }.unwrap();
    let list: ID3D12GraphicsCommandList =
        unsafe { device.CreateCommandList(0, kind, &allocator, None::<&ID3D12PipelineState>) }
            .unwrap();
    let source = texture(&device);
    let _upload = upload(&device, &list, &source);
    let root = root(path, "core-dump-dx12");
    let root_utf8 = utf8(&root);
    let mut frame = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        flags: RSF_FRAME_FLAG_NO_SR | RSF_FRAME_FLAG_NO_FG,
        session_id: 13,
        frame_id: 17,
        view_id: 3,
        render_width: 4,
        output_width: 4,
        render_height: 2,
        output_height: 2,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        rsf_input_dump_configure(
            root_utf8.as_ptr(),
            c"FixtureD3D12".as_ptr(),
            c"fixture".as_ptr(),
            None,
            ptr::null_mut(),
        );
        rsf_input_dump_request();
        assert_ne!(rsf_input_dump_begin_record(&frame, 3), 0);
        let other = rsf_frame_record {
            view_id: 4,
            ..frame
        };
        assert_eq!(rsf_input_dump_wants(&other), 0);
        assert_eq!(
            rsf_input_dump_reserve(&frame, c"../escape".as_ptr(), c"invalid".as_ptr(), 0, 1.0),
            0
        );
        let mut capture = ptr::null_mut();
        assert_eq!(rsf_input_dump12_create(&mut capture), 0);
        assert!(!capture.is_null());
        for name in [c"sr_source_jittered", c"final_color"] {
            assert_eq!(
                rsf_input_dump12_record(
                    capture,
                    list.as_raw(),
                    source.as_raw(),
                    &frame,
                    name.as_ptr(),
                    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32,
                    0,
                    RSF_FIXTURE_DUMP_VIEW_RAW as u32,
                    1.0
                ),
                0
            );
        }
        assert_eq!(
            rsf_input_dump12_destroy(capture),
            RSF_FIXTURE_BACKEND_NOT_READY
        );
        drop(source);
        let bundle = bundles(&root).next().unwrap();
        rsf_input_dump_complete(&frame, 0);
        assert!(contents(&bundle, "manifest.json").contains("waiting_for_gpu"));
        assert!(!bundle.join("final_color.bin").exists());
        list.Close().unwrap();
        queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>().unwrap())]);
        assert_eq!(rsf_input_dump12_submitted(capture, queue.as_raw(), 0), 0);
        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).unwrap();
        let event = CreateEventW(ptr::null(), 0, 0, ptr::null());
        assert!(!event.is_null());
        let done = queue.Signal(&fence, 1).is_ok()
            && fence.SetEventOnCompletion(1, HANDLE(event)).is_ok()
            && WaitForSingleObject(event, 15_000) == WAIT_OBJECT_0;
        CloseHandle(event);
        assert!(done, "observe submitted GPU completion");
        assert_eq!(rsf_input_dump12_poll(capture), 0);
        let expected: Vec<_> = PIXELS.into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(fs::read(bundle.join("final_color.bin")).unwrap(), expected);
        assert!(contents(&bundle, "manifest.json").contains("\"status\":\"complete\""));
        assert!(
            fs::read(bundle.join("final_color.png"))
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );
        assert_eq!(
            rsf_input_dump_reserve(&frame, c"stale".as_ptr(), c"closed".as_ptr(), 0, 1.0),
            0
        );
        assert_eq!(rsf_input_dump12_destroy(capture), 0);

        rsf_input_dump_request();
        frame.frame_id += 1;
        assert_ne!(rsf_input_dump_begin_record(&frame, 0), 0);
        assert_eq!(rsf_input_dump12_create(&mut capture), 0);
        let discarded = texture(&device);
        allocator.Reset().unwrap();
        list.Reset(&allocator, None::<&ID3D12PipelineState>)
            .unwrap();
        assert_eq!(
            rsf_input_dump12_record(
                capture,
                list.as_raw(),
                discarded.as_raw(),
                &frame,
                c"abandoned".as_ptr(),
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32,
                1,
                RSF_FIXTURE_DUMP_VIEW_RAW as u32,
                1.0
            ),
            0
        );
        list.Close().unwrap();
        allocator.Reset().unwrap();
        list.Reset(&allocator, None::<&ID3D12PipelineState>)
            .unwrap();
        list.Close().unwrap();
        rsf_input_dump12_abandon(capture, 1);
        assert_eq!(rsf_input_dump12_destroy(capture), 0);
        rsf_input_dump_cancel(frame.session_id);
        println!("D3D12 core dump fixture: {}", bundle.display());
    }
    debug_messages(&device);
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_input_dump_d3d12(path: *const c_char) -> i32 {
    entry(path, run)
}
