// SPDX-License-Identifier: MIT
//! Byte-pattern discovery of an adapter's code sites and globals, so an adapter finds its sites in
//! whatever build is running instead of trusting fixed RVAs.
//!
//! A pattern is space-separated hex bytes with `??` wildcards for relocations and displacements,
//! e.g. `48 8B 05 ?? ?? ?? ?? 0F 57 C0`. Every pattern must match exactly once across the image's
//! executable sections. A site is then the match plus an offset, or the target of a RIP-relative
//! operand (`disp32` at a fixed offset, relative to the end of its instruction) inside the match.
//! All patterns are matched in one pass: each is anchored on its rarest fixed byte pair.

/// How a site follows from its pattern's unique match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Find {
    /// The site is the match plus this offset.
    At(usize),
    /// The site is the target of the `disp32` at `disp`, relative to the match plus `next`.
    Rip { disp: usize, next: usize },
}

/// A named site, the pattern that finds it and the bytes expected at the site itself.
#[derive(Clone, Copy, Debug)]
pub struct Signature {
    pub name: &'static str,
    pub pattern: &'static str,
    pub find: Find,
    /// Pattern expected at the found site before it is hooked, patched or called, when the
    /// finding pattern does not already cover the site (a function found through its caller).
    /// Empty when there is none.
    pub check: &'static str,
}

impl Signature {
    /// The pattern verified before the site resolved to `rva` is hooked, patched or called, and the
    /// RVA it starts at: the check pattern at the site, or else the finding pattern at its match.
    /// `None` for a global, which has no code to verify.
    pub fn guard(&self, rva: usize) -> Option<(&'static str, usize)> {
        if !self.check.is_empty() {
            return Some((self.check, rva));
        }
        match self.find {
            Find::At(offset) => Some((self.pattern, rva.checked_sub(offset)?)),
            Find::Rip { .. } => None,
        }
    }
}

/// A parsed pattern: `None` is a wildcard byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern(Vec<Option<u8>>);

impl Pattern {
    /// Parses `AB ?? CD`. Refuses empty patterns and patterns without two adjacent fixed bytes,
    /// which could not be anchored.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text
            .split_ascii_whitespace()
            .map(|token| match token {
                "??" => Some(None),
                _ if token.len() == 2 => u8::from_str_radix(token, 16).ok().map(Some),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        bytes
            .windows(2)
            .any(|pair| pair[0].is_some() && pair[1].is_some())
            .then_some(Self(bytes))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `bytes` starts with this pattern.
    pub fn matches(&self, bytes: &[u8]) -> bool {
        bytes.len() >= self.0.len()
            && self
                .0
                .iter()
                .zip(bytes)
                .all(|(expected, byte)| expected.is_none_or(|expected| expected == *byte))
    }

    fn pairs(&self) -> impl Iterator<Item = (usize, u16)> + '_ {
        self.0
            .windows(2)
            .enumerate()
            .filter_map(|(offset, pair)| Some((offset, key(pair[0]?, pair[1]?))))
    }
}

fn key(first: u8, second: u8) -> u16 {
    u16::from_le_bytes([first, second])
}

/// One executable section: its RVA and bytes.
#[derive(Clone, Copy, Debug)]
pub struct Section<'a> {
    pub rva: usize,
    pub bytes: &'a [u8],
}

/// Why a site was not resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    /// The pattern text does not parse.
    Pattern,
    /// No match in the executable sections.
    Missing,
    /// More than one match; the site is ambiguous.
    Ambiguous,
    /// The RIP-relative operand lies outside the match or its target outside the address space.
    Operand,
}

/// A site that did not resolve; adapters refuse and name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub name: &'static str,
    pub miss: Miss,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self.miss {
            Miss::Pattern => "has an invalid pattern",
            Miss::Missing => "was not found",
            Miss::Ambiguous => "matched more than once",
            Miss::Operand => "has an undecodable operand",
        };
        write!(out, "site '{}' {reason}", self.name)
    }
}

