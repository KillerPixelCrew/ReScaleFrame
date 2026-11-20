// SPDX-License-Identifier: MIT
use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::ptr::{null_mut, without_provenance_mut};
use std::slice;

pub(super) const DEVICE: usize = 1;
pub(super) const SOURCE: usize = 2;
pub(super) const CONTEXT: usize = 3;

pub(super) fn ptr(name: usize) -> Ptr {
    without_provenance_mut(name)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Call {
    Support(u32),
    Texture(Texture, u32, Option<u32>),
    View(u32, Option<u32>),
    Shader(String, u32, u32),
    Buffer,
    Raster,
    Blend,
    Depth(u32),
    Save,
    Restore,
    Update([u32; 4]),
    Constant(u32),
    Targets(Vec<usize>, usize),
    Resources(u32, Vec<usize>),
    Stencil(usize),
    Draw(Extent),
}

pub(super) struct Fake {
    pub calls: Vec<Call>,
    pub owned: BTreeMap<usize, usize>,
    pub textures: BTreeMap<usize, Texture>,
    pub view_format: u32,
    pub view_flags: u32,
    pub wrong_view: Option<u32>,
    pub support: u32,
    pub fail_view: Option<u32>,
    pub fail_shader: Option<(&'static str, i32)>,
    pub save: bool,
    next: usize,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            calls: vec![],
            owned: BTreeMap::new(),
            textures: BTreeMap::new(),
            view_format: 28,
            view_flags: 0,
            wrong_view: None,
            support: u32::MAX,
            fail_view: None,
            fail_shader: None,
            save: true,
            next: 10,
        }
    }
}

impl Fake {
    fn retain(&mut self, name: usize) -> Ptr {
        *self.owned.entry(name).or_default() += 1;
        ptr(name)
    }

    fn allocate(&mut self) -> Ptr {
        self.next += 1;
        self.retain(self.next)
    }

    fn release(&mut self, pointer: Ptr) {
        let name = pointer.addr();
        let count = self.owned.get_mut(&name).expect("unowned COM reference");
        *count -= 1;
        if *count == 0 {
            self.owned.remove(&name);
        }
    }
}

thread_local! {
    static FAKE: RefCell<Fake> = RefCell::new(Fake::default());
}

pub(super) fn fake<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
    FAKE.with_borrow_mut(f)
}

unsafe extern "C" fn retain(pointer: Ptr) {
    fake(|state| state.retain(pointer.addr()));
}

unsafe extern "C" fn release(pointer: Ptr) {
    fake(|state| state.release(pointer));
}

unsafe extern "C" fn as_texture(resource: Ptr) -> Ptr {
    fake(|state| {
        if state.textures.contains_key(&resource.addr()) {
            state.retain(resource.addr())
        } else {
            null_mut()
        }
    })
}

unsafe extern "C" fn view_resource(_: Ptr) -> Ptr {
    fake(|state| state.retain(SOURCE))
}

unsafe extern "C" fn texture_device(_: Ptr) -> Ptr {
    fake(|state| state.retain(DEVICE))
}

unsafe extern "C" fn describe_texture(texture: Ptr, out: &mut Texture) {
    *out = fake(|state| state.textures[&texture.addr()]);
}

unsafe extern "C" fn describe_view(_: Ptr, kind: u32, out: &mut View) {
    fake(|state| {
        out.texture2d = u32::from(state.wrong_view != Some(kind));
        out.format = state.view_format;
        out.flags = state.view_flags;
        out.descriptor[0] = 100 + kind;
    });
}

unsafe extern "C" fn format_support(_: Ptr, format: u32) -> u32 {
    fake(|state| {
        state.calls.push(Call::Support(format));
        state.support
    })
}

unsafe extern "C" fn texture(_: Ptr, desc: &Texture, data: *const c_void, pitch: u32) -> Ptr {
    let first_word = (!data.is_null()).then(|| unsafe { data.cast::<u32>().read_unaligned() });
    fake(|state| {
        let reference = state.allocate();
        state.textures.insert(reference.addr(), *desc);
        state.calls.push(Call::Texture(*desc, pitch, first_word));
        reference
    })
}

