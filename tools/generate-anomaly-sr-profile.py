"""Project the Rust SR profile owner (sr_profile.rs) into the native sr_profile.h consumers.

The per-build records are generated between the BEGIN/END markers. The shared layout values, the target-slot and
world-target tables and the absent offsets keep their documented places in the header and take their values from
sr_profile.rs. --check refuses when any projected value has drifted."""
import argparse
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1] / "games/anomaly/src"
PROFILE = ROOT / "sr_profile.rs"
HEADER = ROOT / "sr_profile.h"
START = "// BEGIN GENERATED SR PROFILES"
END = "// END GENERATED SR PROFILES"
# label, heading, executable (native_profile.rs order).
BUILDS = (("standard", "Standard", "AnomalyDX11.exe"), ("avx", "AVX", "AnomalyDX11AVX.exe"))
FIELDS = ("crender_vtable_rva", "device_view_rva", "device_project_rva", "device_full_transform_rva",
          "device_precache_frame_rva", "load_screen_registered_rva", "hw_device_rva", "hw_context_rva",
          "hw_swap_chain_rva", "hw_base_rt_rva", "hw_base_zb_rva", "render_implementation_rva",
          "render_options_word0_rva", "render_options_word3_rva", "sunshafts_mode_rva", "sampler_cache_rva",
          "user_mip_bias_rva", "user_anisotropy_rva", "ssao_set_viewport_rva", "ssao_viewport_rva",
          "ssao_viewport_guard_rva")
TAIL = ("backend_rva", "position_setup_vtable_rva", "get_constant_buffer_rva")
HEX = r"0x[0-9a-f]+"
NAME = r'c"([^"]+)"'


def scalars(source):
    values = {}
    for name, value in re.findall(r"pub\(crate\) const ([A-Z0-9_]+): (?:u32|usize) = ([^;]+);", source):
        values[name.lower()] = re.sub(r"^1 << (\d+)$", r"1u << \1", value.strip())
    return values


def calls(function, body, arity):
    pattern = rf"\b{function}\(\s*" + r",\s*".join([NAME] + [r"([^,()\s]+)"] * (arity - 1)) + r",?\s*\)"
    return re.findall(pattern, body)


def table(source, name):
    return re.search(rf"(?:static|const) {name}: \[[^\]]+\] = \[(.*?)\n\];", source, re.S)[1]


def build_records(source):
    records = {}
    body = re.search(r"pub\(crate\) static PROFILES: \[Profile; 2\] = \[(.*)\n\];", source, re.S)[1]
    labels = re.findall(r'label: c"([^"]+)"', body)
    if labels != [label for label, _, _ in BUILDS]:
        raise ValueError("sr_profile.rs order must match native_profile.rs")
    regions = re.split(r"\n    Profile \{", body)[1:]
    for (label, _, _), region in zip(BUILDS, regions):
        record = {"unique": [(n, r, h) for n, r, h in calls("UniqueGuard::new", table(source, f"{label.upper()}_UNIQUE"), 3)]}
        sites = re.search(r"function_sites: \[(.*?)\n        \],", region, re.S)[1]
        record["functions"] = calls("site", sites, 3)
        record["calls"] = calls("site", re.search(r"call_sites: \[(.*?)\n        \],", region, re.S)[1], 3)
        record["first_person"] = calls("first_person", re.search(r"first_person_sites: \[(.*?)\n        \],", region, re.S)[1], 4)
        for field in FIELDS + TAIL:
            record[field] = re.search(rf"\b{field}: ({HEX}),", region)[1]
        record["ssao_code"] = re.search(rf"ssao_set_viewport_code: {NAME},", region)[1]
        record["reconstruction"] = calls("UniqueGuard::new", re.search(r"reconstruction_guards: \[(.*?)\n        \],", region, re.S)[1], 3)
        counts = (len(record["functions"]), len(record["calls"]), len(record["first_person"]), len(record["reconstruction"]))
        if counts != (17, 6, 6, 4):
            raise ValueError(f"{label} profile has {counts} function, call, first-person and reconstruction records")
        records[label] = record
    return records


def unquote(value):
    return value[2:-1] if value[:2] in ('b"', 'c"') else value


