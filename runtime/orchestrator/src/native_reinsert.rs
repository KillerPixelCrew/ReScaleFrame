// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::{FullscreenBlit, GraphicsCache, SavedBindings, texture_desc};
use crate::d3d11_output_abi::*;
use std::ffi::c_void;
use std::ptr::NonNull;

struct Rect {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl Rect {
    fn new([left, top, right, bottom]: [i32; 4]) -> Option<Self> {
        if left < 0 || top < 0 || right <= left || bottom <= top {
            return None;
        }
        Some(Self {
            left: left as u32,
            top: top as u32,
            right: right as u32,
            bottom: bottom as u32,
        })
    }

    fn width(&self) -> u32 {
        self.right - self.left
    }

    fn height(&self) -> u32 {
        self.bottom - self.top
    }
}

fn float_colour(format: u32) -> bool {
    matches!(
        format,
        RSF_D3D11_FORMAT_RGBA32_FLOAT
            | RSF_D3D11_FORMAT_RGBA16_FLOAT
            | RSF_D3D11_FORMAT_R11G11B10_FLOAT
    )
}

#[derive(Clone, Copy)]
struct Output {
    texture: NonNull<c_void>,
    convert_to: Option<u32>,
}

impl Output {
    unsafe fn preflight(
        texture: NonNull<c_void>,
        device: &ComRef,
        source: &rsf_d3d11_texture_desc,
        rect: &Rect,
    ) -> Option<Self> {
        let ptr = texture.as_ptr();
        let owner = unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(ptr)) }?;
        let dest = unsafe { texture_desc(ptr) };
        if owner.as_ptr() != device.as_ptr()
            || dest.layers != 1
            || dest.samples != 1
            || dest.width < rect.right
            || dest.height < rect.bottom
            || dest.usage == RSF_D3D11_USAGE_IMMUTABLE
        {
            return None;
        }
        let mut convert_to = None;
        if dest.format != source.format {
            let format = unsafe { rsf_d3d11_output_srv_format(dest.format) };
            if !float_colour(source.format)
                || !float_colour(format)
                || source.bind_flags & RSF_D3D11_BIND_SHADER_RESOURCE == 0
            {
                return None;
            }
            convert_to = Some(format);
        }
        Some(Self {
            texture,
            convert_to,
        })
    }
}

struct Conversion {
    target: ComRef,
    texture: ComRef,
    format: u32,
    width: u32,
    height: u32,
}

struct SourceView {
    view: ComRef,
    source: ComRef,
}

struct Conversions {
    blit: FullscreenBlit,
    slots: [Option<Conversion>; 2],
    source: Option<SourceView>,
    device: ComRef,
}

// SAFETY: this cache owns D3D11 device children and the fullscreen pass.
// Calls and teardown are serialized by the graphics owner.
unsafe impl Send for Conversions {}

impl Conversions {
    unsafe fn source_view(&mut self, texture: NonNull<c_void>, format: u32) -> Option<*mut c_void> {
        if let Some(source) = self
            .source
            .as_ref()
            .filter(|source| source.source.as_ptr() == texture.as_ptr())
        {
            return Some(source.view.as_ptr());
        }
        let view = unsafe {
            ComRef::from_owned(rsf_d3d11_output_create_srv(
                self.device.as_ptr(),
                texture.as_ptr(),
                format,
            ))
        }?;
        let source = unsafe { ComRef::retain(texture.as_ptr()) };
        Some(
            self.source
                .insert(SourceView { view, source })
                .view
                .as_ptr(),
        )
    }

    unsafe fn prepare(&mut self, formats: [Option<u32>; 2], rect: &Rect) -> Option<()> {
        let width = rect.width();
        let height = rect.height();
        for (slot, format) in self.slots.iter_mut().zip(formats) {
            let Some(format) = format else { continue };
            if slot.as_ref().is_some_and(|entry| {
                entry.format == format && entry.width == width && entry.height == height
            }) {
                continue;
            }
            let desc = rsf_d3d11_texture_desc {
                width,
                height,
                format,
                mips: 1,
                layers: 1,
                samples: 1,
                usage: RSF_D3D11_USAGE_DEFAULT,
                bind_flags: RSF_D3D11_BIND_RENDER_TARGET,
                ..Default::default()
            };
            let device = self.device.as_ptr();
            let texture =
                unsafe { ComRef::from_owned(rsf_d3d11_output_create_texture(device, &desc)) }?;
            let target = unsafe {
                ComRef::from_owned(rsf_d3d11_output_create_rtv(device, texture.as_ptr()))
            }?;
            *slot = Some(Conversion {
                texture,
                target,
                format,
                width,
                height,
            });
        }
        Some(())
    }

