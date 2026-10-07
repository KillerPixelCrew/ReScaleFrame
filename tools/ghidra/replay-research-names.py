#!/usr/bin/env python3
"""Preflight a schema-1 research manifest against an explicitly selected Ghidra MCP program.

Check language, rebased RVA evidence hashes, function entry addresses, and conflicting names before
any mutation. Default mode reports verified records only. --apply waits for analysis, rechecks names,
renames eligible functions, verifies each rename, and saves the program. HTTP/validation/save errors
abort the run; already completed renames are not rolled back. File hashes in the manifest are provenance,
while this replay validates current imported memory ranges rather than the executable on disk.
"""

import argparse
import hashlib
import json
from pathlib import Path
import time
import urllib.parse
import urllib.request


def main():
    """Parse CLI options and run preflight followed by the explicitly requested mutation phase.

    --program is required for every MCP request. --analysis-timeout is seconds for waiting on
    auto-analysis; individual HTTP requests use a separate 30-second transport timeout.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--url", default="http://127.0.0.1:8089")
    parser.add_argument("--program", required=True)
    parser.add_argument("--apply", action="store_true", help="Rename and save after all checks pass")
    parser.add_argument("--analysis-timeout", type=float, default=180,
                        help="Seconds to wait for Ghidra analysis before writing")
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    if manifest["schema_version"] != 1:
        raise ValueError("Unsupported research manifest schema")

    def request(endpoint, body=None, **params):
        """Issue a JSON GET or body-bearing request scoped to the selected program.

        Raise for HTTP/JSON failures or a server error object; return the decoded response unchanged.
        """
        params["program"] = args.program
        url = args.url.rstrip("/") + endpoint + "?" + urllib.parse.urlencode(params)
        data = None if body is None else json.dumps(body).encode("utf-8")
        req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=30) as response:
            result = json.load(response)
        if isinstance(result, dict) and result.get("error"):
            raise RuntimeError(f"{endpoint}: {result['error']}")
        return result

    metadata = request("/get_metadata")
    if metadata["language"] != manifest["language"]:
        raise ValueError("Program language does not match the research")
    base = int(metadata["base_address"], 16)
    pending = []
    # Preflight every record before changing any name. RVAs support a rebased import.
    for record in manifest["functions"]:
        address = base + int(record["rva"], 16)
        memory = request("/read_memory", address=hex(address), length=record["evidence_length"])
        evidence = bytes.fromhex(memory["hex"])
        if len(evidence) != record["evidence_length"]:
            raise ValueError(f"Short evidence read for {record['name']}")
        if hashlib.sha256(evidence).hexdigest() != record["evidence_sha256"]:
            raise ValueError(f"Binary evidence mismatch for {record['name']}")
        function = request("/get_function_by_address", address=hex(address))
        if int(function["entry_point"], 16) != address:
            raise ValueError(f"Expected function entry at {hex(address)}")
        existing = function["name"]
        if existing != record["name"] and not existing.startswith("FUN_"):
            raise ValueError(f"Preserving existing name {existing} at {hex(address)}")
        pending.append((address, existing, record["name"]))
        print(f"verified {record['rva']} {record['name']} ({existing})")

    if not args.apply:
        print("Preflight passed. No names changed; use --apply to rename and save.")
        return
    deadline = time.monotonic() + args.analysis_timeout
    if request("/analysis_status").get("analyzing"):
        print("Waiting for Ghidra auto-analysis to finish before renaming and saving.", flush=True)
    while request("/analysis_status").get("analyzing"):
        if time.monotonic() >= deadline:
            raise TimeoutError("Auto-analysis is still running. Retry after it finishes.")
        time.sleep(2)
    for address, existing, name in pending:
        # Analysis or another user can rename a function while the wait runs.
        existing = request("/get_function_by_address", address=hex(address))["name"]
        if existing == name:
            continue
        if not existing.startswith("FUN_"):
            raise ValueError(f"Preserving name changed during analysis: {existing}")
        # strict_mode off preserves researched C++ engine symbol spelling in the MCP rename API.
        request("/rename_function", {"old_name": hex(address), "new_name": name,
                                     "strict_mode": "off"})
        actual = request("/get_function_by_address", address=hex(address))
        if actual["name"] != name:
            raise RuntimeError(f"Rename was not retained at {hex(address)}")
    result = request("/save_program")
    if isinstance(result, dict) and result.get("success") is False:
        raise RuntimeError(f"Program save failed: {result}")
    print("Researched names replayed and program saved.")


if __name__ == "__main__":
    main()
