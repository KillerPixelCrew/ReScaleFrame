// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::input_dump::{Log, MAX_IMAGE_BYTES, Pixels, TextureOptions};
use crate::sr_session::checked;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::{offset_of, size_of};
use std::ptr::NonNull;

mod sample;
use sample::Format;

type Handle = *mut c_void;
type DumpResult<T> = Result<T, i32>;

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const UNSUPPORTED: i32 = -3;
const STAGING_FAILED: i32 = -4;
const MAP_FAILED: i32 = -5;
const WRITE_FAILED: i32 = -6;
const FOREIGN_DEVICE: i32 = -7;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Targa,
    Png,
    Raw,
}

#[repr(C)]
#[derive(Default)]
struct Description {
    width: u32,
    height: u32,
    format: u32,
    mip_levels: u32,
    array_size: u32,
    samples: u32,
    immediate: u32,
    context_on_device: u32,
    texture_on_device: u32,
}

#[repr(C)]
#[derive(Default)]
struct MappedData {
    data: *const u8,
    row_pitch: u32,
}

#[repr(C)]
struct Platform {
    struct_size: u32,
    describe: unsafe extern "C" fn(Handle, Handle, Handle, &mut Description) -> u32,
    stage: unsafe extern "C" fn(Handle, &Description) -> Handle,
    copy: unsafe extern "C" fn(Handle, Handle, Handle, u32),
    map: unsafe extern "C" fn(Handle, Handle, &mut MappedData) -> u32,
    unmap: unsafe extern "C" fn(Handle, Handle),
    open_file: unsafe extern "C" fn(*const c_char) -> Handle,
    write_file: unsafe extern "C" fn(Handle, *const u8, usize) -> u32,
    close_file: unsafe extern "C" fn(Handle) -> u32,
    write_png: unsafe extern "C" fn(*const c_char, u32, u32, u32, u32, *mut u8) -> u32,
}

#[repr(C)]
struct Report {
    struct_size: u32,
    width: u32,
    height: u32,
    format: u32,
    bytes_written: u32,
    fraction_unwritten: f32,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

const _: () = {
    assert!(size_of::<Description>() == 36);
    assert!(size_of::<MappedData>() == 16);
    assert!(offset_of!(MappedData, row_pitch) == 8);
    assert!(size_of::<Platform>() == 80);
    assert!(offset_of!(Platform, describe) == 8);
    assert!(offset_of!(Platform, write_png) == 72);
    assert!(size_of::<Report>() == 40);
    assert!(size_of::<Pixels>() == 40);
    assert!(size_of::<TextureOptions>() == 40);
};

fn source_bytes(row_pitch: u32, height: u32, row_bytes: u64) -> Option<u64> {
    (u64::from(row_pitch) * u64::from(height.checked_sub(1)?)).checked_add(row_bytes)
}

pub(super) struct PackedRows<'a> {
    pixels: &'a Pixels,
    pitch: u32,
}

impl<'a> PackedRows<'a> {
    /// The caller lends initialized, readable payload bytes for each row, with
    /// the lifetime and byte capacity described by the pixel descriptor.
    pub(super) unsafe fn new(pixels: &'a Pixels, pixel_bytes: u32) -> Option<Self> {
        let pitch = pixels.width.checked_mul(pixel_bytes)?;
        if pixels.data.is_null() || pitch == 0 || pixels.row_pitch < pitch {
            return None;
        }
        let bytes = source_bytes(pixels.row_pitch, pixels.height, u64::from(pitch))?;
        (bytes <= pixels.data_size && bytes <= isize::MAX as u64).then_some(Self { pixels, pitch })
    }

    pub(super) fn pitch(&self) -> u32 {
        self.pitch
    }

    pub(super) fn packed_size(&self) -> u64 {
        u64::from(self.pitch) * u64::from(self.pixels.height)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &'a [u8]> {
        let pixels = self.pixels;
        let pitch = self.pitch as usize;
        (0..pixels.height).map(move |y| {
            let offset = y as usize * pixels.row_pitch as usize;
            // Mapped row padding can be uninitialized. Only borrow the payload.
            unsafe { std::slice::from_raw_parts(pixels.data.cast::<u8>().add(offset), pitch) }
        })
    }
}

pub(super) fn pixel_bytes(format: u32) -> u32 {
    Format::from_dxgi(format).map_or(0, Format::bytes)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_texture_pixel_bytes(format: u32) -> u32 {
    pixel_bytes(format)
}

struct Request<'a> {
    prefix: &'a CStr,
    view: u32,
    scale: f32,
    log: Log,
}