    unsafe fn render(
        &mut self,
        context: *mut c_void,
        source: NonNull<c_void>,
        source_format: u32,
        formats: [Option<u32>; 2],
        rect: &Rect,
    ) -> Option<[*mut c_void; 2]> {
        let view = unsafe { self.source_view(source, source_format) }?;
        unsafe { self.prepare(formats, rect) }?;
        let mut copies = [source.as_ptr(); 2];
        for ((format, slot), copy) in formats.into_iter().zip(&self.slots).zip(&mut copies) {
            if format.is_none() {
                continue;
            }
            let output = slot.as_ref()?;
            if !unsafe {
                self.blit.draw(
                    context,
                    output.target.as_ptr(),
                    view,
                    rect.width(),
                    rect.height(),
                )
            } {
                return None;
            }
            *copy = output.texture.as_ptr();
        }
        Some(copies)
    }
}

static REINSERT: GraphicsCache<Option<Conversions>> = GraphicsCache::new(None);

unsafe fn reinsert(
    cache: &mut Option<Conversions>,
    context: *mut c_void,
    source: NonNull<c_void>,
    destination: NonNull<c_void>,
    readable: Option<NonNull<c_void>>,
    rect: Rect,
    kernels: &rsf_d3d11_reinsert_kernels,
) -> Option<()> {
    if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
        return None;
    }
    let src = unsafe { texture_desc(source.as_ptr()) };
    if src.width != rect.width()
        || src.height != rect.height()
        || src.layers != 1
        || src.samples != 1
    {
        return None;
    }
    let device = unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(context)) }?;
    let source_device =
        unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(source.as_ptr())) }?;
    if source_device.as_ptr() != device.as_ptr() {
        return None;
    }
    let target = unsafe { Output::preflight(destination, &device, &src, &rect) }?;
    let mirror = match readable.filter(|readable| *readable != destination) {
        Some(texture) => Some(unsafe { Output::preflight(texture, &device, &src, &rect) }?),
        None => None,
    };
    let formats = [
        target.convert_to,
        mirror.and_then(|output| output.convert_to),
    ];
    let _bindings = unsafe { SavedBindings::new(context, kernels.save?, kernels.restore?) }?;
    // Remove conflicting UAVs, native shader stages and predication. The
    // shared state scope restores the bindings and preserves UAV counters.
    unsafe { rsf_d3d11_output_clear_state(context) };
    let copies = if formats.iter().any(Option::is_some) {
        let conversions = if let Some(conversions) = cache
            .as_mut()
            .filter(|entry| entry.device.as_ptr() == device.as_ptr())
        {
            conversions
        } else {
            *cache = None;
            let blit = unsafe {
                FullscreenBlit::new(
                    &device,
                    kernels.create_blit?,
                    kernels.draw_rgba?,
                    kernels.destroy_blit?,
                )
            }?;
            cache.insert(Conversions {
                blit,
                slots: [None, None],
                source: None,
                device: device.clone(),
            })
        };
        // All conversions must succeed before either native output is written.
        unsafe { conversions.render(context, source, src.format, formats, &rect) }?
    } else {
        [source.as_ptr(); 2]
    };
    unsafe { rsf_d3d11_output_unbind_targets(context) };
    for (output, copy) in [Some(target), mirror].into_iter().zip(copies) {
        let Some(output) = output else { continue };
        let dest = output.texture.as_ptr();
        if dest != copy {
            unsafe {
                rsf_d3d11_output_copy_region(
                    context,
                    dest,
                    copy,
                    rect.left,
                    rect.top,
                    rect.width(),
                    rect.height(),
                )
            };
        }
    }
    (unsafe { rsf_d3d11_output_device_ok(device.as_ptr()) } != 0).then_some(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_reinsert(
    context: *mut c_void,
    source: *mut c_void,
    destination: *mut c_void,
    readable: *mut c_void,
    rect: *const [i32; 4],
    kernels: *const rsf_d3d11_reinsert_kernels,
) -> u32 {
    if context.is_null() || rect.is_null() || kernels.is_null() {
        return 0;
    }
    let (Some(source), Some(destination), Some(rect)) = (
        NonNull::new(source),
        NonNull::new(destination),
        Rect::new(unsafe { rect.read() }),
    ) else {
        return 0;
    };
    REINSERT
        .with(|cache| unsafe {
            reinsert(
                cache,
                context,
                source,
                destination,
                NonNull::new(readable),
                rect,
                &*kernels,
            )
        })
        .flatten()
        .is_some()
        .into()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_reinsert_release() {
    REINSERT.with(|cache| *cache = None);
}
