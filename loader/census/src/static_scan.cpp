// SPDX-License-Identifier: GPL-3.0-only
// Static findings: vendor code that is linked into the game image and so never crosses an export.
//
// The scan looks for identifying strings in the main module (FSR2 resource names, NGX entry and
// parameter names, vendor DLL names), finds the code that references each string through a
// RIP-relative lea or mov, and maps that code to its function with the exception directory. It
// also looks for the NVAPI Reflex interface identifiers as 32-bit immediates. A finding names a
// function to look at; it is a lead, not proof that the function is the vendor entry point.
#include <algorithm>
#include <vector>

#include "census_internal.h"

namespace rsf_census {

namespace {

struct Needle {
    const char* vendor;
    const char* text;
    bool wide;
    bool prefix; // true: the string may continue; the whole string is reported
};

constexpr Needle kNeedles[] = {
    {"fsr2", "FSR2_", true, true},
    {"fsr3", "FSR3UPSCALER_", true, true},
    {"ngx", "NVSDK_NGX_", false, true},
    {"ngx", "DLSS.Feature.Create.Flags", false, false},
    {"ngx", "DLSS.Render.Subrect.Dimensions.Width", false, false},
    {"ngx", "DLSS.Pre.Exposure", false, false},
    {"ngx", "Jitter.Offset.X", false, false},
    {"ngx", "FrameTimeDeltaInMsec", false, false},
    {"ngx", "PerfQualityValue", false, false},
    {"ngx", "nvngx.dll", false, false},
    {"ngx", "_nvngx.dll", false, false},
    {"ngx", "nvngx.dll", true, false},
    {"ngx", "_nvngx.dll", true, false},
    {"xess", "libxess.dll", false, false},
    {"xess", "libxess.dll", true, false},
    {"xess", "xessD3D12Execute", false, false},
    {"xefg", "libxess_fg.dll", false, false},
    {"xefg", "libxess_fg.dll", true, false},
    {"xell", "libxell.dll", false, false},
    {"xell", "libxell.dll", true, false},
    {"streamline", "sl.interposer.dll", false, false},
    {"streamline", "sl.interposer.dll", true, false},
    {"streamline", "slEvaluateFeature", false, false},
    {"streamline", "slDLSSGSetOptions", false, false},
    {"nvapi", "nvapi64.dll", false, false},
    {"nvapi", "nvapi64.dll", true, false},
    {"nvapi", "nvapi_QueryInterface", false, false},
    {"nvll_vk", "NvLowLatencyVk.dll", false, false},
    {"nvll_vk", "NvLL_VK_", false, true},
    {"amd", "amdxc64.dll", false, false},
    {"amd", "amdxc64.dll", true, false},
    {"amd", "AmdExtD3DCreateInterface", false, false},
    {"fidelityfx", "ffx_fsr2_api_x64.dll", false, false},
    {"fidelityfx", "ffx_fsr2_api_x64.dll", true, false},
    {"fidelityfx", "amd_fidelityfx_dx12.dll", false, false},
    {"fidelityfx", "amd_fidelityfx_dx12.dll", true, false},
};

struct StringHit {
    uint32_t rva;
    uint8_t needle;
};

struct Ref {
    uint32_t string_index;
    uint32_t rva; // address of the referencing instruction
    uint32_t function_rva;
    bool leaf; // no unwind entry: the start was inferred from the int3 padding before it
};

const IMAGE_SECTION_HEADER* sections(const IMAGE_NT_HEADERS64* nt) noexcept { return IMAGE_FIRST_SECTION(nt); }

uint32_t function_of(const RUNTIME_FUNCTION* table, uint32_t count, uint32_t rva) noexcept {
    uint32_t lo = 0, hi = count;
    while (lo < hi) {
        uint32_t mid = (lo + hi) / 2;
        if (table[mid].EndAddress <= rva) {
            lo = mid + 1;
        } else if (table[mid].BeginAddress > rva) {
            hi = mid;
        } else {
            return table[mid].BeginAddress;
        }
    }
    return 0;
}

// A leaf function without a stack frame has no exception directory entry. MSVC aligns function
// starts to 16 bytes and ends the previous function with ret or pads it with int3, so the nearest
// aligned address at or below the instruction that follows a ret or an int3 is a good guess for the
// start. It can be wrong when a loop head is aligned after a ret.
uint32_t leaf_start(const uint8_t* base, uint32_t rva) noexcept {
    for (uint32_t a = rva & ~15u; a > 16 && rva - a < 512; a -= 16) {
        if (base[a - 1] == 0xCC || base[a - 1] == 0xC3) return a;
    }
    return 0;
}

bool is_data(const IMAGE_SECTION_HEADER& s) noexcept {
    return (s.Characteristics & IMAGE_SCN_MEM_READ) && !(s.Characteristics & IMAGE_SCN_MEM_EXECUTE) &&
           (s.Characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA);
}
bool is_code(const IMAGE_SECTION_HEADER& s) noexcept {
    return (s.Characteristics & IMAGE_SCN_MEM_EXECUTE) && (s.Characteristics & IMAGE_SCN_MEM_READ);
}

struct Scan {
    const uint8_t* base;
    const IMAGE_NT_HEADERS64* nt;
    std::vector<StringHit> strings;
    std::vector<Ref> refs;
    const RUNTIME_FUNCTION* pdata = nullptr;
    uint32_t pdata_count = 0;
};

void find_strings(Scan& s) noexcept {
    // One pass over each data section: a byte table says which needles can start at a byte.
    bool is_first[256] = {};
    for (const Needle& nd : kNeedles) is_first[static_cast<uint8_t>(nd.text[0])] = true;
    const auto* sec = sections(s.nt);
    for (unsigned i = 0; i < s.nt->FileHeader.NumberOfSections; ++i) {
        if (!is_data(sec[i])) continue;
        const uint8_t* begin = s.base + sec[i].VirtualAddress;
        const size_t size = sec[i].Misc.VirtualSize;
        for (size_t o = 0; o + 4 <= size; ++o) {
            if (!is_first[begin[o]]) continue;
            for (size_t n = 0; n < std::size(kNeedles); ++n) {
                const Needle& nd = kNeedles[n];
                if (begin[o] != static_cast<uint8_t>(nd.text[0])) continue;
                const size_t len = std::strlen(nd.text);
                const size_t step = nd.wide ? 2 : 1;
                const size_t bytes = len * step;
                if (nd.wide && (o & 1)) continue;
                if (o + bytes + step > size) continue;
                // Start of a string: the unit before it is not part of the same string.
                if (o >= step) {
                    const uint8_t prev = nd.wide ? begin[o - 2] : begin[o - 1];
                    const uint8_t prev_hi = nd.wide ? begin[o - 1] : 0;
                    if (prev_hi == 0 && prev >= 0x20 && prev <= 0x7E) continue;
                }
                bool match = true;
                for (size_t k = 0; k < len && match; ++k) {
                    if (begin[o + k * step] != static_cast<uint8_t>(nd.text[k])) match = false;
                    if (nd.wide && begin[o + k * step + 1] != 0) match = false;
                }
                if (!match) continue;
                if (!nd.prefix && (begin[o + bytes] != 0 || (nd.wide && begin[o + bytes + 1] != 0))) continue;
                if (s.strings.size() < 400) {
                    s.strings.push_back({static_cast<uint32_t>(sec[i].VirtualAddress + o), static_cast<uint8_t>(n)});
                }
            }
        }
    }
}

void find_refs(Scan& s) noexcept {
    std::vector<uint32_t> targets;
    targets.reserve(s.strings.size());
    for (const auto& h : s.strings) targets.push_back(h.rva);
    std::vector<uint32_t> order(s.strings.size());
    for (uint32_t i = 0; i < order.size(); ++i) order[i] = i;
    std::sort(order.begin(), order.end(), [&](uint32_t a, uint32_t b) { return targets[a] < targets[b]; });
    std::vector<uint32_t> sorted;
    for (uint32_t i : order) sorted.push_back(targets[i]);

    const auto* sec = sections(s.nt);
    for (unsigned i = 0; i < s.nt->FileHeader.NumberOfSections; ++i) {
        if (!is_code(sec[i])) continue;
        const uint8_t* code = s.base + sec[i].VirtualAddress;
        const size_t size = sec[i].Misc.VirtualSize;
        for (size_t o = 0; o + 7 <= size; ++o) {
            const uint8_t rex = code[o];
            if ((rex & 0xFB) != 0x48) continue; // 0x48 or 0x4C
            const uint8_t op = code[o + 1];
            if ((op != 0x8D && op != 0x8B) || (code[o + 2] & 0xC7) != 0x05) continue;
            int32_t disp;
            std::memcpy(&disp, code + o + 3, 4);
            const int64_t target = static_cast<int64_t>(sec[i].VirtualAddress) + static_cast<int64_t>(o) + 7 + disp;
            if (target < 0 || target > 0xFFFFFFFFll) continue;
            auto it = std::lower_bound(sorted.begin(), sorted.end(), static_cast<uint32_t>(target));
            if (it == sorted.end() || *it != target) continue;
            const uint32_t sorted_index = static_cast<uint32_t>(it - sorted.begin());
            const uint32_t instr = static_cast<uint32_t>(sec[i].VirtualAddress + o);
            if (s.refs.size() < 4000) {
                uint32_t fn = function_of(s.pdata, s.pdata_count, instr);
                const bool leaf = fn == 0;
                if (leaf) fn = leaf_start(s.base, instr);
                s.refs.push_back({order[sorted_index], instr, fn, leaf});
            }
        }
    }
}

void scan_reflex_ids(Scan& s) noexcept {
    static const struct {
        uint32_t id;
        const char* name;
    } kIds[] = {{0xAC1CA9E0, "NvAPI_D3D_SetSleepMode id"}, {0x852CD1D2, "NvAPI_D3D_Sleep id"},
                {0xD9984C05, "NvAPI_D3D_SetLatencyMarker id"}, {0x1A587F9C, "NvAPI_D3D_GetLatency id"},
                {0x13C98F73, "NvAPI_D3D12_SetAsyncFrameMarker id"}};
    const auto* sec = sections(s.nt);
    int hits[std::size(kIds)] = {};
    for (unsigned i = 0; i < s.nt->FileHeader.NumberOfSections; ++i) {
        if (!is_code(sec[i])) continue;
        const uint8_t* code = s.base + sec[i].VirtualAddress;
        const size_t size = sec[i].Misc.VirtualSize;
        for (size_t o = 0; o + 4 <= size; ++o) {
            for (size_t k = 0; k < std::size(kIds); ++k) {
                if (hits[k] >= 4) continue;
                const uint8_t* p = code + o;
                if (p[0] == static_cast<uint8_t>(kIds[k].id) && p[1] == static_cast<uint8_t>(kIds[k].id >> 8) &&
                    p[2] == static_cast<uint8_t>(kIds[k].id >> 16) && p[3] == static_cast<uint8_t>(kIds[k].id >> 24)) {
                    const uint32_t rva = static_cast<uint32_t>(sec[i].VirtualAddress + o);
                    add_finding("nvapi_reflex", kIds[k].name, rva, function_of(s.pdata, s.pdata_count, rva));
                    ++hits[k];
                }
            }
        }
    }
}

void read_string(const uint8_t* p, bool wide, char* out, size_t cap) noexcept {
    size_t i = 0;
    for (; i + 1 < cap; ++i) {
        const uint8_t c = wide ? p[i * 2] : p[i];
        if (c == 0 || c < 0x20 || c > 0x7E || (wide && p[i * 2 + 1] != 0)) break;
        out[i] = static_cast<char>(c);
    }
    out[i] = 0;
}

void report(Scan& s) noexcept {
    // Group references by string, then report a few per string and the busiest functions.
    std::vector<uint32_t> per_string(s.strings.size(), 0);
    for (const Ref& r : s.refs) ++per_string[r.string_index];

    for (uint32_t i = 0; i < s.strings.size(); ++i) {
        const Needle& nd = kNeedles[s.strings[i].needle];
        char text[56];
        read_string(s.base + s.strings[i].rva, nd.wide, text, sizeof(text));
        char what[80];
        std::snprintf(what, sizeof(what), "string %s", text);
        if (per_string[i] == 0) {
            // Report the string itself only for vendors where its presence is the finding.
            if (!std::strcmp(nd.vendor, "fsr2") || !std::strcmp(nd.vendor, "fsr3")) continue;
            add_finding(nd.vendor, what, s.strings[i].rva, 0);
            continue;
        }
        uint32_t shown = 0;
        for (const Ref& r : s.refs) {
            if (r.string_index != i) continue;
            if (shown++ >= 2) break;
            char w[80];
            std::snprintf(w, sizeof(w), r.leaf ? "xref to %s [no unwind]" : "xref to %s", text);
            add_finding(nd.vendor, w, r.rva, r.function_rva);
        }
    }

    // Functions that reference several distinct FSR strings are the context creation code.
    struct Func {
        uint32_t rva;
        uint32_t count;
    };
    std::vector<Func> funcs;
    for (const Ref& r : s.refs) {
        const Needle& nd = kNeedles[s.strings[r.string_index].needle];
        if (std::strcmp(nd.vendor, "fsr2") && std::strcmp(nd.vendor, "fsr3")) continue;
        auto it = std::find_if(funcs.begin(), funcs.end(), [&](const Func& f) { return f.rva == r.function_rva; });
        if (it == funcs.end()) {
            funcs.push_back({r.function_rva, 1});
        } else {
            ++it->count;
        }
    }
    std::sort(funcs.begin(), funcs.end(), [](const Func& a, const Func& b) { return a.count > b.count; });
    for (size_t i = 0; i < funcs.size() && i < 4; ++i) {
        char w[80];
        std::snprintf(w, sizeof(w), "function references %u FSR resource strings", funcs[i].count);
        add_finding("fsr_static", w, funcs[i].rva, funcs[i].rva);
    }
}

void run_scan() noexcept {
    HMODULE exe = GetModuleHandleW(nullptr);
    if (!exe) return;
    Scan s;
    s.base = reinterpret_cast<const uint8_t*>(exe);
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(s.base);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return;
    s.nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(s.base + dos->e_lfanew);
    if (s.nt->Signature != IMAGE_NT_SIGNATURE || s.nt->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC) return;
    const auto& dir = s.nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXCEPTION];
    if (dir.VirtualAddress && dir.Size) {
        s.pdata = reinterpret_cast<const RUNTIME_FUNCTION*>(s.base + dir.VirtualAddress);
        s.pdata_count = dir.Size / sizeof(RUNTIME_FUNCTION);
    }
    find_strings(s);
    find_refs(s);
    report(s);
    scan_reflex_ids(s);
}

} // namespace

void static_scan_run() noexcept {
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    // An image can contain unreadable pages (packed or encrypted sections); the scan stops there.
    guard([] { run_scan(); });
}

} // namespace rsf_census