def project_builds(records):
    lines = [START, "// Generated from sr_profile.rs by tools/generate-anomaly-sr-profile.py."]
    for label, heading, executable in BUILDS:
        r = records[label]
        title = f"// ---- {heading} 1.5.3 DX11 ({executable}) "
        lines.append(title + "-" * (119 - len(title)))
        lines.append(f"inline constexpr std::array<UniqueGuard, {len(r['unique'])}> sr_{label}_unique{{{{")
        lines += [f'    {{"{n}", {rva}, "{unquote(h)}"}},' for n, rva, h in r["unique"]]
        lines += ["}};", f"inline constexpr SrProfile sr_{label}{{", "    .sr_function_sites = {{"]
        lines += [f'        {{"{n}", {rva}, signature("{unquote(h)}")}},' for n, rva, h in r["functions"]]
        lines += ["    }},", f"    .sr_unique_guards = sr_{label}_unique,", "    .sr_call_sites = {{"]
        lines += [f'        {{"{n}", {rva}, signature("{unquote(h)}")}},' for n, rva, h in r["calls"]]
        lines += ["    }},", "    .sr_first_person_sites = {{"]
        lines += [f'        {{"{n}", {a}, {b}, {c}}},' for n, a, b, c in r["first_person"]]
        lines.append("    }},")
        lines += [f"    .{field} = {r[field]}," for field in FIELDS]
        lines += ["    .ssao_set_viewport_code =", f'        "{r["ssao_code"]}",']
        lines += [f"    .{field} = {r[field]}," for field in TAIL]
        lines.append("    .reconstruction_guards = {{")
        lines += [f'        {{"{n}", {rva}, "{unquote(h)}"}},' for n, rva, h in r["reconstruction"]]
        lines += ["    }},", "};", ""]
    return "\n".join([*lines[:-1], END])


def substitute(text, pattern, replacements, label):
    found = list(re.finditer(pattern, text))
    if len(found) != len(replacements):
        raise ValueError(f"sr_profile.h has {len(found)} {label} entries, sr_profile.rs {len(replacements)}")
    parts, last = [], 0
    for match, replacement in zip(found, replacements):
        parts += [text[last:match.start()], replacement]
        last = match.end()
    return "".join(parts) + text[last:]


def project_shared(shared, source):
    values = scalars(source)
    declared = []
    for match in re.finditer(r"inline constexpr (?:uint32_t|std::size_t) ([^;]+);", shared):
        declared += [item.split("=")[0].strip() for item in match[1].split(",")]
    if sorted(declared) != sorted(values):
        raise ValueError(f"shared constants differ: {sorted(set(declared) ^ set(values))}")
    for name, value in values.items():
        shared, count = re.subn(rf"(\b{name} = )(?:1u << \d+|{HEX}|\d+)", lambda m: m[1] + value, shared)
        if count != 1:
            raise ValueError(f"{name} is declared {count} times in sr_profile.h")
    slots = [f'{{"{n}", {o}, {f}}}' for n, o, f in calls("slot", table(source, "TARGET_SLOTS"), 3)]
    shared = substitute(shared, r'\{"\$user\$[^"]+", 0x[0-9a-f]+, \d+\}', slots, "target_slots")
    world = [f'{{"{n}", {o}, {f}, {a}, {b}}}' for n, o, f, a, b in calls("world", table(source, "WORLD_TARGETS"), 5)]
    shared = substitute(shared, r'\{"\$user\$[^"]+", 0x[0-9a-f]+, \d+, (?:true|false), (?:true|false)\}', world,
                        "sr_world_targets")
    absent = re.search(r"WORLD_ABSENT_OFFSETS: \[u32; 4\] = \[([^\]]+)\];", source)[1]
    return re.sub(r"(sr_world_absent_offsets\{)[^}]*(\};)", lambda m: m[1] + absent + m[2], shared)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="refuse if the native projection has drifted")
    args = parser.parse_args()
    header = HEADER.read_text(encoding="utf-8")
    source = PROFILE.read_text(encoding="utf-8")
    start = header.index(START)
    end = header.index(END, start) + len(END)
    expected = project_shared(header[:start], source) + project_builds(build_records(source)) + header[end:]
    if args.check:
        if expected != header:
            raise SystemExit("sr_profile.h differs from sr_profile.rs")
    else:
        HEADER.write_text(expected, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
