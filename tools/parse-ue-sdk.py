"""Parse a dumped Unreal reflection SDK into a machine-readable index.

An SDK dump is generated from the game's own reflection data at runtime, so its class layouts,
field offsets and function name lists describe the shipped build exactly, which engine source
cannot. This tool turns the generated headers into JSON so other tools can consume them.

It reads three kinds of file per module:

  <Module>_classes.h    UClass layouts, their parent, size and every property offset
  <Module>_structs.h    UScriptStruct layouts and reflected enums
  <Module>_functions.cpp  every reflected UFunction with its flags

What it records is what the dumper recorded. A field marked `MISSED OFFSET` is padding the
dumper could not attribute, not a known member, and a layout is only valid for the game build the
dump came from.
"""

import argparse
import json
import re
from pathlib import Path

# `// Class Engine.Actor` or `// ScriptStruct Renderer.LightPropagationVolumeSettings`
REFLECTED_RE = re.compile(r"^//\s+(?P<kind>Class|ScriptStruct)\s+(?P<package>[\w.]+)\.(?P<name>[\w]+)\s*$")

# `// 0x0340 (0x0368 - 0x0028)` on classes, `// 0x0040` on structs.
SIZE_RE = re.compile(
    r"^//\s+0x(?P<size>[0-9A-Fa-f]+)"
    r"(?:\s+\(0x(?P<total>[0-9A-Fa-f]+)\s+-\s+0x(?P<inherited>[0-9A-Fa-f]+)\))?\s*$")

DECL_RE = re.compile(
    r"^(?P<keyword>class|struct)\s+(?P<name>[A-Za-z_]\w*)"
    r"(?:\s*:\s*public\s+(?P<parent>[A-Za-z_]\w*))?\s*$")

# `	float   CustomTimeDilation;   // 0x0080(0x0004) (BlueprintVisible, ...)`
FIELD_RE = re.compile(
    r"^\s+(?P<decl>.+?);\s*//\s*0x(?P<offset>[0-9A-Fa-f]+)"
    r"\(0x(?P<size>[0-9A-Fa-f]+)\)(?P<rest>.*)$")

# Split a declaration into its type and its member name, keeping bitfield width and array extent.
NAME_RE = re.compile(
    r"^(?P<type>.*?)\s+(?P<name>[A-Za-z_]\w*)"
    r"(?P<array>(?:\[[^\]]*\])*)"
    r"(?:\s*:\s*(?P<bits>\d+))?$")

ENUM_RE = re.compile(r"^enum class\s+(?P<name>[A-Za-z_]\w*)\s*:\s*(?P<base>[\w:]+)\s*$")
ENUM_VALUE_RE = re.compile(r"^\s+(?P<name>[A-Za-z_]\w*)\s*=\s*(?P<value>-?\d+)\s*,?\s*$")

# `// Function Engine.Actor.WasRecentlyRendered`
FUNCTION_RE = re.compile(r"^//\s+Function\s+(?P<package>[\w.]+)\.(?P<owner>[\w]+)\.(?P<name>[\w]+)\s*$")
FLAGS_RE = re.compile(r"^//\s+\((?P<flags>[^)]*)\)\s*$")

# The definition that follows a function comment block names the C++ class the dumper emitted.
DEFINITION_RE = re.compile(r"^[\w:<>*&\s]+?(?P<cls>[A-Za-z_]\w*)::(?P<fn>[A-Za-z_~]\w*)\s*\(")


def split_declaration(text):
    """Return type, array extent and bitfield width for one member declaration."""
    match = NAME_RE.match(text.strip())
    if not match:
        return None
    return {
        "type": match.group("type").strip(),
        "name": match.group("name"),
        "array": match.group("array") or "",
        "bits": int(match.group("bits")) if match.group("bits") else None,
    }


def parse_flag_list(text):
    return [flag.strip() for flag in text.split(",") if flag.strip()]


