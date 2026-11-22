// SPDX-License-Identifier: MIT
// Static findings identify possible vendor call sites in the loaded executable.
use std::ffi::{CString, c_char};
use std::mem::{size_of, size_of_val};
use windows_sys::Win32::System::{
    Diagnostics::Debug::{
        IMAGE_DIRECTORY_ENTRY_EXCEPTION, IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC,
        IMAGE_RUNTIME_FUNCTION_ENTRY, IMAGE_SCN_CNT_INITIALIZED_DATA, IMAGE_SCN_MEM_EXECUTE,
        IMAGE_SCN_MEM_READ, IMAGE_SECTION_HEADER, ReadProcessMemory,
    },
    LibraryLoader::GetModuleHandleW,
    SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE},
    Threading::{
        GetCurrentProcess, GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    },
};

type Report = unsafe extern "C" fn(*const c_char, *const c_char, u64, u64);
const MAX_STRINGS: usize = 400;
const MAX_REFERENCES: usize = 4000;

struct Needle {
    vendor: &'static str,
    text: &'static str,
    wide: bool,
    prefix: bool,
}

impl Needle {
    const fn new(vendor: &'static str, text: &'static str, wide: bool, prefix: bool) -> Self {
        Self {
            vendor,
            text,
            wide,
            prefix,
        }
    }

    fn match_at(&self, bytes: &[u8], offset: usize) -> Option<String> {
        let step = if self.wide { 2 } else { 1 };
        if !offset.is_multiple_of(step) || bytes[offset] != self.text.as_bytes()[0] {
            return None;
        }
        let tail = bytes.get(offset..)?;
        let end = self.text.len() * step;
        let head = tail.get(..end + step)?;
        if offset >= step {
            let previous = &bytes[offset - step..offset];
            if matches!(previous[0], 0x20..=0x7e) && (!self.wide || previous[1] == 0) {
                return None;
            }
        }
        if !head
            .chunks_exact(step)
            .zip(self.text.bytes())
            .all(|(unit, expected)| unit[0] == expected && (!self.wide || unit[1] == 0))
            || (!self.prefix && head[end..].iter().any(|&byte| byte != 0))
        {
            return None;
        }
        Some(
            tail.chunks_exact(step)
                .take(55)
                .take_while(|unit| matches!(unit[0], 0x20..=0x7e) && (!self.wide || unit[1] == 0))
                .map(|unit| char::from(unit[0]))
                .collect(),
        )
    }
}

const NEEDLES: &[Needle] = &[
    Needle::new("fsr2", "FSR2_", true, true),
    Needle::new("fsr3", "FSR3UPSCALER_", true, true),
    Needle::new("ngx", "NVSDK_NGX_", false, true),
    Needle::new("ngx", "DLSS.Feature.Create.Flags", false, false),
    Needle::new("ngx", "DLSS.Render.Subrect.Dimensions.Width", false, false),
    Needle::new("ngx", "DLSS.Pre.Exposure", false, false),
    Needle::new("ngx", "Jitter.Offset.X", false, false),
    Needle::new("ngx", "FrameTimeDeltaInMsec", false, false),
    Needle::new("ngx", "PerfQualityValue", false, false),
    Needle::new("ngx", "nvngx.dll", false, false),
    Needle::new("ngx", "_nvngx.dll", false, false),
    Needle::new("ngx", "nvngx.dll", true, false),
    Needle::new("ngx", "_nvngx.dll", true, false),
    Needle::new("xess", "libxess.dll", false, false),
    Needle::new("xess", "libxess.dll", true, false),
    Needle::new("xess", "xessD3D12Execute", false, false),
    Needle::new("xefg", "libxess_fg.dll", false, false),
    Needle::new("xefg", "libxess_fg.dll", true, false),
    Needle::new("xell", "libxell.dll", false, false),
    Needle::new("xell", "libxell.dll", true, false),
    Needle::new("streamline", "sl.interposer.dll", false, false),
    Needle::new("streamline", "sl.interposer.dll", true, false),
    Needle::new("streamline", "slEvaluateFeature", false, false),
    Needle::new("streamline", "slDLSSGSetOptions", false, false),
    Needle::new("nvapi", "nvapi64.dll", false, false),
    Needle::new("nvapi", "nvapi64.dll", true, false),
    Needle::new("nvapi", "nvapi_QueryInterface", false, false),
    Needle::new("nvll_vk", "NvLowLatencyVk.dll", false, false),
    Needle::new("nvll_vk", "NvLL_VK_", false, true),
    Needle::new("amd", "amdxc64.dll", false, false),
    Needle::new("amd", "amdxc64.dll", true, false),
    Needle::new("amd", "AmdExtD3DCreateInterface", false, false),
    Needle::new("fidelityfx", "ffx_fsr2_api_x64.dll", false, false),
    Needle::new("fidelityfx", "ffx_fsr2_api_x64.dll", true, false),
    Needle::new("fidelityfx", "amd_fidelityfx_dx12.dll", false, false),
    Needle::new("fidelityfx", "amd_fidelityfx_dx12.dll", true, false),
];

