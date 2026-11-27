# SPDX-License-Identifier: MIT
"""Apply a ReScaleFrame module dump sidecar (schema 2) to an IDA 9.x database.

Run inside IDA (File > Script file, or `ida -S"apply_ida.py sidecar.json [census-summary.json]"`).
The sidecar path may also come from the RSF_DUMP_SIDECAR environment variable and the optional
vendor census summary from RSF_DUMP_CENSUS; otherwise IDA asks for them.

It names import slots `__imp_<name>`, names vtables `vtbl_<Class>`, creates functions from the
exception directory, applies the optional caller name list and the optional census summary.
Everything is keyed by RVA, so the database image base only has to be the module's base in that
database. The planning code is `rsf_dump_plan.py` beside this script and can be checked without
IDA: `python apply_ida.py --self-test sidecar.json`.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rsf_dump_plan as plan_core  # noqa: E402
from rsf_dump_plan import DEFAULT_NAME, parse_int  # noqa: E402

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
    plan_core.report(counts)
    return counts


def main(argv):
    return plan_core.main(argv, "apply_ida.py", _arguments, apply_plan)


if __name__ == "__main__":
    sys.exit(main(sys.argv))