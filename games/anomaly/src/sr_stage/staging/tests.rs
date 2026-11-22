// SPDX-License-Identifier: MIT
use super::*;
use crate::sr_d3d11::{
    Extent, fixture_created, fixture_platform, fixture_references, fixture_source,
};
use std::cell::RefCell;
use std::ptr::{null_mut, without_provenance_mut};

// The stage leaves the burst readback calls. Each test thread records its own calls in order.
struct Leaves {
    calls: Vec<String>,
    immediate: bool,
    device: bool,
    map: bool,
}

impl Default for Leaves {
    fn default() -> Self {
        Self {
            calls: vec![],
            immediate: true,
            device: true,
            map: true,
        }
    }
}

thread_local! {
    static LEAVES: RefCell<Leaves> = RefCell::new(Leaves::default());
}

fn leaves<R>(f: impl FnOnce(&mut Leaves) -> R) -> R {
    LEAVES.with_borrow_mut(f)
}

fn record(call: String) {
    leaves(|state| state.calls.push(call));
}

fn calls() -> Vec<String> {
    leaves(|state| std::mem::take(&mut state.calls))
}

const CONTEXT: usize = 0x900;
const SOURCE: usize = 0x500;
// A static, so every use names the same address.
static ROWS: [u8; 64] = [7; 64];

fn context() -> *mut c_void {
    without_provenance_mut(CONTEXT)
}

fn d3d() -> &'static D3d11Platform {
    unsafe { &*fixture_platform().cast::<D3d11Platform>() }
}

extern "C" fn temporal_platform() -> *const c_void {
    fixture_platform()
}
extern "C" fn immediate_context(context: *mut c_void) -> bool {
    assert_eq!(context.addr(), CONTEXT);
    record("GetType".into());
    leaves(|state| state.immediate)
}
extern "C" fn context_device(_: *mut c_void) -> *mut c_void {
    record("GetDevice".into());
    if !leaves(|state| state.device) {
        return null_mut();
    }
    // An owned reference to the fixture device, the one its textures report.
    unsafe { (d3d().texture_device)(null_mut()) }
}
extern "C" fn copy_subresource(
    _: *mut c_void,
    destination: *mut c_void,
    destination_subresource: u32,
    source: *mut c_void,
    source_subresource: u32,
) {
    record(format!(
        "copy {:#x}:{destination_subresource} <- {:#x}:{source_subresource}",
        destination.addr(),
        source.addr()
    ));
}
extern "C" fn map_read(
    _: *mut c_void,
    texture: *mut c_void,
    subresource: u32,
    data: *mut *const c_void,
    row_pitch: *mut u32,
) -> bool {
    record(format!("map {:#x}:{subresource}", texture.addr()));
    if !leaves(|state| state.map) {
        return false;
    }
    unsafe {
        data.write(ROWS.as_ptr().cast());
        row_pitch.write(32);
    }
    true
}
extern "C" fn unmap(_: *mut c_void, texture: *mut c_void, subresource: u32) {
    record(format!("unmap {:#x}:{subresource}", texture.addr()));
}
extern "C" fn pixel_bytes(format: u32) -> u32 {
    record(format!("pixel {format}"));
    if format == 10 { 8 } else { 0 }
}

fn platform() -> Platform {
    Platform {
        temporal_platform: Some(temporal_platform),
        immediate_context: Some(immediate_context),
        context_device: Some(context_device),
        copy_subresource: Some(copy_subresource),
        map_read: Some(map_read),
        unmap: Some(unmap),
        pixel_bytes: Some(pixel_bytes),
        ..Default::default()
    }
}

// A world twin: three mips, two layers, bound as SRV and RTV, with a misc flag the copy must not keep.
fn twin(samples: u32) -> Texture {
    Texture {
        size: Extent {
            width: 4,
            height: 2,
        },
        mips: 3,
        layers: 2,
        format: 10,
        samples,
        sample_quality: 0,
        usage: 0,
        bindings: 0x28,
        cpu_access: 0,
        misc: 0x4,
    }
}

#[test]
fn copy_describes_one_cpu_readable_subresource_and_copies_it_unpredicated() {
    let p = platform();
    let source = fixture_source(SOURCE, twin(1));
    let copy = copy(&p, context(), source).expect("staging copy");
    let staging = copy.0.as_ptr().addr();
    assert_eq!(
        fixture_created(),
        [Texture {
            mips: 1,
            layers: 1,
            usage: USAGE_STAGING,
            bindings: 0,
            cpu_access: CPU_ACCESS_READ,
            misc: 0,
            ..twin(1)
        }]
    );
    assert_eq!(
        calls(),
        [
            "GetType".to_owned(),
            "GetDevice".to_owned(),
            format!("copy {staging:#x}:0 <- {SOURCE:#x}:0"),
        ]
    );
    // Only the copy stays held: the queried source and both device references were released.
    assert_eq!(fixture_references(), 1);
    drop(copy);
    assert_eq!(fixture_references(), 0);
}

