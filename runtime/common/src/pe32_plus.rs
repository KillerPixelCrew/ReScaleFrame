// SPDX-License-Identifier: MIT
//! Read-only AMD64 PE32+ metadata and file-backed RVA lookup.

const COFF_SIZE: usize = 20;
const SECTION_SIZE: usize = 40;

fn le<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..)?.get(..N)?.try_into().ok()
}

pub(crate) struct Pe32Plus<'a> {
    bytes: &'a [u8],
    coff: &'a [u8],
    optional: &'a [u8],
    sections: &'a [u8],
}

#[allow(dead_code)] // Each consumer uses different metadata.
impl<'a> Pe32Plus<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Self> {
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

    pub(crate) fn timestamp(&self) -> u32 {
        u32::from_le_bytes(le(self.coff, 4).expect("checked COFF header"))
    }

    pub(crate) fn optional_size(&self) -> usize {
        self.optional.len()
    }

    pub(crate) fn image_size(&self) -> Option<u32> {
        Some(u32::from_le_bytes(le(self.optional, 56)?))
    }

    pub(crate) fn preferred_base(&self) -> u64 {
        u64::from_le_bytes(le(self.optional, 24).expect("checked image base"))
    }

    pub(crate) fn section_count(&self) -> usize {
        self.sections.len() / SECTION_SIZE
    }

    pub(crate) fn span(&self, rva: usize, count: usize) -> Option<(&'a [u8], u32)> {
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
