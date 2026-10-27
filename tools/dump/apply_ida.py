# SPDX-License-Identifier: GPL-3.0-only
"""Apply a ReScaleFrame module dump sidecar (schema 2) to an IDA 9.x database.

Run inside IDA (File > Script file, or `ida -S"apply_ida.py sidecar.json [census-summary.json]"`).
The sidecar path may also come from the RSF_DUMP_SIDECAR environment variable and the optional
vendor census summary from RSF_DUMP_CENSUS; otherwise IDA asks for them.

It names import slots `__imp_<name>`, names vtables `vtbl_<Class>`, creates functions from the
exception directory, applies the optional caller name list and the optional census summary.
Everything is keyed by RVA, so the database image base only has to be the module's base in that
database. The pure planning code is shared verbatim with apply_ghidra.py and can be checked
without IDA: `python apply_ida.py --self-test sidecar.json`.
"""

import json
import os
import re
import sys

# ---------------------------------------------------------------------------
# Shared planning core. Keep identical in apply_ida.py and apply_ghidra.py.
# ---------------------------------------------------------------------------

DEFAULT_NAME = re.compile(r"^(sub|loc|off|unk|byte|word|dword|qword|FUN|DAT|LAB|PTR)_[0-9a-fA-F]+$")


def parse_int(value):
    """Accept an int, '0x..' hex or decimal text."""
    if isinstance(value, bool):
        raise ValueError("boolean is not a number")
    if isinstance(value, int):
        return value
    text = str(value).strip()
    if text.lower().startswith("0x"):
        return int(text, 16)
    return int(text, 10)


def sanitize(name):
    """A name every disassembler accepts: letters, digits and underscores."""
    return re.sub(r"[^0-9A-Za-z_]+", "_", name).strip("_") or "unnamed"


def rebase(address, sidecar_base, database_base):
    """Move an absolute address recorded at dump time to the database's image base."""
    return address - sidecar_base + database_base


class Plan(object):
    def __init__(self):
        self.imports = []    # (rva, name)
        self.vtables = []    # (rva, name, comment)
        self.functions = []  # (begin_rva, end_rva)
        self.names = []      # (rva, name)
        self.comments = []   # (rva, text)
        self.census_names = []  # (rva, name): only applied while the target is still unnamed
        self.warnings = []


def _unique(name, used):
    candidate, index = name, 2
    while candidate in used:
        candidate = "%s_%d" % (name, index)
        index += 1
    used.add(candidate)
    return candidate


def _census_entries(census):
    if census is None:
        return []
    if isinstance(census, dict):
        for key in ("entries", "sites", "calls"):
            if isinstance(census.get(key), list):
                return census[key]
        return []
    return census if isinstance(census, list) else []