def parse_layout_file(path, records, enums):
    """Read one _classes.h or _structs.h into records keyed by C++ name."""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    pending = None       # the `// Class Package.Name` header waiting for its declaration
    pending_size = None
    current = None
    current_enum = None

    for line in lines:
        reflected = REFLECTED_RE.match(line)
        if reflected:
            pending = reflected.groupdict()
            pending_size = None
            continue

        size = SIZE_RE.match(line)
        if size and pending:
            pending_size = size.groupdict()
            continue

        enum = ENUM_RE.match(line)
        if enum:
            current_enum = {"name": enum.group("name"), "base": enum.group("base"),
                            "module": path.name, "values": {}}
            enums[enum.group("name")] = current_enum
            continue

        if current_enum is not None:
            value = ENUM_VALUE_RE.match(line)
            if value:
                current_enum["values"][value.group("name")] = int(value.group("value"))
                continue
            if line.startswith("}"):
                current_enum = None
            continue

        declaration = DECL_RE.match(line)
        if declaration:
            name = declaration.group("name")
            record = {
                "name": name,
                "kind": pending["kind"] if pending else declaration.group("keyword"),
                "reflected": f"{pending['package']}.{pending['name']}" if pending else None,
                "parent": declaration.group("parent"),
                "module": path.name.rsplit("_", 1)[0],
                "fields": [],
            }
            if pending_size:
                record["size"] = int(pending_size["size"], 16)
                if pending_size["total"]:
                    record["total_size"] = int(pending_size["total"], 16)
                    record["inherited_size"] = int(pending_size["inherited"], 16)
            records[name] = record
            current = record
            pending = None
            pending_size = None
            continue

        if current is None:
            continue

        if line.startswith("}"):
            current = None
            continue

        field = FIELD_RE.match(line)
        if not field:
            continue
        parts = split_declaration(field.group("decl"))
        if not parts:
            continue
        entry = {
            "name": parts["name"],
            "type": parts["type"],
            "offset": int(field.group("offset"), 16),
            "size": int(field.group("size"), 16),
        }
        if parts["array"]:
            entry["array"] = parts["array"]
        if parts["bits"] is not None:
            entry["bits"] = parts["bits"]
        rest = field.group("rest").strip()
        flags = FLAGS_RE.match("// " + rest) if rest.startswith("(") else None
        if flags:
            entry["flags"] = parse_flag_list(flags.group("flags"))
        if "MISSED OFFSET" in rest:
            entry["padding"] = True
        current["fields"].append(entry)

    return records


def parse_functions_file(path, functions):
    """Read one _functions.cpp into a list of reflected function records."""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    pending = None
    for line in lines:
        header = FUNCTION_RE.match(line)
        if header:
            pending = {
                "package": header.group("package"),
                "owner": header.group("owner"),
                "name": header.group("name"),
                "module": path.name.rsplit("_", 1)[0],
                "flags": [],
            }
            functions.append(pending)
            continue
        if pending is None:
            continue
        flags = FLAGS_RE.match(line)
        if flags and not pending["flags"]:
            pending["flags"] = parse_flag_list(flags.group("flags"))
            continue
        definition = DEFINITION_RE.match(line)
        # The dumper emits reflected statics as `STATIC_<Name>` to avoid clashing with members.
        if definition and definition.group("fn") in (pending["name"], "STATIC_" + pending["name"]):
            pending["cpp_class"] = definition.group("cls")
            pending = None
    return functions


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("sdk", type=Path, help="directory holding the generated SDK headers")
    parser.add_argument("output", type=Path, help="JSON index to write")
    arguments = parser.parse_args()

    records = {}
    enums = {}
    functions = []

    for path in sorted(arguments.sdk.glob("*_classes.h")):
        parse_layout_file(path, records, enums)
    for path in sorted(arguments.sdk.glob("*_structs.h")):
        parse_layout_file(path, records, enums)
    for path in sorted(arguments.sdk.glob("*_functions.cpp")):
        parse_functions_file(path, functions)

    # A function whose body the dumper did not emit, such as a delegate, still has an owner: the
    # reflected `Package.Class` name identifies it without needing a definition line.
    by_reflected = {record["reflected"]: name for name, record in records.items()
                    if record.get("reflected")}
    for entry in functions:
        if "cpp_class" in entry:
            continue
        owner = by_reflected.get(f"{entry['package']}.{entry['owner']}")
        if owner:
            entry["cpp_class"] = owner
            entry["inferred_owner"] = True

    # Group functions by the C++ class that owns them. The native ones are the set that appears in
    # the binary's own registration tables, which is what makes an address match possible.
    by_class = {}
    unresolved = 0
    for entry in functions:
        owner = entry.get("cpp_class")
        if owner is None:
            unresolved += 1
            continue
        bucket = by_class.setdefault(owner, {"native": [], "all": []})
        bucket["all"].append(entry["name"])
        if "Native" in entry["flags"]:
            bucket["native"].append(entry["name"])

    classes = {name: record for name, record in records.items() if record["kind"] == "Class"}
    structs = {name: record for name, record in records.items() if record["kind"] != "Class"}

    index = {
        "sdk": str(arguments.sdk),
        "classes": classes,
        "structs": structs,
        "enums": enums,
        "functions": functions,
        "functions_by_class": by_class,
    }
    arguments.output.write_text(json.dumps(index, indent=1), encoding="utf-8")

    native_total = sum(len(bucket["native"]) for bucket in by_class.values())
    print(f"classes            {len(classes)}")
    print(f"structs            {len(structs)}")
    print(f"enums              {len(enums)}")
    print(f"functions          {len(functions)}")
    print(f"  native           {native_total} across {len(by_class)} classes")
    print(f"  without a C++ definition {unresolved}")
    print(f"fields             {sum(len(r['fields']) for r in records.values())}")
    print(f"written            {arguments.output}")


if __name__ == "__main__":
    main()
