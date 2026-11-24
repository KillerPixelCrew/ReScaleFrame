// SPDX-License-Identifier: MIT
//! Read-only AMD64 PE32+ metadata: file-backed RVA lookup, and the headers of an image the loader
//! mapped (`Mapped`), read through the caller's own guarded copy.

#[cfg(windows)]
pub use mapped::Mapped;

const COFF_SIZE: usize = 20;
const SECTION_SIZE: usize = 40;

fn le<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..)?.get(..N)?.try_into().ok()
}

pub struct Pe32Plus<'a> {
    bytes: &'a [u8],
    coff: &'a [u8],
    optional: &'a [u8],
    sections: &'a [u8],
}

impl<'a> Pe32Plus<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if le::<2>(bytes, 0)? != *b"MZ" {
            return None;
        }
        let nt = usize::try_from(i32::from_le_bytes(le(bytes, 0x3c)?)).ok()?;
        if le::<4>(bytes, nt)? != *b"PE\0\0" {
            return None;
        }
        let coff = bytes.get(nt.checked_add(4)?..)?.get(..COFF_SIZE)?;
        if u16::from_le_bytes(le(coff, 0)?) != 0x8664 {
            return None;
        }
        let count = usize::from(u16::from_le_bytes(le(coff, 2)?));
        let optional_size = usize::from(u16::from_le_bytes(le(coff, 16)?));
        let optional_start = nt.checked_add(4 + COFF_SIZE)?;
        let sections_start = optional_start.checked_add(optional_size)?;
        let optional = bytes.get(optional_start..sections_start)?;
        if u16::from_le_bytes(le(optional, 0)?) != 0x20b {
            return None;
        }
        le::<8>(optional, 24)?;
        let sections = bytes
            .get(sections_start..)?
            .get(..count.checked_mul(SECTION_SIZE)?)?;
        Some(Self {
            bytes,
            coff,
            optional,
            sections,
        })
    }

    pub fn timestamp(&self) -> u32 {
        u32::from_le_bytes(le(self.coff, 4).expect("checked COFF header"))
    }

    pub fn optional_size(&self) -> usize {
        self.optional.len()
    }

    pub fn image_size(&self) -> Option<u32> {
        Some(u32::from_le_bytes(le(self.optional, 56)?))
    }

    pub fn preferred_base(&self) -> u64 {
        u64::from_le_bytes(le(self.optional, 24).expect("checked image base"))
    }

    pub fn section_count(&self) -> usize {
        self.sections.len() / SECTION_SIZE
    }

    pub fn span(&self, rva: usize, count: usize) -> Option<(&'a [u8], u32)> {
        for section in self.sections.chunks_exact(SECTION_SIZE) {
            let virtual_size = u32::from_le_bytes(le(section, 8)?) as usize;
            let virtual_address = u32::from_le_bytes(le(section, 12)?) as usize;
            let raw_size = u32::from_le_bytes(le(section, 16)?) as usize;
            let raw_offset = u32::from_le_bytes(le(section, 20)?) as usize;
            let Some(delta) = rva.checked_sub(virtual_address) else {
                continue;
            };
            if delta >= virtual_size.max(raw_size) {
                continue;
            }
            // Virtual tails have no file bytes, even if another section follows on disk.
            if delta >= raw_size || delta.checked_add(count)? > raw_size {
                return None;
            }
            let start = raw_offset.checked_add(delta)?;
            let span = self.bytes.get(start..)?.get(..count)?;
            return Some((span, u32::from_le_bytes(le(section, 36)?)));
        }
        None
    }
}

#[cfg(windows)]
mod mapped {
    use std::mem::{MaybeUninit, offset_of, size_of};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC, IMAGE_SECTION_HEADER,
    };
    use windows_sys::Win32::System::SystemServices::{
        IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE,
    };

    /// The NT headers of a mapped PE32+ image. Header bytes sit at the same offsets in the file
    /// and in memory, so `read(offset, out, size)` copies `size` bytes at that image offset and
    /// returns whether every byte was copied.
    pub struct Mapped {
        pub nt_offset: usize,
        pub sections_offset: usize,
        pub headers: IMAGE_NT_HEADERS64,
    }

    // Only for the windows-sys PE records, which accept any byte pattern.
    fn record<T: Copy>(
        read: &mut impl FnMut(usize, *mut u8, usize) -> bool,
        offset: usize,
    ) -> Option<T> {
        let mut value = MaybeUninit::<T>::zeroed();
        read(offset, value.as_mut_ptr().cast(), size_of::<T>())
            .then(|| unsafe { value.assume_init() })
    }

    impl Mapped {
        pub fn read(read: &mut impl FnMut(usize, *mut u8, usize) -> bool) -> Option<Self> {
            let dos: IMAGE_DOS_HEADER = record(read, 0)?;
            if dos.e_magic != IMAGE_DOS_SIGNATURE {
                return None;
            }
            let nt_offset = usize::try_from(dos.e_lfanew).ok()?;
            let headers: IMAGE_NT_HEADERS64 = record(read, nt_offset)?;
            if headers.Signature != IMAGE_NT_SIGNATURE
                || headers.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC
            {
                return None;
            }
            let sections_offset = nt_offset
                .checked_add(offset_of!(IMAGE_NT_HEADERS64, OptionalHeader))?
                .checked_add(headers.FileHeader.SizeOfOptionalHeader.into())?;
            Some(Self {
                nt_offset,
                sections_offset,
                headers,
            })
        }

        pub fn image_size(&self) -> usize {
            self.headers.OptionalHeader.SizeOfImage as usize
        }

        pub fn section_count(&self) -> usize {
            usize::from(self.headers.FileHeader.NumberOfSections)
        }

        /// Whether the whole section table lies inside SizeOfImage.
        pub fn sections_fit(&self) -> bool {
            self.section_count()
                .checked_mul(size_of::<IMAGE_SECTION_HEADER>())
                .and_then(|size| self.sections_offset.checked_add(size))
                .is_some_and(|end| end <= self.image_size())
        }

        pub fn section(
            &self,
            read: &mut impl FnMut(usize, *mut u8, usize) -> bool,
            index: usize,
        ) -> Option<IMAGE_SECTION_HEADER> {
            let offset = index.checked_mul(size_of::<IMAGE_SECTION_HEADER>())?;
            record(read, self.sections_offset.checked_add(offset)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> Vec<u8> {
        let mut bytes = vec![0; 0x210];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
        bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes[0xb0..0xb8].copy_from_slice(&0x1_8000_0000u64.to_le_bytes());
        bytes[0x190..0x194].copy_from_slice(&16u32.to_le_bytes());
        bytes[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[0x198..0x19c].copy_from_slice(&8u32.to_le_bytes());
        bytes[0x19c..0x1a0].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[0x1ac..0x1b0].copy_from_slice(&0x6000_0020u32.to_le_bytes());
        bytes[0x200..].fill(0x5a);
        bytes
    }

    #[test]
    fn truncated_section_table_is_refused() {
        let mut bytes = file();
        bytes.truncate(0x188 + SECTION_SIZE - 1);
        assert!(Pe32Plus::parse(&bytes).is_none());
    }

    #[test]
    fn virtual_tail_cannot_read_more_bytes_from_disk() {
        let bytes = file();
        let image = Pe32Plus::parse(&bytes).unwrap();
        assert_eq!(image.preferred_base(), 0x1_8000_0000);
        assert_eq!(image.span(0x1004, 4), Some((&[0x5a; 4][..], 0x6000_0020)));
        assert!(image.span(0x1004, 5).is_none());
        assert!(image.span(0x1008, 1).is_none());
    }
}
