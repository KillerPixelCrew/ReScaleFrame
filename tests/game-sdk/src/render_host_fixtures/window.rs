// SPDX-License-Identifier: MIT
use crate::sys::*;
use rsf_game_sys::{
    RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_PRIMARY,
    RSF_GAME_RENDER_TEXTURE_BINDING, RSF_GAME_RENDER_WINDOW, rsf_game_render_pass as Pass,
};
use rsf_test_support::{qpc, qpc_frequency};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{E_NOINTERFACE, S_FALSE, S_OK};
use windows_sys::core::GUID;

#[repr(C)]
struct UnknownVtable {
    query: unsafe extern "system" fn(*mut Unknown, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut Unknown) -> u32,
    release: unsafe extern "system" fn(*mut Unknown) -> u32,
}

#[repr(C)]
struct Unknown {
    vtable: &'static UnknownVtable,
    identity: *mut Self,
    refs: u32,
    queries: u32,
    available: bool,
}

static VTABLE: UnknownVtable = UnknownVtable {
    query,
    add_ref,
    release,
};

unsafe extern "system" fn query(
    this: *mut Unknown,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> i32 {
    unsafe { out.write(null_mut()) };
    let object = unsafe { &mut *this };
    object.queries += 1;
    let iid = unsafe { &*iid };
    if !object.available
        || (iid.data1, iid.data2, iid.data3, iid.data4) != (0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46])
    {
        return E_NOINTERFACE;
    }
    let identity = if object.identity.is_null() {
        this
    } else {
        object.identity
    };
    unsafe {
        add_ref(identity);
        out.write(identity.cast());
    }
    S_OK
}

unsafe extern "system" fn add_ref(this: *mut Unknown) -> u32 {
    unsafe {
        (*this).refs += 1;
        (*this).refs
    }
}

unsafe extern "system" fn release(this: *mut Unknown) -> u32 {
    unsafe {
        (*this).refs -= 1;
        (*this).refs
    }
}

impl Unknown {
    fn new() -> Self {
        Self {
            vtable: &VTABLE,
            identity: null_mut(),
            refs: 1,
            queries: 0,
            available: true,
        }
    }
}

fn window_pass(chain: *mut Unknown) -> Pass {
    Pass {
        struct_size: size_of::<Pass>() as u32,
        role: RSF_GAME_RENDER_WINDOW,
        flags: RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION,
        session_id: 10,
        source_frame_id: 20,
        scope_id: 30,
        viewport_key: 40,
        window_key: 50,
        rhi_viewport_key: 60,
        swapchain: chain.cast(),
        // The remaining ABI fields are scalar or nullable.
        ..Default::default()
    }
}

