// SPDX-License-Identifier: MIT
//! AC7 engine observations around the typed native trampolines. The only raw runtime memory
//! reads go through the guarded platform copy.

use super::*;
use crate::motion_capture_session::native::checkpoint_native;
use crate::motion_capture_session::{
    rsf_ac7_capture_fail, rsf_ac7_capture_sample, is_sampled,
};
use crate::sys::rsf_ac7_read_memory;
use std::ptr::from_mut;
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Threading::GetCurrentThreadId;

unsafe extern "C" {
    fn rsf_ac7_capture_get_graph_output(
        getter: u64,
        node: u64,
        index: u32,
        output: *mut u64,
    ) -> bool;
}

fn tick() -> u64 {
    let mut ticks = 0;
    let _ = unsafe { QueryPerformanceCounter(&mut ticks) };
    ticks as u64
}

fn read<T>(base: u64, offset: usize, destination: &mut T) -> bool {
    unsafe {
        rsf_ac7_read_memory(
            from_mut(destination).cast(),
            base as usize,
            offset,
            size_of::<T>(),
        )
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_begin_engine(
    event: &mut EngineRecord,
    capture: CaptureInterval,
    primitive: u64,
    view: u64,
    check_visibility: bool,
) {
    event.qpc = tick();
    event.thread = unsafe { GetCurrentThreadId() };
    event.primitive = primitive;
    event.view = view;
    event.interval = capture.interval;
    let mut relevance = 0;
    let mut valid = read(primitive, 8, &mut event.proxy)
        && read(primitive, 0xe4, &mut event.index)
        && read(primitive, 0x10, &mut event.component)
        && read(view, 0x1490, &mut relevance);
    let facts = &mut event.facts;
    facts.visible = 1;
    let proxy = event.proxy;
    if valid && event.index < 1_048_576 {
        valid = read(
            relevance,
            event.index as usize * 8 + 4,
            &mut facts.relevance,
        ) && read(proxy, 0x40, &mut facts.mobility)
            && read(proxy, 0x118, &mut facts.radius)
            && read(proxy, 0x100, &mut event.origin)
            && read(view, 0x40c, &mut event.camera)
            && read(view, 0xc2c, &mut facts.lod_factor)
            && read(view, 0x1174, &mut facts.minimum_size)
            && read(proxy, 0xc0, &mut event.current);
        let (mut cut, mut flags) = (0u8, 0u8);
        valid = valid && read(view, 0xc34, &mut cut) && read(proxy, 0x4a, &mut flags);
        facts.camera_cut = u32::from(cut != 0);
        facts.always_velocity = u32::from(flags & 1);
        if check_visibility {
            let mut storage = 0;
            valid = valid && read(view, 0x1430, &mut storage);
            if storage == 0 {
                storage = view.wrapping_add(0x1420);
            }
            let mut word = 0u32;
            valid = valid && read(storage, event.index as usize / 32 * 4, &mut word);
            facts.visible = (word >> (event.index % 32)) & 1;
        }
        for (from, to) in event.origin.into_iter().zip(event.camera) {
            let distance = from - to;
            facts.distance_squared += distance * distance;
        }
    } else {
        valid = false;
    }
    facts.fields_valid = u32::from(valid);
}

fn initialize_root(root: &mut RootRecord, capture: CaptureInterval, kind: u32, owner: u64) {
    // The fields are numeric observations with zero denoting absence. The helper assigns scope IDs.
    init_root(root, kind, owner);
    root.qpc = tick();
    root.thread = unsafe { GetCurrentThreadId() };
    root.session = capture.session;
    root.interval = capture.interval;
}

fn capture_blob<const N: usize>(base: u64, capture: CaptureInterval) -> u32 {
    if !is_sampled(capture.interval) {
        return 0;
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(N).is_err() {
        rsf_ac7_capture_fail(capture);
        return 0;
    }
    bytes.resize(N, 0);
    if !unsafe { rsf_ac7_read_memory(bytes.as_mut_ptr().cast(), base as usize, 0, N) } {
        return 0;
    }
    store_blob(capture, &bytes).unwrap_or_else(|| {
        rsf_ac7_capture_fail(capture);
        0
    })
}

fn widget_fields(root: &mut RootRecord) {
    let converter = root.owner;
    root.fields_valid = u32::from(
        read(converter, 0x28, &mut root.draw_size)
            && read(converter, 0x30, &mut root.widget)
            && read(converter, 0x48, &mut root.target)
            && read(converter, 0xc0, &mut root.downsample)
            && read(converter, 0xc8, &mut root.blur_x)
            && read(converter, 0xd0, &mut root.blur_y)
            && read(converter, 0xd8, &mut root.glow),
    );
}

fn graph_output(root: &mut RootRecord) {
    root.output_valid = 0;
    root.output_name.fill(0);
    let node = root.output_node;
    let (mut getter, mut output, mut name) = (0, 0, 0);
    if !(read(node, 0, &mut root.vtable)
        && read(root.vtable, 0x38, &mut getter)
        && read(root.vtable, 0x28, &mut root.process_fn)
        && read(root.vtable, 0x70, &mut root.desc_fn)
        && root.output_id <= 7
        && unsafe { rsf_ac7_capture_get_graph_output(getter, node, root.output_id, &mut output) }
        && read(output, 0x14, &mut root.width)
        && read(output, 0x18, &mut root.height)
        && read(output, 0x2c, &mut root.format)
        && read(output, 0x40, &mut name)
        && read(output, 0x50, &mut root.pooled_target))
    {
        return;
    }
    root.output_valid = 1;
    for (index, byte) in root.output_name[..63].iter_mut().enumerate() {
        let mut unit = 0u16;
        if !read(name, index * 2, &mut unit) || unit == 0 {
            break;
        }
        *byte = if (32..127).contains(&unit) && !matches!(unit, 34 | 92) {
            unit as c_char
        } else {
            b'?' as c_char
        };
    }
}

fn associate_commands(root: &mut RootRecord) {
    let (mut link, mut command, mut uid) = (0, 0, 0);
    if !(read(root.command_list, 8, &mut link)
        && read(root.command_list, 0x18, &mut uid)
        && commands_changed(root, link, uid)
        && read(root.queue_begin, 0, &mut command))
    {
        return;
    }
    root.queue_end = link;
    while command != 0 {
        let (mut next, mut execute) = (0, 0);
        if !read(command, 0, &mut next) || !read(command, 8, &mut execute) {
            break;
        }
        match associate_command(root, command, execute) {
            CommandAdvance::Next => command = next,
            CommandAdvance::Stop => break,
            CommandAdvance::Failed => {
                rsf_ac7_capture_fail(CaptureInterval {
                    session: root.session,
                    interval: root.interval,
                });
                break;
            }
        }
    }
}

fn record(root: &RootRecord) {
    if !record_root(root) {
        rsf_ac7_capture_fail(CaptureInterval {
            session: root.session,
            interval: root.interval,
        });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_root_before(
    root: &mut RootRecord,
    capture: CaptureInterval,
    kind: u32,
    owner: u64,
    first: u64,
    second: u64,
    third: u64,
) {
    initialize_root(root, capture, kind, owner);
    match kind {
        POSTPROCESS_BEGIN => {
            root.command_list = first;
            root.view = second;
            root.velocity_reference = third;
            root.fields_valid = u32::from(read(third, 0, &mut root.velocity_rhi));
            // Per-view stride from the matched AC7 renderer.
            root.view_blob = capture_blob::<0x27c0>(second, capture);
        }
        WIDGET_PREPARE_BEFORE | WIDGET_QUEUE_BEFORE => widget_fields(root),
        MAIN_TEMPORAL_BUILD_BEFORE => {
            root.context = owner;
            root.fields_valid = u32::from(
                read(owner, 0x10, &mut root.view)
                    && read(owner, 0x28, &mut root.input_node)
                    && read(first, 0, &mut root.velocity_node),
            );
        }
        MAIN_TEMPORAL_PROCESS_BEGIN => {
            root.context = first;
            root.output_node = owner;
            root.fields_valid = u32::from(
                read(first, 0, &mut root.view)
                    && read(first, 0x28, &mut root.command_list)
                    && read(owner, 0, &mut root.vtable),
            );
            root.node_blob = capture_blob::<0xe0>(owner, capture);
        }
        GRAPH_PASS_BEGIN => {
            root.context = second;
            root.fields_valid = u32::from(
                read(first, 0, &mut root.output_node)
                    && read(first, 8, &mut root.output_id)
                    && read(second, 0, &mut root.view)
                    && read(second, 0x28, &mut root.command_list)
                    && read(root.view, 0, &mut root.family)
                    && read(root.family, 0x68, &mut root.native_frame)
                    && read(root.command_list, 8, &mut root.queue_begin)
                    && read(root.command_list, 0x18, &mut root.queue_uid),
            );
            if root.fields_valid != 0 && root.output_node != 0 {
                graph_output(root);
            }
        }
        _ => return,
    }
    record(root);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_root_after(root: &mut RootRecord, capture: CaptureInterval) {
    let begin = root.kind;
    end_root(root, tick());
    match begin {
        WIDGET_PREPARE_BEFORE | WIDGET_QUEUE_BEFORE => widget_fields(root),
        MAIN_TEMPORAL_BUILD_BEFORE => {
            let mut valid =
                root.fields_valid != 0 && read(root.context, 0x28, &mut root.output_node);
            if root.output_node != 0 {
                valid = valid && read(root.output_node, 0, &mut root.vtable);
            }
            root.fields_valid = u32::from(valid);
        }
        GRAPH_PASS_BEGIN if root.fields_valid != 0 && root.output_node != 0 => {
            graph_output(root);
            associate_commands(root);
        }
        _ => {}
    }
    record(root);
    if begin == POSTPROCESS_BEGIN {
        checkpoint_native(capture);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_motion_capture_native_pass(
    pass: *const rsf_game_render_pass,
    begin: u32,
) {
    if !unsafe { execution_pass(pass) } {
        return;
    }
    let capture = rsf_ac7_capture_sample();
    if capture.active() {
        // Every field of a root record is a number or a byte array.
        let mut root: RootRecord = unsafe { std::mem::zeroed() };
        let kind = if begin != 0 {
            RHI_SCOPE_BEGIN
        } else {
            RHI_SCOPE_RESTORE
        };
        initialize_root(&mut root, capture, kind, 0);
        execution_root(&mut root);
        record(&root);
    }
}