impl<'a> Request<'a> {
    unsafe fn new(options: &'a TextureOptions) -> DumpResult<Self> {
        if options.abi_version != TextureOptions::ABI_VERSION {
            return Err(ABI_MISMATCH);
        }
        Ok(Self {
            prefix: unsafe { CStr::from_ptr(options.prefix) },
            view: options.view,
            scale: if options.scale == 0.0 {
                1.0
            } else {
                options.scale
            },
            log: Log::new(options.log, options.log_user),
        })
    }

    fn path(&self, suffix: &[u8]) -> CString {
        let mut path = self.prefix.to_bytes().to_vec();
        path.extend_from_slice(suffix);
        CString::new(path).expect("C string prefix and extension")
    }
}

struct Mapping<'a> {
    platform: &'a Platform,
    context: Handle,
    texture: ComRef,
    pixels: Pixels,
}

impl Drop for Mapping<'_> {
    fn drop(&mut self) {
        unsafe { (self.platform.unmap)(self.context, self.texture.as_ptr()) };
    }
}

struct Readback<'a> {
    platform: &'a Platform,
    device: Handle,
    context: Handle,
    source: Handle,
}

impl<'a> Readback<'a> {
    fn describe(&self) -> DumpResult<Description> {
        let mut desc = Description::default();
        if unsafe { (self.platform.describe)(self.device, self.context, self.source, &mut desc) }
            == 0
            || desc.immediate == 0
        {
            return Err(INVALID);
        }
        Ok(desc)
    }

    fn map(&self, desc: &Description, raw: bool, log: Log) -> DumpResult<Mapping<'a>> {
        if !raw {
            log.say("dump: creating staging copy");
        }
        let texture = unsafe { ComRef::from_owned((self.platform.stage)(self.device, desc)) }
            .ok_or(STAGING_FAILED)?;
        let top = raw || desc.mip_levels > 1 || desc.array_size > 1;
        if raw {
            log.say("raw dump: copying subresource zero and mapping");
        } else if top {
            log.say("dump: copying (top subresource)");
        } else {
            log.say("dump: copying (whole resource)");
        }
        unsafe {
            (self.platform.copy)(self.context, texture.as_ptr(), self.source, u32::from(top))
        };
        if !raw {
            log.say("dump: mapping");
        }
        let mut mapped = MappedData::default();
        if unsafe { (self.platform.map)(self.context, texture.as_ptr(), &mut mapped) } == 0 {
            return Err(MAP_FAILED);
        }
        Ok(Mapping {
            platform: self.platform,
            context: self.context,
            texture,
            pixels: Pixels {
                struct_size: size_of::<Pixels>() as u32,
                width: desc.width,
                height: desc.height,
                format: desc.format,
                row_pitch: mapped.row_pitch,
                // A successful map provides all rows of the staging subresource.
                data_size: u64::from(mapped.row_pitch) * u64::from(desc.height),
                data: mapped.data.cast(),
            },
        })
    }
}

struct File<'a> {
    platform: &'a Platform,
    handle: Option<NonNull<c_void>>,
}

impl<'a> File<'a> {
    fn open(platform: &'a Platform, path: &CStr) -> DumpResult<Self> {
        let handle =
            NonNull::new(unsafe { (platform.open_file)(path.as_ptr()) }).ok_or(WRITE_FAILED)?;
        Ok(Self {
            platform,
            handle: Some(handle),
        })
    }

    fn write(&self, bytes: &[u8]) -> DumpResult<()> {
        let handle = self.handle.expect("open file");
        (unsafe { (self.platform.write_file)(handle.as_ptr(), bytes.as_ptr(), bytes.len()) } != 0)
            .then_some(())
            .ok_or(WRITE_FAILED)
    }

    fn close(&mut self) -> bool {
        self.handle
            .take()
            .is_none_or(|handle| (unsafe { (self.platform.close_file)(handle.as_ptr()) }) != 0)
    }

    fn finish(mut self) -> DumpResult<()> {
        self.close().then_some(()).ok_or(WRITE_FAILED)
    }
}

impl Drop for File<'_> {
    fn drop(&mut self) {
        self.close();
    }
}

fn write_file(platform: &Platform, path: &CStr, bytes: &[u8]) -> DumpResult<()> {
    let file = File::open(platform, path)?;
    file.write(bytes)?;
    file.finish()
}

