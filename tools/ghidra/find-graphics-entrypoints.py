# Locate the DX11 and DXGI entry points in a Ghidra program and decode COM vtable calls.
#
# Run inside Ghidra with PyGhidra, from the Script Manager or headless. Plain analyzeHeadless
# cannot run Python scripts, so headless goes through PyGhidra's launcher:
#   python -m pyghidra.ghidra_launch --install-dir "$GHIDRA_INSTALL_DIR" \
#       ghidra.app.util.headless.AnalyzeHeadless <project_dir> <project> -process <program> \
#       -noanalysis -postScript find-graphics-entrypoints.py \
#       --vtables .local/ghidra/directx-vtables.json --output .local/ghidra/ac7-graphics.json
#
# The search runs in three widening steps: the functions that call the DX11/DXGI creation imports,
# the functions that reference the objects those calls store, then optional caller/callee levels.
# A resolved vtable slot is a lead about which method an offset belongs to, not proof that the
# path executes or that a hook there is viable.
# @category ReScaleFrame

import argparse
import json
import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.program.model.pcode import PcodeOp
from ghidra.util.task import ConsoleTaskMonitor


IMPORTS = [
    "D3D11CreateDevice", "D3D11CreateDeviceAndSwapChain", "D3D11On12CreateDevice",
    "CreateDXGIFactory", "CreateDXGIFactory1", "CreateDXGIFactory2",
    "DXGIGetDebugInterface", "DXGIGetDebugInterface1",
]

# Interfaces whose slots are worth reporting. Every COM interface shares the IUnknown prefix, so
# an unfiltered lookup returns noise for every offset below 0x18.
INTERFACES = [
    "IDXGIFactory", "IDXGIFactory1", "IDXGIFactory2", "IDXGIFactory3", "IDXGIFactory4",
    "IDXGIFactory5", "IDXGIFactory6", "IDXGIFactory7",
    "IDXGISwapChain", "IDXGISwapChain1", "IDXGISwapChain2", "IDXGISwapChain3", "IDXGISwapChain4",
    "IDXGIAdapter", "IDXGIAdapter1", "IDXGIAdapter2", "IDXGIDevice", "IDXGIDevice1",
    "IDXGIOutput", "IDXGIOutput1", "IDXGISurface", "IDXGIResource",
    "ID3D11Device", "ID3D11Device1", "ID3D11Device2", "ID3D11Device3",
    "ID3D11DeviceContext", "ID3D11DeviceContext1", "ID3D11DeviceContext2", "ID3D11DeviceContext3",
    "ID3D11Texture2D", "ID3D11RenderTargetView", "ID3D11ShaderResourceView",
    "ID3D11DepthStencilView", "ID3D11UnorderedAccessView", "ID3D11Query", "ID3D11Fence",
]

# Argument positions that receive a created interface, for the calls that have no riid argument.
# Counted from one, matching the p-code call inputs after the target.
OUTPUT_PARAMETERS = {
    "D3D11CreateDevice": {8: "ID3D11Device", 10: "ID3D11DeviceContext"},
    "D3D11CreateDeviceAndSwapChain": {9: "IDXGISwapChain", 10: "ID3D11Device",
                                      12: "ID3D11DeviceContext"},
    "D3D11On12CreateDevice": {8: "ID3D11Device", 9: "ID3D11DeviceContext"},
}

MONITOR = ConsoleTaskMonitor()


def parse_arguments(raw):
    parser = argparse.ArgumentParser(prog="find-graphics-entrypoints")
    parser.add_argument("--vtables", default=os.environ.get("RSF_VTABLES", "directx-vtables.json"),
                        help="directx-vtables.json from build-directx-types.py")
    parser.add_argument("--output", default=os.environ.get("RSF_OUTPUT", "graphics-entrypoints.json"))
    parser.add_argument("--depth", type=int, default=1,
                        help="caller and callee levels to expand beyond the located functions")
    parser.add_argument("--limit", type=int, default=400,
                        help="maximum functions to decompile, keeps large programs bounded")
    parser.add_argument("--apply", action="store_true",
                        help="write Ghidra comments at the resolved call sites")
    return parser.parse_args(list(raw))


