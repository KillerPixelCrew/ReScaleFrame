// SPDX-License-Identifier: MIT
use crate::capture_pixels::Pixels;
use crate::dump_options::{
    ENCODED_MOTION, Log, MAX_IMAGE_BYTES, MotionEncoding, MotionTextureOptions, TextureOptions,
};
use rsf_game_sys::ffi_util::checked;
use std::ffi::{CStr, CString, c_void};
use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::mem::{offset_of, size_of};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_RESOURCE_DIMENSION_TEXTURE2D, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::core::Interface;

mod sample;
mod wic;
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

/// rsf_texture_dump_report.
#[repr(C)]
pub struct Report {
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
    assert!(size_of::<Report>() == 40);
    assert!(size_of::<Pixels>() == 40);
    assert!(size_of::<TextureOptions>() == 40);
    assert!(size_of::<MotionTextureOptions>() == 64);
    assert!(offset_of!(MotionTextureOptions, motion) == 40);
};

fn source_bytes(row_pitch: u32, height: u32, row_bytes: u64) -> Option<u64> {
    (u64::from(row_pitch) * u64::from(height.checked_sub(1)?)).checked_add(row_bytes)
}

pub struct PackedRows<'a> {
    pixels: &'a Pixels,
    pitch: u32,
}

impl<'a> PackedRows<'a> {
    /// # Safety
    ///
    /// The caller lends initialized, readable payload bytes for each row, with
    /// the lifetime and byte capacity described by the pixel descriptor.
    pub unsafe fn new(pixels: &'a Pixels, pixel_bytes: u32) -> Option<Self> {
        let pitch = pixels.width.checked_mul(pixel_bytes)?;
        if pixels.data.is_null() || pitch == 0 || pixels.row_pitch < pitch {
            return None;
        }
        let bytes = source_bytes(pixels.row_pitch, pixels.height, u64::from(pitch))?;
        (bytes <= pixels.data_size && bytes <= isize::MAX as u64).then_some(Self { pixels, pitch })
    }

    pub fn pitch(&self) -> u32 {
        self.pitch
    }