const REFLEX_IDS: [(u32, &str); 5] = [
    (0xac1ca9e0, "NvAPI_D3D_SetSleepMode id"),
    (0x852cd1d2, "NvAPI_D3D_Sleep id"),
    (0xd9984c05, "NvAPI_D3D_SetLatencyMarker id"),
    (0x1a587f9c, "NvAPI_D3D_GetLatency id"),
    (0x13c98f73, "NvAPI_D3D12_SetAsyncFrameMarker id"),
];

struct Image(usize);

impl Image {
    fn read(&self, offset: usize, target: *mut std::ffi::c_void, len: usize) -> Option<()> {
        let address = self.0.checked_add(offset)?;
        let mut read = 0;
        (unsafe { ReadProcessMemory(GetCurrentProcess(), address as _, target, len, &mut read) }
            != 0
            && read == len)
            .then_some(())
    }

    // The PE record's fields must accept arbitrary byte patterns.
    unsafe fn record<T: Default>(&self, offset: usize) -> Option<T> {
        let mut value = T::default();
        self.read(offset, (&raw mut value).cast(), size_of_val(&value))?;
        Some(value)
    }

    fn section(&self, rva: u32, size: u32, image_size: u32) -> Option<Vec<u8>> {
        if rva.checked_add(size)? > image_size {
            return None;
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size as usize).ok()?;
        bytes.resize(size as usize, 0);
        if !bytes.is_empty() {
            self.read(rva as usize, bytes.as_mut_ptr().cast(), bytes.len())?;
        }
        Some(bytes)
    }
}

struct Section {
    rva: u32,
    code: bool,
    size: u32,
}

struct Scan {
    image: Image,
    image_size: u32,
    sections: Vec<Section>,
    unwind: Vec<(u32, u32)>,
}

impl Scan {
    fn read(image: Image) -> Option<Self> {
        let dos: IMAGE_DOS_HEADER = unsafe { image.record(0) }?;
        if dos.e_magic != IMAGE_DOS_SIGNATURE {
            return None;
        }
        let nt_offset = usize::try_from(dos.e_lfanew).ok()?;
        let nt: IMAGE_NT_HEADERS64 = unsafe { image.record(nt_offset) }?;
        if nt.Signature != IMAGE_NT_SIGNATURE
            || nt.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC
        {
            return None;
        }
        let image_size = nt.OptionalHeader.SizeOfImage;
        let section_table = nt_offset
            .checked_add(std::mem::offset_of!(IMAGE_NT_HEADERS64, OptionalHeader))?
            .checked_add(nt.FileHeader.SizeOfOptionalHeader.into())?;
        let section_count = nt.FileHeader.NumberOfSections as usize;
        if section_table.checked_add(section_count * size_of::<IMAGE_SECTION_HEADER>())?
            > image_size as usize
        {
            return None;
        }
        let mut sections = Vec::new();
        for index in 0..section_count {
            let section: IMAGE_SECTION_HEADER =
                unsafe { image.record(section_table + index * size_of::<IMAGE_SECTION_HEADER>()) }?;
            let flags = section.Characteristics;
            let code = flags & IMAGE_SCN_MEM_EXECUTE != 0;
            if flags & IMAGE_SCN_MEM_READ != 0
                && (code || flags & IMAGE_SCN_CNT_INITIALIZED_DATA != 0)
            {
                sections.push(Section {
                    rva: section.VirtualAddress,
                    code,
                    size: unsafe { section.Misc.VirtualSize },
                });
            }
        }
        let exception =
            nt.OptionalHeader.DataDirectory[usize::from(IMAGE_DIRECTORY_ENTRY_EXCEPTION)];
        let mut unwind = Vec::new();
        if nt.OptionalHeader.NumberOfRvaAndSizes > u32::from(IMAGE_DIRECTORY_ENTRY_EXCEPTION)
            && exception.VirtualAddress != 0
            && exception.Size != 0
        {
            let entries = image.section(exception.VirtualAddress, exception.Size, image_size)?;
            unwind.extend(
                entries
                    .chunks_exact(size_of::<IMAGE_RUNTIME_FUNCTION_ENTRY>())
                    .map(|record| {
                        (
                            u32::from_le_bytes(record[..4].try_into().unwrap()),
                            u32::from_le_bytes(record[4..8].try_into().unwrap()),
                        )
                    }),
            );
        }
        Some(Self {
            image,
            image_size,
            sections,
            unwind,
        })
    }

    fn snapshot(&self, section: &Section) -> Option<Vec<u8>> {
        self.image
            .section(section.rva, section.size, self.image_size)
    }

    fn function(&self, rva: u32) -> u32 {
        let index = self.unwind.partition_point(|&(_, end)| end <= rva);
        self.unwind
            .get(index)
            .filter(|&&(begin, _)| begin <= rva)
            .map_or(0, |&(begin, _)| begin)
    }
}

struct Hit {
    rva: u32,
    vendor: &'static str,
    text: String,
    refs: Vec<Reference>,
}

struct Reference {
    rva: u32,
    function: u32,
    leaf: bool,
}