fn write_raw(mapping: Mapping<'_>, format: Format, request: &Request<'_>) -> DumpResult<()> {
    let platform = mapping.platform;
    let pixels = &mapping.pixels;
    let rows = unsafe { PackedRows::new(pixels, format.bytes()) }.ok_or(MAP_FAILED)?;
    let binary = File::open(platform, &request.path(b".bin"))?;
    for row in rows.iter() {
        binary.write(row)?;
    }
    binary.finish()?;
    let metadata = format!(
        "{{\"width\":{},\"height\":{},\"format\":{},\"row_pitch\":{},\"source_row_pitch\":{},\"subresource\":0,\"bytes\":{}}}\n",
        pixels.width,
        pixels.height,
        pixels.format,
        rows.pitch(),
        pixels.row_pitch,
        rows.packed_size()
    );
    drop(mapping);
    write_file(platform, &request.path(b"_raw.json"), metadata.as_bytes())
}

fn image_bytes(width: u32, height: u32) -> DumpResult<usize> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_IMAGE_BYTES / 4 {
        return Err(INVALID);
    }
    Ok((pixels * 4) as usize)
}

struct Preview {
    bgra: Vec<u8>,
    report: Report,
}

impl Preview {
    fn encode(rows: PackedRows<'_>, format: Format, request: &Request<'_>) -> DumpResult<Self> {
        let pixels = rows.pixels;
        let bytes = image_bytes(pixels.width, pixels.height)?;
        let mut bgra = Vec::new();
        bgra.try_reserve_exact(bytes).map_err(|_| WRITE_FAILED)?;
        let mut x = sample::Range::default();
        let mut y = sample::Range::default();
        let mut unwritten = 0u32;

        for row in rows.iter() {
            for pixel in row.chunks_exact(format.bytes() as usize) {
                let sample = format.decode(pixel);
                match sample.channels(request.view) {
                    Some((red, green)) => {
                        x.include(red);
                        y.include(green);
                        bgra.extend_from_slice(&sample.bgra(
                            request.view,
                            request.scale,
                            red,
                            green,
                        ));
                    }
                    None => {
                        unwritten += 1;
                        bgra.extend_from_slice(&[220, 0, 0, 255]);
                    }
                }
            }
        }

        let [min_x, max_x] = x.finish();
        let [min_y, max_y] = y.finish();
        Ok(Self {
            bgra,
            report: Report {
                struct_size: size_of::<Report>() as u32,
                width: pixels.width,
                height: pixels.height,
                format: pixels.format,
                bytes_written: bytes as u32,
                fraction_unwritten: unwritten as f32 / (bytes / 4) as f32,
                min_x,
                max_x,
                min_y,
                max_y,
            },
        })
    }

    fn write(
        mut self,
        platform: &Platform,
        request: &Request<'_>,
        output: Output,
        report: *mut Report,
    ) -> DumpResult<()> {
        let stats = &self.report;
        let (width, height) = (stats.width, stats.height);
        let png = output == Output::Png;
        let path = request.path(if png { b".png" } else { b".tga" });
        request
            .log
            .say(&format!("dump: writing {}", path.to_string_lossy()));
        if png {
            let ok = unsafe {
                (platform.write_png)(
                    path.as_ptr(),
                    width,
                    height,
                    width * 4,
                    stats.bytes_written,
                    self.bgra.as_mut_ptr(),
                )
            };
            if ok == 0 {
                return Err(WRITE_FAILED);
            }
        } else {
            let width = u16::try_from(width).map_err(|_| INVALID)?;
            let height = u16::try_from(height).map_err(|_| INVALID)?;
            let mut header = [0; 18];
            header[2] = 2; // uncompressed true colour
            header[12..14].copy_from_slice(&width.to_le_bytes());
            header[14..16].copy_from_slice(&height.to_le_bytes());
            header[16] = 32;
            header[17] = 0x28; // top-left origin, 8 attribute bits
            let file = File::open(platform, &path)?;
            file.write(&header)?;
            file.write(&self.bgra)?;
            file.finish()?;
        }
        let fraction = stats.fraction_unwritten;
        let metadata = format!(
            "{{\n  \"width\": {width},\n  \"height\": {height},\n  \"format\": {},\n  \"view\": {},\n  \"fraction_unwritten\": {fraction:.4},\n  \"x_range\": [{:.4}, {:.4}],\n  \"y_range\": [{:.4}, {:.4}]\n}}\n",
            stats.format, request.view, stats.min_x, stats.max_x, stats.min_y, stats.max_y
        );
        // Preview metadata has always been best effort after the image succeeds.
        let _ = write_file(platform, &request.path(b".json"), metadata.as_bytes());
        if let Some(size) = unsafe { report.cast::<u32>().as_ref() }.copied()
            && size as usize >= size_of::<Report>()
        {
            self.report.struct_size = size;
            unsafe { report.write(self.report) };
        }
        request.log.say(&format!(
            "dump: done, {:.1}% unwritten",
            f64::from(fraction) * 100.0
        ));
        Ok(())
    }
}

