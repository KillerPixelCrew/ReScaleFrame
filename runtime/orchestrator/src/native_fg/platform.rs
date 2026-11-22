// SPDX-License-Identifier: MIT
use super::{DlssFrame, Handle, Log, Pass, Platform, Provider, rsf_fg_frame};
use crate::com_ref::ComRef;
use crate::d3d11_present::{rsf_core_d3d11p_graphics, rsf_core_d3d11p_host};
use crate::d3d11_present_abi::rsf_streamline_graphics;
use crate::d3d12_fence::rsf_core_d3d12_wait_fence;
use crate::fg_leases::rsf_d3d12_fence_completed;
use crate::fg_session::rsf_core_frame_allows_fg;
use crate::native_fg_abi::*;
use crate::native_fg_cpu::rsf_native_fg_cpu_platform;
use crate::native_scene::{rsf_native_scene_classify_present, rsf_native_scene_for_window};
use crate::native_window::rsf_native_window_match;
use crate::shared_surface::{
    ABI_VERSION as SURFACE_ABI, SurfaceSetup, rsf_shared_fence_create, rsf_shared_fence_d3d11,
    rsf_shared_fence_d3d12, rsf_shared_surface_create, rsf_shared_surface_d3d11,
    rsf_shared_surface_d3d12, rsf_shared_surface_destroy,
};
use std::ffi::c_char;
use std::mem::ManuallyDrop;
use std::ptr::{null, null_mut};
use windows::Win32::{
    Foundation::HANDLE,
    Graphics::{
        Direct3D11::{
            D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11DeviceContext4, ID3D11Fence,
            ID3D11Resource, ID3D11Texture2D,
        },
        Direct3D12::*,
        Dxgi::{Common::*, DXGI_PRESENT_TEST},
    },
};
use windows::core::{HRESULT, IUnknown, Interface};
use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::CreateEventW};

const _: () = {
    assert!(RSF_NFG_FORMAT_RGBA16_FLOAT == DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32);
    assert!(RSF_NFG_FORMAT_RG32_FLOAT == DXGI_FORMAT_R32G32_FLOAT.0 as u32);
    assert!(RSF_NFG_FORMAT_RGB10A2_UNORM == DXGI_FORMAT_R10G10B10A2_UNORM.0 as u32);
    assert!(RSF_NFG_FORMAT_RGBA8_UNORM == DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32);
    assert!(RSF_NFG_FORMAT_RGBA8_UNORM_SRGB == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32);
    assert!(RSF_NFG_FORMAT_RG16_FLOAT == DXGI_FORMAT_R16G16_FLOAT.0 as u32);
    assert!(RSF_NFG_FORMAT_R32_FLOAT == DXGI_FORMAT_R32_FLOAT.0 as u32);
    assert!(RSF_NFG_FORMAT_BGRA8_UNORM == DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32);
    assert!(RSF_NFG_FORMAT_BGRA8_UNORM_SRGB == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as u32);
    assert!(RSF_NFG_STATE_COMMON == D3D12_RESOURCE_STATE_COMMON.0 as u32);
    assert!(RSF_NFG_STATE_PRESENT == D3D12_RESOURCE_STATE_PRESENT.0 as u32);
    assert!(RSF_NFG_STATE_UNORDERED_ACCESS == D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32);
    assert!(
        RSF_NFG_STATE_NON_PIXEL_SHADER_RESOURCE
            == D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32
    );
    assert!(RSF_NFG_PRESENT_TEST == DXGI_PRESENT_TEST.0);
};

