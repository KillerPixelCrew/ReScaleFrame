"""Project the verified Rust profiles into the native compile-time consumers."""
import argparse
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1] / "games/anomaly/src"
PROFILE = ROOT / "native_profile.rs"
HEADER = ROOT / "native_profile.h"
START = "// BEGIN GENERATED ANOMALY PROFILES"
END = "// END GENERATED ANOMALY PROFILES"
FIELDS = ("image_size", "device_width_rva", "device_height_rva", "device_frame_rva")
BUILDS = (("standard", "Standard", "dx11_hash"), ("avx", "AVX", "avx_hash"))


def project(source):
    count = int(re.search(r"const HOOK_COUNT: usize = (\d+);", source)[1])
    labels = re.findall(r'Profile \{\s+label: c"([^"]+)"', source)
    if labels != [label for label, _, _ in BUILDS]:
        raise ValueError("Rust profile order must match the native BuildProfile indices")
    lines = [START, "// Generated from native_profile.rs by tools/generate-anomaly-native-profile.py."]
    for label, display, hash_name in BUILDS:
        region = re.search(rf'Profile {{\s+label: c"{label}",(.*?)\n    }},', source, re.S)[1]
        exe = re.search(r'executable: c"([A-Za-z0-9.]+)"', region)[1]
        digest = re.search(r'sha256: c"([0-9a-f]{64})"', region)[1]
        fields = {field: re.search(rf'{field}: (0x[0-9a-f]+),', region)[1] for field in FIELDS}
        sites = re.findall(r'\bsite\(\s*c"([^"]+)",\s*(0x[0-9a-f]+),\s*b"([0-9a-f]{32})",?\s*\)', region)
        guards = re.findall(r'UniqueGuard::new\(\s*c"([^"]+)",\s*(0x[0-9a-f]+),\s*c"([0-9a-f]+)",?\s*\)', region)
        if len(sites) != count or len(guards) != 2:
            raise ValueError(f"{label} profile must supply {count} sites and two extended guards")
        for name, address, extended in guards:
            prefix = next((guard for site, rva, guard in sites if (site, rva) == (name, address)), None)
            if prefix is None or not extended.startswith(prefix) or len(extended) <= len(prefix) or len(extended) % 2:
                raise ValueError(f"{label} extended guard does not match {name}")
        lines.extend([
            f'// ---- {display} 1.5.3 DX11 ({exe}) ----',
            f'inline constexpr char {label}_label[] = "{label}";',
            f'inline constexpr char {label}_executable[] = "{exe}";',
            f'inline constexpr char {hash_name}[] = "{digest}";',
            f'inline constexpr std::array<UniqueGuard, {len(guards)}> native_{label}_unique{{{{',
        ])
        lines.extend(f'    {{"{name}", {rva}, "{guard}"}},' for name, rva, guard in guards)
        lines.extend(["}};", f"inline constexpr NativeProfile native_{label}{{"])
        lines.extend(f"    .{name} = {value}," for name, value in fields.items())
        lines.append("    .native_sites = {{")
        lines.extend(f'        {{"{name}", {rva}, signature("{guard}")}},' for name, rva, guard in sites)
        lines.extend(["    }},", f"    .native_unique_guards = native_{label}_unique,", "};", ""])
    return count, "\n".join([*lines, END])


GEOMETRY_PROFILE = ROOT / "geometry_profile.rs"
GEOMETRY_HEADER = ROOT / "geometry_profile.h"
GEOMETRY_START = "// BEGIN GENERATED ANOMALY GEOMETRY PROFILES"
GEOMETRY_END = "// END GENERATED ANOMALY GEOMETRY PROFILES"
GEOMETRY_SCALARS = ("render_phase_rva", "second_viewport_rva", "world_matrix_rva", "vertex_constant_buffers_rva")
GEOMETRY_BUILDS = (
    ("standard", "Standard", "AnomalyDX11.exe"),
    ("avx", "AVX", "AnomalyDX11AVX.exe"),
)
GEOMETRY_AVX_NOTE = (
    "// CSkeletonX_PM::Render, render_hud/sorted/emissive, rmNear/rmNormal and the FTreeVisual_PM last_lod load contain VEX\n"
    "// instructions in their guard bytes; every detour's first instructions (the bytes MinHook relocates) are integer code."
)


