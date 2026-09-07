# Apply recovered Unreal reflected function names to a Ghidra program.
#
# Reads the JSON that find-native-registrations.py produces from a decrypted module dump and names
# the exec thunks and reflection constructors it resolved. Run inside Ghidra with PyGhidra, from
# the Script Manager or headless:
#   python -m pyghidra.ghidra_launch --install-dir "$GHIDRA_INSTALL_DIR" \
#       ghidra.app.util.headless.AnalyzeHeadless <project_dir> <project> -process <program> \
#       -noanalysis -postScript apply-native-names.py \
#       --names .local/ghidra/ac7-native-names.json --apply
#
# Without --apply the script only reports what it would do. Names are placed in a namespace per
# class, so `AActor::execWasRecentlyRendered` becomes `execWasRecentlyRendered` inside `AActor`.
#
# The names come from the module's own registration arrays, so an applied name is as good as the
# match that produced it. An exec thunk is a generated argument unpacker: naming it does not name
# the engine function it calls.
# @category ReScaleFrame

import argparse
import json
import os

from ghidra.program.model.symbol import SourceType

SOURCE = SourceType.ANALYSIS


def parse_arguments(raw):
    parser = argparse.ArgumentParser(prog="apply-native-names")
    parser.add_argument("--names", default=os.environ.get("RSF_NATIVE_NAMES",
                                                          "ac7-native-names.json"),
                        help="JSON from find-native-registrations.py")
    parser.add_argument("--apply", action="store_true",
                        help="write names into the program instead of only reporting")
    parser.add_argument("--shape", choices=["all", "exec", "constructor"], default="all",
                        help="which recovered arrays to apply")
    parser.add_argument("--exact-only", action="store_true",
                        help="skip arrays whose size did not equal the class's declared count")
    parser.add_argument("--namespace", action="store_true", default=True,
                        help="place each name in a namespace named after its class")
    return parser.parse_args(raw)


def resolve_namespace(program, cache, name):
    """Return the namespace for a class, creating it once per run."""
    if name in cache:
        return cache[name]
    manager = program.getSymbolTable()
    namespace = manager.getNamespace(name, program.getGlobalNamespace())
    if namespace is None:
        namespace = manager.createNameSpace(program.getGlobalNamespace(), name, SOURCE)
    cache[name] = namespace
    return namespace


def main():
    arguments = parse_arguments(getScriptArgs())
    with open(arguments.names) as handle:
        recovered = json.load(handle)

    wanted = {"all": {"FNameNativePtrPair", "FClassFunctionLinkInfo"},
              "exec": {"FNameNativePtrPair"},
              "constructor": {"FClassFunctionLinkInfo"}}[arguments.shape]

    listing = currentProgram.getFunctionManager()
    space = currentProgram.getAddressFactory().getDefaultAddressSpace()
    namespaces = {}

    counts = {"considered": 0, "renamed": 0, "already": 0, "no_function": 0,
              "held": 0, "failed": 0, "aliases": 0}
    missing = []

    for record in recovered["functions"]:
        if record["shape"] not in wanted:
            continue
        if arguments.exact_only and not record.get("exact"):
            continue
        counts["considered"] += 1

        address = space.getAddress(int(record["address"], 16))
        function = listing.getFunctionAt(address)
        if function is None:
            counts["no_function"] += 1
            if len(missing) < 50:
                missing.append(record)
            continue

        owner, _, bare = record["symbol"].partition("::")
        if not bare:
            owner, bare = None, record["symbol"]

        # A name that analysis did not invent is evidence someone already identified this
        # function. Do not overwrite it.
        existing = function.getName()
        if not existing.startswith("FUN_") and existing != bare:
            if function.getSymbol().getSource() == SourceType.USER_DEFINED:
                counts["held"] += 1
                continue
        if existing == bare:
            counts["already"] += 1
            continue

        if not arguments.apply:
            counts["renamed"] += 1
            continue

        try:
            if owner and arguments.namespace:
                function.setName(bare, SOURCE)
                function.setParentNamespace(resolve_namespace(currentProgram, namespaces, owner))
            else:
                function.setName(record["symbol"].replace("::", "_"), SOURCE)
            counts["renamed"] += 1
        except Exception as error:                      # noqa: BLE001 - report and keep going
            counts["failed"] += 1
            print("could not name %s at %s: %s" % (record["symbol"], record["address"], error))
            continue

        # Identical bodies are folded by the linker, so one address can carry several names. The
        # extra names go in a comment rather than silently disappearing.
        folded = record.get("folded_with")
        if folded:
            counts["aliases"] += 1
            function.setComment("Identical code folded with:\n  " + "\n  ".join(folded))

    print("")
    print("recovered names in file  %d" % len(recovered["functions"]))
    print("considered               %d" % counts["considered"])
    print("named                    %d%s" % (counts["renamed"],
                                             "" if arguments.apply else " (dry run)"))
    print("  carrying folded names  %d" % counts["aliases"])
    print("already correct          %d" % counts["already"])
    print("held, user named         %d" % counts["held"])
    print("no function at address   %d" % counts["no_function"])
    print("failed                   %d" % counts["failed"])
    if missing:
        print("")
        print("first addresses with no function:")
        for record in missing[:10]:
            print("  %s  %s" % (record["address"], record["symbol"]))


main()