fn leaf_start(section_rva: u32, bytes: &[u8], rva: u32) -> u32 {
    let mut aligned = rva & !15;
    while aligned > 16 && rva - aligned < 512 && aligned > section_rva {
        if matches!(bytes[(aligned - section_rva - 1) as usize], 0xcc | 0xc3) {
            return aligned;
        }
        aligned -= 16;
    }
    0
}

fn find_strings(scan: &Scan) -> Option<Vec<Hit>> {
    let mut hits = Vec::new();
    let mut first = [false; 256];
    for needle in NEEDLES {
        first[needle.text.as_bytes()[0] as usize] = true;
    }
    for section in scan.sections.iter().filter(|section| !section.code) {
        let bytes = scan.snapshot(section)?;
        for (offset, window) in bytes.windows(4).enumerate() {
            if !first[window[0] as usize] {
                continue;
            }
            for needle in NEEDLES {
                if let Some(text) = needle.match_at(&bytes, offset) {
                    hits.push(Hit {
                        rva: section.rva + offset as u32,
                        vendor: needle.vendor,
                        text,
                        refs: Vec::new(),
                    });
                    if hits.len() == MAX_STRINGS {
                        return Some(hits);
                    }
                }
            }
        }
    }
    Some(hits)
}

fn find_refs(scan: &Scan, hits: &mut [Hit]) -> Option<Vec<(u32, u32)>> {
    let mut targets: Vec<_> = hits
        .iter()
        .enumerate()
        .map(|(index, hit)| (hit.rva, index))
        .collect();
    targets.sort_unstable();
    let mut fsr_functions: Vec<(u32, u32)> = Vec::new();
    let mut references = 0;
    for section in scan.sections.iter().filter(|section| section.code) {
        let code = scan.snapshot(section)?;
        for (offset, instruction) in code.windows(7).enumerate() {
            if instruction[0] & 0xfb != 0x48
                || !matches!(instruction[1], 0x8d | 0x8b)
                || instruction[2] & 0xc7 != 0x05
            {
                continue;
            }
            let disp = i32::from_le_bytes(instruction[3..7].try_into().unwrap());
            let rva = section.rva + offset as u32;
            let Ok(target) = u32::try_from(i64::from(rva) + 7 + i64::from(disp)) else {
                continue;
            };
            let index = targets.partition_point(|&(address, _)| address < target);
            let Some(&(address, hit)) = targets.get(index) else {
                continue;
            };
            if address != target {
                continue;
            }
            let function = scan.function(rva);
            let leaf = function == 0;
            let function = if leaf {
                leaf_start(section.rva, &code, rva)
            } else {
                function
            };
            hits[hit].refs.push(Reference {
                rva,
                function,
                leaf,
            });
            if matches!(hits[hit].vendor, "fsr2" | "fsr3") {
                if let Some(entry) = fsr_functions.iter_mut().find(|entry| entry.0 == function) {
                    entry.1 += 1;
                } else {
                    fsr_functions.push((function, 1));
                }
            }
            references += 1;
            if references == MAX_REFERENCES {
                return Some(fsr_functions);
            }
        }
    }
    Some(fsr_functions)
}

fn report(scan: &Scan, add: Report) {
    let emit = |vendor, what, rva, function| {
        if let (Ok(vendor), Ok(what)) = (CString::new(vendor), CString::new(what)) {
            unsafe {
                add(
                    vendor.as_ptr(),
                    what.as_ptr(),
                    u64::from(rva),
                    u64::from(function),
                )
            };
        }
    };
    let Some(mut hits) = find_strings(scan) else {
        return;
    };
    let Some(mut fsr_functions) = find_refs(scan, &mut hits) else {
        return;
    };
    for hit in hits {
        if hit.refs.is_empty() && !matches!(hit.vendor, "fsr2" | "fsr3") {
            emit(hit.vendor, format!("string {}", hit.text), hit.rva, 0);
        }
        for reference in hit.refs.iter().take(2) {
            let suffix = if reference.leaf { " [no unwind]" } else { "" };
            emit(
                hit.vendor,
                format!("xref to {}{suffix}", hit.text),
                reference.rva,
                reference.function,
            );
        }
    }
    fsr_functions.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
    for (function, count) in fsr_functions.into_iter().take(4) {
        emit(
            "fsr_static",
            format!("function references {count} FSR resource strings"),
            function,
            function,
        );
    }
    let mut counts = [0; REFLEX_IDS.len()];
    for section in scan.sections.iter().filter(|section| section.code) {
        let Some(code) = scan.snapshot(section) else {
            return;
        };
        for (offset, bytes) in code.windows(4).enumerate() {
            let value = u32::from_le_bytes(bytes.try_into().unwrap());
            for (index, (id, name)) in REFLEX_IDS.into_iter().enumerate() {
                if value == id && counts[index] < 4 {
                    let rva = section.rva + offset as u32;
                    emit("nvapi_reflex", name.to_owned(), rva, scan.function(rva));
                    counts[index] += 1;
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_census_static_scan(add_finding: Option<Report>) {
    let Some(add_finding) = add_finding else {
        return;
    };
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    if !module.is_null()
        && let Some(scan) = Scan::read(Image(module as usize))
    {
        report(&scan, add_finding);
    }
}