def load_tables(path):
    """Build an offset lookup limited to the interfaces we care about, plus the IID table."""
    data = json.load(open(path, encoding="utf-8"))
    interfaces = data["interfaces"]
    slots = {}
    for name in INTERFACES:
        for method in interfaces.get(name, []):
            slots.setdefault(int(method["offset"], 16), []).append(
                {"interface": name, "method": method["name"], "slot": method["index"]})
    iids = {key: value for key, value in data.get("iids", {}).items()
            if value["interface"] in interfaces}
    return slots, interfaces, iids


def method_at(interfaces, name, offset):
    """Name the slot at a byte offset in one known interface."""
    for method in interfaces.get(name, []):
        if int(method["offset"], 16) == offset:
            return {"interface": name, "method": method["name"], "slot": method["index"]}
    return None


class Analysis:
    def __init__(self, program, tables, limit):
        self.program = program
        self.slots, self.interfaces, self.iids = tables
        self.limit = limit
        self.functions = program.getFunctionManager()
        self.references = program.getReferenceManager()
        self.memory = program.getMemory()
        self.decompiler = DecompInterface()
        self.decompiler.setOptions(DecompileOptions())
        self.decompiler.openProgram(program)
        self.cache = {}
        self.failures = []
        self.targets = {}

    def address(self, value):
        return self.program.getAddressFactory().getDefaultAddressSpace().getAddress(value)

    def note(self, function, reason):
        """Record a function to decompile, keeping every reason it was reached."""
        if function is None or function.isThunk() or function.isExternal():
            return
        key = str(function.getEntryPoint())
        entry = self.targets.setdefault(key, {"name": function.getName(), "reasons": []})
        if reason not in entry["reasons"]:
            entry["reasons"].append(reason)

    def high_function(self, function):
        key = str(function.getEntryPoint())
        if key not in self.cache:
            results = self.decompiler.decompileFunction(function, 180, MONITOR)
            high = results.getHighFunction()
            if high is None:
                self.failures.append({"function": function.getName(), "address": key,
                                      "error": results.getErrorMessage()})
            self.cache[key] = high
        return self.cache[key]

    def import_addresses(self, name):
        """Every address a call to this import can name.

        Depending on how the program was imported and analysed, the target can be an external
        function, a bare label in the EXTERNAL block, the import address table slot that points
        at it, or a thunk in the code. All of them have to count as the same import.
        """
        table = self.program.getSymbolTable()
        # Import symbols sit in a library namespace, so a global-only lookup does not see them.
        pending = [symbol.getAddress() for symbol in table.getSymbols(name)]
        for function in self.functions.getFunctions(True):
            if function.getName() == name:
                pending.append(function.getEntryPoint())
        found = set()
        while pending:
            address = pending.pop()
            if address in found:
                continue
            found.add(address)
            for reference in self.references.getReferencesTo(address):
                origin = reference.getFromAddress()
                holder = self.functions.getFunctionContaining(origin)
                if holder is None:
                    pending.append(origin)  # an import table slot or another pointer to it
                elif holder.isThunk():
                    pending.append(holder.getEntryPoint())
        return found

    def constant_operand(self, varnode):
        """Resolve a varnode to a program address if it names static storage."""
        if varnode is None:
            return None
        source = varnode.getDef()
        if source is not None and source.getOpcode() in (PcodeOp.COPY, PcodeOp.CAST):
            return self.constant_operand(source.getInput(0))
        if source is not None and source.getOpcode() == PcodeOp.PTRSUB:
            base, offset = source.getInput(0), source.getInput(1)
            if base.isConstant() and base.getOffset() == 0 and offset.isConstant():
                varnode = offset
        if not varnode.isConstant() and not varnode.isAddress():
            return None
        try:
            address = self.address(varnode.getOffset())
        except Exception:
            return None
        return address if self.memory.contains(address) else None

    def definition(self, varnode):
        """Follow a varnode past conversions that do not change the value."""
        source = varnode.getDef() if varnode is not None else None
        while source is not None and source.getOpcode() in (PcodeOp.CAST, PcodeOp.COPY):
            varnode = source.getInput(0)
            source = varnode.getDef()
        return source

    def object_of(self, load):
        """Given the load of a vtable pointer, resolve the object it was read from."""
        if load is None or load.getOpcode() != PcodeOp.LOAD:
            return None
        address = self.constant_operand(load.getInput(1))
        return str(address) if address is not None else None

    def vtable_call(self, operation):
        """Decode an indirect call made through a COM vtable.

        The shape the decompiler produces is a load from the vtable at a constant offset, where
        the vtable itself came from a load at the start of the object:
            LOAD(LOAD(object) + offset)
        """
        source = self.definition(operation.getInput(0))
        if source is None or source.getOpcode() != PcodeOp.LOAD:
            return None
        inner = self.definition(source.getInput(1))
        if inner is None:
            return None
        if inner.getOpcode() == PcodeOp.LOAD:
            return {"offset": 0, "object": self.object_of(inner)}
        if inner.getOpcode() not in (PcodeOp.INT_ADD, PcodeOp.PTRADD, PcodeOp.PTRSUB):
            return None
        offset = None
        obj = None
        for index in range(inner.getNumInputs()):
            operand = inner.getInput(index)
            if operand is None:
                continue
            if operand.isConstant() and offset is None:
                scale = 1
                if inner.getOpcode() == PcodeOp.PTRADD and index == 1:
                    scale = inner.getInput(2).getOffset()
                offset = int(operand.getOffset()) * scale
            elif obj is None:
                obj = self.object_of(self.definition(operand))
        return None if offset is None else {"offset": offset, "object": obj}

    def stored_objects(self, function, import_entries, iid_addresses):
        """Find static storage that a creation call receives as an output parameter.

        The interface is taken from a riid argument when the call has one, because that is an
        exact identification. Otherwise it falls back to the documented argument position, which
        only holds if Ghidra recovered the call's arguments correctly.
        """
        high = self.high_function(function)
        objects = []
        if high is None:
            return objects
        for operation in high.getPcodeOps():
            if operation.getOpcode() != PcodeOp.CALL:
                continue
            callee = self.constant_operand(operation.getInput(0))
            name = import_entries.get(str(callee)) if callee is not None else None
            if name is None:
                continue
            positions = OUTPUT_PARAMETERS.get(name, {})
            pending_iid = None
            for index in range(1, operation.getNumInputs()):
                address = self.constant_operand(operation.getInput(index))
                if address is None:
                    continue
                key = str(address)
                if key in iid_addresses:
                    pending_iid = iid_addresses[key]
                    continue
                interface, evidence = None, None
                if pending_iid:
                    interface, evidence = pending_iid, "riid argument"
                    pending_iid = None
                elif index in positions:
                    interface, evidence = positions[index], "argument position"
                objects.append({"address": key, "argument": index, "import": name,
                                "interface": interface, "evidence": evidence,
                                "call_site": str(operation.getSeqnum().getTarget())})
        return objects

    def vtable_calls(self, function, known_objects):
        high = self.high_function(function)
        if high is None:
            return []
        calls = []
        for operation in high.getPcodeOps():
            if operation.getOpcode() != PcodeOp.CALLIND:
                continue
            decoded = self.vtable_call(operation)
            if decoded is None:
                continue
            offset = decoded["offset"]
            interface = known_objects.get(decoded["object"])
            exact = method_at(self.interfaces, interface, offset) if interface else None
            calls.append({
                "address": str(operation.getSeqnum().getTarget()),
                "vtable_offset": hex(offset),
                "slot": offset // 8,
                "object": decoded["object"],
                "interface": interface,
                "method": exact["method"] if exact else None,
                "candidates": [exact] if exact else self.slots.get(offset, []),
                "resolved": bool(exact) or bool(self.slots.get(offset, [])),
            })
        return calls

    def find_iid_constants(self):
        """Locate every known interface ID that appears verbatim in the program's data.

        A 16 byte GUID match is an exact identification of the interface, unlike a string lead.
        """
        import jpype

        patterns = {bytes.fromhex(key): value for key, value in self.iids.items()}
        found = []
        for block in self.memory.getBlocks():
            if not block.isInitialized() or block.isExecute():
                continue
            buffer = jpype.JArray(jpype.JByte)(int(block.getSize()))
            self.memory.getBytes(block.getStart(), buffer)
            data = bytes(memoryview(buffer).cast("B"))
            for pattern, value in patterns.items():
                position = data.find(pattern)
                while position >= 0:
                    found.append({"address": str(block.getStart().add(position)),
                                  "block": block.getName(), "interface": value["interface"],
                                  "guid": value["guid"]})
                    position = data.find(pattern, position + 1)
        return found

    def expand(self, levels):
        """Add callers and callees so a create call and its users end up in the same report."""
        frontier = list(self.targets)
        for level in range(levels):
            discovered = []
            for key in frontier:
                if len(self.targets) >= self.limit:
                    return
                function = self.functions.getFunctionAt(self.address(int(key, 16)))
                if function is None:
                    continue
                for related, direction in ((function.getCallingFunctions(MONITOR), "caller"),
                                           (function.getCalledFunctions(MONITOR), "callee")):
                    for neighbour in related:
                        before = len(self.targets)
                        self.note(neighbour, f"{direction} of {function.getName()} (level {level + 1})")
                        if len(self.targets) > before:
                            discovered.append(str(neighbour.getEntryPoint()))
            frontier = discovered