fn scope_contract() {
    let (mut chain, mut alias, mut other) = (Unknown::new(), Unknown::new(), Unknown::new());
    let chain_ptr = &raw mut chain;
    let alias_ptr = &raw mut alias;
    let other_ptr = &raw mut other;
    alias.identity = chain_ptr;
    let mut outer = window_pass(chain_ptr);
    let inner = Pass {
        scope_id: outer.scope_id + 1,
        ..window_pass(other_ptr)
    };
    let mut matched = window_pass(null_mut());
    let begin = |pass: &Pass| unsafe { rsf_native_window_scope(pass, 1) };
    let end = |pass: &Pass| unsafe { rsf_native_window_scope(pass, 0) };
    let find = |chain: *mut Unknown, pass: &mut Pass| unsafe {
        rsf_native_window_match(chain.cast(), pass)
    };

    assert_eq!(find(chain_ptr, &mut matched), 0);
    assert_eq!(
        begin(&Pass {
            struct_size: outer.struct_size - 1,
            ..outer
        }),
        0
    );
    assert_eq!(begin(&outer), 1);
    assert_eq!(find(alias_ptr, &mut matched), 1);
    assert_eq!(
        (matched.swapchain, matched.scope_id),
        (outer.swapchain, outer.scope_id)
    );
    assert_eq!((chain.refs, alias.refs), (1, 1));
    unsafe { (*chain_ptr).available = false };
    assert_eq!(find(alias_ptr, &mut matched), 0);
    assert_eq!(
        chain.refs, 1,
        "release the observed identity after the owned query fails"
    );
    unsafe { (*chain_ptr).available = true };

    assert_eq!(begin(&inner), 1);
    assert_eq!(find(chain_ptr, &mut matched), 0);
    assert_eq!(find(other_ptr, &mut matched), 1);
    assert_eq!(matched.scope_id, inner.scope_id);
    assert_eq!(end(&inner), 1);
    assert_eq!(find(chain_ptr, &mut matched), 1);

    assert_eq!(begin(&inner), 1);
    assert_eq!(
        end(&outer),
        0,
        "out-of-order end drops both borrowed scopes"
    );
    assert_eq!(find(other_ptr, &mut matched), 0);
    assert_eq!(find(chain_ptr, &mut matched), 0);
    assert_eq!(end(&inner), 0);
    assert_eq!(begin(&outer), 1);
    assert_eq!(
        end(&Pass {
            swapchain: alias_ptr.cast(),
            ..outer
        }),
        0
    );
    assert_eq!(find(chain_ptr, &mut matched), 0);

    for scope in 1..=32 {
        outer.scope_id = scope;
        assert_eq!(begin(&outer), 1);
    }
    assert_eq!(begin(&inner), 0);
    assert_eq!(begin(&inner), 0);
    assert_eq!(find(chain_ptr, &mut matched), 0);
    assert_eq!(end(&inner), 0);
    assert_eq!(find(chain_ptr, &mut matched), 0);
    assert_eq!(end(&inner), 0);
    assert_eq!(find(chain_ptr, &mut matched), 1);
    assert_eq!(matched.scope_id, 32);
    for scope in (1..=32).rev() {
        outer.scope_id = scope;
        assert_eq!(end(&outer), 1);
    }
    assert_eq!(find(chain_ptr, &mut matched), 0);

    #[repr(C)]
    struct ExtendedPass {
        pass: Pass,
        extension: u64,
    }
    let extended = ExtendedPass {
        pass: Pass {
            struct_size: size_of::<ExtendedPass>() as u32,
            ..outer
        },
        extension: 1,
    };
    assert_eq!(begin(&extended.pass), 1);
    assert_eq!(find(chain_ptr, &mut matched), 1);
    assert_eq!(matched.struct_size, outer.struct_size);
    assert_eq!(end(&extended.pass), 1);
}

