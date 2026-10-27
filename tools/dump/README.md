# Module dump tools

The loader diagnostics (`loader/diagnostics`) write a decrypted runtime image of a protected
executable plus a JSON sidecar. The image's raw offsets equal RVAs. The sidecar (schema 2) carries
every structural fact the runtime knows, so an analysis database can be updated by script instead
of through an MCP adapter that cannot rename, read bytes or list names.

| File | Purpose |
| --- | --- |
| `apply_ida.py` | IDAPython (IDA 9.x): names, vtables, functions, name list, census comments |
| `apply_ghidra.py` | PyGhidra (Ghidra 12): the same behaviour |

Both scripts share one planning core and each has `--self-test <sidecar.json>`, which runs the
parsing, naming and rebasing without IDA or Ghidra:

```powershell
python -I tools/dump/apply_ghidra.py --self-test dumps/rdr2-20261025/RDR2.exe.dump.json
```

A schema 1 sidecar (such as the RDR2 example) still loads. It yields imports only and the scripts
say that the other sections are missing.

## Producing a dump

`rsf_dump_module(base, &options, &report)` writes `<name>.dump` and `<name>.dump.json`. Schema 2
adds two appended option fields; a caller built against the earlier header passes a shorter
`struct_size` and gets the defaults.

- `name_list_path_utf8`: optional TSV of `rva<TAB>name`. The RVA is `0x` hex or decimal, `#` starts a
  comment, blank lines are ignored, and malformed lines are counted in `names.skipped_lines`. A game
  plugin can write this file to contribute engine-specific names.
- `rtti_budget_ms`: wall-clock bound for the RTTI scan, 20000 when zero.

Reads of the target use `VirtualQuery`-checked ranges. A guard page or unmapped range shortens a
result and never faults.

## Applying a dump

Load the `.dump` file at its recorded image base, then run the script. Everything is keyed by RVA,
so only the module base in the database matters. The scripts print the difference when it is not the
dump's `load_base`. Absolute addresses recorded at dump time are moved with
`address - load_base + database_base`.

IDA: `ida -S"tools/dump/apply_ida.py RDR2.exe.dump.json [census-summary.json]" RDR2.exe.dump`, or
File > Script file. Ghidra: Script Manager, or headless
`analyzeHeadless <project> <name> -process RDR2.exe.dump -postScript apply_ghidra.py RDR2.exe.dump.json`.
`RSF_DUMP_SIDECAR` and `RSF_DUMP_CENSUS` are read when no arguments are given.

What is applied:

- Import slots become `__imp_<name>` (`__imp_<dll>_ord<N>` for ordinals), with duplicates suffixed.
- Vtables become `vtbl_<Class>` with `::` and other punctuation turned into `_`. A secondary vtable
  of a multiple-inheritance class gets `_off<offset>`. The base class list is attached as a comment.
- Functions are created at every unchained `.pdata` start.
- The caller name list is applied as labels.
- The optional vendor census summary (`census-summary.json`, to be documented in
  `loader/census/README.md`) is read tolerantly: a list, or an object with `entries`, `sites` or
  `calls`. Each entry carries `module`, `rva` (or `va`, rebased by the sidecar load base), `api`,
  `function` and `count`. Entries for other modules are skipped. The site gets a comment
  `census: <api> x<count>`, and a still default-named function containing it takes `function` when
  that is a real name.

The scripts have been run offline only for planning. The IDA and Ghidra API calls have not been
executed against a database.

## Sidecar schema 2

Existing schema 1 fields keep their meaning: `tool`, `abi_version`, `module`, `code_entropy`,
`iat_references`, `sections`, `modules`, `imports`, `limits`. Addresses and RVAs are `0x` hex strings
unless noted.

- `schema_version`: `2`.
- `sections[]`: adds `dumped_size`, `readable_bytes` (bytes that were actually readable, the rest are
  zero), `entropy` (sampled, same helper as `code_entropy`) and `sha256` over the `dumped_size`
  bytes at that RVA in the dump. Empty sections have none of these.
- `threads`: `list[]` of `{tid, dumping, start_address, start_module, start_rva, description,
  priority, base_priority}`, plus `count`, `truncated`, `start_address_source`, `elapsed_ms`. The
  start address is `ThreadQuerySetWin32StartAddress`. `start_module` and `start_rva` appear when it
  lies in a loaded module. `description` appears when set. `dumping` marks the calling thread.
- `rtti`: `classes[]` of `{vtable_rva, locator_rva, name, decorated, offset, cd_offset, bases[],
  declared_bases}`, where each base is `{name, contained, mdisp, pdisp, vdisp, attributes}` and the
  class itself is not repeated. `count`, `partial`, `budget_ms`, `elapsed_ms`, and `reason` when the
  list is empty or partial (no RTTI, stripped, time budget, class bound). `name` is undecorated;
  templates use dbghelp from System32 when available and fall back to the decorated text.
  The scan finds MSVC x64 `RTTICompleteObjectLocator` records (signature 1, `pSelf` equal to its own
  RVA) referenced from `vtable[-1]`; `vtable_rva` is the address of `vtable[0]`.
- `functions`: `entries[]` of `[begin_rva, end_rva, unwind_rva, chained]` as decimal integers, sorted
  and deduplicated by `begin_rva`. `chained` is 1 for a `UNW_FLAG_CHAININFO` part of another
  function, which is not a function start. Also `raw_entries`, `invalid`, `count`, `chained`,
  `source`, `elapsed_ms` and `reason` when the directory is missing or partly unreadable.
- `names`: `entries[]` of `{rva, name}` from the supplied TSV, plus `source` (null when none),
  `count`, `skipped_lines`, `reason`.
- `timing_ms`: `section_hash_and_entropy`, `structured_sections`, `total`.

Threads, RTTI and function starts describe the process at dump time. The dump is not runnable.