#[test]
fn copy_refuses_before_creating_or_copying() {
    let p = platform();
    let source = fixture_source(SOURCE, twin(1));
    assert!(copy(&p, null_mut(), source).is_none());
    assert!(copy(&p, context(), null_mut()).is_none());
    assert!(calls().is_empty());

    leaves(|state| state.immediate = false);
    assert!(copy(&p, context(), source).is_none());
    assert_eq!(calls(), ["GetType"]);
    leaves(|state| state.immediate = true);

    // Not a 2D texture: QueryInterface fails.
    assert!(copy(&p, context(), without_provenance_mut(0x501)).is_none());
    assert_eq!(calls(), ["GetType"]);

    let multisampled = fixture_source(0x502, twin(4));
    assert!(copy(&p, context(), multisampled).is_none());
    assert_eq!(calls(), ["GetType"]);

    leaves(|state| state.device = false);
    assert!(copy(&p, context(), source).is_none());
    assert_eq!(calls(), ["GetType", "GetDevice"]);
    leaves(|state| state.device = true);

    let missing = Platform {
        copy_subresource: None,
        ..platform()
    };
    assert!(copy(&missing, context(), source).is_none());
    assert!(calls().is_empty());

    assert!(fixture_created().is_empty());
    assert_eq!(fixture_references(), 0);
}

#[test]
fn map_reads_subresource_zero_and_unmaps_it_on_drop() {
    let p = platform();
    let source = fixture_source(SOURCE, twin(1));
    let held = copy(&p, context(), source).expect("staging copy");
    let staging = held.0.as_ptr().addr();
    calls();
    let (layout, mapping) = map(&p, context(), &held);
    let mapping = mapping.expect("mapped");
    assert_eq!(
        (
            layout.width,
            layout.height,
            layout.format,
            layout.pixel_bytes
        ),
        (4, 2, 10, 8)
    );
    assert_eq!(
        mapping.rows().unwrap().next().unwrap().as_ptr(),
        ROWS.as_ptr()
    );
    assert_eq!(
        calls(),
        [
            "pixel 10".to_owned(),
            "GetDevice".to_owned(),
            "GetType".to_owned(),
            format!("map {staging:#x}:0"),
        ]
    );
    // The texture stays held while mapped; only the context's device reference is gone.
    assert_eq!(fixture_references(), 1);
    drop(mapping);
    assert_eq!(calls(), [format!("unmap {staging:#x}:0")]);
    drop(held);
    assert_eq!(fixture_references(), 0);
}

#[test]
fn map_refusals_keep_the_size_and_format_and_never_unmap() {
    let p = platform();
    let source = fixture_source(SOURCE, twin(1));
    let held = copy(&p, context(), source).expect("staging copy");
    calls();

    leaves(|state| state.immediate = false);
    let (layout, mapping) = map(&p, context(), &held);
    assert!(mapping.is_none());
    assert_eq!((layout.width, layout.height, layout.format), (4, 2, 10));
    assert_eq!(calls(), ["pixel 10", "GetDevice", "GetType"]);
    leaves(|state| state.immediate = true);

    leaves(|state| state.map = false);
    let (layout, mapping) = map(&p, context(), &held);
    assert!(mapping.is_none());
    assert_eq!(layout.pixel_bytes, 8);
    assert_eq!(
        calls().last().map(|call| call.starts_with("map ")),
        Some(true)
    );
    leaves(|state| state.map = true);

    // A format without a known pixel size refuses before GetType.
    let unknown = fixture_source(
        0x503,
        Texture {
            format: 2,
            ..twin(1)
        },
    );
    let unknown = GpuLease(d3d().lease(unsafe { (d3d().as_texture)(unknown) }).unwrap());
    let (layout, mapping) = map(&p, context(), &unknown);
    assert!(mapping.is_none());
    assert_eq!((layout.format, layout.pixel_bytes), (2, 0));
    assert_eq!(calls(), ["pixel 2", "GetDevice"]);

    drop(unknown);
    drop(held);
    assert_eq!(fixture_references(), 0);
}