/// Resolves every signature to an RVA, in order, or names the first that does not resolve.
pub fn resolve(sections: &[Section], signatures: &[Signature]) -> Result<Vec<usize>, Refusal> {
    let patterns = signatures
        .iter()
        .map(|signature| {
            Pattern::parse(signature.pattern).ok_or(Refusal {
                name: signature.name,
                miss: Miss::Pattern,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let hits = scan(sections, &patterns);
    signatures
        .iter()
        .zip(&patterns)
        .zip(hits)
        .map(|((signature, pattern), (count, at))| {
            let refuse = |miss| Refusal {
                name: signature.name,
                miss,
            };
            match count {
                0 => return Err(refuse(Miss::Missing)),
                1 => {}
                _ => return Err(refuse(Miss::Ambiguous)),
            }
            let (section, offset) = at;
            let start = sections[section].rva + offset;
            match signature.find {
                Find::At(delta) => start.checked_add(delta).ok_or(refuse(Miss::Operand)),
                Find::Rip { disp, next } => {
                    let bytes = &sections[section].bytes[offset..];
                    let operand = disp
                        .checked_add(4)
                        .filter(|&end| end <= pattern.len() && end <= next)
                        .and_then(|end| bytes.get(disp..end))
                        .ok_or(refuse(Miss::Operand))?;
                    let displacement = i32::from_le_bytes(operand.try_into().expect("4 bytes"));
                    start
                        .checked_add(next)
                        .and_then(|end| end.checked_add_signed(displacement as isize))
                        .ok_or(refuse(Miss::Operand))
                }
            }
        })
        .collect()
}

/// Match count (stopping at two) and first match (section, offset) of every pattern.
fn scan(sections: &[Section], patterns: &[Pattern]) -> Vec<(u32, (usize, usize))> {
    // Anchor each pattern on its least frequent fixed pair in these sections.
    let mut counts = vec![0u32; 1 << 16];
    for section in sections {
        for pair in section.bytes.windows(2) {
            let slot = &mut counts[usize::from(key(pair[0], pair[1]))];
            *slot = slot.saturating_add(1);
        }
    }
    let mut anchors = patterns
        .iter()
        .enumerate()
        .filter_map(|(index, pattern)| {
            let (offset, key) = pattern
                .pairs()
                .min_by_key(|&(_, key)| counts[usize::from(key)])?;
            Some((key, offset, index))
        })
        .collect::<Vec<_>>();
    anchors.sort_unstable();
    // Bucket starts by anchor key, so each position costs one lookup.
    let mut starts = vec![0u32; (1 << 16) + 1];
    for &(key, _, _) in &anchors {
        starts[usize::from(key) + 1] += 1;
    }
    for index in 1..starts.len() {
        starts[index] += starts[index - 1];
    }
    let mut hits = vec![(0u32, (0, 0)); patterns.len()];
    for (number, section) in sections.iter().enumerate() {
        let bytes = section.bytes;
        for (position, pair) in bytes.windows(2).enumerate() {
            let key = usize::from(key(pair[0], pair[1]));
            let bucket = starts[key] as usize..starts[key + 1] as usize;
            for &(_, offset, index) in &anchors[bucket] {
                let Some(start) = position.checked_sub(offset) else {
                    continue;
                };
                let hit = &mut hits[index];
                if hit.0 < 2 && patterns[index].matches(&bytes[start..]) {
                    if hit.0 == 0 {
                        hit.1 = (number, start);
                    }
                    hit.0 += 1;
                }
            }
        }
    }
    hits
}

/// The RVA of the only occurrence of `needle` in `sections` (a string, or a pointer value).
pub fn find_unique(sections: &[Section], needle: &[u8]) -> Result<usize, Miss> {
    let first = *needle.first().ok_or(Miss::Pattern)?;
    let mut found = None;
    for section in sections {
        let bytes = section.bytes;
        let mut start = 0;
        while let Some(offset) = bytes
            .get(start..)
            .and_then(|tail| tail.iter().position(|&byte| byte == first))
        {
            let at = start + offset;
            if bytes[at..].starts_with(needle) {
                if found.is_some() {
                    return Err(Miss::Ambiguous);
                }
                found = Some(section.rva + at);
            }
            start = at + 1;
        }
    }
    found.ok_or(Miss::Missing)
}

/// The RVA of the only instruction `opcode disp32` (nothing after the displacement) whose
/// RIP-relative operand addresses `target`, e.g. the `FF 25` import thunk of an IAT slot.
pub fn find_rip_reference(
    sections: &[Section],
    opcode: &[u8],
    target: usize,
) -> Result<usize, Miss> {
    let length = opcode.len() + 4;
    let mut found = None;
    for section in sections {
        for (offset, window) in section.bytes.windows(length).enumerate() {
            if !window.starts_with(opcode) {
                continue;
            }
            let displacement = i32::from_le_bytes(window[opcode.len()..].try_into().expect("4"));
            let next = section.rva + offset + length;
            if next.checked_add_signed(displacement as isize) == Some(target) {
                if found.is_some() {
                    return Err(Miss::Ambiguous);
                }
                found = Some(section.rva + offset);
            }
        }
    }
    found.ok_or(Miss::Missing)
}

/// The RVA of the import address table slot of `function` imported by name from `library`
/// (ASCII, case-insensitive). `read(rva, out)` copies image bytes at an RVA.
pub fn import_slot(
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
    library: &str,
    function: &str,
) -> Option<usize> {
    let mut u32_at = |rva: usize| {
        let mut bytes = [0; 4];
        read(rva, &mut bytes).then(|| u32::from_le_bytes(bytes) as usize)
    };
    let nt = u32_at(0x3c)?;
    // PE32+ optional header at nt + 24; import directory is data directory 1 at +120.
    let directory = u32_at(nt + 24 + 120)?;
    let size = u32_at(nt + 24 + 124)?;
    fn name(read: &mut impl FnMut(usize, &mut [u8]) -> bool, rva: usize, expected: &str) -> bool {
        let mut bytes = vec![0; expected.len() + 1];
        read(rva, &mut bytes)
            && bytes[expected.len()] == 0
            && bytes[..expected.len()].eq_ignore_ascii_case(expected.as_bytes())
    }
    for descriptor in (directory..directory + size).step_by(20) {
        let mut fields = [0; 20];
        if !read(descriptor, &mut fields) {
            return None;
        }
        let field = |index: usize| {
            u32::from_le_bytes(
                fields[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("4 bytes"),
            ) as usize
        };
        let (lookup, library_name, slots) = (field(0), field(3), field(4));
        if library_name == 0 {
            return None;
        }
        if !name(&mut read, library_name, library) {
            continue;
        }
        // The lookup table keeps the names after the loader overwrote the IAT.
        let names = if lookup != 0 { lookup } else { slots };
        for index in 0.. {
            let mut entry = [0; 8];
            if !read(names + index * 8, &mut entry) {
                return None;
            }
            let entry = u64::from_le_bytes(entry);
            if entry == 0 {
                break;
            }
            // Bit 63 marks an ordinal import; a name import points at a hint/name record.
            if entry >> 63 == 0 && name(&mut read, entry as usize + 2, function) {
                return Some(slots + index * 8);
            }
        }
    }
    None
}

/// Section characteristic of executable code.
pub const EXECUTE: u32 = 0x2000_0000;

/// Copies the sections of a module the loader mapped at `base` for which `keep(name,
/// characteristics)` holds, read through a guarded `read(address, out, size)`, as (RVA, bytes).
#[cfg(windows)]
pub fn mapped_sections(
    base: usize,
    read: unsafe extern "C" fn(usize, *mut std::ffi::c_void, usize) -> bool,
    keep: impl Fn(&[u8; 8], u32) -> bool,
) -> Option<Vec<(usize, Vec<u8>)>> {
    let mut image = |offset: usize, out: *mut u8, size: usize| {
        base.checked_add(offset)
            .is_some_and(|address| unsafe { read(address, out.cast(), size) })
    };
    let headers = crate::pe32_plus::Mapped::read(&mut image)?;
    if base == 0 || !headers.sections_fit() {
        return None;
    }
    let mut kept = Vec::new();
    for index in 0..headers.section_count() {
        let section = headers.section(&mut image, index)?;
        if !keep(&section.Name, section.Characteristics) {
            continue;
        }
        let rva = section.VirtualAddress as usize;
        let size = unsafe { section.Misc.VirtualSize } as usize;
        if rva.checked_add(size)? > headers.image_size() {
            return None;
        }
        let mut bytes = vec![0; size];
        if !image(rva, bytes.as_mut_ptr(), size) {
            return None;
        }
        kept.push((rva, bytes));
    }
    Some(kept)
}

/// `steamapps/common/<relative>` in a Steam library of the default Steam installation, when it
/// exists: adapters' tests verify their patterns against the installed game with it.
pub fn steam_file(relative: &str) -> Option<std::path::PathBuf> {
    let steam = std::path::PathBuf::from(std::env::var_os("ProgramFiles(x86)")?).join("Steam");
    let folders = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf"))
        .unwrap_or_default();
    // Each library is a `"path"  "D:\\Library"` line.
    let libraries = folders.lines().filter_map(|line| {
        let mut fields = line.split('"').filter(|field| !field.trim().is_empty());
        (fields.next()? == "path").then(|| fields.next().map(|path| path.replace("\\\\", "\\")))?
    });
    std::iter::once(steam.clone())
        .chain(libraries.map(std::path::PathBuf::from))
        .map(|library| library.join("steamapps").join("common").join(relative))
        .find(|path| path.is_file())
}

/// Borrowed views of copied sections.
pub fn views(sections: &[(usize, Vec<u8>)]) -> Vec<Section<'_>> {
    sections
        .iter()
        .map(|(rva, bytes)| Section { rva: *rva, bytes })
        .collect()
}

#[cfg(test)]
mod tests;