def build_plan(sidecar, census=None):
    plan = Plan()
    module = sidecar.get("module", {})
    base = parse_int(module.get("load_base", 0))
    module_leaf = os.path.basename(str(module.get("path", "")).replace("\\", "/")).lower()
    used = set()

    for item in sidecar.get("imports", []):
        slot = item.get("slot_rva")
        if slot is None:
            continue
        if "name" in item:
            label = "__imp_" + sanitize(item["name"])
        else:
            dll = os.path.splitext(str(item.get("dll", "dll")))[0].lower()
            label = "__imp_%s_ord%d" % (sanitize(dll), parse_int(item.get("ordinal", 0)))
        plan.imports.append((parse_int(slot), _unique(label, used)))

    rtti = sidecar.get("rtti")
    if rtti is None:
        plan.warnings.append("sidecar has no rtti section (schema 1 dump)")
    else:
        if not rtti.get("classes") and rtti.get("reason"):
            plan.warnings.append("rtti: " + str(rtti["reason"]))
        for entry in rtti.get("classes", []):
            name = "vtbl_" + sanitize(entry.get("name") or entry.get("decorated", "unnamed"))
            if entry.get("offset"):
                name += "_off%d" % entry["offset"]
            bases = [b.get("name", "?") for b in entry.get("bases", [])]
            comment = "RTTI %s" % (entry.get("name") or entry.get("decorated", ""))
            if bases:
                comment += " : " + ", ".join(bases)
            plan.vtables.append((parse_int(entry["vtable_rva"]), _unique(name, used), comment))

    functions = sidecar.get("functions")
    if functions is None:
        plan.warnings.append("sidecar has no functions section (schema 1 dump)")
    else:
        for begin, end, _unwind, chained in functions.get("entries", []):
            if not chained:
                plan.functions.append((begin, end))

    names = sidecar.get("names")
    if names:
        for entry in names.get("entries", []):
            plan.names.append((parse_int(entry["rva"]), sanitize(entry["name"])))

    for entry in _census_entries(census):
        owner = os.path.basename(str(entry.get("module", "")).replace("\\", "/")).lower()
        if owner and module_leaf and owner != module_leaf:
            continue
        try:
            if "rva" in entry:
                rva = parse_int(entry["rva"])
            elif "va" in entry:
                rva = parse_int(entry["va"]) - base
            else:
                continue
        except (ValueError, TypeError):
            continue
        text = "census: %s x%s" % (entry.get("api", "?"), entry.get("count", "?"))
        function = entry.get("function")
        if function:
            text += " in %s" % function
            if isinstance(function, str) and not DEFAULT_NAME.match(function) \
                    and re.match(r"^[A-Za-z_][0-9A-Za-z_:<>~]*$", function):
                plan.census_names.append((rva, sanitize(function)))
        plan.comments.append((rva, text))
    return plan


def load_json(path):
    with open(path, "r", encoding="utf-8") as stream:
        return json.load(stream)


def self_test(sidecar_path):
    assert parse_int("0x10") == 16 and parse_int("16") == 16 and parse_int(16) == 16
    assert sanitize("Foo::Bar<int>") == "Foo_Bar_int" and sanitize("`anonymous namespace'::X") == \
        "anonymous_namespace_X"
    assert rebase(0x140001000, 0x140000000, 0x7FF700000000) == 0x7FF700001000
    sidecar = load_json(sidecar_path)
    plan = build_plan(sidecar)
    synthetic = {
        "module": {"path": "C:\\g\\Game.exe", "load_base": "0x140000000"},
        "imports": [{"dll": "K.dll", "name": "A", "slot_rva": "0x10"},
                    {"dll": "K.dll", "name": "A", "slot_rva": "0x18"},
                    {"dll": "ws2_32.dll", "ordinal": 3, "slot_rva": "0x20"}],
        "rtti": {"classes": [{"vtable_rva": "0x100", "name": "N::Foo", "offset": 0,
                              "bases": [{"name": "Base"}]},
                             {"vtable_rva": "0x200", "name": "N::Foo", "offset": 8, "bases": []}]},
        "functions": {"entries": [[4096, 4200, 9000, 0], [4300, 4400, 9020, 1]]},
        "names": {"entries": [{"rva": "0x300", "name": "Engine::Tick"}]},
    }
    census = {"entries": [
        {"module": "game.exe", "rva": "0x1010", "api": "CreateFileW", "function": "Loader", "count": 3},
        {"module": "Game.exe", "va": "0x140001020", "api": "ReadFile", "function": "sub_1020",
         "count": 1},
        {"module": "Other.dll", "rva": "0x1", "api": "X", "count": 1}]}
    check = build_plan(synthetic, census)
    assert [n for _r, n in check.imports] == ["__imp_A", "__imp_A_2", "__imp_ws2_32_ord3"]
    assert [(r, n) for r, n, _c in check.vtables] == [(0x100, "vtbl_N_Foo"), (0x200, "vtbl_N_Foo_off8")]
    assert check.functions == [(4096, 4200)], "chained entries are not function starts"
    assert check.names == [(0x300, "Engine_Tick")]
    assert [r for r, _t in check.comments] == [0x1010, 0x1020], check.comments
    assert check.census_names == [(0x1010, "Loader")]
    print("self-test ok: %d imports, %d vtables, %d functions, %d names from %s" %
          (len(plan.imports), len(plan.vtables), len(plan.functions), len(plan.names),
           os.path.basename(sidecar_path)))
    for warning in plan.warnings:
        print("note: " + warning)
    return 0