def geometry_blocks(source):
    """Split the Rust geometry profile into one record per build, in native order."""
    body = source[source.index("PROFILES: [Profile; 2] = ["):]
    parts = re.split(r"\n    Profile \{\n", body)[1:]
    if len(parts) != len(GEOMETRY_BUILDS):
        raise ValueError("Rust geometry profile order must match the native BuildProfile indices")
    return parts


def project_geometry(source):
    site = r'site\(\s*c"([^"]+)",\s*(0x[0-9a-f]+),\s*b"([0-9a-f]{32})",?\s*\)'
    lines = [GEOMETRY_START, "// Generated from geometry_profile.rs by tools/generate-anomaly-native-profile.py."]
    counts = [int(m[2]) for m in re.finditer(r"pub\(crate\) const (SITE_COUNT|LAYOUT_COUNT|UNIQUE_COUNT): usize = (\d+);", source)]
    if counts != [17, 7, 9]:
        raise ValueError("geometry profile counts changed; update geometry_profile.h declarations")
    for (label, display, exe), block in zip(GEOMETRY_BUILDS, geometry_blocks(source)):
        sites = re.search(r"sites: \[(.*?)\n        \],\n        layout_sites", block, re.S)[1]
        layout = re.search(r"layout_sites: \[(.*?)\n        \],\n        unique", block, re.S)[1]
        unique = re.search(r"unique: \[(.*?)\n        \],\n        render_phase_rva", block, re.S)[1]
        sites, layout = re.findall(site, sites), re.findall(site, layout)
        unique = re.findall(r'UniqueGuard::new\(\s*c"([^"]+)",\s*(0x[0-9a-f]+),\s*c"([0-9a-f]+)",?\s*\)', unique)
        scalars = {name: re.search(rf"{name}: (0x[0-9a-f]+),", block)[1] for name in GEOMETRY_SCALARS}
        vtables = re.search(r"hierarchy_vtables: \[([^\]]*)\]", block)[1]
        if (len(sites), len(layout), len(unique)) != (17, 7, 9):
            raise ValueError(f"{label} geometry profile has the wrong record counts")
        lines.append(f"// ---- {display} 1.5.3 DX11 ({exe}) " + "-" * (111 - len(f"{display} 1.5.3 DX11 ({exe}) ")))
        if label == "avx":
            lines.append(GEOMETRY_AVX_NOTE)
        lines.append(f"inline constexpr std::array<UniqueGuard, 9> geometry_{label}_unique{{{{")
        lines.extend(f'    {{"{name}", {rva}, "{guard}"}},' for name, rva, guard in unique)
        lines.extend(["}};", f"inline constexpr GeometryProfile geometry_{label}{{", "    .geometry_sites = {{"])
        lines.extend(f'        {{"{name}", {rva}, signature("{guard}")}},' for name, rva, guard in sites)
        lines.extend(["    }},", "    .geometry_layout_sites = {{"])
        lines.extend(f'        {{"{name}", {rva}, signature("{guard}")}},' for name, rva, guard in layout)
        lines.extend(["    }},", f"    .geometry_unique_guards = geometry_{label}_unique,"])
        lines.extend(f"    .{name} = {value}," for name, value in scalars.items())
        lines.extend([f"    .hierarchy_vtables = {{{vtables}}},", "};", ""])
    return "\n".join([*lines, GEOMETRY_END])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="refuse if the native projection has drifted")
    args = parser.parse_args()
    header = HEADER.read_text(encoding="utf-8")
    start = header.index(START)
    end = header.index(END, start) + len(END)
    count, projection = project(PROFILE.read_text(encoding="utf-8"))
    declarations = re.sub(
        r"native_site_count = \d+;", f"native_site_count = {count};", header[:start]
    )
    expected = declarations + projection + header[end:]
    geometry = GEOMETRY_HEADER.read_text(encoding="utf-8")
    start = geometry.index(GEOMETRY_START)
    end = geometry.index(GEOMETRY_END, start) + len(GEOMETRY_END)
    geometry_expected = (
        geometry[:start] + project_geometry(GEOMETRY_PROFILE.read_text(encoding="utf-8")) + geometry[end:]
    )
    if args.check:
        if expected != header:
            raise SystemExit("native_profile.h differs from native_profile.rs")
        if geometry_expected != geometry:
            raise SystemExit("geometry_profile.h differs from geometry_profile.rs")
    else:
        HEADER.write_text(expected, encoding="utf-8", newline="\n")
        GEOMETRY_HEADER.write_text(geometry_expected, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