def main():
    args = parse_arguments(getScriptArgs())
    program = currentProgram
    analysis = Analysis(program, load_tables(args.vtables), args.limit)

    iid_constants = analysis.find_iid_constants()
    iid_addresses = {item["address"]: item["interface"] for item in iid_constants}
    if args.apply:
        for item in iid_constants:
            createLabel(analysis.address(int(item["address"], 16)),
                        f"IID_{item['interface']}", True)

    import_entries = {}
    entry_points = []
    for name in IMPORTS:
        addresses = analysis.import_addresses(name)
        if not addresses:
            continue
        sites = []
        for address in addresses:
            import_entries[str(address)] = name
            for reference in analysis.references.getReferencesTo(address):
                caller = analysis.functions.getFunctionContaining(reference.getFromAddress())
                if caller is None or caller.isThunk():
                    continue
                sites.append({"address": str(reference.getFromAddress()),
                              "reference_type": str(reference.getReferenceType()),
                              "function": caller.getName(),
                              "function_address": str(caller.getEntryPoint())})
                analysis.note(caller, f"calls {name}")
        entry_points.append({"import": name, "entry_addresses": sorted(str(a) for a in addresses),
                             "call_sites": sites})

    objects = []
    for key in list(analysis.targets):
        function = analysis.functions.getFunctionAt(analysis.address(int(key, 16)))
        objects.extend(analysis.stored_objects(function, import_entries, iid_addresses))
    known_objects = {stored["address"]: stored["interface"] for stored in objects
                     if stored["interface"]}
    for stored in objects:
        address = analysis.address(int(stored["address"], 16))
        for reference in analysis.references.getReferencesTo(address):
            user = analysis.functions.getFunctionContaining(reference.getFromAddress())
            analysis.note(user, f"references {stored['import']} output at {stored['address']}")

    analysis.expand(args.depth)

    indirect = []
    for key, entry in sorted(analysis.targets.items()):
        function = analysis.functions.getFunctionAt(analysis.address(int(key, 16)))
        if function is None:
            continue
        for call in analysis.vtable_calls(function, known_objects):
            call["function"] = entry["name"]
            call["function_address"] = key
            call["reasons"] = entry["reasons"]
            indirect.append(call)
            if args.apply and call["resolved"]:
                summary = ", ".join(f"{item['interface']}::{item['method']}"
                                    for item in call["candidates"])
                qualifier = "" if call["interface"] else " (candidates)"
                setEOLComment(analysis.address(int(call["address"], 16)),
                              f"vtable +{call['vtable_offset']}: {summary}{qualifier}")

    analysis.decompiler.dispose()
    report = {
        "program": {"name": program.getName(), "sha256": program.getExecutableSHA256(),
                    "language": str(program.getLanguageID()),
                    "image_base": str(program.getImageBase())},
        "entry_points": entry_points,
        "iid_constants": iid_constants,
        "created_objects": objects,
        "functions_examined": [{"address": key, **entry}
                               for key, entry in sorted(analysis.targets.items())],
        "indirect_calls": indirect,
        "decompile_failures": analysis.failures,
        "limits": ("Static references only. A resolved vtable slot names the method the offset "
                   "belongs to in the listed interfaces; it does not identify which object the "
                   "call uses, prove the path executes, or establish that a hook there works. "
                   "Offsets shared by several interfaces are reported with every candidate. "
                   "Object storage is only recovered when a creation call writes to static memory."),
    }
    with open(args.output, "w", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    exact = sum(1 for call in indirect if call["interface"])
    print(f"{sum(len(item['call_sites']) for item in entry_points)} call sites, "
          f"{len(iid_constants)} IID constants, {len(objects)} stored objects, "
          f"{len(analysis.targets)} functions, {len(indirect)} vtable calls, "
          f"{exact} bound to an interface -> {args.output}")


main()