unsafe extern "C" {
    fn rsf_d3d11_present_has_owner() -> i32;
    fn rsf_d3d11_present_is_owner(chain: Handle) -> i32;
    fn rsf_d3d11_present_provider() -> *const Provider;
    fn rsf_d3d11_present_session() -> Handle;
    fn rsf_d3d11_present_backend() -> u32;
    fn rsf_d3d11_present_generation() -> u64;
    fn rsf_d3d11_present_retire(
        session: Handle,
        list: Handle,
        fence: *mut Handle,
        value: *mut u64,
    ) -> i32;
    fn rsf_d3d11_present_abort(id: u64) -> i32;
    fn rsf_streamline_host_token(host: Handle, source: u64) -> Handle;
    fn rsf_dlss_evaluate(context: Handle, frame: *const DlssFrame) -> i32;
    fn rsf_dlss_evaluate_shared(list: Handle, frame: *const DlssFrame, source: u64) -> i32;
    fn rsf_input_dump_sr(context: Handle, frame: *const DlssFrame, backend: u32);
    fn rsf_input_dump_fg(context: Handle, pass: *const Pass, frame: *const DlssFrame);
    fn rsf_input_dump_fg_prepare(pass: *const Pass, frame: *const rsf_fg_frame, result: i32);
    fn rsf_input_dump_named_texture(
        context: Handle,
        texture: Handle,
        request: *const rsf_named_frame_dump,
        report: *mut rsf_named_frame_dump_report,
    ) -> i32;
}

pub(super) static NATIVE_PLATFORM: Platform = Platform {
    cpu: &rsf_native_fg_cpu_platform,
    present_has_owner: rsf_d3d11_present_has_owner,
    present_is_owner: rsf_d3d11_present_is_owner,
    present_host: rsf_core_d3d11p_host,
    present_provider: rsf_d3d11_present_provider,
    present_session: rsf_d3d11_present_session,
    present_backend: rsf_d3d11_present_backend,
    present_generation: rsf_d3d11_present_generation,
    present_graphics,
    present_retire: rsf_d3d11_present_retire,
    present_abort: rsf_d3d11_present_abort,
    host_token,
    window_match: rsf_native_window_match,
    scene_for_window: rsf_native_scene_for_window,
    scene_present_path: rsf_native_scene_classify_present,
    allows_fg: rsf_core_frame_allows_fg,
    context_device,
    query_context4: query::<ID3D11DeviceContext, ID3D11DeviceContext4>,
    query_texture: query::<IUnknown, ID3D11Texture2D>,
    texture_info,
    create_event,
    close_event,
    fence_create,
    fence_d3d11,
    fence_d3d12,
    surface_create,
    surface_destroy,
    surface_d3d11,
    surface_d3d12,
    fence_completed: rsf_d3d12_fence_completed,
    wait_fence,
    flush,
    copy_resource,
    context_signal,
    context_wait,
    create_commands,
    create_output,
    reset_commands,
    close_commands,
    execute,
    transition,
    copy_transitioned,
    queue_signal,
    queue_wait,
    dlss_evaluate: rsf_dlss_evaluate,
    dlss_evaluate_shared: rsf_dlss_evaluate_shared,
    dump_sr: rsf_input_dump_sr,
    dump_fg: rsf_input_dump_fg,
    dump_fg_prepare: rsf_input_dump_fg_prepare,
    dump_named,
};

unsafe fn borrow<T: Interface>(raw: &Handle) -> &T {
    // The graphics owner supplies a live interface borrowed from an active lease.
    unsafe { T::from_raw_borrowed(raw) }.expect("live native FG interface")
}

fn output(create: impl FnOnce(*mut Handle) -> HRESULT) -> Option<ComRef> {
    let mut raw = null_mut();
    let result = create(&mut raw);
    unsafe { ComRef::from_result(result, raw) }
}

unsafe extern "C" fn present_graphics(
    device: *mut Handle,
    queue: *mut Handle,
    native: *mut Handle,
) -> i32 {
    let mut graphics = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..rsf_streamline_graphics::EMPTY
    };
    if unsafe { rsf_core_d3d11p_graphics(&mut graphics) } == 0 {
        return 0;
    }
    unsafe {
        device.write(graphics.device);
        queue.write(graphics.queue);
        native.write(graphics.native_device);
    }
    1
}

unsafe extern "C" fn host_token(host: Handle, source: u64) -> i32 {
    i32::from(!unsafe { rsf_streamline_host_token(host, source) }.is_null())
}

unsafe extern "C" fn context_device(context: Handle) -> Handle {
    unsafe { borrow::<ID3D11DeviceContext>(&context).GetDevice() }
        .map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn query<S: Interface, T: Interface>(object: Handle) -> Handle {
    let source = unsafe { borrow::<S>(&object) };
    output(|out| unsafe { source.query(&T::IID, out) }).map_or(null_mut(), ComRef::into_raw)
}

unsafe extern "C" fn texture_info(texture: Handle, out: *mut rsf_nfg_texture_info) -> i32 {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        borrow::<ID3D11Texture2D>(&texture).GetDesc(&mut desc);
        out.write(rsf_nfg_texture_info {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
            samples: desc.SampleDesc.Count,
        });
    }
    1
}

