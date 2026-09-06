# Export the data types of the current Ghidra program into a .gdt archive.
#
# Use this on a binary that carries debug information, so the layouts come from the compiler
# rather than from a header parse. For UE4 that means a locally built engine binary with its
# PDB, imported with the PDB analyser enabled:
#   python -m pyghidra.ghidra_launch --install-dir "$GHIDRA_INSTALL_DIR" \
#       ghidra.app.util.headless.AnalyzeHeadless <project_dir> <project> \
#       -import UE4Editor-D3D11RHI.dll -postScript export-types.py --output ue4-d3d11rhi.gdt
#
# Layouts are only as close to a shipped game as the build that produced them. An editor build
# and a monolithic shipping build do not lay structures out the same way.
# @category ReScaleFrame

import argparse

from ghidra.program.model.data import FileDataTypeManager, DataTypeConflictHandler
from ghidra.util.task import ConsoleTaskMonitor
from java.io import File


def parse_arguments(raw):
    parser = argparse.ArgumentParser(prog="export-types")
    parser.add_argument("--output", required=True, help="destination .gdt path")
    parser.add_argument("--prefix", default=None,
                        help="only export types whose category path starts with this")
    return parser.parse_args(list(raw))


def main():
    args = parse_arguments(getScriptArgs())
    monitor = ConsoleTaskMonitor()
    source = currentProgram.getDataTypeManager()

    target = File(args.output)
    if target.exists():
        target.delete()
    # The archive has to carry the program's architecture, otherwise it falls back to the default
    # data organization and every pointer in the exported types comes out the wrong size.
    archive = FileDataTypeManager.createFileArchive(
        target, str(currentProgram.getLanguageID()),
        str(currentProgram.getCompilerSpec().getCompilerSpecID()))
    transaction = archive.startTransaction("export")
    exported = 0
    skipped = 0
    try:
        for data_type in source.getAllDataTypes():
            if args.prefix and not str(data_type.getCategoryPath()).startswith(args.prefix):
                skipped += 1
                continue
            archive.addDataType(data_type, DataTypeConflictHandler.REPLACE_HANDLER)
            exported += 1
    finally:
        archive.endTransaction(transaction, True)
    archive.save()
    archive.close()
    print(f"exported {exported} data types ({skipped} outside the prefix) from "
          f"{currentProgram.getName()} to {args.output}")


main()
