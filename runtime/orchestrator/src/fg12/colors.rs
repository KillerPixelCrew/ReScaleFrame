// SPDX-License-Identifier: MIT
use super::{
    FencePoint, FenceState, IUnknown, PRESENT_STATE, Present, SHADER_READ, log, texture_desc,
};
use crate::com_ref;
use std::ffi::c_void;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use windows::core::Interface;

const RGBA8: u32 = 28;
const BGRA8: u32 = 87;

struct Readback {
    buffer: IUnknown,
    layout: crate::frame_capture12::Layout,
}

// Readbacks are D3D12 resources. The file worker owns them and maps only after
// the provider queue's fence proves that recording and submission completed.
unsafe impl Send for Readback {}

impl Readback {
    unsafe fn allocate(source: *mut c_void) -> Option<Self> {
        let mut layout = std::mem::MaybeUninit::uninit();
        let raw = unsafe { native!(readback(source, layout.as_mut_ptr())) };
        Some(Self {
            buffer: unsafe { com_ref::owned(raw) }?,
            layout: unsafe { layout.assume_init() },
        })
    }

    unsafe fn record(&self, list: *mut c_void, source: *mut c_void, state: u32) {
        unsafe {
            native!(readback_copy(
                list,
                source,
                state,
                self.buffer.as_raw(),
                &self.layout
            ))
        };
    }

    fn write(&self, path: &Path) -> io::Result<()> {
        let layout = self.layout;
        let mapped = unsafe { native!(map(self.buffer.as_raw(), layout.bytes)) };
        if mapped.is_null() {
            return Err(io::Error::other("FG readback mapping failed"));
        }
        struct Mapping<'a>(&'a IUnknown);
        impl Drop for Mapping<'_> {
            fn drop(&mut self) {
                unsafe { native!(unmap(self.0.as_raw())) };
            }
        }
        let _mapping = Mapping(&self.buffer);
        let length = usize::try_from(layout.bytes)
            .ok()
            .filter(|&bytes| bytes <= isize::MAX as usize)
            .ok_or_else(|| io::Error::other("FG readback length out of range"))?;
        let bytes = unsafe { std::slice::from_raw_parts(mapped, length) };
        let mut file = BufWriter::new(File::create(path)?);
        write!(file, "P6\n{} {}\n255\n", layout.width, layout.height)?;
        let mut rgb = vec![0; layout.width as usize * 3];
        let pitch = layout.row_pitch as usize;
        for row in 0..layout.height as usize {
            let start = layout.offset as usize + row * pitch;
            let end = start + layout.width as usize * 4;
            let pixels = bytes
                .get(start..end)
                .ok_or_else(|| io::Error::other("FG readback row out of range"))?;
            for (output, pixel) in rgb
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .zip(pixels.as_chunks::<4>().0)
            {
                *output = if layout.format == BGRA8 {
                    [pixel[2], pixel[1], pixel[0]]
                } else {
                    [pixel[0], pixel[1], pixel[2]]
                };
            }
            file.write_all(&rgb)?;
        }
        file.flush()
    }
}

struct Pair {
    images: [Readback; 2],
    directory: PathBuf,
    present: Present,
    completion: Option<FencePoint>,
}

impl Pair {
    fn save(self) {
        let generation = self.present.generation;
        let mut saved = true;
        for (image, suffix) in self.images.iter().zip(["final", "hudless"]) {
            let path = self
                .directory
                .join(format!("fg-input-{generation}-{suffix}.ppm"));
            saved &= image.write(&path).is_ok();
        }
        if saved {
            log("FG completed-colour and HUD-less input pair saved.".into());
        } else {
            log("FG completed-colour and HUD-less input pair could not be saved.".into());
        }
    }
}

pub(super) struct Colors {
    pub directory: Option<PathBuf>,
    captured_generation: u64,
    pending: Option<Pair>,
}

impl Colors {
    pub const fn new() -> Self {
        Self {
            directory: None,
            captured_generation: 0,
            pending: None,
        }
    }

    pub fn poll(&mut self, generation: u64) {
        let Some(pair) = &self.pending else { return };
        match pair.completion.as_ref().map(FencePoint::state) {
            Some(FenceState::Complete) => {
                if let Some(pair) = self.pending.take() {
                    let _ = std::thread::Builder::new().spawn(move || pair.save());
                }
            }
            None | Some(FenceState::DeviceRemoved) if pair.present.generation != generation => {
                // The old generation drained, but execution of this exact list is unknown.
                self.pending = None;
            }
            _ => {}
        }
    }

    pub unsafe fn record(
        &mut self,
        present: Present,
        list: *mut c_void,
        buffer: *mut c_void,
        hudless: *mut c_void,
    ) {
        let Some(directory) = &self.directory else {
            return;
        };
        let final_desc = unsafe { texture_desc(buffer) };
        let hudless_desc = unsafe { texture_desc(hudless) };
        if !matches!(final_desc.format, RGBA8 | BGRA8)
            || final_desc.format != hudless_desc.format
            || final_desc.width != hudless_desc.width
            || final_desc.height != hudless_desc.height
        {
            return;
        }
        let (Some(final_image), Some(hudless_image)) =
            (unsafe { Readback::allocate(buffer) }, unsafe {
                Readback::allocate(hudless)
            })
        else {
            return;
        };
        let pair = Pair {
            images: [final_image, hudless_image],
            directory: directory.clone(),
            present,
            completion: None,
        };
        unsafe {
            pair.images[0].record(list, buffer, PRESENT_STATE);
            pair.images[1].record(list, hudless, SHADER_READ);
        }
        self.captured_generation = present.generation;
        self.pending = Some(pair);
    }

    pub fn wants(&self, present: Present, samples: u64) -> bool {
        self.directory.is_some()
            && self.pending.is_none()
            && self.captured_generation != present.generation
            && samples >= 30
    }

    pub unsafe fn submitted(&mut self, present: Present, queue: *mut c_void) {
        let Some(pair) = self.pending.as_mut() else {
            return;
        };
        if pair.present.view != present.view
            || pair.present.generation != present.generation
            || pair.completion.is_some()
        {
            return;
        }
        let Some(fence) = (unsafe { com_ref::owned(native!(fence(queue))) }) else {
            return;
        };
        if unsafe { native!(signal(queue, fence.as_raw(), 1)) } != 0 {
            pair.completion = Some(FencePoint { fence, value: 1 });
        }
    }
}