unsafe extern "C" fn create_event() -> Handle {
    unsafe { CreateEventW(null(), 0, 0, null()) }
}

unsafe extern "C" fn close_event(event: Handle) {
    unsafe { CloseHandle(event) };
}

unsafe extern "C" fn fence_create(device11: Handle, device12: Handle) -> Handle {
    let mut fence = null_mut();
    if unsafe { rsf_shared_fence_create(device11, device12, None, null_mut(), &mut fence) } == 0 {
        fence.cast()
    } else {
        null_mut()
    }
}

unsafe extern "C" fn fence_d3d11(fence: Handle) -> Handle {
    unsafe { rsf_shared_fence_d3d11(fence.cast()) }
}

unsafe extern "C" fn fence_d3d12(fence: Handle) -> Handle {
    unsafe { rsf_shared_fence_d3d12(fence.cast()) }
}

unsafe extern "C" fn surface_create(
    device11: Handle,
    device12: Handle,
    width: u32,
    height: u32,
    format: u32,
) -> Handle {
    let setup = SurfaceSetup {
        struct_size: size_of::<SurfaceSetup>() as u32,
        abi_version: SURFACE_ABI,
        width,
        height,
        format,
        render_target: 0,
        log: None,
        log_user: null_mut(),
        unordered_access: 0,
    };
    let mut surface = null_mut();
    if unsafe { rsf_shared_surface_create(device11, device12, &setup, &mut surface) } == 0 {
        surface.cast()
    } else {
        null_mut()
    }
}

unsafe extern "C" fn surface_destroy(surface: Handle) {
    unsafe { rsf_shared_surface_destroy(surface.cast()) };
}

unsafe extern "C" fn surface_d3d11(surface: Handle) -> Handle {
    unsafe { rsf_shared_surface_d3d11(surface.cast()) }
}

unsafe extern "C" fn surface_d3d12(surface: Handle) -> Handle {
    unsafe { rsf_shared_surface_d3d12(surface.cast()) }
}

unsafe extern "C" fn arm_fence(fence: Handle, value: u64, event: Handle) -> i32 {
    unsafe { borrow::<ID3D12Fence>(&fence).SetEventOnCompletion(value, HANDLE(event)) }
        .map_or_else(|error| error.code().0, |()| 0)
}

unsafe extern "C" fn wait_fence(fence: Handle, value: u64, event: Handle) -> i32 {
    i32::from(
        unsafe {
            rsf_core_d3d12_wait_fence(
                fence,
                value,
                event,
                10_000,
                rsf_d3d12_fence_completed,
                arm_fence,
            )
        } >= 0,
    )
}

unsafe extern "C" fn flush(context: Handle) {
    unsafe { borrow::<ID3D11DeviceContext4>(&context).Flush() };
}

unsafe extern "C" fn copy_resource(context: Handle, destination: Handle, source: Handle) {
    unsafe {
        borrow::<ID3D11DeviceContext4>(&context).CopyResource(
            borrow::<ID3D11Resource>(&destination),
            borrow::<ID3D11Resource>(&source),
        );
    }
}

unsafe extern "C" fn context_signal(context: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe {
            borrow::<ID3D11DeviceContext4>(&context).Signal(borrow::<ID3D11Fence>(&fence), value)
        }
        .is_ok(),
    )
}

unsafe extern "C" fn context_wait(context: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe {
            borrow::<ID3D11DeviceContext4>(&context).Wait(borrow::<ID3D11Fence>(&fence), value)
        }
        .is_ok(),
    )
}