fn dump_texture(
    readback: Readback<'_>,
    request: &Request<'_>,
    output: Output,
    report: *mut Report,
) -> DumpResult<()> {
    let desc = readback.describe()?;
    let raw = output == Output::Raw;
    if !raw {
        if desc.samples != 1 {
            return Err(INVALID);
        }
        image_bytes(desc.width, desc.height)?;
    }
    if desc.context_on_device == 0 || raw && desc.texture_on_device == 0 {
        return Err(FOREIGN_DEVICE);
    }
    if !raw {
        request.log.say(&format!(
            "dump {}: {}x{} format {} mips {} slices {} samples {}",
            request.prefix.to_string_lossy(),
            desc.width,
            desc.height,
            desc.format,
            desc.mip_levels,
            desc.array_size,
            desc.samples
        ));
    }
    let format = Format::from_dxgi(desc.format).ok_or(UNSUPPORTED)?;
    if raw {
        let row = u64::from(desc.width) * u64::from(format.bytes());
        if desc.samples != 1
            || row == 0
            || desc.height == 0
            || row > MAX_IMAGE_BYTES / u64::from(desc.height)
        {
            return Err(UNSUPPORTED);
        }
    }
    if desc.texture_on_device == 0 {
        request
            .log
            .say("dump: refused, texture belongs to another device");
        return Err(FOREIGN_DEVICE);
    }
    let mapping = readback.map(&desc, raw, request.log)?;
    if raw {
        return write_raw(mapping, format, request);
    }
    let rows = unsafe { PackedRows::new(&mapping.pixels, format.bytes()) }.ok_or(MAP_FAILED)?;
    request
        .log
        .say(&format!("dump: decoding {} rows", desc.height));
    let preview = Preview::encode(rows, format, request)?;
    drop(mapping);
    preview.write(readback.platform, request, output, report)
}

unsafe fn sized_options<'a>(options: *const TextureOptions) -> DumpResult<&'a TextureOptions> {
    unsafe { checked(options) }
        .filter(|options| !options.prefix.is_null())
        .ok_or(INVALID)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_texture_dump_texture(
    platform: *const Platform,
    device: Handle,
    context: Handle,
    source: Handle,
    options: *const TextureOptions,
    report: *mut Report,
    output: u32,
) -> i32 {
    let output = match output {
        0 => Output::Targa,
        1 => Output::Png,
        2 => Output::Raw,
        _ => return INVALID,
    };
    let Some(platform) = (unsafe { checked(platform) }) else {
        return INVALID;
    };
    if device.is_null() || context.is_null() || source.is_null() {
        return INVALID;
    }
    let readback = Readback {
        platform,
        device,
        context,
        source,
    };
    unsafe { sized_options(options) }
        .and_then(|options| unsafe { Request::new(options) })
        .and_then(|request| dump_texture(readback, &request, output, report))
        .err()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_texture_dump_pixels(
    platform: *const Platform,
    pixels: *const Pixels,
    options: *const TextureOptions,
    report: *mut Report,
) -> i32 {
    let (Some(platform), Some(pixels), Ok(options)) = (
        unsafe { checked(platform) },
        unsafe { checked(pixels) },
        unsafe { sized_options(options) },
    ) else {
        return INVALID;
    };
    let format = Format::from_dxgi(pixels.format);
    let stride = format.map_or(0, Format::bytes);
    let bytes = u64::from(pixels.width) * u64::from(stride);
    if source_bytes(pixels.row_pitch, pixels.height, bytes)
        .is_none_or(|need| need > pixels.data_size)
    {
        return INVALID;
    }
    let result = unsafe { Request::new(options) }.and_then(|request| {
        let format = format.ok_or(UNSUPPORTED)?;
        let rows = unsafe { PackedRows::new(pixels, stride) }.ok_or(INVALID)?;
        Preview::encode(rows, format, &request)?.write(platform, &request, Output::Png, report)
    });
    result.err().unwrap_or(0)
}
