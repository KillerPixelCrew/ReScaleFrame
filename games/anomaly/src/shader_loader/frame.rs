// SPDX-License-Identifier: MIT
use super::{Bytes, Define, Request, Span};
use std::ffi::c_void;

#[repr(C)]
pub(crate) struct FrameLayout {
    calls: [u32; 2],
    call_length: u32,
    source: u32,
    length: u32,
    entry: u32,
    flags: u32,
    defines: u32,
    define_capacity: u32,
}

const _: () = assert!(size_of::<FrameLayout>() == 36);

#[repr(C)]
struct FrameRequest {
    source: Bytes,
    entry: Bytes,
    flags: u32,
    defines: Span<Define>,
}

#[repr(u8)]
pub(crate) enum FrameStatus {
    Ok,
    UnknownCaller,
    Unreadable,
    BadSource,
    BadDefines,
}

pub(crate) type Read = unsafe extern "C" fn(*const c_void, usize, *mut u8, usize) -> bool;
type Publish = unsafe extern "C" fn(*mut c_void, &FrameRequest) -> bool;

#[derive(Default)]
pub(crate) struct DecodedFrame {
    pub(crate) source: Vec<u8>,
    entry: Vec<u8>,
    flags: u32,
    defines: Vec<(Vec<u8>, Vec<u8>)>,
}

impl DecodedFrame {
    fn define_views(&self) -> Vec<Define> {
        self.defines
            .iter()
            .map(|(name, value)| Define {
                name: Bytes::from(name.as_slice()),
                value: Bytes::from(value.as_slice()),
            })
            .collect()
    }

    pub(crate) fn with_request<T>(
        &self,
        target: &[u8],
        bytecode: &[u8],
        call: impl FnOnce(&Request) -> T,
    ) -> T {
        let defines = self.define_views();
        call(&Request {
            target: Bytes::from(target),
            entry: Bytes::from(self.entry.as_slice()),
            flags: self.flags,
            defines: Span::from(defines.as_slice()),
            source: Bytes::from(self.source.as_slice()),
            bytecode: Bytes::from(bytecode),
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_frame(
    layout: &FrameLayout,
    module: usize,
    return_address: usize,
    stack: usize,
    reader: *const c_void,
    read: Read,
    request: *mut c_void,
    publish: Publish,
) -> FrameStatus {
    let decoded = match decode_frame(
        layout,
        module,
        return_address,
        stack,
        |address, bytes| unsafe { read(reader, address, bytes.as_mut_ptr(), bytes.len()) },
    ) {
        Ok(decoded) => decoded,
        Err(status) => return status,
    };
    let defines = decoded.define_views();
    let frame = FrameRequest {
        source: Bytes::from(decoded.source.as_slice()),
        entry: Bytes::from(decoded.entry.as_slice()),
        flags: decoded.flags,
        defines: Span::from(defines.as_slice()),
    };
    if unsafe { publish(request, &frame) } {
        FrameStatus::Ok
    } else {
        FrameStatus::Unreadable
    }
}

pub(crate) fn decode_frame(
    layout: &FrameLayout,
    module: usize,
    return_address: usize,
    stack: usize,
    read: impl Fn(usize, &mut [u8]) -> bool,
) -> Result<DecodedFrame, FrameStatus> {
    if module == 0
        || stack == 0
        || !layout.calls.iter().any(|&call| {
            call != 0
                && module
                    .checked_add(call as usize)
                    .and_then(|site| site.checked_add(layout.call_length as usize))
                    == Some(return_address)
        })
    {
        return Err(FrameStatus::UnknownCaller);
    }
    let guarded = |base: usize, offset: usize, bytes: &mut [u8]| {
        let Some(address) = base.checked_add(offset).filter(|&address| {
            base != 0 && address.checked_add(bytes.len().saturating_sub(1)).is_some()
        }) else {
            return false;
        };
        read(address, bytes)
    };
    let mut source = [0; 8];
    let mut length = [0; 4];
    let mut entry = [0; 8];
    let mut flags = [0; 4];
    if !guarded(stack, layout.source as usize, &mut source)
        || !guarded(stack, layout.length as usize, &mut length)
        || !guarded(stack, layout.entry as usize, &mut entry)
        || !guarded(stack, layout.flags as usize, &mut flags)
    {
        return Err(FrameStatus::Unreadable);
    }
    let source = u64::from_le_bytes(source) as usize;
    let length = u32::from_le_bytes(length) as usize;
    let entry = u64::from_le_bytes(entry) as usize;
    if source == 0 || length == 0 || length > 16 * 1024 * 1024 || entry == 0 {
        return Err(FrameStatus::BadSource);
    }
    let read_string = |address| {
        let mut text = Vec::new();
        for i in 0..512 {
            let mut byte = [0];
            if !guarded(address, i, &mut byte) {
                return None;
            }
            if byte[0] == 0 {
                return Some(text);
            }
            text.push(byte[0]);
        }
        None
    };
    let mut root = vec![0; length];
    if !guarded(source, 0, &mut root) {
        return Err(FrameStatus::BadSource);
    }
    let Some(entry) = read_string(entry) else {
        return Err(FrameStatus::BadSource);
    };
    let mut defines = Vec::new();
    for i in 0..layout.define_capacity as usize {
        let mut pair = [0; 16];
        if !guarded(stack, layout.defines as usize + i * pair.len(), &mut pair) {
            return Err(FrameStatus::BadDefines);
        }
        let name = u64::from_le_bytes(pair[..8].try_into().unwrap()) as usize;
        if name == 0 {
            return Ok(DecodedFrame {
                source: root,
                entry,
                flags: u32::from_le_bytes(flags),
                defines,
            });
        }
        let value = u64::from_le_bytes(pair[8..].try_into().unwrap()) as usize;
        let (Some(name), Some(value)) = (read_string(name), read_string(value)) else {
            return Err(FrameStatus::BadDefines);
        };
        defines.push((name, value));
    }
    Err(FrameStatus::BadDefines)
}