    pub fn packed_size(&self) -> u64 {
        u64::from(self.pitch) * u64::from(self.pixels.height)
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a [u8]> {
        let pixels = self.pixels;
        let pitch = self.pitch as usize;
        (0..pixels.height).map(move |y| {
            let offset = y as usize * pixels.row_pitch as usize;
            // Mapped row padding can be uninitialized. Only borrow the payload.
            unsafe { std::slice::from_raw_parts(pixels.data.cast::<u8>().add(offset), pitch) }
        })
    }
}

pub fn pixel_bytes(format: u32) -> u32 {
    Format::from_dxgi(format).map_or(0, Format::bytes)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_texture_pixel_bytes(format: u32) -> u32 {
    pixel_bytes(format)
}

struct Request<'a> {
    prefix: &'a CStr,
    view: u32,
    scale: f32,
    motion: Option<MotionEncoding>,
    log: Log,
}

impl<'a> Request<'a> {
    unsafe fn new(input: *const TextureOptions, output: Output) -> DumpResult<Self> {
        let options = unsafe { checked(input) }
            .filter(|options| !options.prefix.is_null())
            .ok_or(INVALID)?;
        let motion = match options.abi_version {
            TextureOptions::ABI_VERSION => None,
            MotionTextureOptions::ABI_VERSION => Some(
                unsafe { checked(input.cast::<MotionTextureOptions>()) }
                    .ok_or(INVALID)?
                    .motion,
            ),
            _ => return Err(ABI_MISMATCH),
        };
        if output != Output::Raw
            && options.view == ENCODED_MOTION
            && motion.is_none_or(|motion| {
                !motion
                    .scale
                    .iter()
                    .chain(&motion.bias)
                    .all(|v| v.is_finite())
                    || motion.scale.contains(&0.0)
                    || motion.zero_x_unwritten > 1
            })
        {
            return Err(INVALID);
        }
        Ok(Self {
            prefix: unsafe { CStr::from_ptr(options.prefix) },
            view: options.view,
            scale: if options.scale == 0.0 {
                1.0
            } else {
                options.scale
            },
            motion,
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
    context: &'a ID3D11DeviceContext,
    texture: ID3D11Texture2D,
    pixels: Pixels,
}

impl Drop for Mapping<'_> {
    fn drop(&mut self) {
        unsafe { self.context.Unmap(&self.texture, 0) };
    }
}

struct Readback<'a> {
    device: &'a ID3D11Device,
    context: &'a ID3D11DeviceContext,
    source: &'a ID3D11Resource,
}

impl<'a> Readback<'a> {
    fn describe(&self) -> DumpResult<D3D11_TEXTURE2D_DESC> {
        if unsafe { self.source.GetType() } != D3D11_RESOURCE_DIMENSION_TEXTURE2D
            || unsafe { self.context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE
        {
            return Err(INVALID);
        }
        let texture: ID3D11Texture2D = self.source.cast().map_err(|_| INVALID)?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        Ok(desc)
    }

    fn context_on_device(&self) -> bool {
        unsafe { self.context.GetDevice() }
            .is_ok_and(|device| device.as_raw() == self.device.as_raw())
    }

    fn texture_on_device(&self) -> bool {
        unsafe { self.source.GetDevice() }
            .is_ok_and(|device| device.as_raw() == self.device.as_raw())
    }

    fn map(&self, desc: &D3D11_TEXTURE2D_DESC, raw: bool, log: Log) -> DumpResult<Mapping<'a>> {
        if !raw {
            log.say("dump: creating staging copy");
        }
        let info = D3D11_TEXTURE2D_DESC {
            Width: desc.Width,
            Height: desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut texture = None;
        unsafe { self.device.CreateTexture2D(&info, None, Some(&mut texture)) }
            .map_err(|_| STAGING_FAILED)?;
        let texture = texture.ok_or(STAGING_FAILED)?;
        let top = raw || desc.MipLevels > 1 || desc.ArraySize > 1;
        if raw {
            log.say("raw dump: copying subresource zero and mapping");
        } else if top {
            log.say("dump: copying (top subresource)");
        } else {
            log.say("dump: copying (whole resource)");
        }
        unsafe {
            if top {
                self.context
                    .CopySubresourceRegion(&texture, 0, 0, 0, 0, self.source, 0, None);
            } else {
                self.context.CopyResource(&texture, self.source);
            }
        }
        if !raw {
            log.say("dump: mapping");
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|_| MAP_FAILED)?;
        Ok(Mapping {
            context: self.context,
            texture,
            pixels: Pixels {
                struct_size: size_of::<Pixels>() as u32,
                width: desc.Width,
                height: desc.Height,
                format: desc.Format.0 as u32,
                row_pitch: mapped.RowPitch,
                data_size: u64::from(mapped.RowPitch) * u64::from(desc.Height),
                data: mapped.pData.cast_const(),
            },
        })
    }
}

fn open_file(path: &CStr) -> DumpResult<BufWriter<File>> {
    File::create(path.to_str().map_err(|_| WRITE_FAILED)?)
        .map(BufWriter::new)
        .map_err(|_| WRITE_FAILED)
}

fn write_file(path: &CStr, bytes: &[u8]) -> DumpResult<()> {
    let mut file = open_file(path)?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .map_err(|_| WRITE_FAILED)
}

fn write_raw(mapping: Mapping<'_>, format: Format, request: &Request<'_>) -> DumpResult<()> {
    let pixels = &mapping.pixels;
    let rows = unsafe { PackedRows::new(pixels, format.bytes()) }.ok_or(MAP_FAILED)?;
    let mut binary = open_file(&request.path(b".bin"))?;
    for row in rows.iter() {
        binary.write_all(row).map_err(|_| WRITE_FAILED)?;
    }
    binary.flush().map_err(|_| WRITE_FAILED)?;
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
    write_file(&request.path(b"_raw.json"), metadata.as_bytes())
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
                match sample.channels(request.view, request.motion) {
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
            wic::write_png(&path, width, height, &self.bgra).ok_or(WRITE_FAILED)?;
        } else {
            let width = u16::try_from(width).map_err(|_| INVALID)?;
            let height = u16::try_from(height).map_err(|_| INVALID)?;
            let mut header = [0; 18];
            header[2] = 2; // uncompressed true colour
            header[12..14].copy_from_slice(&width.to_le_bytes());
            header[14..16].copy_from_slice(&height.to_le_bytes());
            header[16] = 32;
            header[17] = 0x28; // top-left origin, 8 attribute bits
            let mut file = open_file(&path)?;
            file.write_all(&header)
                .and_then(|()| file.write_all(&self.bgra))
                .and_then(|()| file.flush())
                .map_err(|_| WRITE_FAILED)?;
        }
        let fraction = stats.fraction_unwritten;
        let metadata = format!(
            "{{\n  \"width\": {width},\n  \"height\": {height},\n  \"format\": {},\n  \"view\": {},\n  \"fraction_unwritten\": {fraction:.4},\n  \"x_range\": [{:.4}, {:.4}],\n  \"y_range\": [{:.4}, {:.4}]\n}}\n",
            stats.format, request.view, stats.min_x, stats.max_x, stats.min_y, stats.max_y
        );
        // Preview metadata has always been best effort after the image succeeds.
        let _ = write_file(&request.path(b".json"), metadata.as_bytes());
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
        if desc.SampleDesc.Count != 1 {
            return Err(INVALID);
        }
        image_bytes(desc.Width, desc.Height)?;
    }
    if !readback.context_on_device() || raw && !readback.texture_on_device() {
        return Err(FOREIGN_DEVICE);
    }
    if !raw {
        request.log.say(&format!(
            "dump {}: {}x{} format {} mips {} slices {} samples {}",
            request.prefix.to_string_lossy(),
            desc.Width,
            desc.Height,
            desc.Format.0 as u32,
            desc.MipLevels,
            desc.ArraySize,
            desc.SampleDesc.Count
        ));
    }
    let format = Format::from_dxgi(desc.Format.0 as u32).ok_or(UNSUPPORTED)?;
    if raw {
        let row = u64::from(desc.Width) * u64::from(format.bytes());
        if desc.SampleDesc.Count != 1
            || row == 0
            || desc.Height == 0
            || row > MAX_IMAGE_BYTES / u64::from(desc.Height)
        {
            return Err(UNSUPPORTED);
        }
    }
    if !readback.texture_on_device() {
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
        .say(&format!("dump: decoding {} rows", desc.Height));
    let preview = Preview::encode(rows, format, request)?;
    drop(mapping);
    preview.write(request, output, report)
}

unsafe fn dump(
    device: Handle,
    context: Handle,
    source: Handle,
    options: *const TextureOptions,
    report: *mut Report,
    output: Output,
) -> i32 {
    let handles = unsafe {
        (
            ID3D11Device::from_raw_borrowed(&device),
            ID3D11DeviceContext::from_raw_borrowed(&context),
            ID3D11Resource::from_raw_borrowed(&source),
        )
    };
    let (Some(device), Some(context), Some(source)) = handles else {
        return INVALID;
    };
    let readback = Readback {
        device,
        context,
        source,
    };
    unsafe { Request::new(options, output) }
        .and_then(|request| dump_texture(readback, &request, output, report))
        .err()
        .unwrap_or(0)
}

/// texture_dump.h: a Targa of `source`, with an optional report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_dump_texture(
    device: Handle,
    context: Handle,
    source: Handle,
    options: *const TextureOptions,
    report: *mut Report,
) -> i32 {
    unsafe { dump(device, context, source, options, report, Output::Targa) }
}

/// texture_dump.h: a PNG of `source`, with an optional report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_dump_texture_png(
    device: Handle,
    context: Handle,
    source: Handle,
    options: *const TextureOptions,
    report: *mut Report,
) -> i32 {
    unsafe { dump(device, context, source, options, report, Output::Png) }
}

/// texture_dump.h: the packed rows of `source` and their sidecar.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_dump_texture_bytes(
    device: Handle,
    context: Handle,
    source: Handle,
    options: *const TextureOptions,
) -> i32 {
    unsafe { dump(device, context, source, options, std::ptr::null_mut(), Output::Raw) }
}

/// texture_dump.h: a PNG of CPU pixels, with an optional report.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_dump_pixels_png(
    pixels: *const Pixels,
    options: *const TextureOptions,
    report: *mut Report,
) -> i32 {
    let Some(pixels) = (unsafe { checked(pixels) }) else {
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
    let result = unsafe { Request::new(options, Output::Png) }.and_then(|request| {
        let format = format.ok_or(UNSUPPORTED)?;
        let rows = unsafe { PackedRows::new(pixels, stride) }.ok_or(INVALID)?;
        Preview::encode(rows, format, &request)?.write(&request, Output::Png, report)
    });
    result.err().unwrap_or(0)
}