fn present_contract() {
    let (mut chain, mut alias, mut surface) = (Unknown::new(), Unknown::new(), Unknown::new());
    let chain_ptr = &raw mut chain;
    alias.identity = chain_ptr;
    let window = window_pass(chain_ptr);
    let scene = Pass {
        role: RSF_GAME_RENDER_FINAL_SCENE,
        submission_id: 70,
        view_key: 80,
        native_frame: 90,
        scene_surface: (&raw mut surface).cast(),
        ..window
    };
    let binding = Pass {
        role: RSF_GAME_RENDER_TEXTURE_BINDING,
        sampled_texture: scene.scene_surface,
        ..window
    };
    let mut record = rsf_native_present_record {
        struct_size: size_of::<rsf_native_present_record>() as u32,
        ..Default::default()
    };
    let begin = rsf_observer_present_event {
        struct_size: size_of::<rsf_observer_present_event>() as u32,
        completed: 0,
        swapchain: (&raw mut alias).cast(),
        sync_interval: 1,
        flags: 0,
        result: 0,
        method: 1,
    };
    let mut end = rsf_observer_present_event {
        completed: 1,
        swapchain: chain_ptr.cast(),
        result: S_OK,
        ..begin
    };
    unsafe {
        assert_eq!(rsf_native_scene_pass(&scene, 1), 1);
        assert_eq!(rsf_native_scene_pass(&scene, 0), 1);
        assert_eq!(rsf_native_scene_texture_binding(&binding), 1);
        assert_eq!(rsf_native_window_scope(&window, 1), 1);

        let queries = chain.queries + alias.queries;
        assert_eq!(rsf_native_window_present(null()), 0);
        assert_eq!(
            rsf_native_window_present(&rsf_observer_present_event {
                struct_size: begin.struct_size - 1,
                ..begin
            }),
            0
        );
        assert_eq!(
            rsf_native_window_present(&rsf_observer_present_event { method: 2, ..begin }),
            0
        );
        assert_eq!(chain.queries + alias.queries, queries);
        let before_begin = qpc();
        assert_eq!(rsf_native_window_present(&begin), 1);
        assert_eq!(rsf_native_window_last_present(&mut record), 0);

        let address = chain_ptr as usize;
        std::thread::spawn(move || {
            let swapchain = address as *mut c_void;
            let mut matched = window_pass(null_mut());
            let mut other = record;
            let end = rsf_observer_present_event {
                struct_size: size_of::<rsf_observer_present_event>() as u32,
                completed: 1,
                swapchain,
                sync_interval: 1,
                flags: 0,
                result: S_OK,
                method: 1,
            };
            assert_eq!(rsf_native_window_match(swapchain, &mut matched), 0);
            assert_eq!(rsf_native_window_present(&end), 0);
            assert_eq!(rsf_native_window_last_present(&mut other), 0);
            rsf_native_window_reset();
        })
        .join()
        .expect("isolated window scope and pending Present on another thread");

        const DXGI_PRESENT_TEST: u32 = 1;
        assert_eq!(
            rsf_native_window_present(&rsf_observer_present_event {
                flags: DXGI_PRESENT_TEST,
                ..begin
            }),
            0
        );
        assert_eq!(
            rsf_native_window_present(&rsf_observer_present_event {
                flags: DXGI_PRESENT_TEST,
                ..end
            }),
            0
        );
        let before_end = qpc();
        assert_eq!(rsf_native_window_present(&end), 1);
        assert_eq!(rsf_native_window_last_present(&mut record), 1);
        let after_end = qpc();
        assert_eq!(
            (
                record.matched,
                record.completed,
                record.accepted,
                record.result,
                record.method,
                record.sync_interval
            ),
            (1, 1, 1, S_OK, 1, 1)
        );
        assert_eq!(
            (
                record.session_id,
                record.source_frame_id,
                record.scope_id,
                record.window_key,
                record.viewport_key
            ),
            (
                window.session_id,
                window.source_frame_id,
                window.scope_id,
                window.window_key,
                window.viewport_key
            )
        );
        assert!((before_begin..=before_end).contains(&record.begin_qpc));
        assert!((before_end..=after_end).contains(&record.end_qpc));
        assert_eq!(record.qpc_frequency, qpc_frequency());
        assert_eq!(
            (
                record.scene_matched,
                record.scene_texture_bound,
                record.scene_direct_present
            ),
            (1, 1, 0)
        );
        assert_eq!(
            (
                record.scene_submission_id,
                record.scene_view_key,
                record.scene_native_frame
            ),
            (scene.submission_id, scene.view_key, scene.native_frame)
        );

        assert_eq!(rsf_native_window_present(&begin), 1);
        assert_eq!(rsf_native_window_last_present(&mut record), 0);
        assert_eq!(
            rsf_native_scene_pass(&scene, 1),
            0,
            "duplicate scene is ambiguous"
        );
        end.result = S_FALSE;
        assert_eq!(rsf_native_window_present(&end), 1);
        assert_eq!(rsf_native_window_last_present(&mut record), 1);
        assert_eq!(
            (record.matched, record.result, record.accepted),
            (1, S_FALSE, 0)
        );
        assert_eq!(
            (
                record.scene_matched,
                record.scene_texture_bound,
                record.scene_direct_present
            ),
            (0, 0, 0)
        );

        assert_eq!(rsf_native_window_present(&begin), 1);
        assert_eq!(
            rsf_native_window_present(&rsf_observer_present_event { method: 0, ..end }),
            0
        );
        assert_eq!(rsf_native_window_last_present(&mut record), 0);
        assert_eq!(rsf_native_window_present(&end), 0);
        assert_eq!(rsf_native_window_present(&begin), 1);
        assert_eq!(rsf_native_window_scope(&window, 0), 1);
        assert_eq!(
            rsf_native_window_present(&end),
            0,
            "post rejects a retired window lease"
        );
        assert_eq!(rsf_native_window_present(&begin), 1);
        assert_eq!(rsf_native_window_present(&end), 1);
        assert_eq!(rsf_native_window_last_present(&mut record), 1);
        assert_eq!((record.matched, record.session_id), (0, 0));
        record.struct_size -= 1;
        assert_eq!(rsf_native_window_last_present(&mut record), 0);
        record.struct_size += 1;
        rsf_native_window_reset();
        assert_eq!(rsf_native_window_last_present(&mut record), 0);
        rsf_native_scene_reset();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_native_window() -> i32 {
    scope_contract();
    present_contract();
    0
}
