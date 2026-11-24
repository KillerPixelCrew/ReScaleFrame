# SPDX-License-Identifier: MIT
# @category ReScaleFrame
"""Apply a ReScaleFrame module dump sidecar (schema 2) to a Ghidra 12 program.

A PyGhidra script: run it from Script Manager (it asks for the sidecar and an optional census
summary) or headless with
`analyzeHeadless <project> <name> -process Game.exe.dump -postScript apply_ghidra.py sidecar.json [census-summary.json]`.
The paths may also come from RSF_DUMP_SIDECAR and RSF_DUMP_CENSUS.

It names import slots `__imp_<name>`, names vtables `vtbl_<Class>`, creates functions from the
exception directory, applies the optional caller name list and the optional census summary.
Everything is keyed by RVA, so the program's image base only has to be the module's base in that
program. The planning code is `rsf_dump_plan.py` beside this script and can be checked without
Ghidra: `python apply_ghidra.py --self-test sidecar.json`.
"""

import os
import sys

# Ghidra injects `sourceFile` when the script runs inside it; `__file__` is set everywhere else.
_HERE = os.path.dirname(os.path.abspath(
    globals().get("__file__") or globals()["sourceFile"].getAbsolutePath()))
sys.path.insert(0, _HERE)
import rsf_dump_plan as plan_core  # noqa: E402
from rsf_dump_plan import DEFAULT_NAME, parse_int  # noqa: E402

# ---------------------------------------------------------------------------
# Ghidra 12 (PyGhidra) backend. The script globals (currentProgram, createFunction, ...) are
# injected by Ghidra and do not exist when the file runs as a plain Python program.
# ---------------------------------------------------------------------------

def _arguments(script):
    arguments = list(script.getScriptArgs())
    sidecar = arguments[0] if arguments else os.environ.get("RSF_DUMP_SIDECAR")
    census = arguments[1] if len(arguments) > 1 else os.environ.get("RSF_DUMP_CENSUS")
    if not sidecar:
        sidecar = str(script.askFile("Module dump sidecar (*.dump.json)", "Open").getAbsolutePath())
        if script.askYesNo("ReScaleFrame dump", "Apply a vendor census summary as well?"):
            census = str(script.askFile("census-summary.json", "Open").getAbsolutePath())
    return sidecar, census


def apply_plan(script, plan, sidecar):
    from ghidra.program.model.address import AddressSet
    from ghidra.program.model.data import PointerDataType
    from ghidra.program.model.symbol import SourceType
    from ghidra.program.model.listing import CodeUnit

    program = script.getCurrentProgram()
    image_base = program.getImageBase()
    load_base = parse_int(sidecar.get("module", {}).get("load_base", image_base.getOffset()))
    if load_base != image_base.getOffset():
        print("[rsf-dump] program base 0x%x differs from dump load base 0x%x; applying by RVA"
              % (image_base.getOffset(), load_base))

    memory = program.getMemory()
    symbols = program.getSymbolTable()
    functions = program.getFunctionManager()
    listing = program.getListing()
    counts = dict(imports=0, vtables=0, functions=0, names=0, comments=0, failed=0)
    monitor = script.getMonitor()

    def at(rva):
        try:
            address = image_base.add(rva)
        except Exception:
            return None
        return address if memory.contains(address) else None

    def label(address, name):
        try:
            symbol = symbols.createLabel(address, name, SourceType.USER_DEFINED)
            symbol.setPrimary()
            return True
        except Exception:
            return False

    for rva, name in plan.imports:
        address = at(rva)
        if address is not None and label(address, name):
            try:
                script.createData(address, PointerDataType.dataType)
            except Exception:
                pass  # data already defined there
            counts["imports"] += 1
        else:
            counts["failed"] += 1

    for rva, name, comment in plan.vtables:
        address = at(rva)
        if address is not None and label(address, name):
            listing.setComment(address, CodeUnit.PLATE_COMMENT, comment)
            counts["vtables"] += 1
        else:
            counts["failed"] += 1

    monitor.setMessage("Creating functions from the exception directory")
    monitor.initialize(len(plan.functions))
    for begin, end in plan.functions:
        monitor.checkCancelled()
        monitor.incrementProgress(1)
        address = at(begin)
        if address is None or functions.getFunctionContaining(address) is not None:
            continue
        try:
            script.disassemble(address)
            created = script.createFunction(address, None)
            if created is None:
                last = at(end - 1)
                if last is not None:
                    created = functions.createFunction(
                        None, address, AddressSet(address, last), SourceType.USER_DEFINED)
            if created is not None:
                counts["functions"] += 1
            else:
                counts["failed"] += 1
        except Exception:
            counts["failed"] += 1

    for rva, name in plan.names:
        address = at(rva)
        if address is not None and label(address, name):
            counts["names"] += 1
        else:
            counts["failed"] += 1

    for rva, text in plan.comments:
        address = at(rva)
        if address is not None:
            listing.setComment(address, CodeUnit.EOL_COMMENT, text)
            counts["comments"] += 1
    for rva, name in plan.census_names:
        address = at(rva)
        function = functions.getFunctionContaining(address) if address is not None else None
        if function is not None and DEFAULT_NAME.match(function.getName()):
            try:
                function.setName(name, SourceType.USER_DEFINED)
            except Exception:
                pass

    plan_core.report(counts)
    return counts


class _Env(object):
    """Reads the names Ghidra injects into the script namespace (currentProgram, monitor,
    askFile, createFunction, ...) at call time, so the module also imports outside Ghidra."""

    def getCurrentProgram(self):
        return globals()["currentProgram"]

    def getMonitor(self):
        return globals()["monitor"]

    def __getattr__(self, name):
        return globals()[name]


def main(argv):
    env = _Env()
    return plan_core.main(
        argv, "apply_ghidra.py", lambda: _arguments(env),
        lambda plan, sidecar: apply_plan(env, plan, sidecar))


if "currentProgram" in globals():
    main([])  # running inside Ghidra: PyGhidra executes the script body
elif __name__ == "__main__":
    sys.exit(main(sys.argv))