unsafe extern "C" fn create_commands(
    device: Handle,
    allocator: *mut Handle,
    list: *mut Handle,
) -> i32 {
    let native = unsafe { borrow::<ID3D12Device>(&device) };
    let kind = D3D12_COMMAND_LIST_TYPE_DIRECT;
    let Some(new_allocator) = output(|out| unsafe {
        (native.vtable().CreateCommandAllocator)(device, kind, &ID3D12CommandAllocator::IID, out)
    }) else {
        return 0;
    };
    let Some(new_list) = output(|out| unsafe {
        (native.vtable().CreateCommandList)(
            device,
            0,
            kind,
            new_allocator.as_ptr(),
            null_mut(),
            &ID3D12GraphicsCommandList::IID,
            out,
        )
    }) else {
        return 0;
    };
    if unsafe { close_commands(new_list.as_ptr()) } == 0 {
        return 0;
    }
    unsafe {
        allocator.write(new_allocator.into_raw());
        list.write(new_list.into_raw());
    }
    1
}

unsafe extern "C" fn create_output(device: Handle, width: u32, height: u32, format: u32) -> Handle {
    let native = unsafe { borrow::<ID3D12Device>(&device) };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(width),
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT(format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    output(|out| unsafe {
        (native.vtable().CreateCommittedResource)(
            device,
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            null(),
            &ID3D12Resource::IID,
            out,
        )
    })
    .map_or(null_mut(), ComRef::into_raw)
}

unsafe extern "C" fn reset_commands(allocator: Handle, list: Handle) -> i32 {
    let allocator = unsafe { borrow::<ID3D12CommandAllocator>(&allocator) };
    i32::from(
        unsafe {
            allocator.Reset().and_then(|()| {
                borrow::<ID3D12GraphicsCommandList>(&list)
                    .Reset(allocator, None::<&ID3D12PipelineState>)
            })
        }
        .is_ok(),
    )
}

unsafe extern "C" fn close_commands(list: Handle) -> i32 {
    i32::from(unsafe { borrow::<ID3D12GraphicsCommandList>(&list).Close() }.is_ok())
}

unsafe extern "C" fn execute(queue: Handle, list: Handle) {
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    // The list stays borrowed for ExecuteCommandLists. No reference is transferred.
    let lists = ManuallyDrop::new([Some(unsafe { ID3D12CommandList::from_raw(list) })]);
    unsafe { queue.ExecuteCommandLists(&lists[..]) };
}

unsafe extern "C" fn transition(list: Handle, resource: Handle, before: u32, after: u32) {
    if before == after {
        return;
    }
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource) })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: D3D12_RESOURCE_STATES(before as i32),
                StateAfter: D3D12_RESOURCE_STATES(after as i32),
            }),
        },
    };
    unsafe { borrow::<ID3D12GraphicsCommandList>(&list).ResourceBarrier(&[barrier]) };
}

unsafe extern "C" fn copy_transitioned(
    list: Handle,
    destination: Handle,
    before_dst: u32,
    after_dst: u32,
    source: Handle,
    before_src: u32,
    after_src: u32,
) {
    let copy_src = D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
    let copy_dst = D3D12_RESOURCE_STATE_COPY_DEST.0 as u32;
    unsafe {
        transition(list, source, before_src, copy_src);
        transition(list, destination, before_dst, copy_dst);
        borrow::<ID3D12GraphicsCommandList>(&list).CopyResource(
            borrow::<ID3D12Resource>(&destination),
            borrow::<ID3D12Resource>(&source),
        );
        transition(list, destination, copy_dst, after_dst);
        transition(list, source, copy_src, after_src);
    }
}

unsafe extern "C" fn queue_signal(queue: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe {
            borrow::<ID3D12CommandQueue>(&queue).Signal(borrow::<ID3D12Fence>(&fence), value)
        }
        .is_ok(),
    )
}

unsafe extern "C" fn queue_wait(queue: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe { borrow::<ID3D12CommandQueue>(&queue).Wait(borrow::<ID3D12Fence>(&fence), value) }
            .is_ok(),
    )
}

unsafe extern "C" fn dump_named(
    context: Handle,
    texture: Handle,
    directory: *const c_char,
    name: *const c_char,
    view: u32,
    scale: f32,
    log: Log,
    user: Handle,
) -> i32 {
    let request = rsf_named_frame_dump {
        struct_size: size_of::<rsf_named_frame_dump>() as u32,
        abi_version: RSF_NAMED_FRAME_DUMP_ABI_VERSION,
        directory_utf8: directory,
        name,
        view,
        scale,
        log,
        log_user: user,
    };
    unsafe { rsf_input_dump_named_texture(context, texture, &request, null_mut()) }
}