# ---------------------------------------------------------------------------
# IDA 9.x backend.
# ---------------------------------------------------------------------------


def _arguments():
    import idc
    arguments = [a for a in getattr(idc, "ARGV", [])[1:]]
    sidecar = arguments[0] if arguments else os.environ.get("RSF_DUMP_SIDECAR")
    census = arguments[1] if len(arguments) > 1 else os.environ.get("RSF_DUMP_CENSUS")
    if not sidecar:
        import ida_kernwin
        sidecar = ida_kernwin.ask_file(0, "*.json", "Module dump sidecar (*.dump.json)")
    return sidecar, census


def apply_plan(plan, sidecar):
    import ida_auto
    import ida_bytes
    import ida_funcs
    import ida_ida
    import ida_name
    import ida_nalt

    image_base = ida_nalt.get_imagebase()
    module = sidecar.get("module", {})
    load_base = parse_int(module.get("load_base", image_base))
    if load_base != image_base:
        print("[rsf-dump] database base 0x%x differs from dump load base 0x%x; applying by RVA"
              % (image_base, load_base))
    limit = ida_ida.inf_get_max_ea()

    def at(rva):
        ea = image_base + rva
        return ea if ea < limit else None

    flags = ida_name.SN_FORCE | ida_name.SN_NOWARN | ida_name.SN_NOCHECK
    counts = dict(imports=0, vtables=0, functions=0, names=0, comments=0, failed=0)

    for rva, name in plan.imports:
        ea = at(rva)
        if ea is not None and ida_bytes.create_qword(ea, 8, True) and \
                ida_name.set_name(ea, name, flags):
            counts["imports"] += 1
        else:
            counts["failed"] += 1

    for rva, name, comment in plan.vtables:
        ea = at(rva)
        if ea is not None and ida_name.set_name(ea, name, flags):
            ida_bytes.set_cmt(ea, comment, True)
            counts["vtables"] += 1
        else:
            counts["failed"] += 1

    for begin, end in plan.functions:
        ea = at(begin)
        if ea is None or ida_funcs.get_func(ea) is not None:
            continue
        # add_func marks the bytes as code and follows flow, so it works on a database that has
        # not been through auto-analysis for these starts.
        if ida_funcs.add_func(ea, image_base + end):
            counts["functions"] += 1
        elif ida_funcs.add_func(ea):
            counts["functions"] += 1
        else:
            counts["failed"] += 1

    for rva, name in plan.names:
        ea = at(rva)
        if ea is not None and ida_name.set_name(ea, name, flags):
            counts["names"] += 1
        else:
            counts["failed"] += 1

    for rva, text in plan.comments:
        ea = at(rva)
        if ea is not None:
            ida_bytes.set_cmt(ea, text, False)
            counts["comments"] += 1
    for rva, name in plan.census_names:
        ea = at(rva)
        function = ida_funcs.get_func(ea) if ea is not None else None
        if function is not None:
            current = ida_name.get_name(function.start_ea)
            if DEFAULT_NAME.match(current):
                ida_name.set_name(function.start_ea, name, flags)

    ida_auto.auto_wait()
    print("[rsf-dump] applied %(imports)d imports, %(vtables)d vtables, %(functions)d functions, "
          "%(names)d names, %(comments)d comments; %(failed)d failed" % counts)
    return counts


def main(argv):
    if len(argv) >= 2 and argv[1] == "--self-test":
        if len(argv) != 3:
            print("usage: apply_ida.py --self-test <sidecar.json>")
            return 2
        return self_test(argv[2])
    sidecar_path, census_path = _arguments()
    if not sidecar_path:
        print("[rsf-dump] no sidecar given")
        return 2
    sidecar = load_json(sidecar_path)
    census = load_json(census_path) if census_path else None
    plan = build_plan(sidecar, census)
    for warning in plan.warnings:
        print("[rsf-dump] " + warning)
    apply_plan(plan, sidecar)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