unsafe extern "C" fn view(_: Ptr, _: Ptr, kind: u32, template: Option<&View>) -> Ptr {
    fake(|state| {
        state
            .calls
            .push(Call::View(kind, template.map(|view| view.descriptor[0])));
        if state.fail_view == Some(kind) {
            null_mut()
        } else {
            state.allocate()
        }
    })
}

unsafe extern "C" fn shader(
    _: Ptr,
    kind: u32,
    vertex: u32,
    entry: *const c_char,
    _: Log,
    _: Ptr,
    out: &mut Ptr,
) -> i32 {
    let entry = unsafe { CStr::from_ptr(entry) }.to_str().unwrap();
    fake(|state| {
        state
            .calls
            .push(Call::Shader(entry.to_owned(), kind, vertex));
        if let Some((failure, code)) = state.fail_shader
            && entry == failure
        {
            return code;
        }
        *out = state.allocate();
        1
    })
}

fn create(call: Call) -> Ptr {
    fake(|state| {
        state.calls.push(call);
        state.allocate()
    })
}

unsafe extern "C" fn constant_buffer(_: Ptr) -> Ptr {
    create(Call::Buffer)
}
unsafe extern "C" fn raster_state(_: Ptr) -> Ptr {
    create(Call::Raster)
}
unsafe extern "C" fn blend_state(_: Ptr) -> Ptr {
    create(Call::Blend)
}
unsafe extern "C" fn depth_state(_: Ptr, write: u32) -> Ptr {
    create(Call::Depth(write))
}

unsafe extern "C" fn save_state(_: Ptr) -> Ptr {
    fake(|state| {
        state.calls.push(Call::Save);
        if state.save { ptr(4) } else { null_mut() }
    })
}

unsafe extern "C" fn restore_state(_: Ptr, snapshot: Ptr) {
    assert_eq!(snapshot, ptr(4));
    fake(|state| state.calls.push(Call::Restore));
}

unsafe extern "C" fn update_constants(_: Ptr, _: Ptr, words: *const u32) {
    let words = unsafe { words.cast::<[u32; 4]>().read() };
    fake(|state| state.calls.push(Call::Update(words)));
}

unsafe extern "C" fn ps_constant(_: Ptr, slot: u32, _: Ptr) {
    fake(|state| state.calls.push(Call::Constant(slot)));
}

unsafe extern "C" fn targets(_: Ptr, count: u32, views: *const Ptr, depth: Ptr) {
    let views = unsafe { slice::from_raw_parts(views, count as usize) };
    fake(|state| {
        state.calls.push(Call::Targets(
            views.iter().map(|view| view.addr()).collect(),
            depth.addr(),
        ))
    });
}

unsafe extern "C" fn ps_resources(_: Ptr, slot: u32, count: u32, inputs: *const Ptr) {
    let inputs = unsafe { slice::from_raw_parts(inputs, count as usize) };
    fake(|state| {
        state.calls.push(Call::Resources(
            slot,
            inputs.iter().map(|view| view.addr()).collect(),
        ))
    });
}

unsafe extern "C" fn clear_stencil(_: Ptr, depth: Ptr) {
    fake(|state| state.calls.push(Call::Stencil(depth.addr())));
}

unsafe extern "C" fn draw(_: Ptr, draw: &Draw) {
    fake(|state| state.calls.push(Call::Draw(draw.size)));
}

pub(super) static PLATFORM: Platform = Platform {
    retain,
    release,
    as_texture,
    view_resource,
    texture_device,
    describe_texture,
    describe_view,
    format_support,
    texture,
    view,
    shader,
    constant_buffer,
    raster_state,
    blend_state,
    depth_state,
    save_state,
    restore_state,
    update_constants,
    ps_constant,
    targets,
    ps_resources,
    clear_stencil,
    draw,
};

pub(super) fn extent(width: u32, height: u32) -> Extent {
    Extent { width, height }
}

pub(super) fn calls() -> Vec<Call> {
    fake(|state| std::mem::take(&mut state.calls))
}
